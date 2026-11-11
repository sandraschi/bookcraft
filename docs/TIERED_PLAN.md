# BookCraft — Tiered Execution Plan

**Parent Standard:** [`RUST_ARTCRAFT_STANDARD.md`](../../mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md)  
**Menu Bar Standard:** [`CRAFT_MENU_BAR_STANDARD.md`](../../mcp-central-docs/standards/rust-artcraft/CRAFT_MENU_BAR_STANDARD.md)  
**Architecture Model:** 4-Tier ArtCraft Incremental Delivery  
**Status:** Active  

---

## Architecture Flow

```
Tier 1: Core Engine ──> Tier 2: MCP / CLI ──> Tier 3: Desktop UI ──> Tier 4: Menu Bar, Help & Packaging
(bookcraft-core)       (bookcraft-mcp)       (bookcraft-app)       (Hardened Workstation)
```

---

## Tier Specifications

### Tier 1 — Core Calibre & Storage Engine (`bookcraft-core`)
- [x] Direct Calibre `metadata.db` SQLite engine with trigger and transaction invariants.
- [x] L1 `Command::dispatch` universal library execution bus.
- [x] Metadata CRUD, tag/series renaming, and duplicate detection.
- [x] Format conversion driver via Calibre `ebook-convert`.
- [x] Saved-page HTML ingest to Markdown.
- [x] `#![forbid(unsafe_code)]` and zero-panic clippy lints.

### Tier 2 — Headless Driver & FastMCP (`bookcraft-mcp`)
- [x] Unified CLI `bookcraft-cli` with 29 subcommands and `--json` output.
- [x] FastMCP stdio server communicating over JSON-RPC 2.0.
- [x] Full-text search and snippet extraction over `full-text-search.db`.
- [x] Headless `--smoke` verification without window initialization.

### Tier 3 — Desktop Workstation UI (`bookcraft-app`)
- [x] Immediate-mode `egui` 0.37 / `wgpu` workstation with warm paper theme.
- [x] 11 integrated views: Covers grid, Book detail, Reader view, Series gaps, Reading stats, etc.
- [x] In-shelf reading viewport paging chapters from disk formats.

### Tier 4 — Craft Menu Bar Standard, Help Suite & Hardened Verification
- [ ] Universal 8-menu bar (`File`, `Edit`, `View`, `Insert`, `Modify`, `Commands`, `Window`, `Help`) with `switch_on_hover`.
- [ ] Mandatory 5-dialog Help suite (`Command Palette...`, `Keyboard Shortcuts...`, `Project Files & Docs...`, `Licenses & Notices...`, `About BookCraft`).
- [ ] Fleet UI/UX Pattern 2 adherence: viewport-centric navigation, top button row mapped to View toggles, panel visibility managed via `View` / `Window`.
- [ ] Tofu Guard verification (100% font glyph coverage, zero tofu placeholder boxes).
- [ ] Headless `--smoke` parity validation.
- [ ] Standalone release binary packaging and NSIS installer.
