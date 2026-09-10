@echo off
setlocal

title irosh installer

rem ============================================================
rem  irosh - Windows one-liner installer (cmd.exe)
rem  Downloads install.ps1 from the irosh Pages site and runs it.
rem
rem  Usage:
rem    curl -fsSL https://irosh.pages.dev/install.cmd -o install.cmd && install.cmd
rem    install.cmd -Service
rem
rem  -Service  also installs the background server service
rem ============================================================

for %%a in (%*) do if /i "%%~a"=="help" goto :help
if /i "%~1"=="/?" goto :help
if /i "%~1"=="-h" goto :help
if /i "%~1"=="--help" goto :help

set "PS_URL=https://irosh.pages.dev/ps"
set "PS1=%TEMP%\irosh-install-%RANDOM%.ps1"

echo [*] Fetching irosh installer...
curl -fsSL "%PS_URL%" -o "%PS1%" 2>nul
if errorlevel 1 (
    echo [+] curl unavailable or failed - retrying with PowerShell...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Invoke-WebRequest -Uri '%PS_URL%' -OutFile '%PS1%'"
    if errorlevel 1 (
        echo [-] Error: could not download the irosh installer.
        echo     URL: %PS_URL%
        exit /b 1
    )
)

echo [*] Running installer...
powershell -NoProfile -ExecutionPolicy Bypass -File "%PS1%" %*
set "RC=%ERRORLEVEL%"
del "%PS1%" >nul 2>&1
exit /b %RC%

:help
echo irosh installer - Install the unified irosh P2P SSH tool
echo.
echo Usage:
echo   curl -fsSL https://irosh.pages.dev/install.cmd -o install.cmd ^&^& install.cmd
echo.
echo Options:
echo   -Service  Also enable the background server service after installation
echo   help      Show this help message
echo.
echo Note: run from cmd.exe, not PowerShell. For PowerShell use:
echo   iwr irosh.pages.dev/ps ^| iex
exit /b 0