#Requires -Version 5.1
<#
.SYNOPSIS
  End-to-end HTTP smoke test for req-engine.

.DESCRIPTION
  Uses --home ./data relative to the repo root (parent of scripts/).
  - Ensures DB/tokens exist (init --seed if missing; seed if DB exists).
  - Starts `serve` in background if health is down; reuses an already-running server.
  - Exercises: health -> projects -> create requirement -> claim -> submit-review -> complete-review.
  - Prints PASS/FAIL and exits 0/1.

.PARAMETER DataHome
  Data directory. Default: <repo>/data

.PARAMETER BindHost
  Bind / connect host. Default: 127.0.0.1

.PARAMETER Port
  Bind / connect port. Default: 7420

.PARAMETER SkipStart
  Do not start the server; fail if health is unreachable.

.PARAMETER KeepServer
  Leave a server started by this script running after the test.
#>
[CmdletBinding()]
param(
    [string]$DataHome = "",
    [string]$BindHost = "127.0.0.1",
    [int]$Port = 7420,
    [switch]$SkipStart,
    [switch]$KeepServer
)

$ErrorActionPreference = "Stop"
$script:Failed = 0
$script:Passed = 0
$script:ServerProc = $null
$script:WeStartedServer = $false

function Write-Step([string]$msg) { Write-Host "-> $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)   { Write-Host "  PASS  $msg" -ForegroundColor Green; $script:Passed++ }
function Write-Fail([string]$msg) { Write-Host "  FAIL  $msg" -ForegroundColor Red;   $script:Failed++ }

# Resolve repo root (parent of scripts/)
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $DataHome) {
    $DataHome = Join-Path $RepoRoot "data"
}
$DataHome = [System.IO.Path]::GetFullPath($DataHome)
$DbPath = Join-Path $DataHome "req-engine.sqlite"
$TokensPath = Join-Path $DataHome "tokens.txt"
$Base = "http://${BindHost}:${Port}/v1"
$Bin = Join-Path $RepoRoot "target\debug\req-engine.exe"
$LogOut = Join-Path $DataHome "smoke-serve-out.log"
$LogErr = Join-Path $DataHome "smoke-serve-err.log"

Write-Host ""
Write-Host "req-engine smoke" -ForegroundColor White
Write-Host "  repo:  $RepoRoot"
Write-Host "  home:  $DataHome"
Write-Host "  base:  $Base"
Write-Host ""

function Get-Token([string]$role) {
    if (-not (Test-Path $TokensPath)) {
        throw "tokens file missing: $TokensPath"
    }
    $line = Select-String -Path $TokensPath -Pattern "^${role}=" | Select-Object -First 1
    if (-not $line) {
        throw "role '$role' not found in $TokensPath"
    }
    return ($line.Line -replace "^${role}=", "").Trim()
}

function Invoke-Api {
    param(
        [string]$Method,
        [string]$Url,
        [string]$Token = $null,
        [string]$Body = $null,
        [int[]]$ExpectStatus = @(200, 201)
    )
    $headers = @{ "Accept" = "application/json" }
    if ($Token) { $headers["Authorization"] = "Bearer $Token" }

    $params = @{
        Method      = $Method
        Uri         = $Url
        Headers     = $headers
        ErrorAction = "Stop"
        UseBasicParsing = $true
    }
    if ($Body) {
        $params["ContentType"] = "application/json; charset=utf-8"
        $params["Body"] = [System.Text.Encoding]::UTF8.GetBytes($Body)
    }
    try {
        $resp = Invoke-WebRequest @params
        $code = [int]$resp.StatusCode
        $json = $null
        if ($resp.Content) {
            try { $json = $resp.Content | ConvertFrom-Json } catch { $json = $resp.Content }
        }
        return @{ Ok = ($ExpectStatus -contains $code); Status = $code; Json = $json; Raw = $resp.Content }
    }
    catch {
        $ex = $_.Exception
        $code = 0
        $raw = $ex.Message
        if ($ex.Response) {
            try { $code = [int]$ex.Response.StatusCode } catch {}
            try {
                $stream = $ex.Response.GetResponseStream()
                $reader = New-Object System.IO.StreamReader($stream)
                $raw = $reader.ReadToEnd()
                $reader.Close()
            } catch {}
        }
        $json = $null
        try { $json = $raw | ConvertFrom-Json } catch {}
        return @{ Ok = ($ExpectStatus -contains $code); Status = $code; Json = $json; Raw = $raw }
    }
}

function Test-Health {
    try {
        $r = Invoke-WebRequest -Uri "$Base/health" -Method GET -TimeoutSec 2 -ErrorAction Stop -UseBasicParsing
        return ($r.StatusCode -eq 200)
    }
    catch {
        return $false
    }
}

function Ensure-Binary {
    if (-not (Test-Path $Bin)) {
        Write-Step "Building req-engine (debug)..."
        Push-Location $RepoRoot
        try {
            & cargo build 2>&1 | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
        }
        finally { Pop-Location }
    }
    if (-not (Test-Path $Bin)) {
        throw "binary not found: $Bin"
    }
}

function Ensure-Data {
    if (-not (Test-Path $DbPath) -or -not (Test-Path $TokensPath)) {
        Write-Step "init --home $DataHome --seed"
        Ensure-Binary
        & $Bin init --home $DataHome --seed
        if ($LASTEXITCODE -ne 0) { throw "init failed (exit $LASTEXITCODE)" }
        Write-Ok "init --seed"
    }
    else {
        Write-Step "DB already present - seed (idempotent-ish)"
        Ensure-Binary
        & $Bin seed --home $DataHome 2>&1 | Out-Host
        Write-Ok "data ready ($DataHome)"
    }
}

function Ensure-Server {
    if (Test-Health) {
        Write-Ok "server already up at $Base"
        return
    }
    if ($SkipStart) {
        Write-Fail "server not reachable and -SkipStart set"
        throw "server not running at $Base"
    }

    Write-Step "starting serve --home $DataHome --host $BindHost --port $Port"
    Ensure-Binary

    # Prefer relative home when data lives under repo (avoids arg encoding issues)
    $homeArg = $DataHome
    $prefix = $RepoRoot.TrimEnd('\') + '\'
    if ($DataHome.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $homeArg = $DataHome.Substring($prefix.Length)
        if (-not $homeArg) { $homeArg = "." }
    }

    if (Test-Path $LogOut) { Remove-Item $LogOut -Force -ErrorAction SilentlyContinue }
    if (Test-Path $LogErr) { Remove-Item $LogErr -Force -ErrorAction SilentlyContinue }

    $argList = @(
        "serve",
        "--home", $homeArg,
        "--host", $BindHost,
        "--port", "$Port"
    )

    $proc = Start-Process -FilePath $Bin `
        -ArgumentList $argList `
        -WorkingDirectory $RepoRoot `
        -PassThru `
        -WindowStyle Hidden `
        -RedirectStandardOutput $LogOut `
        -RedirectStandardError $LogErr

    $script:ServerProc = $proc
    $script:WeStartedServer = $true

    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 300
        # Refresh process state
        try { $proc.Refresh() } catch {}

        if ($proc.HasExited) {
            $err = ""
            if (Test-Path $LogErr) { $err = Get-Content $LogErr -Raw -ErrorAction SilentlyContinue }
            if (Test-Path $LogOut) { $err += "`n" + (Get-Content $LogOut -Raw -ErrorAction SilentlyContinue) }
            if (Test-Health) {
                Write-Ok "port $Port already in use but health OK (reusing external server)"
                $script:WeStartedServer = $false
                $script:ServerProc = $null
                return
            }
            Write-Fail "serve exited early (code $($proc.ExitCode)): $err"
            throw "serve failed to start"
        }
        if (Test-Health) {
            Write-Ok "serve ready (pid $($proc.Id))"
            return
        }
    }

    if (Test-Health) {
        Write-Ok "health OK after wait (may be external server)"
        return
    }
    $err = ""
    if (Test-Path $LogErr) { $err = Get-Content $LogErr -Raw -ErrorAction SilentlyContinue }
    if (Test-Path $LogOut) { $err += "`n" + (Get-Content $LogOut -Raw -ErrorAction SilentlyContinue) }
    Write-Fail "server did not become healthy within 20s. logs: $err"
    throw "serve health timeout"
}

function Stop-ServerIfOurs {
    if (-not $script:WeStartedServer) { return }
    if ($KeepServer) {
        Write-Host "  (keeping server pid $($script:ServerProc.Id) running)" -ForegroundColor DarkGray
        return
    }
    if ($null -ne $script:ServerProc) {
        try { $script:ServerProc.Refresh() } catch {}
        if (-not $script:ServerProc.HasExited) {
            Write-Step "stopping server (pid $($script:ServerProc.Id))"
            try {
                Stop-Process -Id $script:ServerProc.Id -Force -ErrorAction Stop
                Start-Sleep -Milliseconds 200
            }
            catch {
                Write-Host "  warn: could not stop server: $_" -ForegroundColor Yellow
            }
        }
    }
}

try {
    Ensure-Data
    Ensure-Server

    $admin = Get-Token "admin"
    $foreman = Get-Token "foreman"

    Write-Step "GET /v1/health"
    $r = Invoke-Api -Method GET -Url "$Base/health"
    if ($r.Ok -and $r.Json.status -eq "ok") { Write-Ok "health status=ok" }
    else { Write-Fail "health status=$($r.Status) body=$($r.Raw)" }

    Write-Step "GET /v1/projects"
    $r = Invoke-Api -Method GET -Url "$Base/projects" -Token $admin
    if (-not $r.Ok) {
        Write-Fail "list projects status=$($r.Status) body=$($r.Raw)"
        $projectId = $null
    }
    else {
        $projects = @($r.Json)
        if ($projects.Count -ge 1 -and $projects[0].id) {
            Write-Ok "list projects count=$($projects.Count)"
            $demo = $projects | Where-Object { $_.id -eq "demo-shop" } | Select-Object -First 1
            $projectId = if ($demo) { $demo.id } else { $projects[0].id }
        }
        else {
            Write-Fail "list projects empty"
            $projectId = $null
        }
    }

    if (-not $projectId) {
        Write-Step "POST /v1/projects (fallback)"
        $body = '{"name":"Smoke Project","color":"#22c55e","blurb":"smoke"}'
        $r = Invoke-Api -Method POST -Url "$Base/projects" -Token $admin -Body $body -ExpectStatus @(200, 201)
        if ($r.Ok -and $r.Json.id) {
            $projectId = $r.Json.id
            Write-Ok "created project id=$projectId"
        }
        else {
            Write-Fail "create project status=$($r.Status) body=$($r.Raw)"
            throw "cannot continue without a project"
        }
    }

    Write-Step "POST /v1/projects/$projectId/requirements"
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $reqBody = "{`"title`":`"Smoke test $stamp`",`"description`":`"created by scripts/smoke.ps1`",`"priority`":`"medium`",`"scope`":[`"smoke`"],`"acceptance_criteria`":[`"PASS printed`"]}"
    $r = Invoke-Api -Method POST -Url "$Base/projects/$projectId/requirements" -Token $admin -Body $reqBody -ExpectStatus @(200, 201)
    if ($r.Ok -and $r.Json.id -and $r.Json.status -eq "todo") {
        $reqId = $r.Json.id
        Write-Ok "create requirement id=$reqId status=todo"
    }
    else {
        Write-Fail "create requirement status=$($r.Status) body=$($r.Raw)"
        throw "cannot continue without a requirement"
    }

    Write-Step "POST /v1/requirements/$reqId/claim"
    $r = Invoke-Api -Method POST -Url "$Base/requirements/$reqId/claim" -Token $foreman
    if ($r.Ok -and $r.Json.status -eq "in_progress") {
        Write-Ok "claim status=in_progress claimed_by=$($r.Json.claimed_by)"
    }
    else {
        Write-Fail "claim status=$($r.Status) body=$($r.Raw)"
    }

    Write-Step "POST /v1/requirements/$reqId/submit-review"
    $r = Invoke-Api -Method POST -Url "$Base/requirements/$reqId/submit-review" -Token $foreman
    if ($r.Ok -and $r.Json.status -eq "review") {
        Write-Ok "submit-review status=review"
    }
    else {
        Write-Fail "submit-review status=$($r.Status) body=$($r.Raw)"
    }

    Write-Step "POST /v1/requirements/$reqId/complete-review"
    $revBody = '{"pass":true,"reason":"smoke ok"}'
    $r = Invoke-Api -Method POST -Url "$Base/requirements/$reqId/complete-review" -Token $admin -Body $revBody
    if ($r.Ok -and $r.Json.status -eq "done") {
        Write-Ok "complete-review status=done"
    }
    else {
        Write-Fail "complete-review status=$($r.Status) body=$($r.Raw)"
    }

    Write-Step "GET /v1/requirements/$reqId"
    $r = Invoke-Api -Method GET -Url "$Base/requirements/$reqId" -Token $admin
    if ($r.Ok -and $r.Json.status -eq "done") {
        $ev = 0
        if ($r.Json.events) { $ev = @($r.Json.events).Count }
        Write-Ok "detail status=done events=$ev"
    }
    else {
        Write-Fail "detail status=$($r.Status) body=$($r.Raw)"
    }
}
catch {
    Write-Fail "exception: $_"
}
finally {
    Stop-ServerIfOurs
}

Write-Host ""
if ($script:Failed -eq 0) {
    Write-Host ("PASS  ({0} checks)" -f $script:Passed) -ForegroundColor Green
    exit 0
}
else {
    Write-Host ("FAIL  ({0} failed, {1} passed)" -f $script:Failed, $script:Passed) -ForegroundColor Red
    exit 1
}
