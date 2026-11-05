# Changelog

All notable changes to BookCraft. Dates are real clock dates, not guesses.

## 0.3.0 - 2026-11-05

The write path. BookCraft is now a full manager, not a read-only subset. All crates
bumped to 0.3.0.

### Why

`calibredb` refuses every local operation while any Calibre program runs, which on a
lived-in machine is always. BookCraft therefore writes SQL itself instead of shelling
out: one IMMEDIATE transaction per op, 30-second busy timeout, Calibre's own
`title_sort`/`uuid4` functions registered for its triggers, and a timestamped backup
outside the library first. Verified against the live Calibre 9 schema and triggers.

### Added

- **`write` module (L1):** `update` (title, authors, tags, series, series_index,
  publisher, rating 0-5, isbn, languages, comments, pubdate, identifiers),
  `add_book` (file or `--empty`, duplicate policy, automerge), `remove_book`
  (trash by default, `--permanent` deletes), `add_format`, `remove_format`,
  `backup`, `schema`.
- **Conventions code:** Calibre title-sort (article move, live-verified), author-sort,
  Windows-safe path building, microsecond UTC stamps, pubdate validation.
- **L2:** 7 new CLI verbs and 7 new MCP tools (15 total), `--force` backup flag,
  `--set key=value` parsing with scalar coercion.
- **L3:** Manage view (update/add/remove/backup). Covers still render as paths only.
- **EPIPE safety:** closed stdout exits quietly instead of panicking.
- **`book_info` now also returns `uuid`, `sort`, `author_sort`, `last_modified`.**

### Verified

- 34 tests pass (7 new write-path tests against fixtures with the verbatim live DDL
  and triggers).
- Live round-trip on a copy of the real 10,346-book Calibre 9 schema: add, update of
  10 fields, info read-back, remove. No fixture-only claims.

## 0.2.0 - 2026-11-05

L3 desktop shelf. All crates bumped to 0.2.0; no L1/L2 behavior changes.

### Added

- **`bookcraft-app` (L3):** eframe/egui desktop shelf (egui 0.37) with Status, Library,
  Search, Grep, Stats, Convert and Ingest views. Every view calls `BookApp::dispatch`,
  which is `bookcraft_core::dispatch` in-process: no child process, no port, no JSON-RPC.
- **`--smoke` headless checks:** fixture library plus read/grep/ingest parity assertions
  against the core bus, with artifact content checks. Exit 0 on success.
- **Parity tests:** `app_dispatch_matches_core_dispatch` (GUI path returns byte-identical
  payloads to the core for status/list/search/info/grep/stats), missing-library payload
  test, ingest script-rejection test. 27 tests total, all green.

### Known limits (honest)

- Covers render as paths, not images - no image decoding crate yet.
- The list views cap at 100 books; refinement (pagination) is open.
- No `egui_kittest` golden screenshots yet; parity is asserted on payloads, not pixels.

## 0.1.0 - 2026-11-05

Initial L1 + L2 implementation (Wave-2 craft, `craft-fleet-rebuild` PLAN #8).

### Added

- **L1 `bookcraft-core`**
  - `library.rs`: read-only Calibre access (`SQLITE_OPEN_READ_ONLY`, busy timeout, schema
    probe on open). `list`, `search`, `detail`, `stats`, and `grep` with
    `full-text-search.db:books_text` first and a metadata/title/description fallback that
    reports which engine answered.
  - `convert.rs`: discovers `ebook-convert` via `BOOKCRAFT_EBOOK_CONVERT`, `PATH`, then the
    standard install roots (no author-machine path). Validates the target format, never
    overwrites an existing output, and returns the real output path and byte count.
  - `ingest.rs`: deterministic HTML -> Markdown (front matter, `<title>` extraction, script /
    style / noscript removal, block-aware tag stripping, entity decoding), 16 MiB input cap.
  - `dispatch(op, args)`: one bus shared by CLI and MCP; `--root`/`BOOKCRAFT_ROOT` confinement
    with `..`-safe lexical checks.
- **L2 `bookcraft-cli`**
  - `status`, `list`, `search`, `info`, `grep`, `stats`, `convert`, `ingest`, `tools`, `mcp`;
    `--json` on every verb; exit code 1 on error.
  - Hand-rolled MCP 2024-11-05 stdio loop with 8 tools and dotted/underscore name aliases.
- 24 tests (19 core + 5 L2).

### Fixed during verification

- Calibre's `data.name` omits the file extension; format files were reported as missing.
  `data_path()` now resolves `<name>.<format>` and a regression fixture mirrors Calibre's
  real naming.
- `strip_html_tags` inserted a space before closing punctuation (`Arrakis .`); tag
  boundaries now only add a space between words.

### Verified

- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- `cargo test --workspace`: 24 passed.
- `cargo fmt --all --check` clean.
- MCP stdio probe: 5/5 stdout lines JSON, 0 non-JSON, 8 tools listed.
- Live read-only run against a 10,346-book Calibre library: status, stats, list, search,
  info, full-text grep, EPUB -> TXT conversion (98,388 bytes, 3.3 s); `metadata.db`
  mtime unchanged.

### Not included (deliberate)

- No L3 desktop app.
- No Democraft recipe registration in this repo; recipes belong in `democraft-mcp`.
- No live Readly scraping (that remains `readly-mcp`); only saved-HTML ingest.
