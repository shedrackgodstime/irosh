# irosh - Pro Windows Installation Script
# Supports: Windows 10, Windows 11, Windows Server

param(
    [Parameter()]
    [switch]$Service
)

$ErrorActionPreference = "Stop"

# --- Help Function ---
if ($args -contains "help" -or $args -contains "-h" -or $args -contains "/?") {
    Write-Host "irosh installer - Install the unified irosh P2P SSH tool" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  iwr irosh.pages.dev/ps | iex [OPTIONS]"
    Write-Host "  (Or download and run: .\install.ps1 [OPTIONS])"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Service  Enable background server service after installation"
    Write-Host "  help      Show this help message"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  # Install everything"
    Write-Host "  iwr irosh.pages.dev/ps | iex"
    Write-Host ""
    Write-Host "  # Install and start server as a background service"
    Write-Host "  iex `& { $(iwr irosh.pages.dev/ps) } -Service` "
    exit
}

# --- Configuration ---
$Repo = "shedrackgodstime/irosh"

Write-Host "`n[*] Installing irosh P2P SSH Tool for Windows..." -ForegroundColor Cyan
Write-Host "--------------------------------------------------" -ForegroundColor Blue

# --- 1. Detect Environment ---
$Arch = $Env:PROCESSOR_ARCHITECTURE
if ($Arch -eq "AMD64") {
    $TargetArch = "x86_64"
} elseif ($Arch -eq "ARM64") {
    $TargetArch = "aarch64"
} else {
    Write-Error "[-] Error: Unsupported Architecture: $Arch"
}

$AssetName = "irosh-$TargetArch-pc-windows-msvc.tar.gz"
$ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"

# --- 2. Resolve Latest Version ---
Write-Host "[*] Fetching latest release info..."
$ReleaseInfo = Invoke-RestMethod -Uri $ReleaseUrl
$DownloadUrl = ($ReleaseInfo.assets | Where-Object { $_.name -eq $AssetName }).browser_download_url

if (-not $DownloadUrl) {
    Write-Error "[-] Error: Could not find asset $AssetName in the latest release."
}

# --- 3. Secure Download & Unpack ---
$TmpDir = Join-Path $env:TEMP "irosh-install-$(Get-Random)"
New-Item -ItemType Directory -Path $TmpDir | Out-Null
$ZipPath = Join-Path $TmpDir "irosh.tar.gz"

Write-Host "[+] Downloading $AssetName..."
Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath

Write-Host "[*] Unpacking binary..."
tar -xzf $ZipPath -C $TmpDir

# --- 4. Smart Installation ---
$InstallDir = Join-Path $env:LOCALAPPDATA "irosh\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

# Install the unified binary
Copy-Item (Join-Path $TmpDir "irosh.exe") $InstallDir -Force
Write-Host "[+] Installed irosh to $InstallDir" -ForegroundColor Green

# Add to User PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Write-Host "[*] Adding $InstallDir to User PATH..." -ForegroundColor Yellow
    $NewPath = "$UserPath;$InstallDir"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$env:Path;$InstallDir"
}

# --- 5. Clean up ---
Remove-Item $TmpDir -Recurse -Force

# --- 6. Optional Firewall Rules (Requires Admin) ---
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if ($isAdmin) {
    Write-Host "[*] Registering firewall rules for P2P connectivity..." -ForegroundColor Yellow
    $IroshExe = Join-Path $InstallDir "irosh.exe"

    if (Test-Path $IroshExe) {
        New-NetFirewallRule -DisplayName "Irosh P2P (UDP-In)" -Direction Inbound -Action Allow -Protocol UDP -Program $IroshExe -ErrorAction SilentlyContinue | Out-Null
    }
} else {
    Write-Host "[i] Info: Skipping firewall registration (Not running as Administrator)." -ForegroundColor Gray
}

# --- 7. Optional Service Setup ---
if ($Service) {
    Write-Host "[*] Setting up background server service..." -ForegroundColor Yellow
    Start-Process (Join-Path $InstallDir "irosh.exe") -ArgumentList "system", "install" -Wait -NoNewWindow
}

# --- 8. Success & Identity Preview ---
Write-Host "`n[+] Success! irosh has been installed to $InstallDir" -ForegroundColor Green
Write-Host "--------------------------------------------------" -ForegroundColor Blue

# Initialize and show identity
if (Test-Path (Join-Path $InstallDir "irosh.exe")) {
    try {
        & (Join-Path $InstallDir "irosh.exe") identity | Out-String | Write-Host -ForegroundColor Cyan
    } catch {
        # Ignore identity errors during install
    }
}

Write-Host "`n * To start your server:      irosh host"
Write-Host " * To connect to a node:      irosh <ticket>"
Write-Host " * To manage saved peers:     irosh peer list"
Write-Host " * To run in background:      irosh system install"
Write-Host " * To uninstall:              iwr irosh.pages.dev/uninstall-ps | iex"
Write-Host " * Restart your terminal to refresh the PATH.`n"