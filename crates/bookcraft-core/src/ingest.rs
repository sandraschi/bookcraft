//! HTML ingest: turn a saved article page (Readly, a blog, a newsletter export) into a
//! self-contained Markdown artifact.
//!
//! This is the `book ingest-readly-sim` path from the craft plan: fixture HTML in, Markdown
//! out, no browser, no network, no tokens. Live scraping stays in `readly-mcp`.

use crate::{decode_entities, BookError};
use serde_json::{json, Value};
use std::path::Path;

/// Refuse absurd inputs rather than allocating unbounded memory.
const MAX_HTML_BYTES: u64 = 16 * 1024 * 1024;

fn find_ci(haystack_lower: &str, needle: &str, from: usize) -> Option<usize> {
    if from >= haystack_lower.len() {
        return None;
    }
    haystack_lower.get(from..)?.find(needle).map(|i| i + from)
}

/// Remove `<script>` / `<style>` / `<noscript>` elements. `lower` is the ASCII-lowercased copy,
/// which preserves byte offsets, so slicing the original at the same indexes is safe.
fn drop_element(html: &str, name: &str) -> String {
    let open = format!("<{name}");
    let close = format!("</{name}");
    let mut current = html.to_string();
    loop {
        let lower = current.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else {
            break;
        };
        let Some(open_end) = lower.get(start..).and_then(|s| s.find('>')) else {
            current.truncate(start);
            break;
        };
        let body_start = start + open_end + 1;
        let end = match find_ci(&lower, &close, body_start) {
            Some(e) => match lower.get(e..).and_then(|s| s.find('>')) {
                Some(gt) => e + gt + 1,
                None => current.len(),
            },
            None => current.len(),
        };
        if end <= start {
            current.truncate(start);
            break;
        }
        current.replace_range(start..end, " ");
    }
    current
}

fn extract_element(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let start = lower.find(&open)?;
    let gt = lower.get(start..)?.find('>')? + start + 1;
    let close = format!("</{tag}");
    let end = find_ci(&lower, &close, gt)?;
    html.get(gt..end).map(|s| s.trim().to_string())
}

fn body_of(html: &str) -> &str {
    let lower = html.to_ascii_lowercase();
    let Some(body_start) = lower.find("<body") else {
        return html;
    };
    let Some(gt) = lower.get(body_start..).and_then(|s| s.find('>')) else {
        return html;
    };
    let content_start = body_start + gt + 1;
    let content_end = find_ci(&lower, "</body", content_start).unwrap_or(html.len());
    html.get(content_start..content_end).unwrap_or(html)
}

fn tag_name(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "br"
            | "li"
            | "ul"
            | "ol"
            | "tr"
            | "td"
            | "th"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "aside"
            | "nav"
            | "pre"
            | "hr"
            | "figure"
            | "figcaption"
    )
}

/// Block-aware tag strip: block boundaries become newlines, everything else a space.
fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();

    for ch in html.chars() {
        if in_tag {
            match ch {
                '>' => {
                    if is_block(&tag_name(&tag)) {
                        out.push('\n');
                    } else {
                        out.push(' ');
                    }
                    in_tag = false;
                    tag.clear();
                }
                _ => {
                    // Only the tag name matters; attributes are bounded out.
                    if tag.len() < 64 {
                        tag.push(ch);
                    }
                }
            }
        } else if ch == '<' {
            in_tag = true;
        } else {
            out.push(ch);
        }
    }

    let decoded = decode_entities(&out);
    let mut lines: Vec<String> = Vec::new();
    for line in decoded.lines() {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            if lines.last().map(|l| !l.is_empty()).unwrap_or(false) {
                lines.push(String::new());
            }
        } else {
            lines.push(collapsed);
        }
    }
    while lines.last().map(String::is_empty).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

fn yaml_quote(value: &str) -> String {
    let bounded: String = value.chars().take(300).collect();
    let escaped = bounded.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn source_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Ingest one saved HTML page into Markdown at `out`.
///
/// The output is deterministic for the same input; `out` is overwritten.
pub fn ingest_file(
    html_path: &Path,
    out_path: &Path,
    title: Option<&str>,
) -> Result<Value, BookError> {
    if !html_path.is_file() {
        return Err(BookError::NotFound(html_path.display().to_string()));
    }
    let size = std::fs::metadata(html_path)?.len();
    if size > MAX_HTML_BYTES {
        return Err(BookError::Invalid(format!(
            "{} is {size} bytes; the ingest limit is {MAX_HTML_BYTES}",
            html_path.display()
        )));
    }
    let html = std::fs::read_to_string(html_path)?;

    let extracted_title = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| extract_element(&html, "title").map(|t| decode_entities(&t)))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| source_name(html_path));

    let cleaned = drop_element(&html, "script");
    let cleaned = drop_element(&cleaned, "style");
    let cleaned = drop_element(&cleaned, "noscript");
    let body = html_to_text(body_of(&cleaned));

    if body.is_empty() {
        return Err(BookError::Invalid(format!(
            "{} produced no readable text",
            html_path.display()
        )));
    }

    let markdown = format!(
        "---\ntitle: {}\nsource: {}\n---\n\n# {}\n\n{}\n",
        yaml_quote(&extracted_title),
        yaml_quote(&source_name(html_path)),
        extracted_title,
        body
    );

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(out_path, markdown.as_bytes())?;

    let words = body.split_whitespace().count();
    Ok(json!({
        "title": extracted_title,
        "source": source_name(html_path),
        "output": out_path.to_string_lossy(),
        "source_bytes": size,
        "bytes": markdown.len(),
        "words": words,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<!doctype html>
<html><head><title>Issue 42 &amp; the Sea</title>
<style>body { color: red; }</style></head>
<body>
<nav>subscribe now</nav>
<script>window.tracker = 'nope';</script>
<h1>The Deep</h1>
<p>First paragraph &mdash; with <b>bold</b> text.</p>
<p>Second&nbsp;paragraph.</p>
<noscript>enable js</noscript>
</body></html>"#;

    #[test]
    fn ingests_title_paragraphs_and_drops_scripts() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let html = tmp.path().join("issue.html");
        let out = tmp.path().join("issue.md");
        std::fs::write(&html, FIXTURE)?;

        let result = ingest_file(&html, &out, None)?;
        assert_eq!(result["title"], "Issue 42 & the Sea");
        assert!(result["words"].as_u64().unwrap_or_default() > 5);

        let md = std::fs::read_to_string(&out)?;
        assert!(md.starts_with("---\ntitle: \"Issue 42 & the Sea\""));
        assert!(md.contains("# Issue 42 & the Sea"));
        assert!(md.contains("First paragraph — with bold text."));
        assert!(md.contains("Second paragraph."));
        assert!(!md.contains("tracker"));
        assert!(!md.contains("enable js"));
        assert!(!md.contains("color: red"));
        assert!(!md.contains("<p>"));
        Ok(())
    }

    #[test]
    fn explicit_title_wins_and_parents_are_created() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let html = tmp.path().join("page.html");
        let out = tmp.path().join("nested/dir/page.md");
        std::fs::write(&html, "<html><body><p>body text</p></body></html>")?;

        let result = ingest_file(&html, &out, Some("My Issue"))?;
        assert_eq!(result["title"], "My Issue");
        assert!(out.is_file());
        Ok(())
    }

    #[test]
    fn missing_input_is_a_typed_error() {
        let tmp = match tempfile::tempdir() {
            Ok(t) => t,
            Err(_) => return,
        };
        let result = ingest_file(
            &tmp.path().join("absent.html"),
            &tmp.path().join("out.md"),
            None,
        );
        assert!(matches!(result, Err(BookError::NotFound(_))));
    }

    #[test]
    fn empty_documents_are_rejected() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let html = tmp.path().join("empty.html");
        std::fs::write(&html, "<html><body><script>x</script></body></html>")?;
        let result = ingest_file(&html, &tmp.path().join("out.md"), None);
        assert!(matches!(result, Err(BookError::Invalid(_))));
        Ok(())
    }

    #[test]
    fn unclosed_elements_do_not_hang() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let html = tmp.path().join("broken.html");
        std::fs::write(&html, "<html><body><p>text<script>oops</body>")?;
        let result = ingest_file(&html, &tmp.path().join("out.md"), None)?;
        assert!(result["words"].as_u64().unwrap_or_default() >= 1);
        Ok(())
    }
}
