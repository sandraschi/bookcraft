# HANDOVER - `bookcraft`

**Last updated:** 2026-11-05
**Phase:** Wave 2, PLAN #8 - v0.2.0 L1 + L2 + L3 shipped and verified.
**Started in:** Antigravity (`agy`), scaffold only (workspace + L1 `lib.rs` dispatch).
**Continued in:** opencode.

## Where the work stopped

At session start the repo contained only:

- `Cargo.toml` (2-crate workspace, clippy deny gates)
- `.gitignore`
- `crates/bookcraft-core/src/lib.rs` - dispatch for `status/list/search/info/grep/stats/
  convert/ingest_html`, declaring three modules that did not exist
- `crates/bookcraft-mcp/Cargo.toml` - no `src/main.rs`

It did not compile.

## What was done this session

- Implemented the three declared L1 modules:
  - `library.rs` - read-only Calibre reader; FTS-first grep with an honest metadata fallback.
  - `convert.rs` - `ebook-convert` discovery, format validation, collision-free output.
  - `ingest.rs` - deterministic HTML -> Markdown with a 16 MiB cap.
- Implemented L2 `crates/bookcraft-mcp/src/main.rs`: clap CLI (10 verbs) + hand-rolled MCP
  2024-11-05 stdio loop (8 tools, dotted aliases).
- Added 24 tests, `justfile`, `README.md`, `INSTALL.md`, `AGENTS.md`, `CHANGELOG.md`,
  `docs/PRD.md`, `docs/TOOLS.md`, `start.ps1`, `start.bat`, and the ingest fixture.
- Built L3 `crates/bookcraft-app` (egui 0.37, seven views, `--smoke`, parity tests);
  all crates at 0.2.0; added `docs/UI_PARITY.md` and `start.ps1 -Gui`.
- Generated and kept `Cargo.lock`.

## Verification actually run (not assumed)

```text
cargo clippy --workspace --all-targets --all-features -- -D warnings   -> clean
cargo test --workspace                                                 -> 24 passed
cargo fmt --all --check                                                -> clean
MCP stdio probe (5 requests)                                           -> 5/5 JSON, 0 non-JSON, 8 tools
Live library (10,346 books, read-only): status/stats/list/search/info/
  full-text grep                                                       -> ok
Live convert EPUB -> TXT                                               -> 98,388 bytes in 3.3 s
metadata.db mtime before/after convert                                 -> unchanged
```

Two real bugs were found by that run and fixed with regression coverage:

1. Calibre's `data.name` has no extension; format files were reported missing.
2. `strip_html_tags` put a space before closing punctuation.

## Correction 2026-11-05: the thin-craft scope was wrong

The PLAN said "library managers are not crafts", and v0.1.0/v0.2.0 built BookCraft as a
read-only subset. That was the fuckup: BookCraft is supposed to be as powerful as
`calibre-mcp`. v0.3.0 fixes it with the direct-SQL write path (`update`, `add_book`,
`remove_book`, `add_format`, `remove_format`, `backup`, `schema`; 15 MCP tools; L3
Manage view). `calibredb` was tried first and deleted: it refuses every local operation
while any Calibre program runs, which on a lived-in machine is always. The write engine
was verified end to end on a copy of the real Calibre 9 schema, and the engine behind
that verification (not the live library) is what the tests cover.

## Next steps

1. **Democraft recipes - DONE (2026-11-05).** `democraft-mcp` now registers `book` with
   `search-and-info`, `convert-roundtrip`, `fulltext-find`, `readly-ingest-sim` and
   `manage-roundtrip`, plus the automatic `capability-map`.
2. **L3 shelf - DONE (2026-11-05), extended with Manage view.** Eight views now.
   Commit `bookcraft/` into its own repo when ready.
3. **Naming/cutover decision: settled 2026-11-05 - BookCraft is the craft.**
   PLAN #8 said pick one implementation; the call is BookCraft for everything plan-shaped
   (search, info, convert, grep, ingest + Democraft recipes). `rust-calibre-mcp` is deliberately
   NOT archived and NOT edited: it remains the richer 23-command library manager with its own
   egui desktop, releases and users. Revisit a hard cutover only if BookCraft reaches full
   command parity; the duplication is acknowledged and parked, not shipped as two crafts.
4. **Remaining polish (optional):** cover image rendering, list pagination beyond 100,
   `egui_kittest` golden screenshots, installer/CI packaging.
5. **Phase B (next):** catalog rename/merge, duplicate detection, series-gap and
   unread-priority reports, CSV/JSON export, TOC excerpts, open/random, smart
   collections. See `docs/PARITY.md`.
6. **Repo packaging:** no `glama.json`, `llms.txt`, `CONTRIBUTING.md`, installer or CI file
   yet. Add when the repo is prepared for publication - do not claim them now.

## Blockers / traps

- `bookcraft/` is **not its own git repo**; `git status` from inside resolves to the fleet
  root `D:\Dev\repos`. Do not run a commit-all here until that is intentional.
- `L:\Multimedia` and `L:\Multimedia Files\Written Word` have empty `metadata.db` files; the
  real libraries are their `Calibre-Bibliothek*` subdirectories. Point `BOOKCRAFT_LIBRARY` at
  a directory containing a non-empty `metadata.db`.
- Calibre `ebook-convert` is present at `C:\Program Files\Calibre2\ebook-convert.exe` on
  Goliath; discovery is by PATH/env, never that literal path.
