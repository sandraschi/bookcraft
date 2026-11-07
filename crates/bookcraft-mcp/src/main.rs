//! L2 BookCraft CLI and MCP stdio server.
//!
//! One binary: `bookcraft-cli <verb>` for humans and scripts, `bookcraft-cli mcp` for agents.
//! Both surfaces call `bookcraft_core::dispatch`, so they cannot disagree.
//!
//! Stdio invariant: in `mcp` mode stdout is 100% line-delimited JSON-RPC. Diagnostics go to
//! stderr only.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use bookcraft_core::{dispatch, BookError, APP_NAME, APP_VERSION};
use clap::{Parser, Subcommand};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bookcraft-cli",
    about = "BookCraft: read-only Calibre library search, conversion and HTML ingest (CLI-first, MCP over stdio)",
    version = APP_VERSION
)]
struct Cli {
    /// Only read libraries and write outputs beneath this directory.
    #[arg(long, global = true, env = "BOOKCRAFT_ROOT")]
    root: Option<PathBuf>,
    /// Calibre library directory (or its metadata.db).
    #[arg(long, global = true, env = "BOOKCRAFT_LIBRARY")]
    library: Option<PathBuf>,
    /// Emit machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Report the converter engine and optional library health
    Status,
    /// List books, newest first
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Substring search over title, authors, series, tags and descriptions
    Search {
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Full details for one book id
    Info {
        #[arg(long)]
        id: i64,
    },
    /// Search book text when a full-text index exists, else titles and descriptions
    Grep {
        #[arg(long, alias = "query")]
        pattern: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Library-wide counts and format distribution
    Stats,
    /// Dump the Calibre schema (sqlite_master): the ground truth for any write path
    Schema,
    /// Take a timestamped metadata backup outside the library (also automatic before writes)
    Backup {
        #[arg(long)]
        force: bool,
    },
    /// Create a fresh Calibre-shaped library directory (refuses when one exists)
    Init,
    /// Set metadata fields on one book (title, authors, tags, series, rating, ...)
    Update {
        #[arg(long)]
        id: i64,
        /// Repeatable key=value pairs, e.g. --set title="Dune" --set tags="sci-fi, classic"
        #[arg(long = "set")]
        set: Vec<String>,
    },
    /// Add a book file (or an empty record with --empty)
    AddBook {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        empty: bool,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        authors: Option<String>,
        #[arg(long)]
        tags: Option<String>,
        #[arg(long)]
        series: Option<String>,
        #[arg(long)]
        series_index: Option<f64>,
        #[arg(long)]
        languages: Option<String>,
        #[arg(long)]
        isbn: Option<String>,
        #[arg(long)]
        duplicates: bool,
        #[arg(long)]
        automerge: Option<String>,
    },
    /// Remove one book from the library (files go to the Recycle Bin unless --permanent)
    RemoveBook {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        permanent: bool,
    },
    /// Rename a tag, author, series or publisher (merges when the new name exists)
    Rename {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
    },
    /// Books sharing a normalised title: duplicate candidates
    Duplicates {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Series with missing volume numbers
    SeriesGaps,
    /// Books never opened (or everything when the library has no tracking table)
    Unread {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Totals, read/unread split, rating histogram, additions by year, top tags/authors
    ReadingStats,
    /// Export the library (or one search) as CSV or JSON
    Export {
        #[arg(long)]
        format: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        query: Option<String>,
    },
    /// Paginated chapter text from an EPUB (or a raw window from a TXT)
    Excerpt {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        format: Option<String>,
        #[arg(long, default_value_t = 0)]
        chapter: usize,
        #[arg(long, default_value_t = 2000)]
        chars: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Open the best file with the system default application
    Open {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        format: Option<String>,
    },
    /// A random book, optionally filtered by tag, author or format
    Random {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        format: Option<String>,
    },
    /// Save a named search (a smart collection)
    CollectionSave {
        #[arg(long)]
        name: String,
        #[arg(long)]
        query: String,
    },
    /// List saved searches
    CollectionList,
    /// Run a saved search
    CollectionRun {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Delete a saved search
    CollectionDelete {
        #[arg(long)]
        name: String,
    },
    /// Update the same fields on many books, all-or-nothing (ids: "1,2,3")
    BulkUpdate {
        #[arg(long)]
        ids: String,
        #[arg(long = "set")]
        set: Vec<String>,
    },
    /// Attach one format file to a book (replaces the format when present)
    AddFormat {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        file: PathBuf,
    },
    /// Drop one format from a book; a no-op when absent
    RemoveFormat {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        format: String,
    },
    /// Convert one book with Calibre's ebook-convert
    Convert {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        to: String,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long)]
        from: Option<String>,
    },
    /// Turn a saved HTML page into a Markdown artifact
    Ingest {
        #[arg(long)]
        html: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        title: Option<String>,
    },
    /// List the MCP tool surface as JSON
    Tools,
    /// Run the MCP 2024-11-05 stdio server
    Mcp,
}

/// Stdout with a closed pipe exits quietly instead of panicking: the consumer is
/// gone, so there is nothing to report to. Any other write error exits 1.
fn emit(text: &str) {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    match writeln!(out, "{text}") {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(0),
        Err(e) => {
            eprintln!("error: failed to write stdout: {e}");
            std::process::exit(1);
        }
    }
}

macro_rules! out {
    ($($t:tt)*) => {
        emit(&format!($($t)*))
    };
}

/// Parse repeatable `--set key=value` pairs with scalar coercion
/// (integers, floats and booleans travel typed; everything else stays a string).
fn parse_fields(set: &[String]) -> Map<String, Value> {
    let mut fields = Map::new();
    for kv in set {
        match kv.split_once('=') {
            Some((k, v)) if !k.trim().is_empty() => {
                let v = v.trim();
                let value = v
                    .parse::<i64>()
                    .map(Value::from)
                    .or_else(|_| v.parse::<f64>().map(Value::from))
                    .unwrap_or_else(|_| match v.to_ascii_lowercase().as_str() {
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        _ => json!(v),
                    });
                fields.insert(k.trim().to_string(), value);
            }
            _ => {
                eprintln!("error: bad --set '{kv}'; use --set key=value");
                std::process::exit(2);
            }
        }
    }
    fields
}

fn base_args(cli: &Cli) -> Map<String, Value> {
    let mut args = Map::new();
    if let Some(root) = &cli.root {
        args.insert("root".into(), json!(root.to_string_lossy()));
    }
    if let Some(library) = &cli.library {
        args.insert("library".into(), json!(library.to_string_lossy()));
    }
    args
}

fn error_type(error: &BookError) -> &'static str {
    match error {
        BookError::NotFound(_) => "not_found",
        BookError::Invalid(_) => "invalid",
        BookError::Outside(_) => "outside",
        BookError::Db(_) => "database",
        BookError::Io(_) => "io",
    }
}

fn error_payload(error: &BookError) -> Value {
    json!({
        "success": false,
        "error": error.to_string(),
        "error_type": error_type(error),
    })
}

fn authors_of(book: &Value) -> String {
    book["authors"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn formats_of(book: &Value) -> String {
    book["formats"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn print_book_line(book: &Value) {
    let id = book["id"].as_i64().unwrap_or_default();
    let title = book["title"].as_str().unwrap_or("(untitled)");
    let authors = authors_of(book);
    let formats = formats_of(book);
    if formats.is_empty() {
        out!("  #{id}  {title} — {authors}");
    } else {
        out!("  #{id}  {title} — {authors} [{formats}]");
    }
}

fn print_human(op: &str, value: &Value) {
    match op {
        "list" | "search" => {
            let books = value["books"].as_array().cloned().unwrap_or_default();
            out!("{} book(s)", books.len());
            for book in &books {
                print_book_line(book);
            }
        }
        "info" => {
            let book = &value["book"];
            out!("#{}  {}", book["id"], book["title"].as_str().unwrap_or(""));
            out!("authors: {}", authors_of(book));
            if let Some(series) = book["series"].as_str() {
                out!("series:  {series} #{}", book["series_index"]);
            }
            out!("formats: {}", formats_of(book));
            if let Some(cover) = book["cover_path"].as_str() {
                out!("cover:   {cover}");
            }
            if let Some(desc) = book["description"].as_str() {
                out!("\n{desc}");
            }
        }
        "grep" => {
            out!(
                "{} hit(s) via {}",
                value["count"].as_i64().unwrap_or_default(),
                value["engine"].as_str().unwrap_or("unknown")
            );
            for hit in value["hits"].as_array().into_iter().flatten() {
                out!(
                    "  #{} {} — {}",
                    hit["book_id"],
                    hit["title"].as_str().unwrap_or(""),
                    hit["snippet"].as_str().unwrap_or("")
                );
            }
        }
        "status" => {
            out!("{APP_NAME} {APP_VERSION}");
            let engine = &value["ebook_convert"];
            out!(
                "ebook-convert: {}{}",
                engine["status"].as_str().unwrap_or("unknown"),
                engine["path"]
                    .as_str()
                    .map(|p| format!(" ({p})"))
                    .unwrap_or_default()
            );
            if !value["library"].is_null() {
                out!(
                    "library: {}{}",
                    value["library"]["status"].as_str().unwrap_or("unknown"),
                    value["library"]["books"]
                        .as_i64()
                        .map(|n| format!(" ({n} books)"))
                        .unwrap_or_default()
                );
            }
        }
        "convert" => out!(
            "{} -> {} ({} bytes) {}",
            value["from"].as_str().unwrap_or(""),
            value["output"].as_str().unwrap_or(""),
            value["bytes"].as_i64().unwrap_or_default(),
            value["engine"].as_str().unwrap_or("")
        ),
        "ingest_html" => out!(
            "{} -> {} ({} words, {} bytes)",
            value["title"].as_str().unwrap_or(""),
            value["output"].as_str().unwrap_or(""),
            value["words"].as_i64().unwrap_or_default(),
            value["bytes"].as_i64().unwrap_or_default()
        ),
        "rename" => out!(
            "{} '{}' -> '{}' ({} book(s){}) backup {}",
            value["kind"].as_str().unwrap_or(""),
            value["from"].as_str().unwrap_or(""),
            value["to"].as_str().unwrap_or(""),
            value["books"].as_i64().unwrap_or_default(),
            if value["merged"].as_bool().unwrap_or(false) {
                ", merged"
            } else {
                ""
            },
            value["backup"].as_str().unwrap_or("")
        ),
        "duplicates" => {
            out!(
                "{} duplicate group(s)",
                value["count"].as_i64().unwrap_or_default()
            );
            for group in value["groups"].as_array().into_iter().flatten() {
                let ids: Vec<String> = group["books"]
                    .as_array()
                    .map(|b| {
                        b.iter()
                            .filter_map(|x| x["id"].as_i64().map(|n| n.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                out!(
                    "  {} ({}): {}",
                    group["title"].as_str().unwrap_or(""),
                    group["count"].as_i64().unwrap_or_default(),
                    ids.join(", ")
                );
            }
        }
        "series_gaps" => {
            out!("{} series", value["count"].as_i64().unwrap_or_default());
            for series in value["series"].as_array().into_iter().flatten() {
                let missing: Vec<String> = series["missing"]
                    .as_array()
                    .map(|m| {
                        m.iter()
                            .filter_map(Value::as_i64)
                            .map(|n| n.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                if !missing.is_empty() {
                    out!(
                        "  {}: missing {}",
                        series["series"].as_str().unwrap_or(""),
                        missing.join(", ")
                    );
                }
            }
        }
        "unread" => {
            out!(
                "{} unread book(s)",
                value["count"].as_i64().unwrap_or_default()
            );
            for book in value["books"].as_array().into_iter().flatten().take(20) {
                print_book_line(book);
            }
        }
        "reading_stats" => out!(
            "{} books, {} read, {} unread",
            value["books"].as_i64().unwrap_or_default(),
            value["read"].as_i64().unwrap_or_default(),
            value["unread"].as_i64().unwrap_or_default()
        ),
        "export" => out!(
            "{} book(s) -> {} ({} bytes)",
            value["count"].as_i64().unwrap_or_default(),
            value["output"].as_str().unwrap_or(""),
            value["bytes"].as_i64().unwrap_or_default()
        ),
        "excerpt" => {
            out!(
                "#{} {} (chapter {} of {}, {} chars{})",
                value["book_id"].as_i64().unwrap_or_default(),
                value["title"].as_str().unwrap_or(""),
                value["chapter"].as_i64().unwrap_or_default(),
                value["chapters"].as_i64().unwrap_or_default(),
                value["text"].as_str().map(str::len).unwrap_or(0),
                if value["has_more"].as_bool().unwrap_or(false) {
                    ", more"
                } else {
                    ""
                }
            );
            out!("{}", value["text"].as_str().unwrap_or(""));
        }
        "open" => out!("opened {}", value["path"].as_str().unwrap_or("")),
        "random" => print_book_line(&value["book"]),
        "collection_save" | "collection_list" | "collection_delete" | "bulk_update" => {
            match serde_json::to_string_pretty(value) {
                Ok(pretty) => out!("{pretty}"),
                Err(_) => out!("{value}"),
            }
        }
        "collection_run" => {
            out!(
                "{}: {} hit(s)",
                value["name"].as_str().unwrap_or(""),
                value["count"].as_i64().unwrap_or_default()
            );
            for book in value["books"].as_array().into_iter().flatten() {
                print_book_line(book);
            }
        }
        _ => match serde_json::to_string_pretty(value) {
            Ok(pretty) => out!("{pretty}"),
            Err(_) => out!("{value}"),
        },
    }
}

/// One CLI invocation: build the argument object, dispatch, print, map to an exit code.
fn run(cli: &Cli) -> i32 {
    let mut args = base_args(cli);
    let (op, inserts): (&str, Vec<(&str, Value)>) = match &cli.command {
        Commands::Status => ("status", Vec::new()),
        Commands::List { limit, offset } => (
            "list",
            vec![("limit", json!(limit)), ("offset", json!(offset))],
        ),
        Commands::Search { query, limit } => (
            "search",
            vec![("query", json!(query)), ("limit", json!(limit))],
        ),
        Commands::Info { id } => ("info", vec![("id", json!(id))]),
        Commands::Grep { pattern, limit } => (
            "grep",
            vec![("pattern", json!(pattern)), ("limit", json!(limit))],
        ),
        Commands::Stats => ("stats", Vec::new()),
        Commands::Schema => ("schema", Vec::new()),
        Commands::Backup { force } => ("backup", vec![("force", json!(force))]),
        Commands::Init => ("init", Vec::new()),
        Commands::Update { id, set } => {
            let mut pairs = vec![("id", json!(id))];
            pairs.push(("fields", Value::Object(parse_fields(set))));
            ("update", pairs)
        }
        Commands::AddBook {
            file,
            empty,
            title,
            authors,
            tags,
            series,
            series_index,
            languages,
            isbn,
            duplicates,
            automerge,
        } => {
            let mut pairs = vec![("empty", json!(empty)), ("duplicates", json!(duplicates))];
            if let Some(file) = file {
                pairs.push(("file", json!(file.to_string_lossy())));
            }
            for (key, value) in [
                ("title", title),
                ("authors", authors),
                ("tags", tags),
                ("series", series),
                ("languages", languages),
                ("isbn", isbn),
                ("automerge", automerge),
            ] {
                if let Some(value) = value {
                    pairs.push((key, json!(value)));
                }
            }
            if let Some(index) = series_index {
                pairs.push(("series_index", json!(index)));
            }
            ("add_book", pairs)
        }
        Commands::RemoveBook { id, permanent } => (
            "remove_book",
            vec![("id", json!(id)), ("permanent", json!(permanent))],
        ),
        Commands::AddFormat { id, file } => (
            "add_format",
            vec![("id", json!(id)), ("file", json!(file.to_string_lossy()))],
        ),
        Commands::RemoveFormat { id, format } => (
            "remove_format",
            vec![("id", json!(id)), ("format", json!(format))],
        ),
        Commands::Rename { kind, old, new } => (
            "rename",
            vec![
                ("kind", json!(kind)),
                ("old", json!(old)),
                ("new", json!(new)),
            ],
        ),
        Commands::Duplicates { limit } => ("duplicates", vec![("limit", json!(limit))]),
        Commands::SeriesGaps => ("series_gaps", Vec::new()),
        Commands::Unread { limit } => ("unread", vec![("limit", json!(limit))]),
        Commands::ReadingStats => ("reading_stats", Vec::new()),
        Commands::Export { format, out, query } => {
            let mut pairs = vec![
                ("format", json!(format)),
                ("out", json!(out.to_string_lossy())),
            ];
            if let Some(query) = query {
                pairs.push(("query", json!(query)));
            }
            ("export", pairs)
        }
        Commands::Excerpt {
            id,
            format,
            chapter,
            chars,
            offset,
        } => {
            let mut pairs = vec![
                ("id", json!(id)),
                ("chapter", json!(chapter)),
                ("chars", json!(chars)),
                ("offset", json!(offset)),
            ];
            if let Some(format) = format {
                pairs.push(("format", json!(format)));
            }
            ("excerpt", pairs)
        }
        Commands::Open { id, format } => {
            let mut pairs = vec![("id", json!(id))];
            if let Some(format) = format {
                pairs.push(("format", json!(format)));
            }
            ("open", pairs)
        }
        Commands::Random {
            tag,
            author,
            format,
        } => {
            let mut pairs = Vec::new();
            for (key, value) in [("tag", tag), ("author", author), ("format", format)] {
                if let Some(value) = value {
                    pairs.push((key, json!(value)));
                }
            }
            ("random", pairs)
        }
        Commands::CollectionSave { name, query } => (
            "collection_save",
            vec![("name", json!(name)), ("query", json!(query))],
        ),
        Commands::CollectionList => ("collection_list", Vec::new()),
        Commands::CollectionRun { name, limit } => (
            "collection_run",
            vec![("name", json!(name)), ("limit", json!(limit))],
        ),
        Commands::CollectionDelete { name } => ("collection_delete", vec![("name", json!(name))]),
        Commands::BulkUpdate { ids, set } => (
            "bulk_update",
            vec![
                ("ids", json!(ids)),
                ("fields", Value::Object(parse_fields(set))),
            ],
        ),
        Commands::Convert {
            id,
            to,
            out_dir,
            from,
        } => {
            let mut pairs = vec![
                ("id", json!(id)),
                ("to", json!(to)),
                ("out_dir", json!(out_dir.to_string_lossy())),
            ];
            if let Some(from) = from {
                pairs.push(("from", json!(from)));
            }
            ("convert", pairs)
        }
        Commands::Ingest { html, out, title } => {
            let mut pairs = vec![
                ("html", json!(html.to_string_lossy())),
                ("out", json!(out.to_string_lossy())),
            ];
            if let Some(title) = title {
                pairs.push(("title", json!(title)));
            }
            ("ingest_html", pairs)
        }
        Commands::Tools => {
            let tools = tools_list();
            out!(
                "{}",
                serde_json::to_string_pretty(&tools).unwrap_or_else(|_| "[]".to_string())
            );
            return 0;
        }
        Commands::Mcp => {
            run_mcp_stdio(&args);
            return 0;
        }
    };

    for (key, value) in inserts {
        args.insert(key.to_string(), value);
    }

    match dispatch(op, &Value::Object(args)) {
        Ok(value) => {
            if cli.json {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                print_human(op, &value);
            }
            0
        }
        Err(error) => {
            if cli.json {
                out!("{}", error_payload(&error));
            } else {
                eprintln!("error: {error}");
            }
            1
        }
    }
}

fn main() {
    let cli = Cli::parse();
    std::process::exit(run(&cli));
}

fn schema(properties: Value, required: Vec<&str>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": true
    })
}

/// The MCP surface. Pure: no I/O, so it can be asserted in tests.
pub fn tools_list() -> Value {
    let library = || json!({"type": "string", "description": "Calibre library directory or metadata.db path (default: BOOKCRAFT_LIBRARY)"});
    let root = || json!({"type": "string", "description": "Confinement root for reads and writes (default: BOOKCRAFT_ROOT)"});
    json!([
        {
            "name": "book_status",
            "description": "Report BookCraft version, the ebook-convert engine, and optional library health.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_list",
            "description": "List books newest first.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 20},
                "offset": {"type": "integer", "minimum": 0, "default": 0}
            }), vec![])
        },
        {
            "name": "book_search",
            "description": "Substring search over title, author sort, authors, series, tags and descriptions.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 20}
            }), vec!["query"])
        },
        {
            "name": "book_info",
            "description": "Full metadata for one book: description, format files, cover path.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"}
            }), vec!["id"])
        },
        {
            "name": "book_grep",
            "description": "Search book text when Calibre's full-text-search.db exists, else titles and descriptions. Returns the engine used.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "pattern": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 20}
            }), vec!["pattern"])
        },
        {
            "name": "book_stats",
            "description": "Library-wide book/author/tag/series counts and format distribution.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_schema",
            "description": "Dump the Calibre schema (sqlite_master): tables, indexes and triggers.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_backup",
            "description": "Take a timestamped metadata backup outside the library. Writes take one automatically.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "force": {"type": "boolean", "description": "Write a fresh backup even when a recent one exists"}
            }), vec![])
        },
        {
            "name": "book_init",
            "description": "Create a fresh Calibre-shaped library directory. Refuses when one exists.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_update",
            "description": "Set metadata fields on one book via Calibre itself. Takes a backup first.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "fields": {"type": "object", "description": "title, authors, tags, series, series_index, publisher, rating, isbn, languages, comments, pubdate, identifiers"}
            }), vec!["id", "fields"])
        },
        {
            "name": "book_add",
            "description": "Add a book file (or an empty record with empty=true) via Calibre itself. Takes a backup first.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "file": {"type": "string"},
                "empty": {"type": "boolean"},
                "title": {"type": "string"},
                "authors": {"type": "string"},
                "tags": {"type": "string"},
                "series": {"type": "string"},
                "series_index": {"type": "number"},
                "languages": {"type": "string"},
                "isbn": {"type": "string"},
                "duplicates": {"type": "boolean"},
                "automerge": {"type": "string", "description": "new_record, overwrite or ignore"}
            }), vec![])
        },
        {
            "name": "book_remove",
            "description": "Remove one book via Calibre itself (Recycle Bin unless permanent=true). Takes a backup first.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "permanent": {"type": "boolean"}
            }), vec!["id"])
        },
        {
            "name": "book_add_format",
            "description": "Attach one format file to a book via Calibre itself (replaces the format when present).",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "file": {"type": "string"}
            }), vec!["id", "file"])
        },
        {
            "name": "book_remove_format",
            "description": "Drop one format from a book via Calibre itself. A no-op when absent.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "format": {"type": "string"}
            }), vec!["id", "format"])
        },
        {
            "name": "book_rename",
            "description": "Rename a tag, author, series or publisher. Merges when the new name exists.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "kind": {"type": "string", "description": "tag, author, series or publisher"},
                "old": {"type": "string"},
                "new": {"type": "string"}
            }), vec!["kind", "old", "new"])
        },
        {
            "name": "book_duplicates",
            "description": "Books sharing a normalised title: duplicate candidates.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 50}
            }), vec![])
        },
        {
            "name": "book_series_gaps",
            "description": "Series with missing volume numbers.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_unread",
            "description": "Books never opened (or everything when the library has no tracking table).",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50}
            }), vec![])
        },
        {
            "name": "book_reading_stats",
            "description": "Totals, read/unread split, rating histogram, additions by year, top tags/authors.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_export",
            "description": "Export the library (or one search) as CSV or JSON to out.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "format": {"type": "string", "description": "csv or json"},
                "out": {"type": "string"},
                "query": {"type": "string"}
            }), vec!["format", "out"])
        },
        {
            "name": "book_excerpt",
            "description": "Paginated chapter text from an EPUB (or a raw window from a TXT).",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "format": {"type": "string"},
                "chapter": {"type": "integer", "minimum": 0, "default": 0},
                "chars": {"type": "integer", "minimum": 100, "maximum": 20000, "default": 2000},
                "offset": {"type": "integer", "minimum": 0, "default": 0}
            }), vec!["id"])
        },
        {
            "name": "book_open",
            "description": "Open the best file with the system default application (desktop affordance).",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "format": {"type": "string"}
            }), vec!["id"])
        },
        {
            "name": "book_random",
            "description": "A random book, optionally filtered by tag, author or format.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "tag": {"type": "string"},
                "author": {"type": "string"},
                "format": {"type": "string"}
            }), vec![])
        },
        {
            "name": "book_collection_save",
            "description": "Save a named search (a smart collection).",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "name": {"type": "string"},
                "query": {"type": "string"}
            }), vec!["name", "query"])
        },
        {
            "name": "book_collection_list",
            "description": "List saved searches.",
            "inputSchema": schema(json!({"library": library(), "root": root()}), vec![])
        },
        {
            "name": "book_collection_run",
            "description": "Run a saved search.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "name": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 20}
            }), vec!["name"])
        },
        {
            "name": "book_collection_delete",
            "description": "Delete a saved search.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "name": {"type": "string"}
            }), vec!["name"])
        },
        {
            "name": "book_bulk_update",
            "description": "Update the same fields on many books, all-or-nothing in one transaction.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "ids": {"description": "Book ids: array or comma-separated string"},
                "fields": {"type": "object"}
            }), vec!["ids", "fields"])
        },
        {
            "name": "book_convert",
            "description": "Convert one book with ebook-convert into out_dir. Never overwrites an existing file.",
            "inputSchema": schema(json!({
                "library": library(), "root": root(),
                "id": {"type": "integer"},
                "to": {"type": "string", "description": "Target format, e.g. pdf or epub"},
                "out_dir": {"type": "string"},
                "from": {"type": "string", "description": "Source format; default is the best available"}
            }), vec!["id", "to", "out_dir"])
        },
        {
            "name": "book_ingest",
            "description": "Turn a saved HTML page into a Markdown artifact (Readly-style ingest without a browser).",
            "inputSchema": schema(json!({
                "root": root(),
                "html": {"type": "string"},
                "out": {"type": "string"},
                "title": {"type": "string"}
            }), vec!["html", "out"])
        }
    ])
}

/// Map one tool name + arguments onto the shared dispatch bus.
///
/// Accepts both the strict-MCP underscore name and the dotted ArtCraft id.
pub fn call_tool(name: &str, args: &Value) -> Value {
    let op = match name {
        "book_status" | "book.status" => "status",
        "book_list" | "book.list" => "list",
        "book_search" | "book.search" => "search",
        "book_info" | "book.info" => "info",
        "book_grep" | "book.grep" => "grep",
        "book_stats" | "book.stats" => "stats",
        "book_schema" | "book.schema" => "schema",
        "book_backup" | "book.backup" => "backup",
        "book_init" | "book.init" => "init",
        "book_update" | "book.update" => "update",
        "book_add" | "book.add" | "book_add_book" => "add_book",
        "book_remove" | "book.remove" | "book_remove_book" => "remove_book",
        "book_add_format" | "book.add_format" => "add_format",
        "book_remove_format" | "book.remove_format" => "remove_format",
        "book_rename" | "book.rename" => "rename",
        "book_duplicates" | "book.duplicates" => "duplicates",
        "book_series_gaps" | "book.series_gaps" => "series_gaps",
        "book_unread" | "book.unread" => "unread",
        "book_reading_stats" | "book.reading_stats" => "reading_stats",
        "book_export" | "book.export" => "export",
        "book_excerpt" | "book.excerpt" => "excerpt",
        "book_open" | "book.open" => "open",
        "book_random" | "book.random" => "random",
        "book_collection_save" | "book.collection_save" => "collection_save",
        "book_collection_list" | "book.collection_list" => "collection_list",
        "book_collection_run" | "book.collection_run" => "collection_run",
        "book_collection_delete" | "book.collection_delete" => "collection_delete",
        "book_bulk_update" | "book.bulk_update" => "bulk_update",
        "book_convert" | "book.convert" => "convert",
        "book_ingest" | "book.ingest" | "book_ingest_html" => "ingest_html",
        other => {
            return json!({
                "success": false,
                "error": format!("unknown tool '{other}'"),
                "error_type": "invalid"
            })
        }
    };

    match dispatch(op, args) {
        Ok(value) => value,
        Err(error) => error_payload(&error),
    }
}

fn merge_defaults(defaults: &Map<String, Value>, arguments: Value) -> Value {
    let mut merged = defaults.clone();
    if let Some(obj) = arguments.as_object() {
        for (key, value) in obj {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

/// Hand-rolled MCP 2024-11-05 line-delimited stdio loop.
fn run_mcp_stdio(defaults: &Map<String, Value>) {
    eprintln!("[bookcraft-mcp] starting MCP stdio loop (2024-11-05)");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let text = match line {
            Ok(t) => t,
            Err(_) => break,
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(error) => {
                eprintln!("[bookcraft-mcp] JSON parse error: {error}");
                continue;
            }
        };

        let method = request["method"].as_str().unwrap_or("");
        let id = request.get("id").cloned();

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": APP_NAME, "version": APP_VERSION }
                }
            }),
            "notifications/initialized" | "notifications/cancelled" => continue,
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": tools_list() }
            }),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                let tool_name = params["name"].as_str().unwrap_or("");
                let arguments = merge_defaults(
                    defaults,
                    params.get("arguments").cloned().unwrap_or(json!({})),
                );
                let result = call_tool(tool_name, &arguments);
                let is_error = result
                    .get("success")
                    .and_then(Value::as_bool)
                    .map(|ok| !ok)
                    .unwrap_or(true);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": result.to_string() }],
                        "isError": is_error
                    }
                })
            }
            _ => {
                if id.is_some() {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("method not found: {method}") }
                    })
                } else {
                    continue;
                }
            }
        };

        if writeln!(stdout, "{response}").is_err() {
            break;
        }
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_surface_is_complete_and_valid() {
        let tools = tools_list();
        let tools = tools.as_array().cloned().unwrap_or_default();
        assert_eq!(tools.len(), 30);
        for tool in &tools {
            let name = tool["name"].as_str().unwrap_or("");
            assert!(name.starts_with("book_"), "{name}");
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn unknown_tool_is_an_error_payload() {
        let result = call_tool("book_explode", &json!({}));
        assert_eq!(result["success"], false);
        assert_eq!(result["error_type"], "invalid");
    }

    #[test]
    fn dotted_and_underscore_names_dispatch_identically() {
        let dotted = call_tool("book.status", &json!({}));
        let underscore = call_tool("book_status", &json!({}));
        assert_eq!(dotted["success"], true);
        assert_eq!(underscore["success"], true);
        assert_eq!(dotted["version"], underscore["version"]);
    }

    #[test]
    fn defaults_are_overridden_by_call_arguments() {
        let mut defaults = Map::new();
        defaults.insert("library".into(), json!("D:/default"));
        let merged = merge_defaults(&defaults, json!({"library": "D:/explicit", "limit": 5}));
        assert_eq!(merged["library"], "D:/explicit");
        assert_eq!(merged["limit"], 5);
    }

    #[test]
    fn missing_library_surfaces_as_not_success() {
        let result = call_tool("book_list", &json!({}));
        // BOOKCRAFT_LIBRARY may be set on a developer machine; only the failure shape is asserted.
        if std::env::var_os("BOOKCRAFT_LIBRARY").is_none() {
            assert_eq!(result["success"], false);
            assert_eq!(result["error_type"], "invalid");
        }
    }
}
