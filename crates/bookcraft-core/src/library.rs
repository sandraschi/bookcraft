//! Read-only access to a Calibre library (`metadata.db` + optional `full-text-search.db`).
//!
//! The database is opened with `SQLITE_OPEN_READ_ONLY`, so no code path in this module can
//! mutate a library. Book files are resolved and stat-ed, never written.

use crate::{strip_html_tags, BookError};
use rusqlite::{params, Connection, OpenFlags, Row};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Decode a cover file to unpremultiplied RGBA8, thumbnail-bounded to `max_px`.
/// Used by the L3 shelf; read-only like everything else here.
pub fn cover_rgba(path: &Path, max_px: u32) -> Result<(Vec<u8>, u32, u32), BookError> {
    let bound = max_px.clamp(1, 1024);
    let img = image::open(path)
        .map_err(|e| BookError::Invalid(format!("unreadable image {}: {e}", path.display())))?;
    let thumb = img.thumbnail(bound, bound);
    let rgba = thumb.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok((rgba.into_raw(), width, height))
}

/// A discovered Calibre library. Everything is resolved eagerly so later calls cannot
/// accidentally re-resolve to a different directory.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
    metadata_db: PathBuf,
    fts_db: Option<PathBuf>,
}

const SUMMARY_COLUMNS: &str = r#"
    b.id,
    b.title,
    b.path,
    b.series_index,
    b.has_cover,
    b.author_sort,
    CAST(b.pubdate AS TEXT) AS pubdate_str,
    CAST(b.timestamp AS TEXT) AS timestamp_str,
    (SELECT group_concat(a.name, '|')
       FROM books_authors_link bal
       JOIN authors a ON a.id = bal.author
      WHERE bal.book = b.id) AS authors_pipe,
    (SELECT s.name
       FROM books_series_link bsl
       JOIN series s ON s.id = bsl.series
      WHERE bsl.book = b.id LIMIT 1) AS series_name,
    (SELECT group_concat(t.name, '|')
       FROM books_tags_link btl
       JOIN tags t ON t.id = btl.tag
      WHERE btl.book = b.id) AS tags_pipe,
    (SELECT group_concat(upper(d.format), '|')
       FROM data d
      WHERE d.book = b.id) AS formats_pipe,
    (SELECT r.rating
       FROM books_ratings_link brl
       JOIN ratings r ON r.id = brl.rating
      WHERE brl.book = b.id LIMIT 1) AS rating_val,
    (SELECT substr(c.text, 1, 400)
       FROM comments c
      WHERE c.book = b.id LIMIT 1) AS comment_raw
"#;

fn open_readonly(db: &Path) -> Result<Connection, BookError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db, flags)
        .map_err(|e| BookError::Invalid(format!("cannot open {} read-only: {e}", db.display())))?;
    // A busy library (Calibre open in another process) must not fail instantly.
    let _ = conn.busy_timeout(Duration::from_millis(5000));
    Ok(conn)
}

/// Calibre's `data.name` normally omits the extension; the file on disk carries it.
fn data_path(book_dir: &Path, name: &str, format: &str) -> PathBuf {
    let direct = book_dir.join(name);
    if direct.is_file() {
        return direct;
    }
    let ext = format.to_ascii_lowercase();
    let with_ext = book_dir.join(format!("{name}.{ext}"));
    if with_ext.is_file() {
        return with_ext;
    }
    // Neither exists: return the extension-completed guess so `exists` reports false.
    with_ext
}

fn split_pipe(value: Option<String>) -> Vec<String> {
    value
        .map(|s| {
            s.split('|')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn first_10(value: Option<String>) -> Option<String> {
    value.map(|s| s.chars().take(10).collect())
}

/// Escape `%`, `_` and `\` so a caller's text is a literal LIKE pattern.
fn like_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            other => out.push(other),
        }
    }
    out
}

impl Library {
    /// Open a library directory, or a `metadata.db` path directly.
    pub fn open(path: &Path) -> Result<Self, BookError> {
        let (root, metadata_db) = if path
            .file_name()
            .map(|n| n.eq_ignore_ascii_case("metadata.db"))
            .unwrap_or(false)
        {
            let parent = path.parent().ok_or_else(|| {
                BookError::Invalid(format!("{} has no parent directory", path.display()))
            })?;
            (parent.to_path_buf(), path.to_path_buf())
        } else {
            (path.to_path_buf(), path.join("metadata.db"))
        };

        if !metadata_db.is_file() {
            return Err(BookError::NotFound(format!(
                "no metadata.db in {}",
                root.display()
            )));
        }

        let fts = root.join("full-text-search.db");
        let fts_db = if fts.is_file() { Some(fts) } else { None };

        // Fail here, not on the first query, if the file is not a readable SQLite DB.
        let probe = open_readonly(&metadata_db)?;
        probe
            .query_row("SELECT COUNT(*) FROM books", [], |r| r.get::<_, i64>(0))
            .map_err(|e| {
                BookError::Invalid(format!(
                    "{} is not a Calibre metadata.db: {e}",
                    metadata_db.display()
                ))
            })?;

        Ok(Self {
            root,
            metadata_db,
            fts_db,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn metadata_db_path(&self) -> &Path {
        &self.metadata_db
    }

    pub fn fts_db_path(&self) -> Option<&PathBuf> {
        self.fts_db.as_ref()
    }

    /// Raw `sqlite_master` rows: the ground truth for any write path.
    pub fn schema(&self) -> Result<Vec<Value>, BookError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT type, name, sql FROM sqlite_master
             WHERE sql IS NOT NULL ORDER BY type, name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "kind": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "sql": r.get::<_, String>(2)?,
            }))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn has_fts(&self) -> bool {
        self.fts_db.is_some()
    }

    fn conn(&self) -> Result<Connection, BookError> {
        open_readonly(&self.metadata_db)
    }

    /// A read-only connection for sibling modules (analytics, collections).
    pub(crate) fn read_conn(&self) -> Result<Connection, BookError> {
        self.conn()
    }

    fn summary(&self, row: &Row<'_>) -> rusqlite::Result<Value> {
        let id: i64 = row.get(0)?;
        let title: String = row.get(1)?;
        let rel_path: String = row.get(2)?;
        let series_index: f64 = row.get::<_, Option<f64>>(3)?.unwrap_or(1.0);
        let has_cover = row.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0;
        let author_sort: Option<String> = row.get(5)?;
        let pubdate: Option<String> = row.get(6)?;
        let timestamp: Option<String> = row.get(7)?;
        let authors = split_pipe(row.get(8)?);
        let series: Option<String> = row.get(9)?;
        let tags = split_pipe(row.get(10)?);
        let formats = split_pipe(row.get(11)?);
        let rating: Option<i64> = row.get(12)?;
        let comment_raw: Option<String> = row.get(13)?;

        let book_dir = self.root.join(&rel_path);
        let cover = book_dir.join("cover.jpg");
        let cover_path = if has_cover && cover.is_file() {
            Some(cover.to_string_lossy().into_owned())
        } else {
            None
        };
        let summary = comment_raw
            .as_deref()
            .map(strip_html_tags)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
            .map(|s| truncate_chars(&s, 200));

        Ok(json!({
            "id": id,
            "title": title,
            "authors": authors,
            "author_sort": author_sort,
            "series": series,
            "series_index": series_index,
            "tags": tags,
            "formats": formats,
            "rating": rating,
            "pubdate": first_10(pubdate),
            "added": first_10(timestamp),
            "has_cover": has_cover,
            "cover_path": cover_path,
            "path": rel_path,
            "summary": summary,
        }))
    }

    fn collect(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>, BookError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(args, |row| self.summary(row))?;
        let mut books = Vec::new();
        for row in rows {
            books.push(row?);
        }
        Ok(books)
    }

    /// Newest additions first.
    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<Value>, BookError> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM books b \
             ORDER BY b.timestamp DESC, b.id DESC LIMIT ?1 OFFSET ?2"
        );
        self.collect(&sql, &[&(limit as i64), &(offset as i64)])
    }

    /// Substring match on title, author sort, author names and comments.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Value>, BookError> {
        let needle = format!("%{}%", like_escape(query.trim()));
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM books b \
             WHERE b.title LIKE ?1 ESCAPE '\\' \
                OR b.author_sort LIKE ?1 ESCAPE '\\' \
                OR EXISTS (SELECT 1 FROM comments c \
                            WHERE c.book = b.id AND c.text LIKE ?1 ESCAPE '\\') \
                OR EXISTS (SELECT 1 FROM books_authors_link bal \
                             JOIN authors a ON a.id = bal.author \
                            WHERE bal.book = b.id AND a.name LIKE ?1 ESCAPE '\\') \
                OR EXISTS (SELECT 1 FROM books_tags_link btl \
                             JOIN tags t ON t.id = btl.tag \
                            WHERE btl.book = b.id AND t.name LIKE ?1 ESCAPE '\\') \
                OR EXISTS (SELECT 1 FROM books_series_link bsl \
                             JOIN series s ON s.id = bsl.series \
                            WHERE bsl.book = b.id AND s.name LIKE ?1 ESCAPE '\\') \
             ORDER BY b.timestamp DESC, b.id DESC LIMIT ?2"
        );
        self.collect(&sql, &[&needle, &(limit as i64)])
    }

    /// One book with its full description, format files and cover.
    pub fn detail(&self, id: i64) -> Result<Value, BookError> {
        let sql = format!("SELECT {SUMMARY_COLUMNS} FROM books b WHERE b.id = ?1");
        let mut book = self
            .collect(&sql, &[&id])?
            .into_iter()
            .next()
            .ok_or_else(|| BookError::NotFound(format!("book id {id}")))?;

        let conn = self.conn()?;
        let comment: Option<String> = conn
            .query_row(
                "SELECT text FROM comments WHERE book = ?1 LIMIT 1",
                params![id],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap_or(None);

        let rel_path: String =
            conn.query_row("SELECT path FROM books WHERE id = ?1", params![id], |r| {
                r.get(0)
            })?;
        let book_dir = self.root.join(&rel_path);

        let mut stmt =
            conn.prepare("SELECT upper(format), uncompressed_size, name FROM data WHERE book = ?1 ORDER BY format")?;
        let files = stmt.query_map(params![id], |r| {
            let format: String = r.get(0)?;
            let size: Option<i64> = r.get(1)?;
            let name: String = r.get(2)?;
            Ok((format, size, name))
        })?;
        let mut formats = Vec::new();
        for entry in files {
            let (format, size, name) = entry?;
            let path = data_path(&book_dir, &name, &format);
            formats.push(json!({
                "format": format,
                "size_bytes": size,
                "file": name,
                "path": path.to_string_lossy(),
                "exists": path.is_file(),
            }));
        }

        if let Some(obj) = book.as_object_mut() {
            obj.insert(
                "description".into(),
                comment
                    .as_deref()
                    .map(strip_html_tags)
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            obj.insert("format_files".into(), Value::Array(formats));
        }
        let extra: (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT uuid, sort, CAST(last_modified AS TEXT) FROM books WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((None, None, None));
        if let Some(obj) = book.as_object_mut() {
            obj.insert(
                "uuid".into(),
                extra.0.map(Value::String).unwrap_or(Value::Null),
            );
            obj.insert(
                "sort".into(),
                extra.1.map(Value::String).unwrap_or(Value::Null),
            );
            obj.insert(
                "last_modified".into(),
                extra.2.map(Value::String).unwrap_or(Value::Null),
            );
        }
        Ok(book)
    }

    /// All format files that exist on disk for a book, best format first.
    pub fn format_files(&self, id: i64) -> Result<Vec<(String, PathBuf)>, BookError> {
        let conn = self.conn()?;
        let rel_path: String = conn
            .query_row("SELECT path FROM books WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .map_err(|_| BookError::NotFound(format!("book id {id}")))?;
        let book_dir = self.root.join(&rel_path);

        let mut stmt = conn.prepare("SELECT upper(format), name FROM data WHERE book = ?1")?;
        let rows = stmt.query_map(params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        let mut out = Vec::new();
        for entry in rows {
            let (format, name) = entry?;
            let path = data_path(&book_dir, &name, &format);
            if path.is_file() {
                out.push((format, path));
            }
        }
        // Deterministic preference: EPUB, PDF, AZW3, MOBI, then alphabetical.
        let rank = |f: &str| match f {
            "EPUB" => 0,
            "PDF" => 1,
            "AZW3" => 2,
            "MOBI" => 3,
            _ => 4,
        };
        out.sort_by(|a, b| {
            rank(&a.0)
                .cmp(&rank(&b.0))
                .then_with(|| a.0.cmp(&b.0))
                .then_with(|| a.1.cmp(&b.1))
        });
        Ok(out)
    }

    /// Resolve one readable source file, optionally pinned to a format.
    pub fn file_for(
        &self,
        id: i64,
        preferred: Option<&str>,
    ) -> Result<(String, PathBuf), BookError> {
        let files = self.format_files(id)?;
        if files.is_empty() {
            return Err(BookError::NotFound(format!(
                "book id {id} has no format file on disk"
            )));
        }
        match preferred {
            Some(want) => {
                let want = want.trim().to_ascii_uppercase();
                files
                    .into_iter()
                    .find(|(f, _)| *f == want)
                    .ok_or_else(|| BookError::NotFound(format!("book id {id} has no {want} file")))
            }
            None => files
                .into_iter()
                .next()
                .ok_or_else(|| BookError::NotFound(format!("book id {id} has no format file"))),
        }
    }

    /// Full-text search when Calibre built `full-text-search.db`, else metadata search.
    pub fn grep(&self, pattern: &str, limit: usize) -> Result<Value, BookError> {
        let needle = pattern.trim();
        if needle.is_empty() {
            return Err(BookError::Invalid("empty grep pattern".into()));
        }

        let mut hits = Vec::new();
        if let Some(fts) = &self.fts_db {
            let conn = open_readonly(fts)?;
            let like = format!("%{}%", like_escape(needle));
            let sql = "SELECT book, format, \
                       substr(searchable_text, \
                              max(1, instr(lower(searchable_text), lower(?1)) - 90), 240) \
                       FROM books_text \
                       WHERE searchable_text LIKE ?2 ESCAPE '\\' LIMIT ?3";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![needle, like, limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                ))
            })?;
            for row in rows {
                let (book_id, format, snippet) = row?;
                hits.push(json!({
                    "book_id": book_id,
                    "title": String::new(),
                    "authors": Vec::<String>::new(),
                    "format": format,
                    "snippet": snippet.split_whitespace().collect::<Vec<_>>().join(" "),
                    "source": "full-text-search.db:books_text",
                }));
            }
        }

        let mut engine = if self.fts_db.is_some() {
            "full_text_search_db"
        } else {
            "metadata_db"
        };
        if hits.is_empty() {
            engine = "metadata_db";
            let like = format!("%{}%", like_escape(needle));
            let sql = format!(
                "SELECT {SUMMARY_COLUMNS} FROM books b \
                 WHERE b.title LIKE ?1 ESCAPE '\\' \
                    OR EXISTS (SELECT 1 FROM comments c \
                                WHERE c.book = b.id AND c.text LIKE ?1 ESCAPE '\\') \
                 ORDER BY b.timestamp DESC LIMIT ?2"
            );
            for book in self.collect(&sql, &[&like, &(limit as i64)])? {
                hits.push(json!({
                    "book_id": book["id"],
                    "title": book["title"],
                    "authors": book["authors"],
                    "format": null,
                    "snippet": book["summary"],
                    "source": "metadata.db:titles+comments",
                }));
            }
        }

        // Fill in title/authors for full-text hits so the caller does not need a second call.
        if engine == "full_text_search_db" {
            let conn = self.conn()?;
            let mut stmt = conn.prepare(
                "SELECT b.title, \
                        (SELECT group_concat(a.name, '|') \
                           FROM books_authors_link bal \
                           JOIN authors a ON a.id = bal.author \
                          WHERE bal.book = b.id) \
                   FROM books b WHERE b.id = ?1",
            )?;
            for hit in &mut hits {
                let id = hit["book_id"].as_i64().unwrap_or_default();
                let resolved = stmt.query_row(params![id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                });
                if let Ok((title, authors)) = resolved {
                    hit["title"] = Value::String(title);
                    hit["authors"] = json!(split_pipe(authors));
                }
            }
        }

        Ok(json!({
            "engine": engine,
            "pattern": needle,
            "count": hits.len(),
            "hits": hits,
        }))
    }

    /// Library-wide counts and format distribution.
    pub fn stats(&self) -> Result<Value, BookError> {
        let conn = self.conn()?;
        let count =
            |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0) };

        let mut formats = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT upper(format) AS fmt, COUNT(*) AS cnt FROM data \
             GROUP BY upper(format) ORDER BY cnt DESC, fmt ASC LIMIT 20",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (format, count) = row?;
            formats.push(json!({"format": format, "count": count}));
        }

        Ok(json!({
            "library": self.root.to_string_lossy(),
            "metadata_db": self.metadata_db.to_string_lossy(),
            "has_fts_db": self.fts_db.is_some(),
            "books": count("SELECT COUNT(*) FROM books"),
            "authors": count("SELECT COUNT(*) FROM authors"),
            "tags": count("SELECT COUNT(*) FROM tags"),
            "series": count("SELECT COUNT(*) FROM series"),
            "publishers": count("SELECT COUNT(*) FROM publishers"),
            "books_with_cover": count("SELECT COUNT(*) FROM books WHERE has_cover = 1"),
            "books_with_description": count(
                "SELECT COUNT(*) FROM comments WHERE text IS NOT NULL AND trim(text) != ''"
            ),
            "formats": formats,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &Path) -> Result<Library, BookError> {
        let db_path = dir.join("metadata.db");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT, author_sort TEXT,
                path TEXT, series_index REAL DEFAULT 1.0, has_cover INTEGER DEFAULT 0,
                pubdate TIMESTAMP, timestamp TIMESTAMP);
            CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT);
            CREATE TABLE books_authors_link (book INTEGER, author INTEGER);
            CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT);
            CREATE TABLE books_tags_link (book INTEGER, tag INTEGER);
            CREATE TABLE series (id INTEGER PRIMARY KEY, name TEXT);
            CREATE TABLE books_series_link (book INTEGER, series INTEGER);
            CREATE TABLE publishers (id INTEGER PRIMARY KEY, name TEXT);
            CREATE TABLE books_publishers_link (book INTEGER, publisher INTEGER);
            CREATE TABLE ratings (id INTEGER PRIMARY KEY, rating INTEGER);
            CREATE TABLE books_ratings_link (book INTEGER, rating INTEGER);
            CREATE TABLE comments (book INTEGER, text TEXT);
            CREATE TABLE data (book INTEGER, format TEXT, uncompressed_size INTEGER, name TEXT);

            INSERT INTO books VALUES (1, 'Dune', 'Herbert, Frank', 'Herbert, Frank/Dune (1)',
                1.0, 1, '1965-08-01', '2020-01-01');
            INSERT INTO books VALUES (2, 'Emma', 'Austen, Jane', 'Austen, Jane/Emma (7)',
                1.0, 0, '1815-12-23', '2021-02-02');
            INSERT INTO authors VALUES (1, 'Frank Herbert');
            INSERT INTO authors VALUES (2, 'Jane Austen');
            INSERT INTO books_authors_link VALUES (1, 1);
            INSERT INTO books_authors_link VALUES (2, 2);
            INSERT INTO tags VALUES (1, 'Science Fiction');
            INSERT INTO books_tags_link VALUES (1, 1);
            INSERT INTO series VALUES (1, 'Dune');
            INSERT INTO books_series_link VALUES (1, 1);
            INSERT INTO comments VALUES (1, '<p>The desert planet <b>Arrakis</b>.</p>');
            INSERT INTO comments VALUES (2, '<p>A matchmaking comedy.</p>');
            INSERT INTO data VALUES (1, 'EPUB', 1000, 'Dune');
            INSERT INTO data VALUES (2, 'EPUB', 900, 'Emma');
            "#,
        )?;
        drop(conn);

        let dune_dir = dir.join("Herbert, Frank/Dune (1)");
        std::fs::create_dir_all(&dune_dir)?;
        std::fs::write(dune_dir.join("Dune.epub"), b"not really an epub")?;
        std::fs::write(dune_dir.join("cover.jpg"), b"not really a jpeg")?;

        Library::open(dir)
    }

    #[test]
    fn opens_and_reports_stats() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let lib = fixture(tmp.path())?;
        let stats = lib.stats()?;
        assert_eq!(stats["books"], 2);
        assert_eq!(stats["authors"], 2);
        assert_eq!(stats["books_with_description"], 2);
        assert_eq!(stats["formats"][0]["format"], "EPUB");
        assert!(!lib.has_fts());
        Ok(())
    }

    #[test]
    fn search_matches_title_author_and_comment() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let lib = fixture(tmp.path())?;

        assert_eq!(lib.search("dune", 10)?.len(), 1);
        assert_eq!(lib.search("austen", 10)?.len(), 1);
        assert_eq!(lib.search("arrakis", 10)?.len(), 1);
        assert_eq!(lib.search("no-such-book", 10)?.len(), 0);
        Ok(())
    }

    #[test]
    fn search_escapes_like_wildcards() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let lib = fixture(tmp.path())?;
        // '%' must be literal, not "match everything".
        assert_eq!(lib.search("%", 10)?.len(), 0);
        Ok(())
    }

    #[test]
    fn detail_includes_formats_and_cover() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let lib = fixture(tmp.path())?;
        let book = lib.detail(1)?;
        assert_eq!(book["title"], "Dune");
        assert_eq!(book["description"], "The desert planet Arrakis.");
        assert!(book["cover_path"].is_string());
        assert_eq!(book["format_files"][0]["exists"], true);
        assert!(matches!(lib.detail(999), Err(BookError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn file_for_honours_format_and_missing_files() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let lib = fixture(tmp.path())?;
        let (format, path) = lib.file_for(1, Some("epub"))?;
        assert_eq!(format, "EPUB");
        assert!(path.ends_with("Dune.epub"));
        assert!(matches!(
            lib.file_for(1, Some("PDF")),
            Err(BookError::NotFound(_))
        ));
        // Book 2 has a metadata row but no file on disk.
        assert!(matches!(lib.file_for(2, None), Err(BookError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn grep_falls_back_to_metadata_without_fts() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let lib = fixture(tmp.path())?;
        let hit = lib.grep("matchmaking", 5)?;
        assert_eq!(hit["engine"], "metadata_db");
        assert_eq!(hit["count"], 1);
        assert_eq!(hit["hits"][0]["title"], "Emma");
        Ok(())
    }

    #[test]
    fn open_rejects_a_directory_without_metadata_db() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        assert!(matches!(
            Library::open(tmp.path()),
            Err(BookError::NotFound(_))
        ));
        Ok(())
    }

    #[test]
    fn cover_rgba_decodes_and_thumbnails() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let path = tmp.path().join("cover.png");
        let img = image::RgbImage::from_pixel(8, 4, image::Rgb([200, 30, 30]));
        img.save(&path)
            .map_err(|e| BookError::Invalid(format!("cannot write fixture: {e}")))?;
        let (pixels, width, height) = cover_rgba(&path, 4)?;
        assert!(width <= 4 && height <= 4);
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        // Thumbnail of an 8x4 image bounded to 4 keeps the 2:1 aspect.
        assert_eq!((width, height), (4, 2));
        assert!(matches!(
            cover_rgba(&tmp.path().join("absent.png"), 64),
            Err(BookError::Invalid(_))
        ));
        Ok(())
    }
}
