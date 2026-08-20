@echo off
rem Builds and runs the Nuvio desktop client.
rem
rem Same three steps as `npm run dev`, but each one is checked and named, so a
rem failure says which stage broke instead of scrolling past. Double-click it or
rem run `dev.bat` from a terminal; pass `release` for an optimised build.
setlocal

rem Run from this file's own folder, so double-clicking works regardless of
rem where the shell happens to be.
cd /d "%~dp0"

set "PROFILE_ARGS="
set "PROFILE_NAME=debug"
if /i "%~1"=="release" (
  set "PROFILE_ARGS=--release"
  set "PROFILE_NAME=release"
)

rem cargo cannot replace an executable Windows still has open, and the error it
rem gives for that is a bare "Access is denied (os error 5)" against a path.
tasklist /fi "imagename eq nuvio-rust-webview-poc.exe" 2>nul | find /i "nuvio-rust-webview-poc.exe" >nul
if not errorlevel 1 (
  echo.
  echo Nuvio is already running.
  echo Close the window first - the build cannot replace the running .exe.
  echo.
  exit /b 1
)

echo [1/3] libmpv runtime
rem `call` is required for npm: it is itself a .cmd, and without call this
rem script would exit at the first npm invocation rather than continuing.
call npm run prepare:runtime
if errorlevel 1 goto :failed

echo.
echo [2/3] UI bundle
call npm run build:shared-ui
if errorlevel 1 goto :failed

echo.
echo [3/3] Rust shell (%PROFILE_NAME%)
cargo run --manifest-path shell\Cargo.toml %PROFILE_ARGS%
if errorlevel 1 goto :failed

exit /b 0

:failed
set "CODE=%errorlevel%"
echo.
echo Failed at the step above with exit code %CODE%.
rem Keep the window up when this was double-clicked, so the error is readable
rem rather than vanishing with the console.
if "%~1"=="" pause
exit /b %CODE%
