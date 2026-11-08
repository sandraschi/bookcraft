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
    Catalog,
    Share,
    Read,
}

impl View {
    fn all() -> [View; 11] {
        [
            View::Status,
            View::Library,
            View::Search,
            View::Grep,
            View::Stats,
            View::Convert,
            View::Ingest,
            View::Manage,
            View::Catalog,
            View::Share,
            View::Read,
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
            View::Catalog => "Catalog",
            View::Share => "Share",
            View::Read => "Read",
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
    bulk_ids: String,
    bulk_result: Option<Value>,
    rename_kind: String,
    rename_old: String,
    rename_new: String,
    rename_result: Option<Value>,
    catalog_result: Option<Value>,
    coll_name: String,
    coll_query: String,
    coll_result: Option<Value>,
    coll_list: Option<Value>,
    export_format: String,
    export_out: String,
    export_query: String,
    export_result: Option<Value>,
    excerpt_id: String,
    excerpt_format: String,
    excerpt_chapter: String,
    excerpt_chars: String,
    excerpt_result: Option<Value>,
    open_id: String,
    open_format: String,
    open_result: Option<Value>,
    random_tag: String,
    random_author: String,
    random_format: String,
    random_result: Option<Value>,
    textures: std::collections::HashMap<i64, egui::TextureHandle>,
    marked: std::collections::BTreeSet<i64>,
    read_id: String,
    read_chapter: usize,
    read_result: Option<Value>,
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
            bulk_ids: String::new(),
            bulk_result: None,
            rename_kind: "tag".to_string(),
            rename_old: String::new(),
            rename_new: String::new(),
            rename_result: None,
            catalog_result: None,
            coll_name: String::new(),
            coll_query: String::new(),
            coll_result: None,
            coll_list: None,
            export_format: "csv".to_string(),
            export_out: String::new(),
            export_query: String::new(),
            export_result: None,
            excerpt_id: String::new(),
            excerpt_format: String::new(),
            excerpt_chapter: "0".to_string(),
            excerpt_chars: "2000".to_string(),
            excerpt_result: None,
            open_id: String::new(),
            open_format: String::new(),
            open_result: None,
            random_tag: String::new(),
            random_author: String::new(),
            random_format: String::new(),
            random_result: None,
            textures: std::collections::HashMap::new(),
            marked: std::collections::BTreeSet::new(),
            read_id: String::new(),
            read_chapter: 0,
            read_result: None,
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Warm paper-and-amber theme: a library should feel like one.
        // egui 0.37 keeps one style per theme; restyle the active one.
        let theme = cc.egui_ctx.theme();
        let mut style = (*cc.egui_ctx.style_of(theme)).clone();
        let paper = egui::Color32::from_rgb(26, 21, 16);
        let shelf = egui::Color32::from_rgb(34, 28, 21);
        let amber = egui::Color32::from_rgb(216, 162, 64);
        let parchment = egui::Color32::from_rgb(235, 222, 200);
        style.visuals.window_fill = paper;
        style.visuals.panel_fill = shelf;
        style.visuals.faint_bg_color = egui::Color32::from_rgb(42, 34, 25);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(18, 14, 11);
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(96, 64, 24);
        style.visuals.selection.stroke.color = amber;
        style.visuals.hyperlink_color = amber;
        style.visuals.override_text_color = Some(parchment);
        style.visuals.warn_fg_color = amber;
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(22.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("book-title".into()),
            egui::FontId::new(20.0, egui::FontFamily::Proportional),
        );
        cc.egui_ctx.set_style_of(theme, style);
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
        ui.heading("Bulk update (same fields, all-or-nothing)");
        ui.horizontal(|ui| {
            ui.label("ids (comma separated):");
            ui.text_edit_singleline(&mut self.bulk_ids);
            if ui.button("Apply fields above").clicked() {
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
                match self.manage_rating.trim() {
                    "" => {}
                    raw => match raw.parse::<i64>() {
                        Ok(n) => {
                            fields.insert("rating".to_string(), json!(n));
                        }
                        Err(_) => {
                            self.notice = "bulk rating must be an integer".to_string();
                        }
                    },
                }
                if self.notice.is_empty() {
                    self.bulk_result = Some(self.dispatch(
                        "bulk_update",
                        json!({"ids": self.bulk_ids.trim(), "fields": fields}),
                    ));
                }
            }
        });
        show_result(ui, &self.bulk_result);

        ui.separator();
        ui.heading("Bulk update (same fields, all-or-nothing)");
        ui.horizontal(|ui| {
            ui.label("ids (comma separated):");
            ui.text_edit_singleline(&mut self.bulk_ids);
            if ui.button("Take marks").clicked() {
                self.bulk_ids = self
                    .marked
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
            }
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

    fn show_catalog(&mut self, ui: &mut egui::Ui) {
        ui.heading("Rename a vocabulary entry");
        ui.label("Merges when the new name exists; otherwise renames in place.");
        ui.horizontal(|ui| {
            ui.label("kind (tag/author/series/publisher):");
            ui.text_edit_singleline(&mut self.rename_kind);
            ui.label("old:");
            ui.text_edit_singleline(&mut self.rename_old);
            ui.label("new:");
            ui.text_edit_singleline(&mut self.rename_new);
            if ui.button("Rename").clicked() {
                self.rename_result = Some(self.dispatch(
                    "rename",
                    json!({
                        "kind": self.rename_kind.trim(),
                        "old": self.rename_old.trim(),
                        "new": self.rename_new.trim(),
                    }),
                ));
            }
        });
        show_result(ui, &self.rename_result);

        ui.separator();
        ui.heading("Catalog analytics");
        ui.horizontal(|ui| {
            if ui.button("Duplicates").clicked() {
                self.catalog_result = Some(self.dispatch("duplicates", json!({"limit": 50})));
            }
            if ui.button("Series gaps").clicked() {
                self.catalog_result = Some(self.dispatch("series_gaps", json!({})));
            }
            if ui.button("Unread").clicked() {
                self.catalog_result = Some(self.dispatch("unread", json!({"limit": 50})));
            }
            if ui.button("Reading stats").clicked() {
                self.catalog_result = Some(self.dispatch("reading_stats", json!({})));
            }
        });
        if let Some(result) = self.catalog_result.clone() {
            if result.get("success") == Some(&Value::Bool(true)) {
                ui.monospace(
                    serde_json::to_string(&result)
                        .unwrap_or_default()
                        .chars()
                        .take(1500)
                        .collect::<String>(),
                );
            } else {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("failed: {}", text(&result, "error")),
                );
            }
        }

        ui.separator();
        ui.heading("Saved searches");
        ui.horizontal(|ui| {
            ui.label("name:");
            ui.text_edit_singleline(&mut self.coll_name);
            ui.label("query:");
            ui.text_edit_singleline(&mut self.coll_query);
            if ui.button("Save").clicked() {
                self.coll_result = Some(self.dispatch(
                    "collection_save",
                    json!({"name": self.coll_name.trim(), "query": self.coll_query.trim()}),
                ));
            }
            if ui.button("Run").clicked() {
                self.coll_result = Some(self.dispatch(
                    "collection_run",
                    json!({"name": self.coll_name.trim(), "limit": 20}),
                ));
            }
            if ui.button("Delete").clicked() {
                self.coll_result = Some(
                    self.dispatch("collection_delete", json!({"name": self.coll_name.trim()})),
                );
            }
            if ui.button("List").clicked() {
                self.coll_list = Some(self.dispatch("collection_list", json!({})));
            }
        });
        show_result(ui, &self.coll_result);
        if let Some(listed) = self.coll_list.clone() {
            ui.monospace(format!(
                "saved: {}",
                listed
                    .get("names")
                    .and_then(Value::as_array)
                    .map(|n| n
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default()
            ));
        }
    }

    fn show_share(&mut self, ui: &mut egui::Ui) {
        ui.heading("Export");
        ui.horizontal(|ui| {
            ui.label("format (csv/json):");
            ui.text_edit_singleline(&mut self.export_format);
            ui.label("out:");
            ui.text_edit_singleline(&mut self.export_out);
            ui.label("query (optional):");
            ui.text_edit_singleline(&mut self.export_query);
            if ui.button("Export").clicked() {
                let mut args = json!({
                    "format": self.export_format.trim(),
                    "out": self.export_out.trim(),
                });
                if !self.export_query.trim().is_empty() {
                    args["query"] = Value::String(self.export_query.trim().to_string());
                }
                self.export_result = Some(self.dispatch("export", args));
            }
        });
        show_result(ui, &self.export_result);

        ui.separator();
        ui.heading("Excerpt");
        ui.horizontal(|ui| {
            ui.label("id:");
            ui.text_edit_singleline(&mut self.excerpt_id);
            ui.label("format:");
            ui.text_edit_singleline(&mut self.excerpt_format);
            ui.label("chapter:");
            ui.text_edit_singleline(&mut self.excerpt_chapter);
            ui.label("chars:");
            ui.text_edit_singleline(&mut self.excerpt_chars);
            if ui.button("Read").clicked() {
                let id: i64 = self.excerpt_id.trim().parse().unwrap_or(-1);
                let chapter: usize = self.excerpt_chapter.trim().parse().unwrap_or(0);
                let chars: usize = self.excerpt_chars.trim().parse().unwrap_or(2000);
                let mut args = json!({"id": id, "chapter": chapter, "chars": chars});
                if !self.excerpt_format.trim().is_empty() {
                    args["format"] = Value::String(self.excerpt_format.trim().to_string());
                }
                self.excerpt_result = Some(self.dispatch("excerpt", args));
            }
        });
        if let Some(result) = self.excerpt_result.clone() {
            if result.get("success") == Some(&Value::Bool(true)) {
                ui.heading(format!(
                    "{} (chapter {} of {})",
                    result["title"].as_str().unwrap_or(""),
                    result["chapter"].as_i64().unwrap_or(0),
                    result["chapters"].as_i64().unwrap_or(0)
                ));
                ui.label(result["text"].as_str().unwrap_or(""));
            } else {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("failed: {}", text(&result, "error")),
                );
            }
        }

        ui.separator();
        ui.heading("Open in system viewer");
        ui.horizontal(|ui| {
            ui.label("id:");
            ui.text_edit_singleline(&mut self.open_id);
            ui.label("format:");
            ui.text_edit_singleline(&mut self.open_format);
            if ui.button("Open").clicked() {
                let id: i64 = self.open_id.trim().parse().unwrap_or(-1);
                let mut args = json!({"id": id});
                if !self.open_format.trim().is_empty() {
                    args["format"] = Value::String(self.open_format.trim().to_string());
                }
                self.open_result = Some(self.dispatch("open", args));
            }
        });
        show_result(ui, &self.open_result);

        ui.separator();
        ui.heading("Random pick");
        ui.horizontal(|ui| {
            ui.label("tag:");
            ui.text_edit_singleline(&mut self.random_tag);
            ui.label("author:");
            ui.text_edit_singleline(&mut self.random_author);
            ui.label("format:");
            ui.text_edit_singleline(&mut self.random_format);
            if ui.button("Surprise me").clicked() {
                let mut args = json!({});
                for (key, field) in [
                    ("tag", &self.random_tag),
                    ("author", &self.random_author),
                    ("format", &self.random_format),
                ] {
                    if !field.trim().is_empty() {
                        args[key] = Value::String(field.trim().to_string());
                    }
                }
                self.random_result = Some(self.dispatch("random", args));
            }
        });
        if let Some(result) = self.random_result.clone() {
            if result.get("success") == Some(&Value::Bool(true)) {
                if let Some(book) = result.get("book") {
                    ui.heading(format!(
                        "#{} {}",
                        book["id"].as_i64().unwrap_or(0),
                        book["title"].as_str().unwrap_or("")
                    ));
                    ui.label(format!(
                        "{} - {}",
                        joined(book, "authors"),
                        joined(book, "formats")
                    ));
                }
            } else {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("failed: {}", text(&result, "error")),
                );
            }
        }
    }

    fn show_read(&mut self, ui: &mut egui::Ui) {
        ui.heading("Read");
        ui.horizontal(|ui| {
            ui.label("id:");
            ui.text_edit_singleline(&mut self.read_id);
            if ui.button("Open").clicked() {
                self.read_chapter = 0;
                self.fetch_chapter();
            }
            if ui.button("<< Prev").clicked() {
                self.read_chapter = self.read_chapter.saturating_sub(1);
                self.fetch_chapter();
            }
            if ui.button("Next >>").clicked() {
                self.read_chapter = self.read_chapter.saturating_add(1);
                self.fetch_chapter();
            }
        });
        let Some(result) = self.read_result.clone() else {
            ui.label("Open a book (or press Read on its card) to start reading.");
            return;
        };
        if result.get("success") != Some(&Value::Bool(true)) {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("failed: {}", text(&result, "error")),
            );
            return;
        }
        let chapter = result["chapter"].as_i64().unwrap_or(0) + 1;
        let chapters = result["chapters"].as_i64().unwrap_or(1).max(1);
        ui.heading(format!(
            "{} - chapter {chapter} of {chapters}",
            result["title"].as_str().unwrap_or("")
        ));
        if result["has_more"].as_bool().unwrap_or(false) {
            ui.label("(excerpt continues past this window)");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(result["text"].as_str().unwrap_or(""));
        });
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

    /// Cached cover thumbnail: loaded once per book, then served from the texture map.
    fn cover_texture(
        &mut self,
        ctx: &egui::Context,
        book: &Value,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        let id = book.get("id")?.as_i64()?;
        if let Some(tex) = self.textures.get(&id) {
            let [w, h] = tex.size();
            let scale = (240.0 / h.max(1) as f32).min(1.0);
            return Some((
                tex.id(),
                egui::Vec2::new(w as f32 * scale, h as f32 * scale),
            ));
        }
        let path = book.get("cover_path")?.as_str()?;
        if path.is_empty() {
            return None;
        }
        let (pixels, w, h) =
            bookcraft_core::library::cover_rgba(std::path::Path::new(path), 360).ok()?;
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
        let tex = ctx.load_texture(format!("cover-{id}"), image, egui::TextureOptions::LINEAR);
        let scale = (240.0 / h.max(1) as f32).min(1.0);
        let size = egui::Vec2::new(w as f32 * scale, h as f32 * scale);
        let handle = tex.id();
        self.textures.insert(id, tex);
        Some((handle, size))
    }

    fn fetch_chapter(&mut self) {
        let id: i64 = self.read_id.trim().parse().unwrap_or(-1);
        self.read_result = Some(self.dispatch(
            "excerpt",
            json!({"id": id, "chapter": self.read_chapter, "chars": 4000}),
        ));
    }

    fn show_library(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("book-list").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Books");
                if ui.button("Load").clicked() {
                    self.load_list();
                }
                if ui.button("Clear marks").clicked() {
                    self.marked.clear();
                }
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut pick: Option<i64> = None;
                let mut toggle: Option<i64> = None;
                for book in &self.books {
                    let id = book.get("id").and_then(Value::as_i64).unwrap_or(0);
                    let label = format!("#{id} {}", text(book, "title"));
                    ui.horizontal(|ui| {
                        let mut marked = self.marked.contains(&id);
                        if ui.checkbox(&mut marked, "").clicked() {
                            toggle = Some(id);
                        }
                        if ui
                            .selectable_value(&mut self.selected, Some(id), label)
                            .clicked()
                        {
                            pick = Some(id);
                        }
                    });
                }
                if let Some(id) = toggle {
                    if !self.marked.remove(&id) {
                        self.marked.insert(id);
                    }
                }
                if let Some(id) = pick {
                    self.show_detail(id);
                }
            });
            if !self.marked.is_empty() {
                ui.separator();
                ui.label(format!("{} marked", self.marked.len()));
                ui.horizontal(|ui| {
                    if ui.button("Bulk edit").clicked() {
                        self.bulk_ids = self
                            .marked
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.view = View::Manage;
                    }
                    if ui.button("Trash marked").clicked() {
                        let ids: Vec<i64> = self.marked.iter().copied().collect();
                        let mut trashed = 0;
                        for id in &ids {
                            let result = self.dispatch("remove_book", json!({"id": id}));
                            if result.get("success") == Some(&Value::Bool(true)) {
                                trashed += 1;
                            }
                        }
                        self.marked.clear();
                        self.load_list();
                        self.notice = format!("trashed {trashed} of {} marked", ids.len());
                    }
                });
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(book) = self.detail.clone() else {
                ui.label("Pick a book on the left.");
                return;
            };
            let cover = self.cover_texture(ui.ctx(), &book);
            ui.horizontal(|ui| {
                if let Some((id, size)) = cover {
                    ui.image((id, size));
                }
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(text(&book, "title"))
                            .strong()
                            .size(20.0),
                    );
                    ui.label(format!("by {}", joined(&book, "authors")));
                    if !text(&book, "series").is_empty() {
                        ui.label(format!(
                            "{} #{}",
                            text(&book, "series"),
                            book.get("series_index")
                                .and_then(Value::as_f64)
                                .unwrap_or(1.0)
                        ));
                    }
                    ui.label(format!("formats: {}", joined(&book, "formats")));
                    if let Some(rating) = book.get("rating").and_then(Value::as_i64) {
                        let stars = "*".repeat(rating.clamp(0, 5) as usize);
                        ui.label(format!("rating: {stars}"));
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button("Read").clicked() {
                    self.read_id = book["id"]
                        .as_i64()
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    self.read_chapter = 0;
                    self.fetch_chapter();
                    self.view = View::Read;
                }
                if ui.button("Edit in Manage").clicked() {
                    self.manage_id = book["id"]
                        .as_i64()
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    self.view = View::Manage;
                }
            });
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
                View::Catalog => self.show_catalog(ui),
                View::Share => self.show_share(ui),
                View::Read => self.show_read(ui),
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

    // Catalog and share paths on the same fixture.
    let renamed = book.dispatch("collection_save", json!({"name": "smoke", "query": "dune"}));
    if renamed.get("success") != Some(&Value::Bool(true)) {
        return Err(format!("smoke collection save unsuccessful: {renamed}"));
    }
    let ran = book.dispatch("collection_run", json!({"name": "smoke"}));
    if ran.get("count").and_then(Value::as_i64).unwrap_or(0) < 1 {
        return Err(format!("smoke collection run empty: {ran}"));
    }
    log.push("smoke collection roundtrip: ok".to_string());

    let exported = book.dispatch(
        "export",
        json!({"format": "csv", "out": tmp.path().join("s.csv").to_string_lossy()}),
    );
    if exported.get("success") != Some(&Value::Bool(true)) || !tmp.path().join("s.csv").is_file() {
        return Err(format!("smoke export unsuccessful: {exported}"));
    }
    log.push("smoke export: ok".to_string());

    let picked = book.dispatch("random", json!({}));
    if picked["book"]["id"].as_i64().unwrap_or(-1) < 0 {
        return Err(format!("smoke random unsuccessful: {picked}"));
    }
    log.push("smoke random: ok".to_string());
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
