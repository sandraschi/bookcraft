# BookCraft - AGENTS.md

**Parent hub:** [`mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md`](../mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md)
**Containment:** [`mcp-central-docs/standards/rust-artcraft/CONTAINMENT_AND_CRATES.md`](../mcp-central-docs/standards/rust-artcraft/CONTAINMENT_AND_CRATES.md)

BookCraft is a **Pure-Rust ArtCraft craft**: Calibre libraries are the document, and the verbs
are search / info / convert / ingest. It is Bucket A (SQLite + CLI + files), so the Python,
FastMCP, Starlette, React, Vite and port-claiming standards do **not** apply here.

## Layout

| Crate | Tier | Contents |
|---|---|---|
| `crates/bookcraft-core` | L1 | `library.rs` (read-only Calibre), `convert.rs` (`ebook-convert`), `ingest.rs` (HTML->Markdown), `dispatch` |
| `crates/bookcraft-mcp` | L2 | `bookcraft-cli` - clap CLI + hand-rolled MCP 2024-11-05 stdio loop |
| `crates/bookcraft-app` | L3 | `bookcraft-app` - eframe/egui desktop shelf calling `dispatch` in-process, `--smoke` headless checks |

There is an L3 app. It must never spawn `bookcraft-cli.exe`, never open a port, and never
route GUI actions through JSON-RPC. Every view calls `BookApp::dispatch`, and
`app_dispatch_matches_core_dispatch` proves the GUI and core payloads are identical.

## Non-negotiable rules

1. **Zero panic.** `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` are denied at
   workspace level and repeated at every crate root. Return `Result<T, BookError>`.
2. **Writes are direct SQL under Calibre's own rules.** `calibredb` refuses to run while
   any Calibre program is open, so BookCraft writes SQL itself: one IMMEDIATE transaction
   per op, 30-second busy timeout, `title_sort`/`uuid4` registered for Calibre's triggers,
   and a timestamped backup outside the library first. Never bypass the `write` module
   with ad-hoc SQL, and never write `metadata.db` or `full-text-search.db` by hand.
3. **Outputs go where the caller says.** `convert` and `ingest` write only to the supplied
   output paths. Never default an output path into a Calibre library.
4. **Stdio invariant.** In `mcp` mode, `stdout` is 100% line-delimited JSON-RPC. Logs go to
   `stderr`. One `println!` in L1 breaks the handshake.
5. **One dispatch bus.** CLI verbs and MCP tools must both construct arguments for
   `bookcraft_core::dispatch`. Do not add a capability to only one surface.
6. **No silent downgrade.** A missing `ebook-convert` is reported (`status: down`), not faked.
   A missing `full-text-search.db` is reported as `engine: metadata_db`.
7. **Hostile input.** Cap input sizes, escape `LIKE` wildcards, validate the target format
   (`normalise_format`), and never index strings by byte.

## Multi-agent rules

Several agents work in `D:\Dev\repos` from different IDEs. Before editing:

- Check `git status`; this repo is currently untracked inside the fleet workspace, so do not
  commit fleet-wide by accident.
- Max 10 files per commit and per script invocation. 3+ file edits need a timestamped `.bak`
  per file first (`*.bak` is gitignored).
- Do not start two `cargo check` runs in this workspace; wait for the peer's lock.

## Verification (must pass before claiming done)

```powershell
just fmt-check
just check
just test
```

Then the MCP probe: pipe `initialize`, `tools/list` and one `tools/call` into
`bookcraft-cli mcp` and confirm every stdout line parses as JSON and `stderr` holds only
diagnostics. A gate that reports success having examined nothing is a failure, not a pass.
