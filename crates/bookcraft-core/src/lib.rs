//! L1 BookCraft core: Calibre library access, metadata management, format
//! conversion, HTML ingest.
//!
//! Reads open the library read-only. Every mutation goes through Calibre's own
//! `calibredb` (which owns uuids, link tables and the full-text index), and every
//! mutation first takes a timestamped backup outside the library.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

pub mod convert;
pub mod ingest;
pub mod library;
pub mod write;

use serde_json::{json, Value};
use std::fmt;
use std::path::{Component, Path, PathBuf};

pub const APP_NAME: &str = "bookcraft";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every way a BookCraft operation can fail. A missing engine is NOT an error:
/// it is reported as `status: "down"` in a successful result.
#[derive(Debug)]
pub enum BookError {
    NotFound(String),
    Invalid(String),
    Outside(String),
    Db(rusqlite::Error),
    Io(std::io::Error),
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookError::NotFound(m) => write!(f, "not found: {m}"),
            BookError::Invalid(m) => write!(f, "invalid argument: {m}"),
            BookError::Outside(m) => write!(f, "path outside allowed root: {m}"),
            BookError::Db(e) => write!(f, "database error: {e}"),
            BookError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for BookError {}

impl From<rusqlite::Error> for BookError {
    fn from(e: rusqlite::Error) -> Self {
        BookError::Db(e)
    }
}

impl From<std::io::Error> for BookError {
    fn from(e: std::io::Error) -> Self {
        BookError::Io(e)
    }
}

/// Lexically normalise an absolute path (resolve `.` and `..` without touching disk).
fn clean(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for comp in abs.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn fold(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// True when `path` is `root` or lies beneath it (lexical, `..`-safe).
pub fn within(root: &Path, path: &Path) -> bool {
    let r = fold(&clean(root));
    let p = fold(&clean(path));
    p == r || p.starts_with(&format!("{}/", r.trim_end_matches('/')))
}

pub(crate) fn ensure_within(root: Option<&Path>, path: &Path) -> Result<(), BookError> {
    match root {
        Some(r) if !within(r, path) => Err(BookError::Outside(format!(
            "{} is not under {}",
            path.display(),
            r.display()
        ))),
        _ => Ok(()),
    }
}

/// Locate a Calibre executable without assuming one machine's install layout.
///
/// Precedence: explicit env var > `PATH` > the standard Calibre install roots.
pub fn find_calibre_tool(windows_exe: &str, unix_exe: &str, env_var: &str) -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(env_var) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
        return None;
    }

    let exe = if cfg!(windows) { windows_exe } else { unix_exe };

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            roots.push(base.join("Calibre2").join(exe));
            roots.push(base.join("Programs").join("Calibre2").join(exe));
            roots.push(base.join("Calibre").join(exe));
        }
    }
    roots.into_iter().find(|p| p.is_file())
}

/// Strip HTML tags and decode the entities Calibre descriptions and ingested pages use.
///
/// Deliberately a small, dependency-free scanner: it never indexes by byte, so arbitrary
/// UTF-8 input cannot panic it.
pub fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut pending_space = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                pending_space = true;
            }
            _ if in_tag => {}
            _ => {
                if pending_space {
                    let after_word = out
                        .chars()
                        .last()
                        .map(|c| !c.is_whitespace())
                        .unwrap_or(false);
                    let punctuation = matches!(
                        ch,
                        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '…'
                    );
                    if after_word && !punctuation {
                        out.push(' ');
                    }
                    pending_space = false;
                }
                out.push(ch);
            }
        }
    }
    decode_entities(&out)
}

/// Decode the small entity set found in Calibre descriptions and ingested HTML pages.
pub fn decode_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, BookError> {
    arg_str(args, key).ok_or_else(|| BookError::Invalid(format!("missing '{key}'")))
}

fn root_of(args: &Value) -> Option<PathBuf> {
    arg_str(args, "root")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("BOOKCRAFT_ROOT").map(PathBuf::from))
}

fn library_of(args: &Value, root: Option<&Path>) -> Result<library::Library, BookError> {
    let raw = arg_str(args, "library")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("BOOKCRAFT_LIBRARY").map(PathBuf::from))
        .ok_or_else(|| {
            BookError::Invalid("no library: pass 'library' or set BOOKCRAFT_LIBRARY".into())
        })?;
    ensure_within(root, &raw)?;
    library::Library::open(&raw)
}

/// Single entry point shared by the CLI subcommands and the MCP tools.
pub fn dispatch(op: &str, args: &Value) -> Result<Value, BookError> {
    let root = root_of(args);
    let root = root.as_deref();
    let limit = arg_i64(args, "limit").unwrap_or(20).clamp(1, 1000) as usize;

    let mut body = match op {
        "status" => {
            let engine = convert::find_ebook_convert();
            let mut v = json!({
                "app": APP_NAME,
                "version": APP_VERSION,
                "ebook_convert": match &engine {
                    Some(p) => json!({"status": "ok", "path": p.to_string_lossy()}),
                    None => json!({"status": "down", "message": "ebook-convert not found"}),
                },
            });
            if arg_str(args, "library").is_some() || std::env::var_os("BOOKCRAFT_LIBRARY").is_some()
            {
                v["library"] = match library_of(args, root) {
                    Ok(lib) => json!({"status": "ok", "root": lib.root().to_string_lossy(),
                                      "books": lib.stats()?["books"]}),
                    Err(e) => json!({"status": "error", "error": e.to_string()}),
                };
            }
            v
        }
        "list" => {
            let lib = library_of(args, root)?;
            let offset = arg_i64(args, "offset").unwrap_or(0).max(0) as usize;
            let books = lib.list(limit, offset)?;
            json!({"count": books.len(), "books": books})
        }
        "search" => {
            let lib = library_of(args, root)?;
            let q = require_str(args, "query")?;
            let books = lib.search(q, limit)?;
            json!({"query": q, "count": books.len(), "books": books})
        }
        "info" => {
            let lib = library_of(args, root)?;
            let id =
                arg_i64(args, "id").ok_or_else(|| BookError::Invalid("missing 'id'".into()))?;
            json!({"book": lib.detail(id)?})
        }
        "grep" => {
            let lib = library_of(args, root)?;
            let q = require_str(args, "pattern")?;
            lib.grep(q, limit)?
        }
        "stats" => {
            let lib = library_of(args, root)?;
            json!({"stats": lib.stats()?})
        }
        "schema" => {
            let lib = library_of(args, root)?;
            let objects = lib.schema()?;
            json!({"count": objects.len(), "objects": objects})
        }
        "convert" => {
            let lib = library_of(args, root)?;
            let id =
                arg_i64(args, "id").ok_or_else(|| BookError::Invalid("missing 'id'".into()))?;
            let to = require_str(args, "to")?;
            let out_dir = PathBuf::from(require_str(args, "out_dir")?);
            ensure_within(root, &out_dir)?;
            convert::convert(&lib, id, to, &out_dir, arg_str(args, "from"))?
        }
        "ingest_html" => {
            let html_path = PathBuf::from(require_str(args, "html")?);
            let out_path = PathBuf::from(require_str(args, "out")?);
            ensure_within(root, &html_path)?;
            ensure_within(root, &out_path)?;
            ingest::ingest_file(&html_path, &out_path, arg_str(args, "title"))?
        }
        "backup" => {
            let lib = library_of(args, root)?;
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let path = write::ensure_backup(&lib, force)?;
            json!({"backup": path.to_string_lossy()})
        }
        "update" => {
            let lib = library_of(args, root)?;
            let id =
                arg_i64(args, "id").ok_or_else(|| BookError::Invalid("missing 'id'".into()))?;
            let fields = args
                .get("fields")
                .and_then(Value::as_object)
                .ok_or_else(|| BookError::Invalid("missing 'fields' object".into()))?;
            write::set_metadata(&lib, id, fields)?
        }
        "add_book" => {
            let lib = library_of(args, root)?;
            let empty = args.get("empty").and_then(Value::as_bool).unwrap_or(false);
            if empty {
                write::add_empty_book(
                    &lib,
                    arg_str(args, "title"),
                    arg_str(args, "authors"),
                    arg_str(args, "tags"),
                )?
            } else {
                let file = PathBuf::from(require_str(args, "file")?);
                write::add_book(
                    &lib,
                    &file,
                    root,
                    arg_str(args, "title"),
                    arg_str(args, "authors"),
                    arg_str(args, "tags"),
                    arg_str(args, "series"),
                    args.get("series_index").and_then(Value::as_f64),
                    arg_str(args, "languages"),
                    arg_str(args, "isbn"),
                    args.get("duplicates")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    arg_str(args, "automerge"),
                    false,
                )?
            }
        }
        "remove_book" => {
            let lib = library_of(args, root)?;
            let id =
                arg_i64(args, "id").ok_or_else(|| BookError::Invalid("missing 'id'".into()))?;
            let permanent = args
                .get("permanent")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            write::remove_book(&lib, id, permanent)?
        }
        "add_format" => {
            let lib = library_of(args, root)?;
            let id =
                arg_i64(args, "id").ok_or_else(|| BookError::Invalid("missing 'id'".into()))?;
            let file = PathBuf::from(require_str(args, "file")?);
            write::add_format(&lib, id, &file, root)?
        }
        "remove_format" => {
            let lib = library_of(args, root)?;
            let id =
                arg_i64(args, "id").ok_or_else(|| BookError::Invalid("missing 'id'".into()))?;
            let format = require_str(args, "format")?;
            write::remove_format(&lib, id, format)?
        }
        other => return Err(BookError::Invalid(format!("unknown operation '{other}'"))),
    };

    if let Some(obj) = body.as_object_mut() {
        obj.insert("success".into(), json!(true));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_blocks_dotdot_escape() {
        let root = Path::new("D:/Dev/repos");
        assert!(within(root, Path::new("D:/Dev/repos/bookcraft/x")));
        assert!(within(root, Path::new("D:/Dev/repos")));
        assert!(!within(root, Path::new("D:/Dev/repos/../secrets")));
        assert!(!within(root, Path::new("D:/Dev/repos-evil/x")));
        assert!(!within(root, Path::new("C:/Windows")));
    }

    #[test]
    fn unknown_op_is_an_error() {
        assert!(matches!(
            dispatch("explode", &json!({})),
            Err(BookError::Invalid(_))
        ));
    }

    #[test]
    fn missing_library_is_an_error_not_a_default() {
        let args = json!({"query": "x"});
        let res = dispatch("search", &args);
        // BOOKCRAFT_LIBRARY may legitimately be set in the environment; both outcomes are honest.
        if std::env::var_os("BOOKCRAFT_LIBRARY").is_none() {
            assert!(matches!(res, Err(BookError::Invalid(_))));
        }
    }
}
