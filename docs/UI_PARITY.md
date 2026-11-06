# BookCraft UI Parity

The L3 shelf (`bookcraft-app`) is a viewer over the same in-process `dispatch` the CLI and
MCP server use. No pixels are asserted; payloads are.

## Contract

1. **One bus.** Every view calls `BookApp::dispatch`, which injects the configured library
   and forwards to `bookcraft_core::dispatch`. The CLI and MCP surfaces construct the same
   argument objects.
2. **No process boundary.** The GUI never spawns `bookcraft-cli.exe`, never opens a port,
   and never speaks JSON-RPC. A GUI action and the equivalent CLI verb cannot disagree
   because they execute the same code path.
3. **No network, no writes to the library.** Views are read calls plus the two explicit
   output verbs (`convert`, `ingest`), which write only to caller-chosen paths.
4. **Headless proof.** `--smoke` runs every read verb plus ingest through both the GUI
   method and the core function and requires byte-identical successful payloads, including
   artifact content checks. `app_dispatch_matches_core_dispatch` is the same contract as a
   unit test.

## Views and the verbs they call

| View | Verbs |
|---|---|
| Status | `status` |
| Library | `list`, `info` |
| Search | `search` (click-through loads `info`) |
| Grep | `grep` (engine receipt shown verbatim) |
| Stats | `stats` |
| Convert | `convert` |
| Ingest | `ingest_html` |
| Manage | `update`, `add_book`, `remove_book`, `backup`, `bulk_update` |
| Catalog | `rename`, `duplicates`, `series_gaps`, `unread`, `reading_stats`, `collection_save/run/list/delete` |
| Share | `export`, `excerpt`, `open`, `random` |
| Read | `excerpt` (chapter prev/next, opened from any book card) |

## Known limits

- Covers decode from the library's own files (no downloads). Missing or unreadable
  covers show no art rather than an error box; thumbnails cap at 360 px.
- `Library` caps the list at 100 books; refine with search.
- PDF and comics still open externally via `open`.
- Empty states say what to do (set a library, press Refresh) instead of showing zeros.
