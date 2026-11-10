@echo off
cd /d "%~dp0"
if not exist "target\release\perimon.exe" (
    echo Building perimon release binary...
    cargo build --release
)
start "" "target\release\perimon.exe" %*
