# BookCraft start: builds the CLI, then shows status.
# -Mcp runs the stdio MCP server instead (a client normally starts it directly).
# -Gui builds and opens the desktop shelf instead.
param(
    [switch]$Mcp,
    [switch]$Gui,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$env:Path = "$cargoBin;$env:Path"

if ($null -eq (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "ERROR: cargo not found. Install Rust (https://rustup.rs) and reopen this window." -ForegroundColor Red
    exit 1
}

$repoRoot = $PSScriptRoot
Push-Location -LiteralPath $repoRoot
try {
    if ($Gui) {
        if ($Release) {
            & cargo build --release -p bookcraft-app
            $app = Join-Path $repoRoot "target\release\bookcraft-app.exe"
        } else {
            & cargo build -p bookcraft-app
            $app = Join-Path $repoRoot "target\debug\bookcraft-app.exe"
        }

        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
        if (-not (Test-Path -LiteralPath $app)) { throw "expected binary not found: $app" }

        Write-Host "Opening BookCraft desktop shelf..." -ForegroundColor Cyan
        & $app
        return
    }

    if ($Release) {
        & cargo build --release -p bookcraft-mcp
        $exe = Join-Path $repoRoot "target\release\bookcraft-cli.exe"
    } else {
        & cargo build -p bookcraft-mcp
        $exe = Join-Path $repoRoot "target\debug\bookcraft-cli.exe"
    }

    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    if (-not (Test-Path -LiteralPath $exe)) { throw "expected binary not found: $exe" }

    if ($Mcp) {
        Write-Host "Starting BookCraft MCP server on stdio (Ctrl+C to stop)..." -ForegroundColor Cyan
        & $exe mcp
    } else {
        & $exe status
        Write-Host ""
        Write-Host "BookCraft is built. Examples:" -ForegroundColor Cyan
        Write-Host "  $exe --library <calibre-library> stats"
        Write-Host "  $exe --library <calibre-library> search --query dune"
        Write-Host "  $exe --library <calibre-library> convert --id 1 --to pdf --out-dir .\outbox"
        Write-Host "  $exe mcp     # stdio server for an MCP client"
    }
} finally {
    Pop-Location
}
