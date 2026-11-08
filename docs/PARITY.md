# BookCraft vs calibre-mcp parity

`calibre-mcp` (Python, FastMCP) exposes 33 portmanteau tools. This is the honest
scoreboard for BookCraft, kept current. "Done" means implemented, tested, and verified
against the live schema - not planned.

## Done (v0.3.0 + v0.4.0)

| calibre-mcp capability | BookCraft equivalent |
|---|---|
| `query_books` search/list/recent/by_author | `list`, `search` |
| `get_book` / `manage_books` details | `info` (uuid, sort, author_sort, last_modified included) |
| `fulltext_search` | `grep` (FTS first, metadata fallback, engine receipt) |
| `get_series_analysis`, tag stats, library health basics | `stats` |
| `update_book` / `manage_metadata` update | `update` (12 fields), `bulk_update` (all-or-nothing) |
| `add_book` (incl. `--empty`), `delete_book` | `add_book`, `remove_book` (trash by default) |
| format add/remove, `convert` | `add_format`, `remove_format`, `convert` |
| tags/authors/series/publishers management | `rename` (merge-safe, sorts recomputed) |
| reading analytics, series analysis, unread priority | `reading_stats`, `series_gaps`, `unread`, `duplicates` |
| export CSV/JSON, saved searches | `export`, `collection_save`/`list`/`run`/`delete` |
| paginated excerpts, open, random | `excerpt` (EPUB/TXT), `open`, `random` |
| schema introspection | `schema` |
| ingest HTML | `ingest` |
| MCP stdio + CLI + desktop UI | L2 hand-rolled MCP, clap CLI, L3 egui shelf (11 views, covers, in-shelf reading) |

## Phase B (DONE 2026-11-05, v0.4.0)

- Catalog rename/merge, duplicate detection, series gaps, unread queue, reading stats.
- CSV/JSON export, TOC excerpts (EPUB chapters), open/random, smart collections.
- `init` for fresh libraries, `bulk_update`, `book/manage-roundtrip` + `book/catalog-roundtrip` recipes.

Remaining ideas, not promised: comic page counts, cover extraction.

## Deliberately out of scope

## Deliberately out of scope

- Driving the Calibre GUI or content server (Calibre owns its UI).
- `calibredb` shell-outs: refused while any Calibre program runs, so never used.
- LanceDB/RAG embeddings in-process (heavy native deps; export feeds external RAG).
- Online metadata fetch, Gutenberg/Anna's/arXiv import (network; revisit with `curl`).
- OPDS/server mode, accounts, social features (calibre-mcp server territory).
- OCR, bookcase catalog, canon coverage (specialist tools, not the core manager).
