# BookCraft PRD

**Source plan:** `mcp-central-docs/projects/craft-fleet-rebuild/PLAN.md` §5 #8,
corrected 2026-11-05: BookCraft is a full-power Calibre manager, not a thin subset.
**Bucket:** A (SQLite + CLI + files) -> Pure-Rust ArtCraft
**Status:** v0.3.0 L1/L2/L3 shipped with the write path; catalog analytics next.

## 1. Model

A **book** is one Calibre row: id, uuid, title, sort, author_sort, authors[], series,
series_index, tags[], formats[], rating, pubdate, last_modified, path, cover,
description, plus the real files on disk.

The same model covers EPUB/PDF prose, PDF magazines, and CBZ/CBR comics because Calibre
stores all of them the same way. BookCraft does not distinguish "book" from "issue" in its
verbs; the format and path tell you what it is.

## 2. Seam

| Concern | Owner |
|---|---|
| Library storage, metadata CRUD, covers, full text | **BookCraft** (direct SQL under Calibre's rules) |
| Format conversion | **BookCraft** -> Calibre `ebook-convert` |
| Saved-page HTML -> Markdown | **BookCraft** `ingest` |
| Live magazine scraping, login, browser | `readly-mcp` (unchanged) |
| Server-side library service, accounts | not a craft (`calibre-mcp` server mode) |
| Calibre desktop GUI | Calibre itself (BookCraft never drives it) |

Correction 2026-11-05: the original plan said "library managers are not crafts". That was
the fuckup this revision fixes - BookCraft manages the library. `calibredb` is not used
because it refuses every local operation while any Calibre program runs; BookCraft writes
SQL itself inside Calibre's own trigger and transaction invariants instead.

## 3. Verbs (normative)

```text
bookcraft-cli status
bookcraft-cli list   --limit N --offset N
bookcraft-cli search --query TEXT --limit N
bookcraft-cli info   --id ID
bookcraft-cli grep   --pattern TEXT --limit N
bookcraft-cli stats
bookcraft-cli schema
bookcraft-cli backup [--force]
bookcraft-cli update --id ID [--set key=value ...]
bookcraft-cli add-book --file FILE [--title T --authors A --tags T ...] | --empty
bookcraft-cli remove-book --id ID [--permanent]
bookcraft-cli add-format --id ID --file FILE
bookcraft-cli remove-format --id ID --format FMT
bookcraft-cli rename --kind tag|author|series|publisher --old OLD --new NEW
bookcraft-cli duplicates [--limit N]
bookcraft-cli series-gaps
bookcraft-cli unread [--limit N]
bookcraft-cli reading-stats
bookcraft-cli export --format csv|json --out FILE [--query TEXT]
bookcraft-cli excerpt --id ID [--format F] [--chapter N --chars N --offset N]
bookcraft-cli open --id ID [--format F]
bookcraft-cli random [--tag T --author A --format F]
bookcraft-cli collection-save --name NAME --query TEXT
bookcraft-cli collection-list
bookcraft-cli collection-run --name NAME [--limit N]
bookcraft-cli collection-delete --name NAME
bookcraft-cli bulk-update --ids "1,2,3" [--set key=value ...]
bookcraft-cli init
bookcraft-cli convert --id ID --to FORMAT [--from FORMAT] --out-dir DIR
bookcraft-cli ingest  --html FILE --out FILE [--title TEXT]
bookcraft-cli tools
bookcraft-cli mcp
```

`update` fields: title, authors, tags, series, series_index, publisher, rating (0-5),
isbn, languages, comments, pubdate, identifiers. Every mutation takes a timestamped
backup first and reports its path; `add-book` rejects duplicates unless asked.

## 4. Recipe targets (implemented in `democraft-mcp`, not here)

1. `book/search-and-info` - search a fixture library, dump one record, assert keys.
2. `book/convert-roundtrip` - EPUB -> PDF/TXT, measure bytes; CBZ -> PDF when a comic fixture
   is present.
3. `book/fulltext-find` - FTS query with snippet; assert the reported engine.
4. `book/readly-ingest-sim` - `tests/fixtures/sample-issue.html` -> `issue.md`, assert title
   and word count; never a browser.
5. `book/manage-roundtrip` - scratch library, backup, add, update, verify, remove;
   asserts the round-trip payloads (DONE 2026-11-05).

Fixtures must be repository-local and DRM-free. No real library path goes into a recipe.

## 5. Safety and failure modes

- Reads open the library read-only; writes run inside one IMMEDIATE transaction with a
  30-second busy timeout, so a held Calibre library fails fast instead of half-writing.
- Every mutation takes a timestamped backup outside the library first and reports it.
- `remove-book` moves files to a trash area by default; only `--permanent` deletes.
- `add-book` rejects duplicates without writing anything unless `duplicates`/`automerge`.
- A non-Calibre `metadata.db` is rejected on open.
- `--root` confinement covers library, inputs and outputs.
- Conversion never overwrites; the destination is `<title>[-n].<ext>`.
- Missing `ebook-convert` -> `status: down` + actionable error from `convert`.
- Missing `full-text-search.db` -> `engine: metadata_db`, not an empty success.
- Missing format file on disk -> `NotFound`, not a zero-byte output.
- Readly tokens never enter this repo; live scraping is out of scope.

## 6. Non-goals

- A server with accounts, OPDS, or a web UI.
- Embedding a converter; Calibre remains the engine.
- Duplicating `readly-mcp`'s browser automation.

## 7. Open decisions

1. **L3 app: built 2026-11-05, beautified the same day.** `bookcraft-app` ships eleven
   views over the same in-process `dispatch`: covers render from the library's own
   files, the Read view pages chapters in-shelf, the Library list multi-selects into
   bulk actions. Warm paper-and-amber theme throughout.
2. **Comic/manga specifics:** CBZ/CBR are convertible through `ebook-convert`; a dedicated
   page-count/cover path is not implemented.
3. **Naming:** settled - BookCraft is the craft; `rust-calibre-mcp` stays as the richer
   read-only dashboard (see HANDOVER). A hard cutover waits on full parity.
4. **Phase B: DONE 2026-11-05** (v0.4.0) - renames, analytics, export, excerpts,
   open/random, collections, bulk update, init. See `docs/PARITY.md`.
5. **Phase C (open):** semantic search/RAG, online metadata fetch, OPDS/server mode.
