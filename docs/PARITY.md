# BookCraft vs calibre-mcp parity

`calibre-mcp` (Python, FastMCP) exposes 33 portmanteau tools. This is the honest
scoreboard for BookCraft, kept current. "Done" means implemented, tested, and verified
against the live schema - not planned.

## Done (v0.3.0)

| calibre-mcp capability | BookCraft equivalent |
|---|---|
| `query_books` search/list/recent/by_author | `list`, `search` |
| `get_book` / `manage_books` details | `info` (uuid, sort, author_sort, last_modified included) |
| `fulltext_search` | `grep` (FTS first, metadata fallback, engine receipt) |
| `get_series_analysis`, tag stats, library health basics | `stats` |
| `update_book` / `manage_metadata` update | `update` (12 fields) |
| `add_book` (incl. `--empty`), `delete_book` | `add_book`, `remove_book` (trash by default) |
| format add/remove, `convert` | `add_format`, `remove_format`, `convert` |
| schema introspection | `schema` |
| ingest HTML | `ingest` |
| MCP stdio + CLI + desktop UI | L2 hand-rolled MCP, clap CLI, L3 egui shelf |

## Next (Phase B)

- Catalog rename/merge for tags, authors, series, publishers (direct SQL, UNIQUE-safe).
- Duplicate detection, series-gap analysis, unread-priority queue, reading stats.
- CSV/JSON export for external RAG pipelines.
- Excerpt reading (paginated chapter text from EPUB).
- `open` a book in the system viewer, `random` pick.
- Smart collections (saved searches), bulk update.
- `book/manage-roundtrip` Democraft recipe.

## Deliberately out of scope

- Driving the Calibre GUI or content server (Calibre owns its UI).
- `calibredb` shell-outs: refused while any Calibre program runs, so never used.
- LanceDB/RAG embeddings in-process (heavy native deps; export feeds external RAG).
- Online metadata fetch, Gutenberg/Anna's/arXiv import (network; revisit with `curl`).
- OPDS/server mode, accounts, social features (calibre-mcp server territory).
- OCR, bookcase catalog, canon coverage (specialist tools, not the core manager).
