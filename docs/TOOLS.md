# BookCraft MCP tools

Fifteen tools, all routed through `bookcraft_core::dispatch`. Both `book_status` and
`book.status` are accepted (strict MCP hosts reject dots, ArtCraft ids use them).

Common optional arguments on every tool: `library` (Calibre directory or `metadata.db`),
`root` (confinement root). They default to `BOOKCRAFT_LIBRARY` / `BOOKCRAFT_ROOT`, and can be
set once in the client config.

| Tool | Arguments | Returns |
|---|---|---|
| `book_status` | - | `app`, `version`, `ebook_convert{status,path?}`, optional `library{status,root,books}` |
| `book_list` | `limit` (1-1000, default 20), `offset` | `count`, `books[]` newest first |
| `book_search` | `query` (required), `limit` | `query`, `count`, `books[]` |
| `book_info` | `id` (required) | `book` with `description`, `format_files[{format,size_bytes,file,path,exists}]`, `cover_path`, `uuid`, `sort`, `author_sort`, `last_modified` |
| `book_grep` | `pattern` (required), `limit` | `engine` (`full_text_search_db` or `metadata_db`), `count`, `hits[]` |
| `book_stats` | - | `books`, `authors`, `tags`, `series`, `publishers`, cover/description counts, `formats[]` |
| `book_schema` | - | `count`, `objects[]` with `kind`, `name`, `sql` from `sqlite_master` |
| `book_backup` | `force` | `backup` path (fresh copy, or the recent one reused) |
| `book_update` | `id`, `fields` object (required) | `book_id`, `updated_fields[]`, `backup` |
| `book_add` | `file` (or `empty: true`), `title`, `authors`, `tags`, `series`, `series_index`, `languages`, `isbn`, `duplicates`, `automerge` | `book_id`, `new`, `format?`, `file?`, `bytes?`, `merged?`, `backup` |
| `book_remove` | `id` (required), `permanent` | `book_id`, `title`, `permanent`, `files`, `backup` |
| `book_add_format` | `id`, `file` (required) | `book_id`, `format`, `new`, `file`, `bytes`, `backup` |
| `book_remove_format` | `id`, `format` (required) | `book_id`, `format`, `removed` |
| `book_convert` | `id`, `to`, `out_dir` (required), `from` | `output`, `bytes`, `from`, `to`, `engine`, `elapsed_ms` |
| `book_ingest` | `html`, `out` (required), `title` | `title`, `source`, `output`, `words`, `bytes` |

## Return shape

Success: a JSON object with `"success": true` and the fields above.
Failure: `{"success": false, "error": "...", "error_type": "not_found|invalid|outside|database|io"}`.

MCP wraps the payload as one `text` content item and sets `isError: true` when `success` is
false. Nothing is returned as a binary blob.

## Examples

```json
{"name":"book_search","arguments":{"query":"hyperart","limit":3}}
{"name":"book_info","arguments":{"id":14362}}
{"name":"book_update","arguments":{"id":14362,"fields":{"tags":"sci-fi, classic","rating":4}}}
{"name":"book_add","arguments":{"file":"D:/inbox/dune.txt","title":"Dune","authors":"Frank Herbert"}}
{"name":"book_convert","arguments":{"id":14362,"from":"EPUB","to":"txt","out_dir":"D:/outbox"}}
{"name":"book_ingest","arguments":{"html":"tests/fixtures/sample-issue.html","out":"outbox/issue.md"}}
```

## Notes

- Mutations never go through `calibredb`: it refuses every local operation while any
  Calibre program runs. BookCraft writes SQL itself inside Calibre's trigger invariants
  (one IMMEDIATE transaction, 30-second busy timeout, `title_sort`/`uuid4` registered).
- Every mutation takes a timestamped backup outside the library first and reports it;
  `book_add` rejects duplicates unless `duplicates` or `automerge` is set.
- `book_remove` moves files to a trash area by default; only `permanent: true` deletes.
- `book_convert` needs Calibre. Without `ebook-convert` the call fails with `not_found`; it
  never fabricates an output file.
- `book_grep` reports which engine answered. A library without `full-text-search.db` answers
  from titles and descriptions only - that is a scope downgrade, and the tool says so.
- Library, input and output paths must be beneath `root` when a root is configured.
