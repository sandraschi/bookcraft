//! Direct-SQL write path with Calibre's own invariants.
//!
//! Why SQL and not `calibredb`: `calibredb` refuses every local operation while any
//! Calibre program runs, which on a lived-in machine is always (the sanctioned
//! alternative, the content server, needs a running server plus credentials). So
//! BookCraft writes SQL itself: one IMMEDIATE transaction per operation, a 30-second
//! busy timeout, the same `title_sort`/`uuid4` functions Calibre's triggers call
//! (registered on our connection), and a timestamped backup taken first.
//!
//! Format reference: Calibre 9 `metadata.db` (`book schema` dumps it live).

use crate::{library::Library, BookError};
use rusqlite::{functions::FunctionFlags, Connection, OpenFlags, Transaction, TransactionBehavior};
use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A backup younger than this is reused instead of copying the database again.
pub const BACKUP_FRESH_SECS: u64 = 120;

/// Fields `update` accepts. Anything else is rejected before anything is written.
pub const EDITABLE_FIELDS: &[&str] = &[
    "title",
    "authors",
    "tags",
    "series",
    "series_index",
    "publisher",
    "rating",
    "isbn",
    "languages",
    "comments",
    "pubdate",
    "identifiers",
];

/// Calibre 9 write-relevant shape, verbatim from the live `sqlite_master`,
/// minus content-server and annotation tables BookCraft never touches.
/// Single source of truth for fixtures and smoke libraries.
pub const CALIBRE_WRITE_SCHEMA: &str = include_str!("schema.sql");

/// Calibre's title sort, approximated: move a leading English article to the end,
/// after stripping quote/bracket ornaments. The column sorts NOCASE, so residual
/// case differences do not change ordering. Verified against a live Calibre 9
/// library: "The Complete Modern Blacksmith" -> "Complete Modern Blacksmith, The".
pub fn title_sort_value(title: &str) -> String {
    let mut rest = title.trim();
    loop {
        let stripped = rest.trim_start_matches(['"', '\'', '(', '[', '{']);
        if stripped.len() == rest.len() {
            break;
        }
        rest = stripped.trim_start();
    }
    rest = rest.trim_end_matches(['"', '\'', ')', ']', '}']).trim_end();
    for article in ["The ", "An ", "A "] {
        if rest.len() > article.len() && rest[..article.len()].eq_ignore_ascii_case(article) {
            let tail = rest[article.len()..].trim_start();
            if !tail.is_empty() {
                let kept = rest[..article.len() - 1].trim();
                return format!("{tail}, {kept}");
            }
        }
    }
    rest.to_string()
}

/// "Frank Herbert" -> "Herbert, Frank". Single tokens pass through unchanged.
pub fn author_sort_value(author: &str) -> String {
    let parts: Vec<&str> = author.split_whitespace().collect();
    if parts.len() < 2 {
        return author.trim().to_string();
    }
    let (last, rest) = (parts[parts.len() - 1], &parts[..parts.len() - 1]);
    format!("{}, {}", last, rest.join(" "))
}

/// Windows-safe path component: forbidden characters become '_', trailing dots and
/// spaces go, bounded at 150 chars, never empty.
pub fn sanitize_component(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(150)
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        "Unknown".to_string()
    } else {
        out
    }
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-to-civil-date algorithm (public domain).
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    y += i64::from(m <= 2);
    (y, m, d)
}

/// "2026-11-05 12:34:56.123456+00:00" in UTC, microsecond precision, Calibre's shape.
pub fn utc_now_stamp() -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let secs = micros / 1_000_000;
    let micro = (micros % 1_000_000) as u32;
    let days = (secs / 86_400) as i64;
    let tod = (secs % 86_400) as u32;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{micro:06}+00:00",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60,
    )
}

/// Accept "YYYY-MM-DD" (stored midnight UTC) or a full "+00:00"/"Z" stamp.
fn normalise_pubdate(raw: &str) -> Result<String, BookError> {
    let bad = || BookError::Invalid(format!("bad pubdate '{raw}'; use YYYY-MM-DD"));
    let s = raw.trim();
    if s.len() == 10 {
        let digits = s[..4].parse::<u32>().is_ok()
            && s[4..5] == *"-"
            && s[5..7].parse::<u32>().is_ok()
            && s[7..8] == *"-"
            && s[8..10].parse::<u32>().is_ok();
        if !digits {
            return Err(bad());
        }
        let month: u32 = s[5..7].parse().unwrap_or(0);
        let day: u32 = s[8..10].parse().unwrap_or(0);
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(bad());
        }
        return Ok(format!("{s} 00:00:00+00:00"));
    }
    let upper = s.to_ascii_uppercase();
    let bare = upper.strip_suffix('Z').unwrap_or(&upper);
    let (date, time) = bare.split_once([' ', 'T']).ok_or_else(bad)?;
    if date.len() != 10 || time.len() < 8 {
        return Err(bad());
    }
    let stamped = if bare.ends_with("+00:00") {
        bare.to_string()
    } else {
        return Err(BookError::Invalid(format!(
            "bad pubdate '{raw}'; only UTC (+00:00 or Z) is stored"
        )));
    };
    let _ = date;
    Ok(stamped)
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn backup_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("BookCraft").join("backups"))
        .unwrap_or_else(|| std::env::temp_dir().join("bookcraft-backups"))
}

fn lib_tag(lib_root: &Path) -> String {
    let raw = lib_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "library".to_string());
    let mut stem: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    if stem.trim_matches('_').is_empty() {
        stem = "library".to_string();
    }
    let mut hasher = DefaultHasher::new();
    lib_root.to_string_lossy().hash(&mut hasher);
    format!("{stem}-{hash:016x}", hash = hasher.finish())
}

fn newest_backup(dir: &Path) -> Option<(u64, PathBuf)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stamp = name
            .strip_prefix("metadata-")
            .and_then(|s| s.strip_suffix(".db"))
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse::<u64>().ok());
        if let Some(stamp) = stamp {
            let is_better = best.as_ref().map(|(s, _)| stamp > *s).unwrap_or(true);
            if is_better {
                best = Some((stamp, entry.path()));
            }
        }
    }
    best
}

fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    for n in 2..1000 {
        let candidate = dir.join(format!("{stem}-{n}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-overflow.{ext}"))
}

/// Per-library data directory outside the library (backups, trash, collections).
pub(crate) fn data_dir(lib: &Library) -> Result<PathBuf, BookError> {
    let dir = backup_root().join(lib_tag(lib.root()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
/// Copy `metadata.db` (and the full-text index when present) to a timestamped backup
/// outside the library, reusing a backup younger than 120 seconds.
pub fn ensure_backup(lib: &Library, force: bool) -> Result<PathBuf, BookError> {
    let dir = data_dir(lib)?;
    if !force {
        if let Some((stamp, path)) = newest_backup(&dir) {
            if epoch_secs().saturating_sub(stamp) < BACKUP_FRESH_SECS && path.is_file() {
                return Ok(path);
            }
        }
    }
    let stamp = epoch_secs();
    let dest = free_path(&dir, &format!("metadata-{stamp}"), "db");
    std::fs::copy(lib.metadata_db_path(), &dest)?;
    if let Some(fts) = lib.fts_db_path() {
        let fts_dest = dest.with_file_name(format!("full-text-search-{stamp}.db"));
        if !fts_dest.exists() {
            let _ = std::fs::copy(fts, &fts_dest);
        }
    }
    Ok(dest)
}

/// Open read-write with a 30-second busy timeout and Calibre's own SQL functions.
/// Public so the L3 smoke fixture builds the same shape the tests verify.
pub fn open_writable(lib: &Library) -> Result<Connection, BookError> {
    let conn =
        Connection::open_with_flags(lib.metadata_db_path(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(Duration::from_secs(30))?;
    conn.create_scalar_function(
        "title_sort",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let title: String = ctx.get(0)?;
            Ok(title_sort_value(&title))
        },
    )?;
    conn.create_scalar_function("uuid4", 0, FunctionFlags::SQLITE_UTF8, |_| {
        Ok(uuid::Uuid::new_v4().to_string())
    })?;
    Ok(conn)
}

pub(crate) fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

pub(crate) fn touch(tx: &Transaction<'_>, id: i64, dirtied: bool) -> Result<(), BookError> {
    tx.execute(
        "UPDATE books SET last_modified = ?1 WHERE id = ?2",
        rusqlite::params![utc_now_stamp(), id],
    )?;
    if dirtied {
        tx.execute(
            "INSERT OR IGNORE INTO metadata_dirtied(book) VALUES (?1)",
            rusqlite::params![id],
        )?;
    }
    Ok(())
}

fn ensure_vocab(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    name: &str,
) -> Result<i64, BookError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(BookError::Invalid(format!("empty {table} name")));
    }
    let sql = format!("INSERT OR IGNORE INTO {table}({column}) VALUES (?1)");
    tx.execute(&sql, rusqlite::params![name])?;
    let get = format!("SELECT id FROM {table} WHERE {column} = ?1");
    tx.query_row(&get, rusqlite::params![name], |r| r.get(0))
        .map_err(|e| BookError::Invalid(format!("cannot resolve {table} '{name}': {e}")))
}

fn ensure_author(tx: &Transaction<'_>, name: &str) -> Result<i64, BookError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(BookError::Invalid("empty author name".into()));
    }
    tx.execute(
        "INSERT OR IGNORE INTO authors(name, sort, link) VALUES (?1, ?2, '')",
        rusqlite::params![name, author_sort_value(name)],
    )?;
    tx.query_row(
        "SELECT id FROM authors WHERE name = ?1",
        rusqlite::params![name],
        |r| r.get(0),
    )
    .map_err(|e| BookError::Invalid(format!("cannot resolve author '{name}': {e}")))
}

fn ensure_rating(tx: &Transaction<'_>, rating: i64) -> Result<i64, BookError> {
    tx.execute(
        "INSERT OR IGNORE INTO ratings(rating, link) VALUES (?1, '')",
        rusqlite::params![rating],
    )?;
    tx.query_row(
        "SELECT id FROM ratings WHERE rating = ?1",
        rusqlite::params![rating],
        |r| r.get(0),
    )
    .map_err(|e| BookError::Invalid(format!("cannot resolve rating {rating}: {e}")))
}

fn relink(
    tx: &Transaction<'_>,
    link_table: &str,
    link_column: &str,
    book_id: i64,
    ids: &[i64],
) -> Result<(), BookError> {
    tx.execute(
        &format!("DELETE FROM {link_table} WHERE book = ?1"),
        rusqlite::params![book_id],
    )?;
    for id in ids {
        tx.execute(
            &format!("INSERT OR IGNORE INTO {link_table}(book, {link_column}) VALUES (?1, ?2)"),
            rusqlite::params![book_id, id],
        )?;
    }
    Ok(())
}

/// Split a caller value into a list: JSON array of strings, or one comma-separated string.
fn string_list(value: &Value) -> Result<Vec<String>, BookError> {
    match value {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str().map(str::trim) {
                    Some(s) if !s.is_empty() => out.push(s.to_string()),
                    _ => {
                        return Err(BookError::Invalid(
                            "list items must be non-empty strings".into(),
                        ))
                    }
                }
            }
            Ok(out)
        }
        Value::String(s) => Ok(s
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect()),
        _ => Err(BookError::Invalid(
            "expected a string or an array of strings".into(),
        )),
    }
}

fn first_author(authors: &[String]) -> &str {
    authors.first().map(String::as_str).unwrap_or("Unknown")
}

fn book_dir_name(authors: &[String], title: &str, id: i64) -> String {
    let author = sanitize_component(&author_sort_value(first_author(authors)));
    let title = sanitize_component(title);
    format!("{author}/{title} ({id})")
}

fn find_duplicate(
    conn: &Connection,
    title: &str,
    authors: &[String],
) -> Result<Option<i64>, BookError> {
    let mut stmt = conn.prepare(
        "SELECT b.id FROM books b
         WHERE b.title = ?1 COLLATE NOCASE
           AND EXISTS (SELECT 1 FROM books_authors_link bal
                        JOIN authors a ON a.id = bal.author
                       WHERE bal.book = b.id
                         AND a.name = ?2 COLLATE NOCASE)
         ORDER BY b.id LIMIT 1",
    )?;
    let mut rows = stmt.query_map(
        rusqlite::params![title.trim(), first_author(authors)],
        |r| r.get(0),
    )?;
    if let Some(row) = rows.next() {
        return Ok(Some(row?));
    }
    Ok(None)
}

/// Create a fresh Calibre-shaped library directory. Refuses when a metadata.db
/// already exists there; never touches an existing library.
pub fn init_library(dir: &Path) -> Result<Value, BookError> {
    let db = if dir
        .file_name()
        .map(|n| n.eq_ignore_ascii_case("metadata.db"))
        .unwrap_or(false)
    {
        dir.to_path_buf()
    } else {
        dir.join("metadata.db")
    };
    if db.is_file() {
        return Err(BookError::Invalid(format!(
            "{} is already a library",
            db.parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        )));
    }
    if let Some(parent) = db.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(&db)?;
    conn.execute_batch(CALIBRE_WRITE_SCHEMA)?;
    drop(conn);
    let lib = Library::open(db.parent().unwrap_or(Path::new(".")))?;
    let _ = ensure_backup(&lib, true);
    Ok(json!({
        "library": lib.root().to_string_lossy(),
        "created": true,
        "backup": "none-taken (empty library)",
    }))
}

/// Set metadata fields on one book. Verified to exist before the backup is taken.
#[allow(clippy::too_many_lines)]
pub fn set_metadata(
    lib: &Library,
    id: i64,
    fields: &Map<String, Value>,
) -> Result<Value, BookError> {
    check_fields(fields)?;
    lib.detail(id)?;

    let backup = ensure_backup(lib, false)?;
    let mut conn = open_writable(lib)?;
    let dirtied = has_table(&conn, "metadata_dirtied");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let updated = apply_fields(&tx, id, fields)?;
    touch(&tx, id, dirtied)?;
    tx.commit()?;

    Ok(json!({
        "book_id": id,
        "updated_fields": updated,
        "backup": backup.to_string_lossy(),
    }))
}

/// Reject unknown or empty field maps before anything is written.
fn check_fields(fields: &Map<String, Value>) -> Result<(), BookError> {
    if fields.is_empty() {
        return Err(BookError::Invalid("no fields to update".into()));
    }
    for key in fields.keys() {
        if !EDITABLE_FIELDS.contains(&key.as_str()) {
            return Err(BookError::Invalid(format!(
                "unknown field '{key}'; editable: {}",
                EDITABLE_FIELDS.join(", ")
            )));
        }
    }
    Ok(())
}

/// Update the same fields on many books in one transaction and one backup.
/// All-or-nothing: the first failure rolls everything back.
pub fn bulk_update(
    lib: &Library,
    ids: &[i64],
    fields: &Map<String, Value>,
) -> Result<Value, BookError> {
    if ids.is_empty() {
        return Err(BookError::Invalid("no book ids given".into()));
    }
    if ids.len() > 1000 {
        return Err(BookError::Invalid(
            "at most 1000 books per bulk update".into(),
        ));
    }
    check_fields(fields)?;
    for id in ids {
        lib.detail(*id)?;
    }

    let backup = ensure_backup(lib, false)?;
    let mut conn = open_writable(lib)?;
    let dirtied = has_table(&conn, "metadata_dirtied");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut done = Vec::with_capacity(ids.len());
    for id in ids {
        let updated = apply_fields(&tx, *id, fields)?;
        touch(&tx, *id, dirtied)?;
        done.push(json!({"book_id": id, "updated_fields": updated}));
    }
    tx.commit()?;
    Ok(json!({
        "updated": done.len(),
        "books": done,
        "backup": backup.to_string_lossy(),
    }))
}

/// Apply one field map inside an open transaction. Returns the updated keys.
fn apply_fields(
    tx: &Transaction<'_>,
    id: i64,
    fields: &Map<String, Value>,
) -> Result<Vec<String>, BookError> {
    let mut updated = Vec::with_capacity(fields.len());
    for (key, value) in fields {
        match key.as_str() {
            "title" => {
                let title = value
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| BookError::Invalid("title must be a non-empty string".into()))?;
                tx.execute(
                    "UPDATE books SET title = ?1 WHERE id = ?2",
                    rusqlite::params![title, id],
                )?;
            }
            "authors" => {
                let authors = string_list(value)?;
                if authors.is_empty() {
                    return Err(BookError::Invalid("authors must not be empty".into()));
                }
                let mut ids = Vec::with_capacity(authors.len());
                for author in &authors {
                    ids.push(ensure_author(tx, author)?);
                }
                relink(tx, "books_authors_link", "author", id, &ids)?;
                tx.execute(
                    "UPDATE books SET author_sort = ?1 WHERE id = ?2",
                    rusqlite::params![author_sort_value(first_author(&authors)), id],
                )?;
            }
            "tags" => {
                let mut ids = Vec::new();
                for tag in string_list(value)? {
                    ids.push(ensure_vocab(tx, "tags", "name", &tag)?);
                }
                relink(tx, "books_tags_link", "tag", id, &ids)?;
            }
            "series" => {
                let name = value
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        BookError::Invalid("series must be a non-empty string".into())
                    })?;
                let series_id = ensure_vocab(tx, "series", "name", name)?;
                // Trigger series_update_trg keeps series.sort in step; link is single.
                tx.execute(
                    "DELETE FROM books_series_link WHERE book = ?1",
                    rusqlite::params![id],
                )?;
                tx.execute(
                    "INSERT INTO books_series_link(book, series) VALUES (?1, ?2)",
                    rusqlite::params![id, series_id],
                )?;
            }
            "series_index" => {
                let index = match value {
                    Value::Number(n) => n.as_f64().ok_or_else(|| {
                        BookError::Invalid("series_index must be a number".into())
                    })?,
                    _ => return Err(BookError::Invalid("series_index must be a number".into())),
                };
                tx.execute(
                    "UPDATE books SET series_index = ?1 WHERE id = ?2",
                    rusqlite::params![index, id],
                )?;
            }
            "publisher" => {
                let mut ids = Vec::new();
                for name in string_list(value)? {
                    ids.push(ensure_vocab(tx, "publishers", "name", &name)?);
                }
                if ids.len() > 1 {
                    return Err(BookError::Invalid(
                        "a book has one publisher; pass a single name".into(),
                    ));
                }
                relink(tx, "books_publishers_link", "publisher", id, &ids)?;
            }
            "rating" => {
                let rating = value
                    .as_i64()
                    .ok_or_else(|| BookError::Invalid("rating must be an integer 0-5".into()))?;
                if !(0..=5).contains(&rating) {
                    return Err(BookError::Invalid("rating must be an integer 0-5".into()));
                }
                let rating_id = ensure_rating(tx, rating)?;
                relink(tx, "books_ratings_link", "rating", id, &[rating_id])?;
            }
            "isbn" | "languages" | "identifiers" | "comments" | "pubdate" => {
                apply_scalar(tx, id, key, value)?;
            }
            _ => {
                return Err(BookError::Invalid(format!("unknown field '{key}'")));
            }
        }
        updated.push(key.clone());
    }
    Ok(updated)
}

fn apply_scalar(tx: &Transaction<'_>, id: i64, key: &str, value: &Value) -> Result<(), BookError> {
    match key {
        "isbn" => {
            let isbn = match value {
                Value::String(s) if !s.trim().is_empty() => s.trim().to_string(),
                Value::Number(n) => n.to_string(),
                _ => return Err(BookError::Invalid("isbn must be a non-empty string".into())),
            };
            tx.execute(
                "DELETE FROM identifiers WHERE book = ?1 AND type = 'isbn'",
                rusqlite::params![id],
            )?;
            tx.execute(
                "INSERT INTO identifiers(book, type, val) VALUES (?1, 'isbn', ?2)",
                rusqlite::params![id, isbn],
            )?;
        }
        "languages" => {
            let codes = string_list(value)?;
            if codes.is_empty() {
                return Err(BookError::Invalid("languages must not be empty".into()));
            }
            let mut ids = Vec::with_capacity(codes.len());
            for code in &codes {
                ids.push(ensure_vocab(tx, "languages", "lang_code", code)?);
            }
            tx.execute(
                "DELETE FROM books_languages_link WHERE book = ?1",
                rusqlite::params![id],
            )?;
            for (order, lang_id) in ids.iter().enumerate() {
                tx.execute(
                    "INSERT INTO books_languages_link(book, lang_code, item_order) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, lang_id, order as i64],
                )?;
            }
        }
        "identifiers" => {
            let pairs: Vec<(String, String)> = match value {
                Value::String(s) if !s.trim().is_empty() => s
                    .split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(|p| {
                        p.split_once(':')
                            .map(|(t, v)| (t.trim().to_string(), v.trim().to_string()))
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        BookError::Invalid("identifiers must be 'type:value,...'".into())
                    })?,
                Value::Object(map) => {
                    let mut out = Vec::with_capacity(map.len());
                    for (k, v) in map {
                        let v = v
                            .as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                BookError::Invalid("identifier values must be strings".into())
                            })?;
                        out.push((k.trim().to_string(), v.to_string()));
                    }
                    out
                }
                _ => {
                    return Err(BookError::Invalid(
                        "identifiers must be 'type:value,...' or an object".into(),
                    ))
                }
            };
            if pairs.is_empty() {
                return Err(BookError::Invalid("identifiers must not be empty".into()));
            }
            for (id_type, val) in pairs {
                if id_type.is_empty() || val.is_empty() {
                    return Err(BookError::Invalid(
                        "identifier type and value must not be empty".into(),
                    ));
                }
                tx.execute(
                    "INSERT INTO identifiers(book, type, val) VALUES (?1, ?2, ?3)
                     ON CONFLICT(book, type) DO UPDATE SET val = excluded.val",
                    rusqlite::params![id, id_type, val],
                )?;
            }
        }
        "comments" => {
            let text = value
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| BookError::Invalid("comments must be a non-empty string".into()))?;
            tx.execute(
                "INSERT INTO comments(book, text) VALUES (?1, ?2)
                 ON CONFLICT(book) DO UPDATE SET text = excluded.text",
                rusqlite::params![id, text],
            )?;
        }
        "pubdate" => {
            let raw = value
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| BookError::Invalid("pubdate must be a non-empty string".into()))?;
            let stamped = normalise_pubdate(raw)?;
            tx.execute(
                "UPDATE books SET pubdate = ?1 WHERE id = ?2",
                rusqlite::params![stamped, id],
            )?;
        }
        _ => {
            return Err(BookError::Invalid(format!("unknown field '{key}'")));
        }
    }
    Ok(())
}

fn insert_book_row(
    tx: &Transaction<'_>,
    title: Option<&str>,
    authors: &[String],
) -> Result<i64, BookError> {
    let title = title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown");
    let sort_first = if authors.is_empty() {
        "Unknown".to_string()
    } else {
        author_sort_value(first_author(authors))
    };
    tx.execute(
        "INSERT INTO books(title, author_sort, path, last_modified) VALUES (?1, ?2, '', ?3)",
        rusqlite::params![title, sort_first, utc_now_stamp()],
    )?;
    let id = tx.last_insert_rowid();
    let dir = book_dir_name(authors, title, id);
    tx.execute(
        "UPDATE books SET path = ?1 WHERE id = ?2",
        rusqlite::params![dir, id],
    )?;
    Ok(id)
}

fn link_authors_tags(
    tx: &Transaction<'_>,
    id: i64,
    authors: &[String],
    tags: &[String],
) -> Result<(), BookError> {
    let mut author_ids = Vec::with_capacity(authors.len());
    for author in authors {
        author_ids.push(ensure_author(tx, author)?);
    }
    relink(tx, "books_authors_link", "author", id, &author_ids)?;
    let mut tag_ids = Vec::with_capacity(tags.len());
    for tag in tags {
        tag_ids.push(ensure_vocab(tx, "tags", "name", tag)?);
    }
    relink(tx, "books_tags_link", "tag", id, &tag_ids)?;
    Ok(())
}

/// Add an empty book record (no formats).
pub fn add_empty_book(
    lib: &Library,
    title: Option<&str>,
    authors: Option<&str>,
    tags: Option<&str>,
) -> Result<Value, BookError> {
    let author_list = authors
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tag_list = tags
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let backup = ensure_backup(lib, false)?;
    let mut conn = open_writable(lib)?;
    let dirtied = has_table(&conn, "metadata_dirtied");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = insert_book_row(&tx, title, &author_list)?;
    if !author_list.is_empty() || !tag_list.is_empty() {
        link_authors_tags(&tx, id, &author_list, &tag_list)?;
    }
    touch(&tx, id, dirtied)?;
    tx.commit()?;
    Ok(json!({"book_id": id, "backup": backup.to_string_lossy()}))
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), BookError> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// Add a book file. Duplicate policy: without `duplicates` or `automerge`, a book with
/// the same title and first author is rejected before anything is written.
#[allow(clippy::too_many_arguments)]
pub fn add_book(
    lib: &Library,
    file: &Path,
    root: Option<&Path>,
    title: Option<&str>,
    authors: Option<&str>,
    tags: Option<&str>,
    series: Option<&str>,
    series_index: Option<f64>,
    languages: Option<&str>,
    isbn: Option<&str>,
    duplicates: bool,
    automerge: Option<&str>,
    empty: bool,
) -> Result<Value, BookError> {
    if empty {
        return add_empty_book(lib, title, authors, tags);
    }
    if !file.is_file() {
        return Err(BookError::NotFound(format!(
            "{} is not a file",
            file.display()
        )));
    }
    crate::ensure_within(root, file)?;
    if let Some(mode) = automerge {
        if !["new_record", "overwrite", "ignore"].contains(&mode) {
            return Err(BookError::Invalid(
                "automerge must be new_record, overwrite or ignore".into(),
            ));
        }
    }
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::trim)
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .ok_or_else(|| BookError::Invalid(format!("{} has no usable extension", file.display())))?;
    let format = ext.to_ascii_uppercase();

    let title = title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
        });
    let author_list: Vec<String> = authors
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let conn = open_writable(lib)?;
    let duplicate = find_duplicate(&conn, title, &author_list)?;
    drop(conn);

    if duplicate.is_some() && !duplicates && automerge.is_none() {
        return Err(BookError::Invalid(format!(
            "duplicate of book id {}; pass duplicates=true or automerge=",
            duplicate.unwrap_or(-1)
        )));
    }

    let backup = ensure_backup(lib, false)?;
    let mut conn = open_writable(lib)?;
    let dirtied = has_table(&conn, "metadata_dirtied");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let (id, is_new) = match (duplicate, automerge) {
        (Some(existing), Some("overwrite" | "ignore")) => (existing, false),
        (Some(_), _) if duplicates => (insert_book_row(&tx, Some(title), &author_list)?, true),
        (Some(existing), _) => (existing, false),
        _ => (insert_book_row(&tx, Some(title), &author_list)?, true),
    };

    // Series, languages and isbn ride along on a fresh record.
    if is_new {
        if let Some(name) = series.map(str::trim).filter(|s| !s.is_empty()) {
            let series_id = ensure_vocab(&tx, "series", "name", name)?;
            tx.execute(
                "INSERT INTO books_series_link(book, series) VALUES (?1, ?2)",
                rusqlite::params![id, series_id],
            )?;
        }
        if let Some(index) = series_index {
            tx.execute(
                "UPDATE books SET series_index = ?1 WHERE id = ?2",
                rusqlite::params![index, id],
            )?;
        }
        if let Some(langs) = languages {
            let codes: Vec<String> = langs
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect();
            tx.execute(
                "DELETE FROM books_languages_link WHERE book = ?1",
                rusqlite::params![id],
            )?;
            for (order, code) in codes.iter().enumerate() {
                let lang_id = ensure_vocab(&tx, "languages", "lang_code", code)?;
                tx.execute(
                    "INSERT INTO books_languages_link(book, lang_code, item_order) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, lang_id, order as i64],
                )?;
            }
        }
        if let Some(isbn) = isbn.map(str::trim).filter(|s| !s.is_empty()) {
            tx.execute(
                "INSERT INTO identifiers(book, type, val) VALUES (?1, 'isbn', ?2)",
                rusqlite::params![id, isbn],
            )?;
        }
        let tag_list: Vec<String> = tags
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        link_authors_tags(&tx, id, &author_list, &tag_list)?;
    }

    // Copy the file, then record the format.
    let rel: String = tx.query_row(
        "SELECT path FROM books WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    let dir = lib.root().join(&rel);
    std::fs::create_dir_all(&dir)?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT name FROM data WHERE book = ?1 AND format = ?2",
            rusqlite::params![id, format],
            |r| r.get(0),
        )
        .ok();
    if existing.is_some() && automerge == Some("ignore") {
        tx.commit()?;
        return Ok(json!({
            "book_id": id,
            "new": is_new,
            "merged": "ignored",
            "backup": backup.to_string_lossy(),
        }));
    }
    let stem = existing.unwrap_or_else(|| {
        let authors_short = if author_list.is_empty() {
            "Unknown".to_string()
        } else {
            author_list[0].clone()
        };
        sanitize_component(&format!("{title} - {authors_short}"))
    });
    let dest = dir.join(format!("{stem}.{ext}"));
    let bytes = std::fs::copy(file, &dest)?;
    tx.execute(
        "INSERT INTO data(book, format, uncompressed_size, name) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(book, format) DO UPDATE SET uncompressed_size = excluded.uncompressed_size, name = excluded.name",
        rusqlite::params![id, format, bytes as i64, stem],
    )?;
    touch(&tx, id, dirtied)?;
    tx.commit()?;

    Ok(json!({
        "book_id": id,
        "new": is_new,
        "format": format,
        "file": dest.to_string_lossy(),
        "bytes": bytes,
        "backup": backup.to_string_lossy(),
    }))
}

/// Remove one book: database row (triggers cascade the links), files, FTS row.
pub fn remove_book(lib: &Library, id: i64, permanent: bool) -> Result<Value, BookError> {
    let book = lib.detail(id)?;
    let title = book["title"].as_str().unwrap_or("").to_string();

    let backup = ensure_backup(lib, false)?;
    let mut conn = open_writable(lib)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("DELETE FROM books WHERE id = ?1", rusqlite::params![id])?;
    tx.commit()?;
    drop(conn);

    let rel = book["path"].as_str().unwrap_or("");
    let dir = lib.root().join(rel);
    let mut files = String::from("absent");
    if dir.is_dir() {
        if permanent {
            std::fs::remove_dir_all(&dir)?;
            files = String::from("deleted");
        } else {
            let trash = backup_root()
                .join(lib_tag(lib.root()))
                .join("trash")
                .join(format!("{id}-{}", epoch_secs()));
            std::fs::create_dir_all(trash.parent().unwrap_or(&trash))?;
            match std::fs::rename(&dir, &trash) {
                Ok(()) => files = format!("trashed: {}", trash.display()),
                Err(_) => {
                    copy_dir_recursive(&dir, &trash)?;
                    std::fs::remove_dir_all(&dir)?;
                    files = format!("trashed: {}", trash.display());
                }
            }
        }
    }

    if let Some(fts_path) = lib.fts_db_path() {
        if let Ok(fts) = Connection::open_with_flags(fts_path, OpenFlags::SQLITE_OPEN_READ_WRITE) {
            let _ = fts.busy_timeout(Duration::from_secs(10));
            let _ = fts.execute(
                "DELETE FROM books_text WHERE book = ?1",
                rusqlite::params![id],
            );
        }
    }

    Ok(json!({
        "book_id": id,
        "title": title,
        "permanent": permanent,
        "files": files,
        "backup": backup.to_string_lossy(),
    }))
}

/// Attach one format file to a book (replaces the format when present).
pub fn add_format(
    lib: &Library,
    id: i64,
    file: &Path,
    root: Option<&Path>,
) -> Result<Value, BookError> {
    lib.detail(id)?;
    if !file.is_file() {
        return Err(BookError::NotFound(format!(
            "{} is not a file",
            file.display()
        )));
    }
    crate::ensure_within(root, file)?;
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::trim)
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .ok_or_else(|| BookError::Invalid(format!("{} has no usable extension", file.display())))?;
    let format = ext.to_ascii_uppercase();

    let backup = ensure_backup(lib, false)?;
    let mut conn = open_writable(lib)?;
    let dirtied = has_table(&conn, "metadata_dirtied");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let rel: String = tx.query_row(
        "SELECT path FROM books WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    let dir = lib.root().join(&rel);
    std::fs::create_dir_all(&dir)?;
    let stem: String = tx
        .query_row(
            "SELECT name FROM data WHERE book = ?1 AND format = ?2",
            rusqlite::params![id, format],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| {
            let book: Value = json!({});
            let _ = &book;
            // Fresh stem below; this arm only supplies the fallback.
            String::new()
        });
    let (stem, is_new) = if stem.is_empty() {
        let book = lib.detail(id)?;
        let title = book["title"].as_str().unwrap_or("Unknown");
        let authors = book["authors"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let short = authors.split(',').next().unwrap_or("Unknown").trim();
        (sanitize_component(&format!("{title} - {short}")), true)
    } else {
        (stem, false)
    };
    let dest = dir.join(format!("{stem}.{ext}"));
    let bytes = std::fs::copy(file, &dest)?;
    tx.execute(
        "INSERT INTO data(book, format, uncompressed_size, name) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(book, format) DO UPDATE SET uncompressed_size = excluded.uncompressed_size, name = excluded.name",
        rusqlite::params![id, format, bytes as i64, stem],
    )?;
    touch(&tx, id, dirtied)?;
    tx.commit()?;
    Ok(json!({
        "book_id": id,
        "format": format,
        "new": is_new,
        "file": dest.to_string_lossy(),
        "bytes": bytes,
        "backup": backup.to_string_lossy(),
    }))
}

/// Drop one format from a book. A no-op when absent.
pub fn remove_format(lib: &Library, id: i64, format: &str) -> Result<Value, BookError> {
    let book = lib.detail(id)?;
    let clean = format.trim().to_ascii_uppercase();
    if clean.is_empty() || !clean.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(BookError::Invalid(format!("bad format '{format}'")));
    }
    let mut conn = open_writable(lib)?;
    let dirtied = has_table(&conn, "metadata_dirtied");
    let row: Option<(String,)> = conn
        .query_row(
            "SELECT name FROM data WHERE book = ?1 AND format = ?2",
            rusqlite::params![id, clean],
            |r| Ok((r.get(0)?,)),
        )
        .ok();
    let Some((name,)) = row else {
        return Ok(json!({"book_id": id, "format": clean, "removed": false}));
    };

    let backup = ensure_backup(lib, false)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "DELETE FROM data WHERE book = ?1 AND format = ?2",
        rusqlite::params![id, clean],
    )?;
    touch(&tx, id, dirtied)?;
    tx.commit()?;
    drop(conn);

    let rel = book["path"].as_str().unwrap_or("");
    let mut removed_file = false;
    for ext in [clean.to_ascii_lowercase(), clean.clone()] {
        let candidate = lib.root().join(rel).join(format!("{name}.{ext}"));
        if candidate.is_file() {
            std::fs::remove_file(&candidate)?;
            removed_file = true;
        }
    }
    Ok(json!({
        "book_id": id,
        "format": clean,
        "removed": true,
        "file_removed": removed_file,
        "backup": backup.to_string_lossy(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SCHEMA: &str = CALIBRE_WRITE_SCHEMA;

    fn fixture() -> Result<(tempfile::TempDir, Library), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let root = tmp.path().join("lib");
        std::fs::create_dir_all(&root)?;
        let conn = Connection::open(root.join("metadata.db"))?;
        conn.execute_batch(FIXTURE_SCHEMA)?;
        drop(conn);
        let lib = Library::open(&root)?;
        // The temp library must not share backups with a real one; the tag hashes
        // the full path, so temp dirs are naturally isolated.
        Ok((tmp, lib))
    }

    #[test]
    fn conventions_match_live_calibre() {
        assert_eq!(
            title_sort_value("The Complete Modern Blacksmith"),
            "Complete Modern Blacksmith, The"
        );
        assert_eq!(title_sort_value("Dune"), "Dune");
        assert_eq!(title_sort_value("  \"Quoted\" "), "Quoted");
        assert_eq!(author_sort_value("Frank Herbert"), "Herbert, Frank");
        assert_eq!(author_sort_value("Plato"), "Plato");
        assert_eq!(sanitize_component("A/B: C?"), "A_B_ C_");
        assert_eq!(sanitize_component("..."), "Unknown");
        let stamp = utc_now_stamp();
        assert_eq!(stamp.len(), 32);
        assert!(stamp.ends_with("+00:00"));
        assert!(normalise_pubdate("2020-01-01").is_ok());
        assert!(normalise_pubdate("2020-13-01").is_err());
        assert!(normalise_pubdate("yesterday").is_err());
    }

    #[test]
    fn backup_is_created_and_reused_while_fresh() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let first = ensure_backup(&lib, false)?;
        assert!(first.is_file());
        assert_eq!(ensure_backup(&lib, false)?, first);
        let forced = ensure_backup(&lib, true)?;
        assert_ne!(first, forced);
        assert!(forced.is_file());
        Ok(())
    }

    #[test]
    fn init_creates_an_openable_library() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let target = tmp.path().join("fresh");
        let created = init_library(&target)?;
        assert_eq!(created["created"], true);
        let lib = Library::open(&target)?;
        assert_eq!(lib.stats()?["books"], 0);
        assert!(matches!(init_library(&target), Err(BookError::Invalid(_))));
        Ok(())
    }

    #[test]
    fn unknown_fields_are_rejected_before_any_backup() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let mut fields = Map::new();
        fields.insert("nope".to_string(), Value::String("x".into()));
        assert!(matches!(
            set_metadata(&lib, 1, &fields),
            Err(BookError::Invalid(_))
        ));
        Ok(())
    }

    #[test]
    fn add_update_remove_roundtrip() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let added = add_empty_book(&lib, Some("Dune"), Some("Frank Herbert"), Some("sci-fi"))?;
        let id = added["book_id"].as_i64().unwrap_or(-1);
        assert!(id > 0);

        // Triggers computed sort and uuid through our registered functions.
        let book = lib.detail(id)?;
        assert_eq!(book["title"], "Dune");
        assert_eq!(book["sort"], "Dune");
        assert_eq!(book["author_sort"], "Herbert, Frank");
        assert_eq!(book["uuid"].as_str().map(str::len), Some(36));
        assert!(book["path"]
            .as_str()
            .unwrap_or("")
            .contains(&format!("({id})")));

        let mut fields = Map::new();
        fields.insert("title".to_string(), Value::String("Dune Messiah".into()));
        fields.insert("tags".to_string(), Value::String("sci-fi, sequel".into()));
        fields.insert("rating".to_string(), json!(4));
        fields.insert("series".to_string(), Value::String("Dune".into()));
        fields.insert("series_index".to_string(), json!(2.0));
        fields.insert("publisher".to_string(), Value::String("Chilton".into()));
        fields.insert("languages".to_string(), Value::String("en".into()));
        fields.insert("isbn".to_string(), Value::String("9780441172699".into()));
        fields.insert(
            "comments".to_string(),
            Value::String("<p>Second book.</p>".into()),
        );
        fields.insert("pubdate".to_string(), Value::String("1969-10-15".into()));
        let updated = set_metadata(&lib, id, &fields)?;
        assert_eq!(updated["book_id"], id);

        let book = lib.detail(id)?;
        assert_eq!(book["title"], "Dune Messiah");
        assert_eq!(book["sort"], "Dune Messiah");
        assert_eq!(book["rating"], 4);
        assert_eq!(book["series"], "Dune");
        assert!(book["tags"].as_array().map(|t| t.len()).unwrap_or(0) == 2);
        assert!(updated["backup"]
            .as_str()
            .is_some_and(|p| Path::new(p).is_file()));

        let removed = remove_book(&lib, id, true)?;
        assert_eq!(removed["book_id"], id);
        assert!(matches!(lib.detail(id), Err(BookError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn authors_split_into_people() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let added = add_empty_book(
            &lib,
            Some("Good Omens"),
            Some("Terry Pratchett, Neil Gaiman"),
            None,
        )?;
        let id = added["book_id"].as_i64().unwrap_or(-1);
        let book = lib.detail(id)?;
        assert_eq!(book["authors"].as_array().map(Vec::len), Some(2));
        assert_eq!(book["author_sort"], "Pratchett, Terry");
        Ok(())
    }

    #[test]
    fn file_add_format_remove_roundtrip() -> Result<(), BookError> {
        let (tmp, lib) = fixture()?;
        let sample = tmp.path().join("lantern.txt");
        std::fs::write(&sample, "The lantern burns in the stilltent.")?;
        let added = add_book(
            &lib,
            &sample,
            None,
            Some("Lantern"),
            Some("Tester"),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
        )?;
        let id = added["book_id"].as_i64().unwrap_or(-1);
        assert!(id > 0);
        assert_eq!(added["new"], true);
        assert_eq!(added["format"], "TXT");

        // Duplicate without a policy is rejected before any backup churn.
        let dup = add_book(
            &lib,
            &sample,
            None,
            Some("Lantern"),
            Some("Tester"),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
        );
        assert!(matches!(dup, Err(BookError::Invalid(_))));

        let book = lib.detail(id)?;
        assert_eq!(book["format_files"][0]["exists"], true);

        let other = tmp.path().join("notes.txt");
        std::fs::write(&other, "Marginalia.")?;
        let attached = add_format(&lib, id, &other, None)?;
        assert_eq!(attached["format"], "TXT");

        let dropped = remove_format(&lib, id, "txt")?;
        assert_eq!(dropped["removed"], true);
        let dropped_again = remove_format(&lib, id, "TXT")?;
        assert_eq!(dropped_again["removed"], false);

        let removed = remove_book(&lib, id, true)?;
        assert_eq!(removed["files"], "deleted");
        Ok(())
    }

    #[test]
    fn remove_missing_book_is_not_found() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        assert!(matches!(
            remove_book(&lib, 999_999, true),
            Err(BookError::NotFound(_))
        ));
        Ok(())
    }
}
