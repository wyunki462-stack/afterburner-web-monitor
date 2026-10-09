@echo off
REM Build script: AfterburnerWebMonitor v2.4.0 (Rust)
REM Notes:
REM  - Bypass rustup shim, call real toolchain binary directly
REM  - CARGO_TARGET_DIR must be a pure-ASCII path, otherwise GNU ld
REM    fails with "Non-UTF-8 output" when the user profile has non-ASCII chars
REM  - Edit MINGW and TC below if your paths differ

set MINGW=C:\Users\%USERNAME%\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin
set TC=%USERPROFILE%\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin
set PATH=%MINGW%;%TC%;%PATH%
set CARGO_TARGET_DIR=D:\abwm_target

echo [1/2] Building release...
"%TC%\cargo.exe" build --release
if errorlevel 1 (
    echo Build failed
    exit /b 1
)

echo [2/2] Copying artifact...
set OUTDIR=%~dp0dist
if not exist "%OUTDIR%" mkdir "%OUTDIR%"
copy /Y "%CARGO_TARGET_DIR%\release\AfterburnerWebMonitor.exe" "%OUTDIR%\AfterburnerWebMonitor.exe"
echo Done: %OUTDIR%\AfterburnerWebMonitor.exe
