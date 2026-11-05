//! L3 BookCraft desktop shelf (eframe/egui).
//!
//! The GUI calls `bookcraft_core::dispatch` in-process: the same bus the CLI and the MCP
//! server use, so all three surfaces agree by construction. `--smoke` runs the headless
//! parity checks without opening a window.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use bookcraft_core::dispatch;
use eframe::egui;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SMOKE_TITLE: &str = "The Lantern Issue";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Status,
    Library,
    Search,
    Grep,
    Stats,
    Convert,
    Ingest,
    Manage,
}

impl View {
    fn all() -> [View; 8] {
        [
            View::Status,
            View::Library,
            View::Search,
            View::Grep,
            View::Stats,
            View::Convert,
            View::Ingest,
            View::Manage,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            View::Status => "Status",
            View::Library => "Library",
            View::Search => "Search",
            View::Grep => "Grep",
            View::Stats => "Stats",
            View::Convert => "Convert",
            View::Ingest => "Ingest",
            View::Manage => "Manage",
        }
    }
}

/// The whole GUI state. Every action flows through [`BookApp::dispatch`], which is the
/// same `bookcraft_core::dispatch` the CLI and MCP surfaces use.
pub struct BookApp {
    library: String,
    view: View,
    status: Option<Value>,
    stats: Option<Value>,
    books: Vec<Value>,
    detail: Option<Value>,
    selected: Option<i64>,
    search_query: String,
    search_limit: String,
    grep_pattern: String,
    grep_result: Option<Value>,
    convert_id: String,
    convert_to: String,
    convert_from: String,
    convert_out: String,
    convert_result: Option<Value>,
    ingest_html: String,
    ingest_out: String,
    ingest_title: String,
    ingest_result: Option<Value>,
    manage_id: String,
    manage_title: String,
    manage_authors: String,
    manage_tags: String,
    manage_series: String,
    manage_publisher: String,
    manage_rating: String,
    manage_comment: String,
    manage_pubdate: String,
    manage_result: Option<Value>,
    add_file: String,
    add_title: String,
    add_authors: String,
    add_result: Option<Value>,
    remove_id: String,
    remove_permanent: bool,
    remove_result: Option<Value>,
    backup_result: Option<Value>,
    notice: String,
}

impl Default for BookApp {
    fn default() -> Self {
        Self {
            library: std::env::var("BOOKCRAFT_LIBRARY").unwrap_or_default(),
            view: View::Status,
            status: None,
            stats: None,
            books: Vec::new(),
            detail: None,
            selected: None,
            search_query: String::new(),
            search_limit: "20".to_string(),
            grep_pattern: String::new(),
            grep_result: None,
            convert_id: String::new(),
            convert_to: "pdf".to_string(),
            convert_from: String::new(),
            convert_out: String::new(),
            convert_result: None,
            ingest_html: String::new(),
            ingest_out: String::new(),
            ingest_title: String::new(),
            ingest_result: None,
            manage_id: String::new(),
            manage_title: String::new(),
            manage_authors: String::new(),
            manage_tags: String::new(),
            manage_series: String::new(),
            manage_publisher: String::new(),
            manage_rating: String::new(),
            manage_comment: String::new(),
            manage_pubdate: String::new(),
            manage_result: None,
            add_file: String::new(),
            add_title: String::new(),
            add_authors: String::new(),
            add_result: None,
            remove_id: String::new(),
            remove_permanent: false,
            remove_result: None,
            backup_result: None,
            notice: String::new(),
        }
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn joined(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn count(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_i64)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".to_string())
}

impl BookApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// The GUI entry to the shared bus: injects the configured library, then dispatches.
    /// `dispatch` never panics, so the GUI cannot crash on a bad library path.
    pub fn dispatch(&self, op: &str, args: Value) -> Value {
        let mut with_library = args;
        if let Some(obj) = with_library.as_object_mut() {
            if !self.library.trim().is_empty() && !obj.contains_key("library") {
                obj.insert(
                    "library".to_string(),
                    Value::String(self.library.trim().to_string()),
                );
            }
        }
        match dispatch(op, &with_library) {
            Ok(value) => value,
            Err(error) => json!({"success": false, "error": error.to_string()}),
        }
    }

    fn refresh(&mut self) {
        self.status = Some(self.dispatch("status", json!({})));
        self.stats = Some(self.dispatch("stats", json!({})));
        self.notice.clear();
    }

    fn load_list(&mut self) {
        let result = self.dispatch("list", json!({"limit": 100}));
        self.books = result
            .get("books")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if result.get("success") != Some(&Value::Bool(true)) {
            self.notice = format!("list failed: {}", text(&result, "error"));
        } else {
            self.notice.clear();
        }
    }

    fn show_detail(&mut self, id: i64) {
        let result = self.dispatch("info", json!({"id": id}));
        if result.get("success") == Some(&Value::Bool(true)) {
            self.detail = result.get("book").cloned();
            self.notice.clear();
        } else {
            self.notice = format!("info failed: {}", text(&result, "error"));
        }
    }

    fn show_manage(&mut self, ui: &mut egui::Ui) {
        ui.heading("Update one book");
        ui.horizontal(|ui| {
            ui.label("id:");
            ui.text_edit_singleline(&mut self.manage_id);
        });
        for (label, field) in [
            ("title", &mut self.manage_title),
            ("authors (comma separated)", &mut self.manage_authors),
            ("tags (comma separated)", &mut self.manage_tags),
            ("series", &mut self.manage_series),
            ("publisher", &mut self.manage_publisher),
            ("rating 0-5", &mut self.manage_rating),
            ("pubdate YYYY-MM-DD", &mut self.manage_pubdate),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.text_edit_singleline(field);
            });
        }
        ui.horizontal(|ui| {
            ui.label("description:");
            ui.text_edit_multiline(&mut self.manage_comment);
        });
        if ui.button("Update").clicked() {
            self.notice.clear();
            let id: i64 = self.manage_id.trim().parse().unwrap_or(-1);
            let rating = match self.manage_rating.trim() {
                "" => None,
                raw => match raw.parse::<i64>() {
                    Ok(n) => Some(json!(n)),
                    Err(_) => {
                        self.notice = "rating must be an integer 0-5".to_string();
                        None
                    }
                },
            };
            if self.notice.is_empty() {
                let mut fields = serde_json::Map::new();
                for (key, value) in [
                    ("title", self.manage_title.trim().to_string()),
                    ("authors", self.manage_authors.trim().to_string()),
                    ("tags", self.manage_tags.trim().to_string()),
                    ("series", self.manage_series.trim().to_string()),
                    ("publisher", self.manage_publisher.trim().to_string()),
                    ("pubdate", self.manage_pubdate.trim().to_string()),
                    ("comments", self.manage_comment.trim().to_string()),
                ] {
                    if !value.is_empty() {
                        fields.insert(key.to_string(), Value::String(value));
                    }
                }
                if let Some(rating) = rating {
                    fields.insert("rating".to_string(), rating);
                }
                self.manage_result =
                    Some(self.dispatch("update", json!({"id": id, "fields": fields})));
            }
        }
        show_result(ui, &self.manage_result);

        ui.separator();
        ui.heading("Add a book");
        ui.horizontal(|ui| {
            ui.label("file:");
            ui.text_edit_singleline(&mut self.add_file);
        });
        ui.horizontal(|ui| {
            ui.label("title:");
            ui.text_edit_singleline(&mut self.add_title);
            ui.label("authors:");
            ui.text_edit_singleline(&mut self.add_authors);
        });
        ui.horizontal(|ui| {
            if ui.button("Add file").clicked() {
                let mut args = json!({"file": self.add_file.trim()});
                if !self.add_title.trim().is_empty() {
                    args["title"] = Value::String(self.add_title.trim().to_string());
                }
                if !self.add_authors.trim().is_empty() {
                    args["authors"] = Value::String(self.add_authors.trim().to_string());
                }
                self.add_result = Some(self.dispatch("add_book", args));
            }
            if ui.button("Add empty record").clicked() {
                self.add_result = Some(self.dispatch(
                    "add_book",
                    json!({
                        "empty": true,
                        "title": self.add_title.trim(),
                        "authors": self.add_authors.trim(),
                    }),
                ));
            }
        });
        show_result(ui, &self.add_result);

        ui.separator();
        ui.heading("Remove a book");
        ui.horizontal(|ui| {
            ui.label("id:");
            ui.text_edit_singleline(&mut self.remove_id);
            ui.checkbox(&mut self.remove_permanent, "permanent (skip Recycle Bin)");
            if ui.button("Remove").clicked() {
                let id: i64 = self.remove_id.trim().parse().unwrap_or(-1);
                self.remove_result = Some(self.dispatch(
                    "remove_book",
                    json!({"id": id, "permanent": self.remove_permanent}),
                ));
            }
        });
        show_result(ui, &self.remove_result);

        ui.separator();
        ui.heading("Backup");
        if ui.button("Backup now").clicked() {
            self.backup_result = Some(self.dispatch("backup", json!({"force": true})));
        }
        show_result(ui, &self.backup_result);
    }

    fn show_status(&mut self, ui: &mut egui::Ui) {
        ui.heading("Engine & library");
        if ui.button("Refresh status").clicked() {
            self.refresh();
        }
        let Some(status) = self.status.clone() else {
            ui.label("Press Refresh to probe the engine and the library.");
            return;
        };
        let engine = status.get("ebook_convert").cloned().unwrap_or(Value::Null);
        ui.monospace(format!(
            "ebook-convert: {}{}",
            text(&engine, "status"),
            text(&engine, "path")
        ));
        let library = status.get("library").cloned().unwrap_or(Value::Null);
        if library.is_null() {
            ui.label("No library configured: type a Calibre directory or metadata.db above.");
        } else {
            ui.monospace(format!(
                "library: {} ({} books)",
                text(&library, "root"),
                count(&library, "books")
            ));
            if text(&library, "status") == "error" {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("library error: {}", text(&library, "error")),
                );
            }
        }
    }

    fn show_library(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("book-list").show(ui, |ui| {
            ui.heading("Books");
            if ui.button("Load list").clicked() {
                self.load_list();
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut pick: Option<i64> = None;
                for book in &self.books {
                    let id = book.get("id").and_then(Value::as_i64).unwrap_or(0);
                    let label = format!("#{id} {}", text(book, "title"));
                    if ui
                        .selectable_value(&mut self.selected, Some(id), label)
                        .clicked()
                    {
                        pick = Some(id);
                    }
                }
                if let Some(id) = pick {
                    self.show_detail(id);
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(book) = self.detail.clone() else {
                ui.label("Pick a book on the left.");
                return;
            };
            ui.heading(text(&book, "title"));
            ui.label(format!("authors: {}", joined(&book, "authors")));
            if !text(&book, "series").is_empty() {
                ui.label(format!("series: {}", text(&book, "series")));
            }
            ui.label(format!("formats: {}", joined(&book, "formats")));
            if !text(&book, "cover_path").is_empty() {
                ui.monospace(format!("cover: {}", text(&book, "cover_path")));
            }
            ui.separator();
            ui.label(text(&book, "description"));
            ui.separator();
            ui.heading("Format files");
            for file in book
                .get("format_files")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let exists = file.get("exists").and_then(Value::as_bool).unwrap_or(false);
                let mark = if exists { "ok  " } else { "miss" };
                ui.monospace(format!(
                    "{mark} {} ({} bytes)",
                    text(&file, "file"),
                    file.get("size_bytes")
                        .and_then(Value::as_i64)
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ));
            }
        });
    }

    fn show_search(&mut self, ui: &mut egui::Ui) {
        ui.heading("Search the library");
        ui.horizontal(|ui| {
            ui.label("query:");
            ui.text_edit_singleline(&mut self.search_query);
            ui.label("limit:");
            ui.text_edit_singleline(&mut self.search_limit);
            if ui.button("Search").clicked() {
                let limit: i64 = self.search_limit.trim().parse().unwrap_or(20);
                let result = self.dispatch(
                    "search",
                    json!({"query": self.search_query.trim(), "limit": limit}),
                );
                self.books = result
                    .get("books")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if result.get("success") != Some(&Value::Bool(true)) {
                    self.notice = format!("search failed: {}", text(&result, "error"));
                } else {
                    self.notice.clear();
                }
            }
        });
        ui.separator();
        let mut pick: Option<i64> = None;
        for book in self.books.clone() {
            let id = book.get("id").and_then(Value::as_i64).unwrap_or(0);
            if ui
                .selectable_value(
                    &mut self.selected,
                    Some(id),
                    format!(
                        "#{id} {} — {}",
                        text(&book, "title"),
                        joined(&book, "authors")
                    ),
                )
                .clicked()
            {
                pick = Some(id);
            }
        }
        if let Some(id) = pick {
            self.show_detail(id);
            self.view = View::Library;
        }
    }

    fn show_grep(&mut self, ui: &mut egui::Ui) {
        ui.heading("Search book text");
        ui.horizontal(|ui| {
            ui.label("pattern:");
            ui.text_edit_singleline(&mut self.grep_pattern);
            if ui.button("Grep").clicked() {
                self.grep_result = Some(self.dispatch(
                    "grep",
                    json!({"pattern": self.grep_pattern.trim(), "limit": 20}),
                ));
                self.notice.clear();
            }
        });
        let Some(result) = self.grep_result.clone() else {
            ui.label(
                "Reports which engine answered: the full-text index or the metadata fallback.",
            );
            return;
        };
        if result.get("success") != Some(&Value::Bool(true)) {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("grep failed: {}", text(&result, "error")),
            );
            return;
        }
        ui.monospace(format!(
            "engine: {} ({} hit(s))",
            text(&result, "engine"),
            count(&result, "count")
        ));
        egui::ScrollArea::vertical().show(ui, |ui| {
            for hit in result
                .get("hits")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                ui.heading(format!("#{} {}", hit["book_id"], text(&hit, "title")));
                ui.label(text(&hit, "snippet"));
                ui.separator();
            }
        });
    }

    fn show_stats(&mut self, ui: &mut egui::Ui) {
        ui.heading("Library statistics");
        if ui.button("Refresh stats").clicked() {
            self.stats = Some(self.dispatch("stats", json!({})));
        }
        let Some(stats) = self.stats.clone() else {
            ui.label("Press Refresh stats.");
            return;
        };
        for key in [
            "books",
            "authors",
            "tags",
            "series",
            "publishers",
            "books_with_cover",
            "books_with_description",
        ] {
            ui.monospace(format!("{key}: {}", count(&stats, key)));
        }
        ui.separator();
        ui.heading("Formats");
        for format in stats
            .get("formats")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            ui.monospace(format!(
                "{}: {}",
                text(&format, "format"),
                count(&format, "count")
            ));
        }
    }

    fn show_convert(&mut self, ui: &mut egui::Ui) {
        ui.heading("Convert one book");
        ui.horizontal(|ui| {
            ui.label("id:");
            ui.text_edit_singleline(&mut self.convert_id);
            ui.label("to:");
            ui.text_edit_singleline(&mut self.convert_to);
            ui.label("from:");
            ui.text_edit_singleline(&mut self.convert_from);
        });
        ui.horizontal(|ui| {
            ui.label("out dir:");
            ui.text_edit_singleline(&mut self.convert_out);
            if ui.button("Convert").clicked() {
                let id: i64 = self.convert_id.trim().parse().unwrap_or(-1);
                let mut args = json!({
                    "id": id,
                    "to": self.convert_to.trim(),
                    "out_dir": self.convert_out.trim(),
                });
                if !self.convert_from.trim().is_empty() {
                    args["from"] = Value::String(self.convert_from.trim().to_string());
                }
                self.convert_result = Some(self.dispatch("convert", args));
            }
        });
        if let Some(result) = self.convert_result.clone() {
            if result.get("success") == Some(&Value::Bool(true)) {
                ui.monospace(format!(
                    "{} -> {} ({} bytes)",
                    text(&result, "from"),
                    text(&result, "output"),
                    count(&result, "bytes")
                ));
            } else {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("convert failed: {}", text(&result, "error")),
                );
            }
        }
    }

    fn show_ingest(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ingest a saved HTML page");
        ui.horizontal(|ui| {
            ui.label("html:");
            ui.text_edit_singleline(&mut self.ingest_html);
        });
        ui.horizontal(|ui| {
            ui.label("out:");
            ui.text_edit_singleline(&mut self.ingest_out);
        });
        ui.horizontal(|ui| {
            ui.label("title:");
            ui.text_edit_singleline(&mut self.ingest_title);
            if ui.button("Ingest").clicked() {
                let mut args = json!({
                    "html": self.ingest_html.trim(),
                    "out": self.ingest_out.trim(),
                });
                if !self.ingest_title.trim().is_empty() {
                    args["title"] = Value::String(self.ingest_title.trim().to_string());
                }
                self.ingest_result = Some(self.dispatch("ingest_html", args));
            }
        });
        if let Some(result) = self.ingest_result.clone() {
            if result.get("success") == Some(&Value::Bool(true)) {
                ui.monospace(format!(
                    "{} -> {} ({} words)",
                    text(&result, "title"),
                    text(&result, "output"),
                    count(&result, "words")
                ));
            } else {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("ingest failed: {}", text(&result, "error")),
                );
            }
        }
    }
}

fn show_result(ui: &mut egui::Ui, result: &Option<Value>) {
    let Some(result) = result else {
        return;
    };
    if result.get("success") == Some(&Value::Bool(true)) {
        let mut line = String::from("ok");
        for key in ["book_id", "title", "file", "output", "backup"] {
            if let Some(text) = result.get(key).and_then(value_text) {
                line.push_str(&format!(" | {key}: {text}"));
            }
        }
        ui.monospace(line);
    } else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            format!("failed: {}", text(result, "error")),
        );
    }
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

impl eframe::App for BookApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("bookcraft-top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("BookCraft");
                ui.label("library:");
                ui.text_edit_singleline(&mut self.library);
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }
            });
            ui.horizontal(|ui| {
                for view in View::all() {
                    ui.selectable_value(&mut self.view, view, view.label());
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let view = self.view;
            match view {
                View::Status => self.show_status(ui),
                View::Library => self.show_library(ui),
                View::Search => self.show_search(ui),
                View::Grep => self.show_grep(ui),
                View::Stats => self.show_stats(ui),
                View::Convert => self.show_convert(ui),
                View::Ingest => self.show_ingest(ui),
                View::Manage => self.show_manage(ui),
            }
            if !self.notice.is_empty() {
                ui.separator();
                ui.colored_label(egui::Color32::LIGHT_RED, &self.notice);
            }
        });
    }
}

/// Deterministic fixture library for `--smoke` and unit tests: two books, comments and a
/// full-text index. No format files and no network.
pub fn fixture_library(dir: &Path) -> Result<PathBuf, String> {
    use rusqlite::Connection;

    let library = dir.join("library");
    std::fs::create_dir_all(&library).map_err(|e| e.to_string())?;
    let plain = Connection::open(library.join("metadata.db")).map_err(|e| e.to_string())?;
    plain
        .execute_batch(bookcraft_core::write::CALIBRE_WRITE_SCHEMA)
        .map_err(|e| e.to_string())?;
    drop(plain);
    let lib = bookcraft_core::library::Library::open(&library).map_err(|e| e.to_string())?;
    let conn = bookcraft_core::write::open_writable(&lib).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "INSERT INTO books(title, author_sort, path) VALUES
             ('Dune','Herbert, Frank','Frank Herbert/Dune (1)'),
             ('Emma','Austen, Jane','Jane Austen/Emma (2)');
         INSERT INTO authors(name, sort, link) VALUES
             ('Frank Herbert','Herbert, Frank',''),
             ('Jane Austen','Austen, Jane','');
         INSERT INTO books_authors_link(book, author) VALUES (1,1),(2,2);
         INSERT INTO comments(book, text) VALUES
             (1,'<p>The desert planet <b>Arrakis</b>.</p>');",
    )
    .map_err(|e| e.to_string())?;
    drop(conn);

    let fts = Connection::open(library.join("full-text-search.db")).map_err(|e| e.to_string())?;
    fts.execute_batch(
        "CREATE TABLE books_text (book INTEGER, format TEXT, searchable_text TEXT);
         INSERT INTO books_text VALUES
             (1,'EPUB','A lantern burns in the stilltent while the blacksmith waits.');",
    )
    .map_err(|e| e.to_string())?;
    Ok(library)
}

fn check(op: &str, via_app: &Value, via_core: &Value) -> Result<String, String> {
    if via_app != via_core {
        return Err(format!("GUI/core mismatch on '{op}'"));
    }
    if via_app.get("success") != Some(&Value::Bool(true)) {
        return Err(format!("'{op}' unsuccessful: {via_app}"));
    }
    Ok(format!("{op}: ok"))
}

/// Headless parity contract: the GUI path and the core bus must return identical
/// successful payloads for every read verb plus ingest.
pub fn run_smoke_checks() -> Result<Vec<String>, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let html = tmp.path().join("issue.html");
    let out = tmp.path().join("issue.md");
    std::fs::write(
        &html,
        "<html><head><title>The Lantern Issue</title></head>\
         <body><p>A lantern burns in the stilltent.</p>\
         <script>window.x = 'should never appear';</script></body></html>",
    )
    .map_err(|e| e.to_string())?;

    let library = fixture_library(tmp.path())?;
    let book = BookApp {
        library: library.to_string_lossy().into_owned(),
        ..BookApp::default()
    };

    let cases: Vec<(&str, Value)> = vec![
        ("status", json!({})),
        ("list", json!({"limit": 10})),
        ("search", json!({"query": "dune"})),
        ("info", json!({"id": 1})),
        ("grep", json!({"pattern": "lantern"})),
        ("stats", json!({})),
        (
            "ingest_html",
            json!({
                "html": html.to_string_lossy(),
                "out": out.to_string_lossy(),
                "title": SMOKE_TITLE,
            }),
        ),
    ];

    let mut log = Vec::new();
    for (op, args) in cases {
        let via_app = book.dispatch(op, args.clone());
        let mut core_args = args.clone();
        if let Some(obj) = core_args.as_object_mut() {
            obj.insert(
                "library".to_string(),
                Value::String(library.to_string_lossy().into_owned()),
            );
        }
        let via_core = dispatch(op, &core_args).map_err(|e| e.to_string())?;
        log.push(check(op, &via_app, &via_core)?);
    }

    let markdown = std::fs::read_to_string(&out).map_err(|e| e.to_string())?;
    if !markdown.contains(SMOKE_TITLE) || markdown.contains("should never appear") {
        return Err("ingest artifact failed content checks".to_string());
    }
    log.push("ingest artifact content: ok".to_string());

    // Manage view path on the same fixture: add, update, verify, remove.
    let added = book.dispatch(
        "add_book",
        json!({"empty": true, "title": "Smoke", "authors": "Tester"}),
    );
    if added.get("success") != Some(&Value::Bool(true)) {
        return Err(format!("smoke add_book unsuccessful: {added}"));
    }
    let smoke_id = added.get("book_id").and_then(Value::as_i64).unwrap_or(-1);
    log.push("smoke add: ok".to_string());

    let updated = book.dispatch(
        "update",
        json!({"id": smoke_id, "fields": {"title": "Smoke Revised", "rating": 5}}),
    );
    if updated.get("success") != Some(&Value::Bool(true)) {
        return Err(format!("smoke update unsuccessful: {updated}"));
    }
    let detail = book.dispatch("info", json!({"id": smoke_id}));
    if detail["book"]["title"] != "Smoke Revised" || detail["book"]["rating"] != 5 {
        return Err(format!("smoke read-back mismatch: {detail}"));
    }
    log.push("smoke update + read-back: ok".to_string());

    let removed = book.dispatch("remove_book", json!({"id": smoke_id, "permanent": true}));
    if removed.get("success") != Some(&Value::Bool(true)) {
        return Err(format!("smoke remove unsuccessful: {removed}"));
    }
    let gone = book.dispatch("info", json!({"id": smoke_id}));
    if gone.get("success") != Some(&Value::Bool(false)) {
        return Err("smoke remove did not delete the record".to_string());
    }
    log.push("smoke remove: ok".to_string());
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke_book() -> Result<(tempfile::TempDir, BookApp), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let library = fixture_library(tmp.path()).map_err(std::io::Error::other)?;
        let book = BookApp {
            library: library.to_string_lossy().into_owned(),
            ..BookApp::default()
        };
        Ok((tmp, book))
    }

    #[test]
    fn app_dispatch_matches_core_dispatch() -> Result<(), Box<dyn std::error::Error>> {
        let (_tmp, book) = smoke_book()?;
        let cases: Vec<(&str, Value)> = vec![
            ("status", json!({})),
            ("list", json!({"limit": 10})),
            ("search", json!({"query": "emma"})),
            ("info", json!({"id": 2})),
            ("grep", json!({"pattern": "lantern"})),
            ("stats", json!({})),
        ];
        for (op, args) in cases {
            let via_app = book.dispatch(op, args.clone());
            let mut core_args = args;
            core_args
                .as_object_mut()
                .ok_or("args must be an object")?
                .insert("library".to_string(), Value::String(book.library.clone()));
            let via_core = dispatch(op, &core_args).map_err(|e| e.to_string())?;
            assert_eq!(via_app, via_core, "GUI/core mismatch on '{op}'");
            assert_eq!(via_app["success"], true);
        }
        Ok(())
    }

    #[test]
    fn missing_library_is_a_payload_not_a_crash() -> Result<(), Box<dyn std::error::Error>> {
        let book = BookApp::default();
        let result = book.dispatch("list", json!({}));
        assert_eq!(result["success"], false);
        Ok(())
    }

    #[test]
    fn ingest_rejects_script_content() -> Result<(), Box<dyn std::error::Error>> {
        let (tmp, book) = smoke_book()?;
        let html = tmp.path().join("page.html");
        let out = tmp.path().join("page.md");
        std::fs::write(
            &html,
            "<html><body><p>visible</p><script>hidden()</script></body></html>",
        )?;
        let result = book.dispatch(
            "ingest_html",
            json!({"html": html.to_string_lossy(), "out": out.to_string_lossy()}),
        );
        assert_eq!(result["success"], true);
        let markdown = std::fs::read_to_string(&out)?;
        assert!(markdown.contains("visible"));
        assert!(!markdown.contains("hidden()"));
        Ok(())
    }
}
