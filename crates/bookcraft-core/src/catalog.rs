//! Catalog management and library analytics (Phase B).
//!
//! Renames and merges for the four single-name vocabularies (tags, authors, series,
//! publishers), duplicate detection, series-gap analysis, the unread queue, reading
//! stats, and named saved searches (smart collections). Mutations run through the
//! same IMMEDIATE-transaction + backup discipline as `write`; Calibre's triggers
//! (foreign-key guards, series sort) fire on our connection too.

use crate::{library::Library, write, BookError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Map, Value};

/// (vocabulary table, link table, link column) per kind.
fn vocab_kind(kind: &str) -> Result<(&str, &str, &str), BookError> {
    match kind {
        "tag" => Ok(("tags", "books_tags_link", "tag")),
        "author" => Ok(("authors", "books_authors_link", "author")),
        "series" => Ok(("series", "books_series_link", "series")),
        "publisher" => Ok(("publishers", "books_publishers_link", "publisher")),
        _ => Err(BookError::Invalid(
            "vocabulary must be tag, author, series or publisher".into(),
        )),
    }
}

fn vocab_id(
    conn: &rusqlite::Connection,
    table: &str,
    name: &str,
) -> Result<Option<i64>, BookError> {
    let sql = format!("SELECT id FROM {table} WHERE name = ?1");
    conn.query_row(&sql, params![name], |r| r.get(0))
        .optional()
        .map_err(BookError::from)
}

fn sort_for(kind: &str, name: &str) -> Option<String> {
    match kind {
        "author" => Some(write::author_sort_value(name)),
        "series" | "publisher" => Some(write::title_sort_value(name)),
        _ => None,
    }
}

/// Recompute `books.author_sort` from the book's authors in link order.
fn refresh_author_sort(tx: &Transaction<'_>, book_id: i64) -> Result<(), BookError> {
    let mut stmt = tx.prepare(
        "SELECT a.name FROM books_authors_link bal
         JOIN authors a ON a.id = bal.author
         WHERE bal.book = ?1 ORDER BY bal.id",
    )?;
    let names: Vec<String> = stmt
        .query_map(params![book_id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let first = names.first().map(String::as_str).unwrap_or("Unknown");
    tx.execute(
        "UPDATE books SET author_sort = ?1 WHERE id = ?2",
        params![write::author_sort_value(first), book_id],
    )?;
    Ok(())
}

/// Rename a vocabulary entry. When `new` already exists the two are merged:
/// every book is re-linked (duplicates collapse via INSERT OR IGNORE) and the
/// old row is deleted. Returns the affected book count.
pub fn rename_vocab(lib: &Library, kind: &str, old: &str, new: &str) -> Result<Value, BookError> {
    let (table, link_table, link_col) = vocab_kind(kind)?;
    let old = old.trim();
    let new = new.trim();
    if old.is_empty() || new.is_empty() {
        return Err(BookError::Invalid("names must not be empty".into()));
    }
    if old.eq_ignore_ascii_case(new) {
        return Err(BookError::Invalid("old and new names are identical".into()));
    }

    let mut conn = write::open_writable(lib)?;
    if !write::has_table(&conn, table) {
        return Err(BookError::Invalid(format!(
            "no {table} table in this library"
        )));
    }
    let old_id = vocab_id(&conn, table, old)?
        .ok_or_else(|| BookError::NotFound(format!("{kind} '{old}'")))?;
    let new_id = vocab_id(&conn, table, new)?;

    let backup = write::ensure_backup(lib, false)?;
    let dirtied = write::has_table(&conn, "metadata_dirtied");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let mut stmt = tx.prepare(&format!(
        "SELECT DISTINCT book FROM {link_table} WHERE {link_col} = ?1 ORDER BY book"
    ))?;
    let affected: Vec<i64> = stmt
        .query_map(params![old_id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let merged = new_id.is_some();
    if let Some(target) = new_id {
        for book in &affected {
            tx.execute(
                &format!("INSERT OR IGNORE INTO {link_table}(book, {link_col}) VALUES (?1, ?2)"),
                params![book, target],
            )?;
            tx.execute(
                &format!("DELETE FROM {link_table} WHERE book = ?1 AND {link_col} = ?2"),
                params![book, old_id],
            )?;
        }
        tx.execute(
            &format!("DELETE FROM {table} WHERE id = ?1"),
            params![old_id],
        )?;
    } else {
        tx.execute(
            &format!("UPDATE {table} SET name = ?1 WHERE id = ?2"),
            params![new, old_id],
        )?;
        if let Some(sort) = sort_for(kind, new) {
            tx.execute(
                &format!("UPDATE {table} SET sort = ?1 WHERE id = ?2"),
                params![sort, old_id],
            )?;
        }
    }
    if kind == "author" {
        for book in &affected {
            refresh_author_sort(&tx, *book)?;
        }
    }
    for book in &affected {
        write::touch(&tx, *book, dirtied)?;
    }
    tx.commit()?;

    Ok(json!({
        "kind": kind,
        "from": old,
        "to": new,
        "merged": merged,
        "books": affected.len(),
        "backup": backup.to_string_lossy(),
    }))
}

/// Books sharing a normalised title: the duplicate candidates.
pub fn duplicates(lib: &Library, limit: usize) -> Result<Value, BookError> {
    let conn = lib.read_conn()?;
    let mut stmt = conn.prepare(
        "SELECT lower(trim(title)) AS t, COUNT(*) AS c
         FROM books GROUP BY t HAVING c > 1 ORDER BY c DESC, t LIMIT ?1",
    )?;
    let titles: Vec<String> = stmt
        .query_map(params![limit as i64], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut groups = Vec::with_capacity(titles.len());
    for title in titles {
        let mut stmt = conn
            .prepare("SELECT id FROM books WHERE lower(trim(title)) = ?1 ORDER BY id LIMIT 25")?;
        let ids: Vec<i64> = stmt
            .query_map(params![title], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut books = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Ok(detail) = lib.detail(*id) {
                books.push(detail);
            }
        }
        let display = books
            .first()
            .and_then(|b| b["title"].as_str())
            .unwrap_or(&title)
            .to_string();
        groups.push(json!({"title": display, "count": books.len(), "books": books}));
    }
    Ok(json!({"groups": groups, "count": groups.len()}))
}

/// Per-series volume lists with the missing integer indices.
pub fn series_gaps(lib: &Library) -> Result<Value, BookError> {
    let conn = lib.read_conn()?;
    let mut stmt = conn.prepare(
        "SELECT s.name, b.id, b.title, b.series_index
         FROM books_series_link l
         JOIN series s ON s.id = l.series
         JOIN books b ON b.id = l.book
         ORDER BY s.name, b.series_index LIMIT 2000",
    )?;
    let rows: Vec<(String, i64, String, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut series: Vec<Value> = Vec::new();
    let mut current = String::new();
    let mut have: Vec<f64> = Vec::new();
    let mut volumes: Vec<Value> = Vec::new();
    let flush = |name: &str, have: &[f64], volumes: &[Value]| -> Value {
        let max = have.iter().fold(0.0_f64, |a, b| a.max(*b)).ceil() as i64;
        let mut missing = Vec::new();
        for n in 1..=max.max(0) {
            if !have.iter().any(|h| (*h - n as f64).abs() < 1e-9) {
                missing.push(n);
            }
        }
        json!({"series": name, "books": volumes.len(), "have": have, "missing": missing, "volumes": volumes})
    };
    for (name, id, title, index) in rows {
        if name != current && !current.is_empty() {
            series.push(flush(&current, &have, &volumes));
            have.clear();
            volumes.clear();
        }
        current = name;
        have.push(index);
        volumes.push(json!({"id": id, "title": title, "series_index": index}));
    }
    if !current.is_empty() {
        series.push(flush(&current, &have, &volumes));
    }
    Ok(json!({"series": series, "count": series.len()}))
}

fn has_tracking_table(conn: &rusqlite::Connection) -> bool {
    write::has_table(conn, "last_read_positions")
}

/// Books never opened according to Calibre's read tracking (or everything when
/// the library has no tracking table yet). Newest first.
pub fn unread(lib: &Library, limit: usize) -> Result<Value, BookError> {
    let conn = lib.read_conn()?;
    let tracked = has_tracking_table(&conn);
    let sql = if tracked {
        "SELECT b.id FROM books b
         WHERE NOT EXISTS (SELECT 1 FROM last_read_positions l WHERE l.book = b.id)
         ORDER BY b.timestamp DESC, b.id DESC LIMIT ?1"
    } else {
        "SELECT b.id FROM books b ORDER BY b.timestamp DESC, b.id DESC LIMIT ?1"
    };
    let mut stmt = conn.prepare(sql)?;
    let ids: Vec<i64> = stmt
        .query_map(params![limit.clamp(1, 500) as i64], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let mut books = Vec::with_capacity(ids.len());
    for id in ids {
        if let Ok(book) = lib.detail(id) {
            books.push(book);
        }
    }
    Ok(json!({
        "count": books.len(),
        "books": books,
        "heuristic": if tracked {
            "not present in last_read_positions"
        } else {
            "no last_read_positions table: everything counts as unread"
        },
    }))
}

/// Totals, read/unread split, rating histogram, additions by year, top tags/authors.
pub fn reading_stats(lib: &Library) -> Result<Value, BookError> {
    let conn = lib.read_conn()?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM books", [], |r| r.get(0))?;
    let read: i64 = if has_tracking_table(&conn) {
        conn.query_row(
            "SELECT COUNT(DISTINCT book) FROM last_read_positions",
            [],
            |r| r.get(0),
        )?
    } else {
        0
    };

    let mut hist = conn.prepare(
        "SELECT r.rating, COUNT(*) FROM books_ratings_link l
         JOIN ratings r ON r.id = l.rating GROUP BY r.rating ORDER BY r.rating",
    )?;
    let ratings: Vec<Value> = hist
        .query_map([], |r| {
            let rating: i64 = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok(json!({"rating": rating, "count": count}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(hist);

    let mut years = conn.prepare(
        "SELECT substr(timestamp, 1, 4) AS y, COUNT(*) FROM books
         GROUP BY y ORDER BY y DESC LIMIT 15",
    )?;
    let by_year: Vec<Value> = years
        .query_map([], |r| {
            let year: String = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok(json!({"year": year, "count": count}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(years);

    let mut tags = conn.prepare(
        "SELECT t.name, COUNT(*) AS c FROM books_tags_link l
         JOIN tags t ON t.id = l.tag GROUP BY t.name ORDER BY c DESC, t.name LIMIT 10",
    )?;
    let top_tags: Vec<Value> = tags
        .query_map([], |r| {
            let tag: String = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok(json!({"tag": tag, "count": count}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(tags);

    let mut authors = conn.prepare(
        "SELECT a.name, COUNT(*) AS c FROM books_authors_link l
         JOIN authors a ON a.id = l.author GROUP BY a.name ORDER BY c DESC, a.name LIMIT 10",
    )?;
    let top_authors: Vec<Value> = authors
        .query_map([], |r| {
            let author: String = r.get(0)?;
            let count: i64 = r.get(1)?;
            Ok(json!({"author": author, "count": count}))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!({
        "books": total,
        "read": read,
        "unread": total - read,
        "ratings": ratings,
        "added_by_year": by_year,
        "top_tags": top_tags,
        "top_authors": top_authors,
    }))
}

fn collections_file(lib: &Library) -> Result<std::path::PathBuf, BookError> {
    Ok(write::data_dir(lib)?.join("collections.json"))
}

fn load_collections(lib: &Library) -> Result<Map<String, Value>, BookError> {
    let path = collections_file(lib)?;
    if !path.is_file() {
        return Ok(Map::new());
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text)
        .map_err(|e| BookError::Invalid(format!("collections file is corrupt: {e}")))
}

fn store_collections(lib: &Library, map: &Map<String, Value>) -> Result<(), BookError> {
    let path = collections_file(lib)?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".into()),
    )?;
    Ok(())
}

/// Save a named search (a smart collection): just a name and a query string.
pub fn collection_save(lib: &Library, name: &str, query: &str) -> Result<Value, BookError> {
    let name = name.trim();
    let query = query.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(BookError::Invalid("name must be 1-80 characters".into()));
    }
    if query.is_empty() {
        return Err(BookError::Invalid("query must not be empty".into()));
    }
    let mut map = load_collections(lib)?;
    map.insert(name.to_string(), Value::String(query.to_string()));
    store_collections(lib, &map)?;
    Ok(json!({"name": name, "query": query}))
}

/// List all saved searches.
pub fn collection_list(lib: &Library) -> Result<Value, BookError> {
    let map = load_collections(lib)?;
    let names: Vec<&String> = map.keys().collect();
    Ok(json!({"names": names, "collections": map}))
}

/// Run a saved search through the normal search path.
pub fn collection_run(lib: &Library, name: &str, limit: usize) -> Result<Value, BookError> {
    let map = load_collections(lib)?;
    let query = map
        .get(name.trim())
        .and_then(Value::as_str)
        .ok_or_else(|| BookError::NotFound(format!("collection '{name}'")))?;
    let books = lib.search(query, limit)?;
    Ok(json!({"name": name, "query": query, "count": books.len(), "books": books}))
}

/// Delete a saved search. A no-op when absent.
pub fn collection_delete(lib: &Library, name: &str) -> Result<Value, BookError> {
    let mut map = load_collections(lib)?;
    let removed = map.remove(name.trim()).is_some();
    if removed {
        store_collections(lib, &map)?;
    }
    Ok(json!({"name": name, "deleted": removed}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::CALIBRE_WRITE_SCHEMA;
    use std::path::Path;

    fn fixture() -> Result<(tempfile::TempDir, Library), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let root = tmp.path().join("lib");
        std::fs::create_dir_all(&root)?;
        // DDL first on a plain connection (triggers are created, not fired),
        // then seeds through the function-registering connection.
        let init = rusqlite::Connection::open(root.join("metadata.db"))?;
        init.execute_batch(CALIBRE_WRITE_SCHEMA)?;
        drop(init);
        let lib = Library::open(&root)?;
        let seeded = write::open_writable(&lib)?;
        seeded.execute_batch(
            "INSERT INTO books(title, author_sort, path) VALUES
                 ('Dune','Herbert, Frank','Frank Herbert/Dune (1)'),
                 ('Dune Messiah','Herbert, Frank','Frank Herbert/Dune Messiah (2)'),
                 ('Emma','Austen, Jane','Jane Austen/Emma (3)'),
                 ('Dune','Someone Else','Someone Else/Dune (4)');
             INSERT INTO authors(name, sort, link) VALUES
                 ('Frank Herbert','Herbert, Frank',''),
                 ('Jane Austen','Austen, Jane',''),
                 ('Someone Else','Else, Someone','');
             INSERT INTO books_authors_link(book, author) VALUES
                 (1,1),(2,1),(3,2),(4,3);
             INSERT INTO tags(name, link) VALUES ('sci-fi',''),('classic','');
             INSERT INTO books_tags_link(book, tag) VALUES (1,1),(2,1),(2,2);
             INSERT INTO series(name, link) VALUES ('Dune','');
             INSERT INTO books_series_link(book, series) VALUES (1,1),(2,1);
             UPDATE books SET series_index = 3.0 WHERE id = 2;
             INSERT INTO ratings(rating, link) VALUES (4,''),(5,'');
             INSERT INTO books_ratings_link(book, rating) VALUES (1,1),(3,2);
             INSERT INTO last_read_positions(book) VALUES (3);",
        )?;
        drop(seeded);
        Ok((tmp, lib))
    }

    #[test]
    fn rename_tag_simple_and_merge() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let renamed = rename_vocab(&lib, "tag", "sci-fi", "science-fiction")?;
        assert_eq!(renamed["merged"], false);
        assert_eq!(renamed["books"], 2);
        assert!(renamed["backup"]
            .as_str()
            .is_some_and(|p| Path::new(p).is_file()));

        // Merge: book 2 already has both; UNIQUE must not explode.
        let merged = rename_vocab(&lib, "tag", "classic", "science-fiction")?;
        assert_eq!(merged["merged"], true);
        assert_eq!(merged["books"], 1);
        let book = lib.detail(2)?;
        let tags = book["tags"].as_array().cloned().unwrap_or_default();
        assert_eq!(tags.len(), 1);

        assert!(matches!(
            rename_vocab(&lib, "tag", "absent", "x"),
            Err(BookError::NotFound(_))
        ));
        assert!(matches!(
            rename_vocab(&lib, "genre", "a", "b"),
            Err(BookError::Invalid(_))
        ));
        Ok(())
    }

    #[test]
    fn rename_author_refreshes_sorts() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let renamed = rename_vocab(&lib, "author", "Frank Herbert", "Franklin Herbert")?;
        assert_eq!(renamed["books"], 2);
        let book = lib.detail(1)?;
        assert_eq!(book["author_sort"], "Herbert, Franklin");
        let authors = book["authors"].as_array().cloned().unwrap_or_default();
        assert!(authors.iter().any(|a| a == "Franklin Herbert"));
        Ok(())
    }

    #[test]
    fn rename_series_keeps_links() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let renamed = rename_vocab(&lib, "series", "Dune", "Dune Saga")?;
        assert_eq!(renamed["books"], 2);
        assert_eq!(lib.detail(1)?["series"], "Dune Saga");
        Ok(())
    }

    #[test]
    fn duplicates_groups_by_normalised_title() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let found = duplicates(&lib, 10)?;
        assert_eq!(found["count"], 1);
        assert_eq!(found["groups"][0]["count"], 2);
        Ok(())
    }

    #[test]
    fn gaps_unread_and_stats() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let gaps = series_gaps(&lib)?;
        assert_eq!(gaps["count"], 1);
        assert_eq!(gaps["series"][0]["missing"], json!([2]));

        let unread_list = unread(&lib, 50)?;
        // Book 3 is marked read via last_read_positions.
        assert_eq!(unread_list["count"], 3);

        let stats = reading_stats(&lib)?;
        assert_eq!(stats["books"], 4);
        assert_eq!(stats["read"], 1);
        assert_eq!(stats["unread"], 3);
        Ok(())
    }

    #[test]
    fn collections_roundtrip() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        collection_save(&lib, "dune books", "dune")?;
        let listed = collection_list(&lib)?;
        assert!(listed["names"].as_array().map(|n| n.len()).unwrap_or(0) == 1);
        let ran = collection_run(&lib, "dune books", 10)?;
        assert!(ran["count"].as_i64().unwrap_or(0) >= 2);
        let deleted = collection_delete(&lib, "dune books")?;
        assert_eq!(deleted["deleted"], true);
        assert!(matches!(
            collection_run(&lib, "dune books", 10),
            Err(BookError::NotFound(_))
        ));
        Ok(())
    }
}
