# irosh - Windows Uninstall Script
# Supports: Windows 10, Windows 11, Windows Server

param(
    [switch]$Yes,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

# --- Help Function ---
if ($Help) {
    Write-Host "irosh uninstaller - Remove irosh binaries and services" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  iwr irosh.pages.dev/uninstall-ps | iex [OPTIONS]"
    Write-Host "  (Or download and run: .\uninstall.ps1 [OPTIONS])"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Yes     Skip confirmation prompts and remove everything"
    Write-Host "  -Help    Show this help message"
    Write-Host ""
    Write-Host "What this removes:"
    Write-Host "  - irosh-server.exe and irosh-client.exe binaries"
    Write-Host "  - from PATH environment variable"
    Write-Host "  - optionally, your state directory (%USERPROFILE%\.irosh)"
    exit
}

Write-Host "`n🗑️ Uninstalling irosh..." -ForegroundColor Red
Write-Host "--------------------------------------------------" -ForegroundColor Blue

$Found = $false

# --- Remove binaries ---
$InstallDir = Join-Path $env:LOCALAPPDATA "irosh\bin"

if (Test-Path $InstallDir) {
    $Irosh = Join-Path $InstallDir "irosh.exe"
    $Server = Join-Path $InstallDir "irosh-server.exe"
    $Client = Join-Path $InstallDir "irosh-client.exe"
    
    if (Test-Path $Irosh) {
        Remove-Item $Irosh -Force
        Write-Host "✅ Removed irosh.exe" -ForegroundColor Green
        $Found = $true
    }
    
    if (Test-Path $Server) {
        Remove-Item $Server -Force
        Write-Host "✅ Removed irosh-server.exe" -ForegroundColor Green
        $Found = $true
    }
    
    if (Test-Path $Client) {
        Remove-Item $Client -Force
        Write-Host "✅ Removed irosh-client.exe" -ForegroundColor Green
        $Found = $true
    }
    
    # Remove the directory if empty
    if ((Get-ChildItem $InstallDir -Force | Measure-Object).Count -eq 0) {
        Remove-Item $InstallDir -Force
    }
}

# --- Remove from User PATH ---
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -like "*$InstallDir*") {
    $NewPath = ($UserPath -split ';' | Where-Object { $_ -ne $InstallDir }) -join ';'
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host "✅ Removed $InstallDir from PATH" -ForegroundColor Green
    $Found = $true
}

# --- Ask about state directory ---
$StateDir = Join-Path $env:USERPROFILE ".irosh"
if (Test-Path $StateDir) {
    if ($Yes) {
        Remove-Item $StateDir -Recurse -Force
        Write-Host "✅ Removed state directory" -ForegroundColor Green
    } else {
        Write-Host "`n⚠️  Found state directory: $StateDir" -ForegroundColor Yellow
        Write-Host "   This contains your keys, trust records, and saved peers." -ForegroundColor Yellow
        $answer = Read-Host "   Do you want to remove it? (y/N)"
        
        if ($answer -eq "y" -or $answer -eq "Y") {
            Remove-Item $StateDir -Recurse -Force
            Write-Host "✅ Removed state directory" -ForegroundColor Green
        } else {
            Write-Host "   Preserved state directory" -ForegroundColor Gray
        }
    }
}

if (-not $Found) {
    Write-Host "`n⚠️  No irosh binaries found in standard locations." -ForegroundColor Yellow
    Write-Host "   You may need to manually remove them." -ForegroundColor Yellow
}

Write-Host "`n✅ Uninstall complete!" -ForegroundColor Green
Write-Host "--------------------------------------------------" -ForegroundColor Blue
Write-Host "👉 To reinstall: iwr irosh.pages.dev/ps | iex`n"