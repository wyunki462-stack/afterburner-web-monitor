@echo off
REM 构建脚本：AfterburnerWebMonitor v2.0 (Rust)
REM 绕过 rustup shim，直接用真实工具链 + MinGW

set MINGW=C:\Users\刘锦城\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin
set TC=C:\Users\刘锦城\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin
set PATH=%MINGW%;%TC%;%PATH%

echo [1/2] 编译 release 版本...
"%TC%\cargo.exe" build --release
if errorlevel 1 (
    echo 编译失败
    exit /b 1
)

echo [2/2] 复制产物...
copy /Y "target\release\AfterburnerWebMonitor.exe" "..\AfterburnerWebMonitor_2.0.0_x64-portable\AfterburnerWebMonitor.exe"
echo 构建完成: AfterburnerWebMonitor_2.0.0_x64-portable\AfterburnerWebMonitor.exe
