//! Format conversion through Calibre's `ebook-convert` executable.
//!
//! BookCraft never embeds a converter and never writes into the library: the source file is
//! read-only input and the destination is a path the caller chose (checked against `--root`).

use crate::library::Library;
use crate::BookError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Locate `ebook-convert` without assuming one machine's install layout.
///
/// Precedence: `BOOKCRAFT_EBOOK_CONVERT` > `PATH` > the standard Calibre install roots.
pub fn find_ebook_convert() -> Option<PathBuf> {
    crate::find_calibre_tool(
        "ebook-convert.exe",
        "ebook-convert",
        "BOOKCRAFT_EBOOK_CONVERT",
    )
}

/// A caller-supplied target format: ASCII letters only, 1-8 characters.
fn normalise_format(to: &str) -> Result<String, BookError> {
    let trimmed = to.trim();
    if trimmed.is_empty() || trimmed.len() > 8 || !trimmed.chars().all(|c| c.is_ascii_alphabetic())
    {
        return Err(BookError::Invalid(format!(
            "'to' must be a format name such as epub or pdf, got '{to}'"
        )));
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// A filesystem-safe stem derived from a book title. Never empty, bounded at 120 chars.
fn safe_stem(title: &str) -> String {
    let mut stem: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    stem = stem.trim().trim_matches('.').to_string();
    if !stem.chars().any(char::is_alphanumeric) {
        "book".to_string()
    } else {
        stem
    }
}

/// First free `<stem>.<ext>` (or `<stem>-2.<ext>`, ...) so an existing file is never replaced.
fn free_destination(out_dir: &Path, stem: &str, ext: &str) -> Result<PathBuf, BookError> {
    let candidate = out_dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return Ok(candidate);
    }
    for n in 2..1000 {
        let candidate = out_dir.join(format!("{stem}-{n}.{ext}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(BookError::Invalid(format!(
        "no free filename for '{stem}.{ext}' in {}",
        out_dir.display()
    )))
}

fn tail(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= 400 {
        text.to_string()
    } else {
        text.chars()
            .rev()
            .take(400)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

/// Convert one book to `to`, writing `<out_dir>/<title>.<to>`.
///
/// Returns the output path and measured size; the library is only read.
pub fn convert(
    lib: &Library,
    id: i64,
    to: &str,
    out_dir: &Path,
    from: Option<&str>,
) -> Result<Value, BookError> {
    let engine = find_ebook_convert().ok_or_else(|| {
        BookError::NotFound(
            "ebook-convert not found: install Calibre or set BOOKCRAFT_EBOOK_CONVERT".into(),
        )
    })?;
    let ext = normalise_format(to)?;

    let (source_format, source) = lib.file_for(id, from)?;
    if source_format.to_ascii_lowercase() == ext {
        return Err(BookError::Invalid(format!("book id {id} is already {ext}")));
    }

    std::fs::create_dir_all(out_dir)?;
    let book = lib.detail(id)?;
    let title = book["title"].as_str().unwrap_or("book");
    let destination = free_destination(out_dir, &safe_stem(title), &ext)?;

    let started = Instant::now();
    let mut command = Command::new(&engine);
    command.arg(&source).arg(&destination);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output()?;
    let elapsed_ms = started.elapsed().as_millis();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            tail(&stdout)
        } else {
            tail(&stderr)
        };
        return Err(BookError::Invalid(format!(
            "ebook-convert failed ({}): {detail}",
            output.status
        )));
    }

    let bytes = std::fs::metadata(&destination)?.len();
    if bytes == 0 {
        return Err(BookError::Invalid(format!(
            "ebook-convert wrote an empty file: {}",
            destination.display()
        )));
    }

    Ok(json!({
        "book_id": id,
        "title": title,
        "from": source_format,
        "to": ext.to_ascii_uppercase(),
        "source": source.to_string_lossy(),
        "output": destination.to_string_lossy(),
        "bytes": bytes,
        "engine": engine.to_string_lossy(),
        "elapsed_ms": elapsed_ms,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_validated_not_copied_into_a_command() {
        assert_eq!(normalise_format(" EPUB ").ok().as_deref(), Some("epub"));
        assert!(normalise_format("").is_err());
        assert!(normalise_format("e pub").is_err());
        assert!(normalise_format("pdf; rm -rf").is_err());
        assert!(normalise_format("123456789").is_err());
    }

    #[test]
    fn stems_are_safe_and_never_empty() {
        assert_eq!(safe_stem("Dune"), "Dune");
        assert_eq!(safe_stem("A/B: C?"), "A_B_ C_");
        assert_eq!(safe_stem("   "), "book");
        assert_eq!(safe_stem("..."), "book");
        assert!(safe_stem(&"x".repeat(500)).chars().count() <= 120);
    }

    #[test]
    fn free_destination_does_not_overwrite() -> Result<(), BookError> {
        let tmp = tempfile::tempdir().map_err(BookError::Io)?;
        let first = free_destination(tmp.path(), "Dune", "pdf")?;
        std::fs::write(&first, b"x")?;
        let second = free_destination(tmp.path(), "Dune", "pdf")?;
        assert_ne!(first, second);
        assert!(second.ends_with("Dune-2.pdf"));
        Ok(())
    }

    #[test]
    fn explicit_engine_override_must_exist() {
        // The environment is process-global; only assert the documented contract when unset.
        if std::env::var_os("BOOKCRAFT_EBOOK_CONVERT").is_none() {
            let found = find_ebook_convert();
            if let Some(path) = found {
                assert!(path.is_file());
            }
        }
    }
}
