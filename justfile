set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

# BookCraft automation (Pure-Rust ArtCraft pattern)

default:
    @just --list

# Strict ArtCraft lint gate across all workspace members
check:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo clippy --workspace --all-targets --all-features -- -D warnings

# Unit + integration tests
test:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo test --workspace

# Formatting gate
fmt-check:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo fmt --all --check

# All gates in one command
ci: fmt-check check test

# Start the MCP server over stdio (a client normally does this; useful for a manual probe)
run-mcp:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo run -q -p bookcraft-mcp -- mcp

# CLI smoke: engine status, then library stats if BOOKCRAFT_LIBRARY is set
run-cli:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo run -q -p bookcraft-mcp -- status; cargo run -q -p bookcraft-mcp -- stats

# Fixture ingest: HTML -> Markdown (no network, no browser)
run-ingest:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo run -q -p bookcraft-mcp -- ingest --html tests/fixtures/sample-issue.html --out outbox/sample-issue.md

# Headless L3 parity checks (no window)
run-app:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo run -q -p bookcraft-app -- --smoke

# Production build
build-release:
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo build --release --workspace
