# BookCraft

Full-power Calibre library manager in pure Rust: search, inspect, edit, add, remove,
convert, and ingest for the Storytold ArtCraft suite.

One binary, three surfaces: `bookcraft-cli <verb>` for humans and scripts,
`bookcraft-cli mcp` for agents over stdio, and the `bookcraft-app` desktop shelf.
All three call `bookcraft_core::dispatch`, so they cannot disagree.

**Status: v0.3.0 - L1 core, L2 CLI/MCP (15 tools) and the L3 shelf, all verified
(34 tests). Democraft recipes live in `democraft-mcp`. See `HANDOVER.md`.**

## Guarantees

- **Writes go through Calibre's own rules.** Reads open `metadata.db` read-only. Every
  mutation is direct SQL inside one IMMEDIATE transaction with a 30-second busy timeout,
  registering the same `title_sort`/`uuid4` functions Calibre's triggers call.
  `calibredb` is deliberately not used: it refuses every local operation while any Calibre
  program runs. A database that is not a Calibre library is rejected on open.
- **A backup is taken first.** Every mutation copies `metadata.db` (plus the full-text
  index) to a timestamped backup under `%LOCALAPPDATA%\BookCraft\backups` (reused while
  younger than 120 s); the backup path is in every mutation result. `book backup` takes
  one on demand.
- **Conversion output never overwrites.** `bookcraft convert` writes into the `--out_dir` you
  name and picks `<title>-2.<ext>` when the name is taken.
- **Removed books are recoverable.** `remove-book` moves the book directory to the trash
  area beside the backups; only `--permanent` deletes. The database row is always removed
  through Calibre's own cascade trigger.
- **Confinement is enforced when you ask for it.** Pass `--root DIR` (or `BOOKCRAFT_ROOT`) and
  every library, input and output path must resolve beneath it, including `..` traversal.
- **No secrets, no network.** Readly ingest takes a *saved* HTML page; the live browser path
  stays in `readly-mcp`.

## Quickstart

```powershell
# Build
cargo build --release -p bookcraft-mcp

# Is the converter engine present? What is in the library?
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" status
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" stats

# Find a book, then read its full record
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" search --query "dune"
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" info --id 42 --json

# Search book text when Calibre built its full-text index (reports which engine answered)
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" grep --pattern "blacksmith"

# Convert one book (needs Calibre's ebook-convert on PATH or BOOKCRAFT_EBOOK_CONVERT)
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" convert --id 42 --from EPUB --to txt --out-dir .\outbox

# Edit metadata (backup is automatic; fields: title authors tags series series_index
# publisher rating isbn languages comments pubdate identifiers)
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" update --id 42 --set title="Dune" --set tags="sci-fi, classic" --set rating=4

# Add and remove books (add --empty for a record without files)
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" add-book --file .\dune.txt --title "Dune" --authors "Frank Herbert"
.\target\release\bookcraft-cli.exe --library "D:\Books\Calibre Library" remove-book --id 42

# Turn a saved article page into Markdown
.\target\release\bookcraft-cli.exe ingest --html .\tests\fixtures\sample-issue.html --out .\outbox\issue.md

# MCP server (stdio)
.\target\release\bookcraft-cli.exe mcp

# Desktop shelf (same core, in-process; --smoke runs the headless checks)
.\target\debug\bookcraft-app.exe --smoke
.\target\debug\bookcraft-app.exe
```

## Verbs

| Verb | What it does |
|---|---|
| `status` | Engine (`ebook-convert`) and optional library health |
| `list` | Books, newest first (`--limit`, `--offset`) |
| `search` | Substring match on title, author sort, authors, series, tags, descriptions |
| `info` | Full record: description, format files with real paths, cover |
| `grep` | Book text via `full-text-search.db` when present, else titles/descriptions |
| `stats` | Counts and format distribution |
| `convert` | `ebook-convert` into `--out-dir`, with the source format you choose |
| `ingest` | Saved HTML page -> Markdown artifact (front matter + body) |
| `schema` | Dump the Calibre schema (`sqlite_master`): tables, indexes, triggers |
| `backup` | Timestamped metadata backup outside the library (`--force` for a fresh one) |
| `update` | Set metadata fields on one book (`--set key=value`, repeatable) |
| `add-book` | Add a book file, or an empty record with `--empty` |
| `remove-book` | Remove one book (Recycle Bin unless `--permanent`) |
| `add-format` | Attach one format file to a book (replaces the format when present) |
| `remove-format` | Drop one format from a book; a no-op when absent |
| `tools` | Print the MCP tool surface as JSON |
| `mcp` | Run the MCP 2024-11-05 stdio server |

Every verb accepts `--json`. The MCP surface exposes 15 tools (`book_status`,
`book_list`, `book_search`, `book_info`, `book_grep`, `book_stats`, `book_schema`,
`book_backup`, `book_update`, `book_add`, `book_remove`, `book_add_format`,
`book_remove_format`, `book_convert`, `book_ingest`) and
accepts dotted ids too (`book.status`, …). Full detail: `docs/TOOLS.md`.

## Environment

| Variable | Meaning |
|---|---|
| `BOOKCRAFT_LIBRARY` | Default Calibre library directory or `metadata.db` path |
| `BOOKCRAFT_ROOT` | Default confinement root |
| `BOOKCRAFT_EBOOK_CONVERT` | Explicit `ebook-convert` path (overrides PATH discovery) |

If `ebook-convert` is absent, `status` reports `{"status":"down"}` and `convert` fails with
an actionable error; nothing pretends to have converted.

## Build and verify

```powershell
just ci          # fmt-check + clippy -D warnings + cargo test
just run-cli
just run-ingest
```

The 6-gate ArtCraft bar (`mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md` §2):
clippy clean with `unwrap/expect/panic/todo/unimplemented` denied, tests green, `cargo fmt
--check` clean, MCP stdio probe byte-clean, `Cargo.lock` committed, `CHANGELOG.md` current.

## Docs

- `docs/PRD.md` - model, verbs, boundaries, recipe targets
- `docs/TOOLS.md` - MCP tool reference
- `docs/UI_PARITY.md` - shelf contract and known limits
- `docs/PARITY.md` - calibre-mcp parity checklist and what is deliberately out of scope
- `INSTALL.md` - prerequisites and client config
- `AGENTS.md` - containment rules for agents working here
- `HANDOVER.md` - what was done, what is next, what was deliberately skipped

## Status — does today / next week / non-goals

**Does today:** v0.3.0, 3-tier (core/mcp/app); 15 MCP tools + 17 CLI
verbs; 34 test fns; backup-before-write, trash-not-delete; PARITY docs;
cutover made (BookCraft is the craft, rust-calibre-mcp stays the
read-only dashboard).

**Next week:** Phase B (catalog rename/merge, analytics, export);
Calibre-version matrix; convert-roundtrip timing baseline.

**Non-goals:** no cloud sync; no living-room-server scope creep — library
manager, versioned honestly.

## Lineage — the Storytold craft-app pattern

Storytold ArtCraft ([storytold/artcraft](https://github.com/storytold/artcraft) — "the IDE for artists") was the starting point. The 16 craft apps in the Storytold stable each ship one binary that is both the human CLI and the agent MCP server (stdio), headless-first, with `run.json` + report artifacts. That modus operandi and harness is what this repo implements; the code here is independent, the pattern is shared.
