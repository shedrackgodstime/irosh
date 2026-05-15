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
    Write-Host "  - irosh unified binary"
    Write-Host "  - from PATH environment variable"
    Write-Host "  - optionally, your state directory (%USERPROFILE%\.irosh)"
    exit
}

Write-Host "`n[*] Uninstalling irosh..." -ForegroundColor Red
Write-Host "--------------------------------------------------" -ForegroundColor Blue

$Found = $false

# --- Detect Privileges ---
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

# --- Stop and Uninstall Service ---
$InstallDir = Join-Path $env:LOCALAPPDATA "irosh\bin"
$Irosh = Join-Path $InstallDir "irosh.exe"

if (Test-Path $Irosh) {
    Write-Host "[*] Cleaning up background service..." -ForegroundColor Yellow
    try {
        # Try to stop and uninstall gracefully using the binary
        & $Irosh system stop | Out-Null
        & $Irosh system uninstall | Out-Null
    } catch {
        # Fallback to direct cleanup if binary fails
        schtasks /delete /tn irosh /f 2>$null
        sc.exe stop irosh 2>$null
        sc.exe delete irosh 2>$null
    }
} else {
    # If binary is gone, try direct cleanup anyway
    schtasks /delete /tn irosh /f 2>$null
    sc.exe stop irosh 2>$null
    sc.exe delete irosh 2>$null
}

# --- Cleanup Firewall Rules (Admin only) ---
if ($isAdmin) {
    Write-Host "[*] Removing firewall rules..." -ForegroundColor Yellow
    Remove-NetFirewallRule -DisplayName "Irosh P2P (UDP-In)" -ErrorAction SilentlyContinue
}

# --- Remove binaries ---
if (Test-Path $InstallDir) {
    $LegacyServer = Join-Path $InstallDir "irosh-server.exe"
    $LegacyClient = Join-Path $InstallDir "irosh-client.exe"
    
    if (Test-Path $Irosh) {
        # Final safety: Ensure process is killed if still alive
        taskkill /IM irosh.exe /F 2>$null
        Remove-Item $Irosh -Force
        Write-Host "[*] Removed irosh.exe" -ForegroundColor Green
        $Found = $true
    }
    
    # Cleanup legacy binaries
    if (Test-Path $LegacyServer) {
        Remove-Item $LegacyServer -Force
        Write-Host "[*] Removed legacy irosh-server.exe" -ForegroundColor Green
        $Found = $true
    }
    
    if (Test-Path $LegacyClient) {
        Remove-Item $LegacyClient -Force
        Write-Host "[*] Removed legacy irosh-client.exe" -ForegroundColor Green
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
    Write-Host "[*] Removed $InstallDir from PATH" -ForegroundColor Green
    $Found = $true
}

# --- Ask about state directory ---
$StateDir = Join-Path $env:USERPROFILE ".irosh"
if (Test-Path $StateDir) {
    if ($Yes) {
        Remove-Item $StateDir -Recurse -Force
        Write-Host "[*] Removed state directory" -ForegroundColor Green
    } else {
        Write-Host "`n[!] Found state directory: $StateDir" -ForegroundColor Yellow
        Write-Host "   This contains your keys, trust records, and saved peers." -ForegroundColor Yellow
        $answer = Read-Host "   Do you want to remove it? (y/N)"
        
        if ($answer -eq "y" -or $answer -eq "Y") {
            Remove-Item $StateDir -Recurse -Force
            Write-Host "[*] Removed state directory" -ForegroundColor Green
        } else {
            Write-Host "   Preserved state directory" -ForegroundColor Gray
        }
    }
}

if (-not $Found) {
    Write-Host "`n[!] No irosh binaries found in standard locations." -ForegroundColor Yellow
    Write-Host "   You may need to manually remove them." -ForegroundColor Yellow
}

Write-Host "`n[*] Uninstall complete!" -ForegroundColor Green
Write-Host "--------------------------------------------------" -ForegroundColor Blue
Write-Host "[*] To reinstall: iwr irosh.pages.dev/ps | iex`n"