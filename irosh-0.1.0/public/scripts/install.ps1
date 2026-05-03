# irosh - Pro Windows Installation Script
# Supports: Windows 10, Windows 11, Windows Server

param(
    [Parameter(Position=0)]
    [ValidateSet("server", "client", "")]
    [string]$Mode = "",
    [Parameter()]
    [switch]$Service
)

$ErrorActionPreference = "Stop"

# --- Help Function ---
if ($args -contains "help" -or $args -contains "-h" -or $args -contains "/?") {
    Write-Host "irosh installer - Install irosh P2P SSH binaries" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  iwr irosh.pages.dev/ps | iex [OPTIONS]"
    Write-Host "  (Or download and run: .\install.ps1 [OPTIONS])"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  server    Install only the server binary"
    Write-Host "  client    Install only the client binary"
    Write-Host "  -Service  Enable background service after installation (Server set only)"
    Write-Host "  help      Show this help message"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  # Install everything and start server as a background service"
    Write-Host "  iex `"(iwr irosh.pages.dev/ps).Content | & { process { `$input | iex } } -Service`""
    Write-Host "  # (Or more simply: iex `& { $(iwr irosh.pages.dev/ps) } -Service` )"
    Write-Host ""
    Write-Host "  # Install only server as a service"
    Write-Host "  iex `"& { $(iwr irosh.pages.dev/ps) } server -Service`""
    exit
}

# --- Configuration ---
$Repo = "shedrackgodstime/irosh"

Write-Host "`n🚀 Installing irosh P2P SSH Suite for Windows..." -ForegroundColor Cyan
Write-Host "--------------------------------------------------" -ForegroundColor Blue

# --- 1. Detect Environment ---
$Arch = $Env:PROCESSOR_ARCHITECTURE
if ($Arch -eq "AMD64") {
    $TargetArch = "x86_64"
} elseif ($Arch -eq "ARM64") {
    $TargetArch = "aarch64"
} else {
    Write-Error "❌ Error: Unsupported Architecture: $Arch"
}

$AssetName = "irosh-$TargetArch-pc-windows-msvc.tar.gz"
$ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"

# --- 2. Resolve Latest Version ---
Write-Host "📡 Fetching latest release info..."
$ReleaseInfo = Invoke-RestMethod -Uri $ReleaseUrl
$DownloadUrl = ($ReleaseInfo.assets | Where-Object { $_.name -eq $AssetName }).browser_download_url

if (-not $DownloadUrl) {
    Write-Error "❌ Error: Could not find asset $AssetName in the latest release."
}

# --- 3. Secure Download & Unpack ---
$TmpDir = Join-Path $env:TEMP "irosh-install-$(Get-Random)"
New-Item -ItemType Directory -Path $TmpDir | Out-Null
$ZipPath = Join-Path $TmpDir "irosh.tar.gz"

Write-Host "📥 Downloading $AssetName..."
Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath

Write-Host "📦 Unpacking binaries..."
tar -xzf $ZipPath -C $TmpDir

# --- 4. Smart Installation ---
$InstallDir = Join-Path $env:LOCALAPPDATA "irosh\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

if ($Mode -eq "server") {
    Copy-Item (Join-Path $TmpDir "irosh-server.exe") $InstallDir -Force
    Write-Host "✅ Installed irosh-server to $InstallDir" -ForegroundColor Green
} elseif ($Mode -eq "client") {
    Copy-Item (Join-Path $TmpDir "irosh-client.exe") $InstallDir -Force
    Write-Host "✅ Installed irosh-client to $InstallDir" -ForegroundColor Green
} else {
    Copy-Item (Join-Path $TmpDir "irosh.exe") $InstallDir -Force
    Copy-Item (Join-Path $TmpDir "irosh-server.exe") $InstallDir -Force
    Copy-Item (Join-Path $TmpDir "irosh-client.exe") $InstallDir -Force
    Write-Host "✅ Installed irosh Suite (Manager, Server & Client) to $InstallDir" -ForegroundColor Green
}

# Add to User PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Write-Host "⚙️ Adding $InstallDir to User PATH..." -ForegroundColor Yellow
    $NewPath = "$UserPath;$InstallDir"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$env:Path;$InstallDir"
}

# --- 5. Clean up ---
Remove-Item $TmpDir -Recurse -Force

# --- 6. Optional Service Setup ---
if ($Service) {
    if ($Mode -ne "client") {
        Write-Host "⚙️ Setting up background service..." -ForegroundColor Yellow
        Start-Process (Join-Path $InstallDir "irosh-server.exe") -ArgumentList "service", "install" -Wait -NoNewWindow
    } else {
        Write-Host "⚠️ Ignoring -Service flag (not installing server binary)." -ForegroundColor Red
    }
}

# --- 6. Success & Guidance ---
Write-Host "`n✅ Success! irosh has been installed to $InstallDir" -ForegroundColor Green
Write-Host "--------------------------------------------------" -ForegroundColor Blue
Write-Host "👉 To start your server, run: irosh-server --simple"
Write-Host "👉 To list saved peers:      irosh list"
Write-Host "👉 To uninstall:              iwr irosh.pages.dev/uninstall-ps | iex"
Write-Host "👉 Restart your terminal to refresh the PATH.`n"