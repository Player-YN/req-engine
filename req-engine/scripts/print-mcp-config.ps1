# Print MCP stdio config using pair-codes.json (product path).
# Usage (from req-engine directory):
#   .\scripts\print-mcp-config.ps1
#   .\scripts\print-mcp-config.ps1 -HomeDir "C:\path\to\data"

param(
  [string]$HomeDir = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exeCandidates = @(
  (Join-Path $root "target\release\req-engine.exe"),
  (Join-Path $root "target\debug\req-engine.exe")
)
$exe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) {
  Write-Error "req-engine.exe not found. Run: cargo build --release"
}

if (-not $HomeDir) {
  $HomeDir = Join-Path $root "data"
}
$resolved = Resolve-Path $HomeDir -ErrorAction SilentlyContinue
if (-not $resolved) {
  Write-Error "data home not found: run desktop once or init --home ./data"
}
$HomeDir = $resolved.Path

$pairFile = Join-Path $HomeDir "pair-codes.json"
if (-not (Test-Path $pairFile)) {
  Write-Error "missing $pairFile — open desktop, select a project, then copy a seat pack (or create a project)."
}

$pair = Get-Content -Raw -Path $pairFile | ConvertFrom-Json
$projects = @($pair.projects.PSObject.Properties)
if (-not $projects.Count) {
  Write-Error "pair-codes.json has no projects"
}

$first = $projects[0]
$pid = $first.Name
$discuss = $first.Value.discuss
$build = $first.Value.build

Write-Host @"
# Project $pid
# Paste into your agent host MCP config. Do not commit pair codes.

{
  "mcpServers": {
    "req-engine-discuss": {
      "command": "$exe",
      "args": ["mcp", "--pair", "$discuss", "--home", "$HomeDir"]
    },
    "req-engine-build": {
      "command": "$exe",
      "args": ["mcp", "--pair", "$build", "--home", "$HomeDir"]
    }
  }
}

# discuss — list/get/create/update/cancel todo only; do not implement code
# build   — claim / progress / submit-review / release
# Human still does complete-review in the desktop UI.
"@
