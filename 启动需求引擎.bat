@echo off
setlocal EnableExtensions
REM Double-click: hand off to silent VBS (no lingering cmd window).
REM First-run build still uses this visible console.

cd /d "%~dp0req-engine" || (
  echo ERROR: cannot cd into req-engine
  pause
  exit /b 1
)

set "EXE="
if exist "target\release\req-engine.exe" set "EXE=target\release\req-engine.exe"
if not defined EXE if exist "target\debug\req-engine.exe" set "EXE=target\debug\req-engine.exe"

if not defined EXE (
  echo No built binary found. Building release ^(needs cargo in PATH^)...
  where cargo >nul 2>&1
  if errorlevel 1 (
    echo ERROR: cargo not found. Install Rust from https://rustup.rs then re-run.
    pause
    exit /b 1
  )
  cargo build --release
  if errorlevel 1 (
    echo ERROR: cargo build failed.
    pause
    exit /b 1
  )
)

if not exist "data" mkdir data

REM Detach: VBS starts desktop with hidden console and does not wait.
wscript //nologo "%~dp0启动需求引擎.vbs"
exit /b 0
