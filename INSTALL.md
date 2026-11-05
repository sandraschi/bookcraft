# Install BookCraft

BookCraft is a single Rust binary. It runs with zero runtime dependencies (no Python, no
`uv`, no Node). Calibre is only needed for `convert`; library browsing and ingest work
without it.

## 1. Prerequisites

- **Rust toolchain** (1.79 or newer; `std::path::absolute` is used). Verify:
  `cargo --version`.
- **Calibre** (optional, for `convert`). The Windows installer puts `ebook-convert.exe` in
  `C:\Program Files\Calibre2\`. BookCraft also scans `PATH`, `%ProgramFiles%`,
  `%ProgramFiles(x86)%` and `%LOCALAPPDATA%\Programs\Calibre2`, or you can point at it:
  `$env:BOOKCRAFT_EBOOK_CONVERT = "D:\Tools\Calibre\ebook-convert.exe"`.
- **A Calibre library** - any directory containing `metadata.db`.

## 2. Build

```powershell
git clone https://github.com/sandraschi/bookcraft.git
cd bookcraft
cargo build --release -p bookcraft-mcp
```

The binary is `target\release\bookcraft-cli.exe`.

For a normal developer loop use `cargo build -p bookcraft-mcp` and `target\debug\bookcraft-cli.exe`.

If you run the BookCraft demos through `democraft-mcp`, either build debug (above) or run
`just build-release` so the demo harness finds `target\release\bookcraft-cli.exe`. The
harness checks `target\release` first, then `target\debug`, and reports the app as not
installed when neither exists.

## 3. Configure

Set defaults once (optional):

```powershell
[Environment]::SetEnvironmentVariable("BOOKCRAFT_LIBRARY", "L:\Books\Calibre-Bibliothek", "User")
[Environment]::SetEnvironmentVariable("BOOKCRAFT_ROOT", "L:\Books", "User")
```

`BOOKCRAFT_ROOT` is a confinement fence: with it set, every library and output path must
resolve beneath it. Leave it unset to allow any path you pass explicitly.

## 4. Verify

```powershell
.\target\release\bookcraft-cli.exe status
.\target\release\bookcraft-cli.exe stats
.\target\release\bookcraft-cli.exe ingest --html .\tests\fixtures\sample-issue.html --out .\outbox\issue.md
```

Expected: `ebook-convert.status` is `ok` (or `down` if Calibre is absent), `stats` prints a
book count, and ingest writes an `issue.md` with front matter.

Desktop shelf: `cargo run -p bookcraft-app` opens the GUI; `cargo run -p bookcraft-app --
--smoke` runs the headless parity checks instead.

## 5. Register the MCP server

BookCraft is stdio-only; it opens no ports. Example client entry:

```json
{
  "mcpServers": {
    "bookcraft": {
      "type": "local",
      "command": [
        "D:\\Dev\\repos\\bookcraft\\target\\release\\bookcraft-cli.exe",
        "mcp"
      ],
      "env": {
        "BOOKCRAFT_LIBRARY": "L:\\Books\\Calibre-Bibliothek",
        "BOOKCRAFT_ROOT": "L:\\Books"
      },
      "enabled": true,
      "timeout": 120000
    }
  }
}
```

A 120 s timeout is deliberate: a large EPUB -> PDF conversion can take longer than the
default 60 s.

## 6. Uninstall

Delete the checkout and the binary. BookCraft stores nothing outside the paths you pass it.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `ebook-convert not found` | Calibre is not installed or not discoverable; set `BOOKCRAFT_EBOOK_CONVERT`. |
| `no metadata.db in …` | The path is not a Calibre library directory (point at the folder that contains `metadata.db`). |
| `path outside allowed root` | `BOOKCRAFT_ROOT`/`--root` does not contain the library or output path. |
| `is not a Calibre metadata.db` | The file exists but is not a Calibre schema (wrong `metadata.db`). |
| `grep` returns `engine: metadata_db` | This library has no `full-text-search.db`; only titles and descriptions are searchable. |
