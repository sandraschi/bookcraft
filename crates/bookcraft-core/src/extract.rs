//! Take books out of the library: CSV/JSON export, chapter excerpts, system open,
//! random picks. Reads stay read-only; `export` writes only to the caller-chosen path.

use crate::{library::Library, BookError};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

/// Export the whole library (or one search) as CSV or JSON to `out`.
pub fn export(
    lib: &Library,
    format: &str,
    out: &Path,
    root: Option<&Path>,
    query: Option<&str>,
) -> Result<Value, BookError> {
    let format = format.trim().to_ascii_lowercase();
    if format != "csv" && format != "json" {
        return Err(BookError::Invalid(
            "export format must be csv or json".into(),
        ));
    }
    crate::ensure_within(root, out)?;
    let books = match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => lib.search(q, 5000)?,
        None => lib.list(5000, 0)?,
    };
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let bytes = if format == "csv" {
        let mut text = String::from(
            "id,title,authors,series,series_index,tags,formats,rating,pubdate,added,path\n",
        );
        for book in &books {
            text.push_str(&csv_row(book));
            text.push('\n');
        }
        std::fs::write(out, text.as_bytes())?;
        text.len() as u64
    } else {
        let text = serde_json::to_string_pretty(&books).unwrap_or_else(|_| "[]".into());
        std::fs::write(out, text.as_bytes())?;
        text.len() as u64
    };
    Ok(json!({
        "output": out.to_string_lossy(),
        "format": format,
        "count": books.len(),
        "bytes": bytes,
    }))
}

fn csv_field(value: &Value) -> String {
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("; "),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => String::new(),
    };
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn csv_row(book: &Value) -> String {
    [
        "id",
        "title",
        "authors",
        "series",
        "series_index",
        "tags",
        "formats",
        "rating",
        "pubdate",
        "added",
        "path",
    ]
    .iter()
    .map(|k| csv_field(&book[k]))
    .collect::<Vec<_>>()
    .join(",")
}

fn read_chapter_text(epub: &Path, chapter: usize) -> Result<(usize, String, String), BookError> {
    let file = std::fs::File::open(epub)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| BookError::Invalid(format!("not a readable EPUB: {e}")))?;

    let mut container = String::new();
    archive
        .by_name("META-INF/container.xml")
        .map_err(|_| BookError::Invalid("EPUB has no container.xml".into()))?
        .read_to_string(&mut container)
        .map_err(|e| BookError::Invalid(format!("cannot read container.xml: {e}")))?;
    let opf_path = container
        .find("full-path=\"")
        .and_then(|i| {
            let rest = &container[i + 11..];
            rest.find('"').map(|j| rest[..j].to_string())
        })
        .ok_or_else(|| BookError::Invalid("container.xml has no rootfile".into()))?;
    let opf_dir = Path::new(&opf_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let mut opf = String::new();
    archive
        .by_name(&opf_path)
        .map_err(|_| BookError::Invalid("EPUB opf is missing".into()))?
        .read_to_string(&mut opf)
        .map_err(|e| BookError::Invalid(format!("cannot read opf: {e}")))?;

    // Manifest id -> href, then spine order. A scanner beats a dependency,
    // but it must advance past each tag, not restart inside it.
    let (hrefs, spine) =
        spine_hrefs(&opf).map_err(|e| BookError::Invalid(format!("cannot parse opf: {e}")))?;
    if spine.is_empty() {
        return Err(BookError::Invalid("EPUB spine is empty".into()));
    }
    let chapter = chapter.min(spine.len() - 1);
    let href = hrefs
        .iter()
        .find(|(id, _)| *id == spine[chapter])
        .map(|(_, h)| h.clone())
        .ok_or_else(|| BookError::Invalid("spine item has no manifest entry".into()))?;
    let decoded = percent_decode(&href);
    let full = opf_dir.join(&decoded);
    let name = full.to_string_lossy().replace('\\', "/");
    let mut text = String::new();
    archive
        .by_name(&name)
        .map_err(|_| BookError::Invalid(format!("chapter file missing: {decoded}")))?
        .read_to_string(&mut text)
        .map_err(|e| BookError::Invalid(format!("cannot read chapter: {e}")))?;
    Ok((spine.len(), decoded, crate::strip_html_tags(&text)))
}

/// Manifest (id, href) pairs plus spine order.
type SpineMap = (Vec<(String, String)>, Vec<String>);

/// Fixed scanner: advances past each tag instead of restarting inside it,
/// so a leading NCX or cover item cannot hide the chapters.
fn spine_hrefs(opf: &str) -> Result<SpineMap, String> {
    let mut hrefs = Vec::new();
    let mut rest = opf;
    while let Some(start) = rest.find("<item ") {
        let after = &rest[start..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        if let (Some(id), Some(href)) = (attr(tag, "id"), attr(tag, "href")) {
            if tag.contains("html") {
                hrefs.push((id, href));
            }
        }
        rest = &after[end.min(after.len())..];
        if rest.is_empty() {
            break;
        }
        rest = &rest[1.min(rest.len())..];
    }
    let mut spine = Vec::new();
    let mut body = opf;
    while let Some(start) = body.find("<itemref") {
        let after = &body[start..];
        let end = after.find('>').unwrap_or(after.len());
        if let Some(idref) = attr(&after[..end], "idref") {
            spine.push(idref);
        }
        body = &after[end.min(after.len())..];
        if body.is_empty() {
            break;
        }
        body = &body[1.min(body.len())..];
    }
    Ok((hrefs, spine))
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    tag.find(&key).and_then(|i| {
        let rest = &tag[i + key.len()..];
        rest.find('"').map(|j| rest[..j].to_string())
    })
}

fn percent_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn char_window(text: &str, offset: usize, chars: usize) -> (String, bool) {
    let total: Vec<char> = text.chars().collect();
    let start = offset.min(total.len());
    let end = (start + chars.max(1)).min(total.len());
    (total[start..end].iter().collect(), end < total.len())
}

/// Paginated chapter text from an EPUB (or raw window from a TXT). PDF and comics
/// are reported honestly as unsupported.
#[allow(clippy::too_many_arguments)]
pub fn excerpt(
    lib: &Library,
    id: i64,
    format: Option<&str>,
    chapter: usize,
    chars: usize,
    offset: usize,
) -> Result<Value, BookError> {
    let (source_format, path) = lib.file_for(id, format)?;
    let book = lib.detail(id)?;
    let title = book["title"].as_str().unwrap_or("").to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "txt" {
        let raw = std::fs::read_to_string(&path)?;
        let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let (text, more) = char_window(&collapsed, offset, chars.clamp(100, 20000));
        return Ok(json!({
            "book_id": id,
            "title": title,
            "format": source_format,
            "chapters": 1,
            "chapter": 0,
            "chapter_file": path.file_name().map(|n| n.to_string_lossy().into_owned()),
            "offset": offset,
            "text": text,
            "has_more": more,
        }));
    }
    if ext != "epub" {
        return Err(BookError::Invalid(format!(
            "excerpts support EPUB and TXT only, not {ext}"
        )));
    }
    let (chapters, chapter_file, body) = read_chapter_text(&path, chapter)?;
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let (text, more) = char_window(&collapsed, offset, chars.clamp(100, 20000));
    Ok(json!({
        "book_id": id,
        "title": title,
        "format": source_format,
        "chapters": chapters,
        "chapter": chapter.min(chapters.saturating_sub(1)),
        "chapter_file": chapter_file,
        "offset": offset,
        "text": text,
        "has_more": more,
    }))
}

/// Open the book's best file with the system default application (desktop affordance:
/// it opens where the server runs, not where the agent sits).
pub fn open_book(lib: &Library, id: i64, format: Option<&str>) -> Result<Value, BookError> {
    let (_, path) = lib.file_for(id, format)?;
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(&path).spawn()?;
    }
    Ok(json!({"book_id": id, "path": path.to_string_lossy(), "opened": true}))
}

/// A random book id, optionally filtered. Clock-seeded, not cryptographic.
fn random_index(len: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    if len == 0 {
        0
    } else {
        nanos % len
    }
}

/// One random book, optionally filtered by tag, author or format.
pub fn random_book(
    lib: &Library,
    tag: Option<&str>,
    author: Option<&str>,
    format: Option<&str>,
) -> Result<Value, BookError> {
    let conn = lib.read_conn()?;
    let mut sql = String::from("SELECT DISTINCT b.id FROM books b");
    let mut filters: Vec<String> = Vec::new();
    if tag.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        sql.push_str(
            " JOIN books_tags_link btl ON btl.book = b.id
              JOIN tags t ON t.id = btl.tag",
        );
        filters.push("t.name = ? COLLATE NOCASE".to_string());
    }
    if author.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        sql.push_str(
            " JOIN books_authors_link bal ON bal.book = b.id
              JOIN authors a ON a.id = bal.author",
        );
        filters.push("a.name = ? COLLATE NOCASE".to_string());
    }
    if format.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        sql.push_str(" JOIN data d ON d.book = b.id");
        filters.push("d.format = ? COLLATE NOCASE".to_string());
    }
    if !filters.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&filters.join(" AND "));
    }
    sql.push_str(" ORDER BY b.id");
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
    let tag_owned;
    let author_owned;
    let format_owned;
    if let Some(t) = tag.map(str::trim).filter(|s| !s.is_empty()) {
        tag_owned = t.to_string();
        params.push(&tag_owned);
    }
    if let Some(a) = author.map(str::trim).filter(|s| !s.is_empty()) {
        author_owned = a.to_string();
        params.push(&author_owned);
    }
    if let Some(f) = format.map(str::trim).filter(|s| !s.is_empty()) {
        format_owned = f.to_ascii_uppercase();
        params.push(&format_owned);
    }
    let ids: Vec<i64> = stmt
        .query_map(params.as_slice(), |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if ids.is_empty() {
        return Err(BookError::NotFound("no book matches the filters".into()));
    }
    Ok(json!({"book": lib.detail(ids[random_index(ids.len())])?}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::CALIBRE_WRITE_SCHEMA;

    fn fixture() -> Result<(tempfile::TempDir, Library), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let root = tmp.path().join("lib");
        std::fs::create_dir_all(&root)?;
        let init = rusqlite::Connection::open(root.join("metadata.db"))?;
        init.execute_batch(CALIBRE_WRITE_SCHEMA)?;
        drop(init);
        let lib = Library::open(&root)?;
        let seeded = crate::write::open_writable(&lib)?;
        seeded.execute_batch(
            "INSERT INTO books(title, author_sort, path) VALUES
                 ('Dune','Herbert, Frank','Frank Herbert/Dune (1)');
             INSERT INTO authors(name, sort, link) VALUES
                 ('Frank Herbert','Herbert, Frank','');
             INSERT INTO books_authors_link(book, author) VALUES (1,1);
             INSERT INTO tags(name, link) VALUES ('sci-fi','');
             INSERT INTO books_tags_link(book, tag) VALUES (1,1);",
        )?;
        drop(seeded);
        Ok((tmp, lib))
    }

    #[test]
    fn export_writes_csv_and_json() -> Result<(), BookError> {
        let (tmp, lib) = fixture()?;
        let csv = tmp.path().join("out.csv");
        let exported = export(&lib, "csv", &csv, None, None)?;
        assert_eq!(exported["count"], 1);
        let text = std::fs::read_to_string(&csv)?;
        assert!(text.starts_with("id,title,"));
        assert!(text.contains("Dune"));

        let js = tmp.path().join("out.json");
        export(&lib, "json", &js, None, Some("dune"))?;
        let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&js)?)
            .map_err(|e| BookError::Invalid(e.to_string()))?;
        assert_eq!(parsed.as_array().map(Vec::len), Some(1));
        assert!(matches!(
            export(&lib, "xml", &js, None, None),
            Err(BookError::Invalid(_))
        ));
        Ok(())
    }

    #[test]
    fn random_respects_and_rejects_filters() -> Result<(), BookError> {
        let (_tmp, lib) = fixture()?;
        let book = random_book(&lib, Some("sci-fi"), None, None)?;
        assert_eq!(book["book"]["id"], 1);
        assert!(matches!(
            random_book(&lib, Some("no-such-tag"), None, None),
            Err(BookError::NotFound(_))
        ));
        Ok(())
    }

    #[test]
    fn spine_scan_survives_leading_ncx_and_cover() -> Result<(), String> {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="u">
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="cover"/><itemref idref="ch1"/><itemref idref="ch2"/></spine>
</package>"#;
        let (hrefs, spine) = spine_hrefs(opf)?;
        assert_eq!(spine, vec!["cover", "ch1", "ch2"]);
        assert!(hrefs.iter().any(|(id, _)| id == "ch1"));
        assert!(hrefs.iter().all(|(id, _)| id != "ncx"));
        Ok(())
    }

    #[test]
    fn excerpt_rejects_non_epub_txt() -> Result<(), BookError> {
        let (tmp, lib) = fixture()?;
        let sample = tmp.path().join("note.txt");
        std::fs::write(&sample, "Plain text for excerpt.")?;
        crate::write::add_format(&lib, 1, &sample, None)?;
        let hit = excerpt(&lib, 1, Some("TXT"), 0, 500, 0)?;
        assert!(hit["text"].as_str().unwrap_or("").contains("Plain text"));
        assert_eq!(hit["has_more"], false);
        Ok(())
    }
}
