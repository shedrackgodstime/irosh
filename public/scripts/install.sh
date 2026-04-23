#!/bin/sh
# irosh - Pro Installation Script
# Supports: Linux, macOS, Android (Termux)

set -e

# --- Help Function ---
show_help() {
    printf "irosh installer - Install irosh P2P SSH binaries\n\n"
    printf "Usage:\n"
    printf "  curl -fsSL irosh.pages.dev/install | sh [OPTIONS]\n\n"
    printf "Options:\n"
    printf "  server    Install only the server binary\n"
    printf "  client    Install only the client binary\n"
    printf "  service   Enable background service after installation (Server set only)\n"
    printf "  help      Show this help message\n\n"
    printf "Examples:\n"
    printf "  # Install everything and start server as a background service\n"
    printf "  curl -fsSL irosh.pages.dev/install | sh -s -- service\n\n"
    printf "  # Install only server as a service\n"
    printf "  curl -fsSL irosh.pages.dev/install | sh -s -- server service\n"
    exit 0
}

# --- Configuration ---
REPO="shedrackgodstime/irosh"
MODE=""  # Can be 'server', 'client', or empty (both)

# --- Parse Arguments ---
INSTALL_SERVICE=false
for arg in "$@"; do
    case "$arg" in
        server|client) MODE="$arg" ;;
        service) INSTALL_SERVICE=true ;;
        help|--help|-h) show_help ;;
    esac
done

# --- Aesthetic Header ---
printf "\n\033[1;36m[*] Installing irosh P2P SSH Suite...\033[0m\n"
printf "\033[0;34m--------------------------------------------------\033[0m\n"

# --- 1. Detect Environment ---
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux)
    case "$ARCH" in
      x86_64) TARGET_ARCH="x86_64"; PLATFORM="unknown-linux-gnu" ;;
      aarch64|arm64) TARGET_ARCH="aarch64"; PLATFORM="unknown-linux-musl" ;;
      *) printf "\n\033[0;31m[-] Error: Unsupported Linux Architecture: $ARCH\033[0m\n"; exit 1 ;;
    esac
    ;;
  darwin)
    PLATFORM="apple-darwin"
    case "$ARCH" in
      x86_64) TARGET_ARCH="x86_64" ;;
      aarch64|arm64) TARGET_ARCH="aarch64" ;;
      *) printf "\n\033[0;31m[-] Error: Unsupported macOS Architecture: $ARCH\033[0m\n"; exit 1 ;;
    esac
    ;;
  *)
    printf "\n\033[0;31m[-] Error: Unsupported OS: $OS\033[0m\n"
    exit 1
    ;;
esac

ASSET_NAME="irosh-${TARGET_ARCH}-${PLATFORM}.tar.gz"
RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"

# --- 2. Resolve Latest Version ---
printf "[*] Fetching latest release info...\n"
DOWNLOAD_URL=$(curl -s "$RELEASE_URL" | grep "browser_download_url" | grep "$ASSET_NAME" | cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
  printf "\n\033[0;31m[-] Error: Could not find asset $ASSET_NAME in the latest release.\033[0m\n"
  exit 1
fi

# --- 3. Secure Download & Unpack ---
TMP_DIR=$(mktemp -d)
printf "[+] Downloading $ASSET_NAME...\n"
curl -sL "$DOWNLOAD_URL" -o "$TMP_DIR/irosh.tar.gz"

printf "[*] Unpacking binaries...\n"
tar -xzf "$TMP_DIR/irosh.tar.gz" -C "$TMP_DIR"

# --- 4. Smart Installation ---
DEST_DIR="/usr/local/bin"
if [ ! -w "$DEST_DIR" ]; then
  DEST_DIR="$HOME/.local/bin"
  mkdir -p "$DEST_DIR"
fi

if [ "$MODE" = "server" ]; then
  cp "$TMP_DIR/irosh-server" "$DEST_DIR/"
  chmod +x "$DEST_DIR/irosh-server"
  printf "[+] Installed irosh-server to $DEST_DIR\n"
elif [ "$MODE" = "client" ]; then
  cp "$TMP_DIR/irosh-client" "$DEST_DIR/"
  chmod +x "$DEST_DIR/irosh-client"
  printf "[+] Installed irosh-client to $DEST_DIR\n"
else
  cp "$TMP_DIR/irosh" "$DEST_DIR/"
  cp "$TMP_DIR/irosh-server" "$DEST_DIR/"
  cp "$TMP_DIR/irosh-client" "$DEST_DIR/"
  chmod +x "$DEST_DIR/irosh" "$DEST_DIR/irosh-server" "$DEST_DIR/irosh-client"
  printf "[+] Installed irosh Suite (Manager, Server & Client) to $DEST_DIR\n"
fi

# --- 5. Clean up ---
rm -rf "$TMP_DIR"

# --- 6. Optional Service Setup ---
if [ "$INSTALL_SERVICE" = true ]; then
  if [ "$MODE" != "client" ]; then
    printf "[*] Setting up background service...\n"
    "$DEST_DIR/irosh-server" service install || printf "[!] Failed to install background service automatically.\n"
  else
    printf "[!] Ignoring 'service' flag (not installing server binary).\n"
  fi
fi

# --- 7. Success & Identity Preview ---
printf "\n\033[1;32m[+] Success! irosh has been installed to $DEST_DIR\033[0m\n"
printf "\033[0;34m--------------------------------------------------\033[0m\n"

# Initialize and show identity
if [ -f "$DEST_DIR/irosh" ]; then
    printf "\033[0;36m"
    "$DEST_DIR/irosh" identity 2>/dev/null || true
    printf "\033[0m"
fi

# Verify if the destination is in PATH
if ! echo "$PATH" | grep -q "$DEST_DIR"; then
    printf "\033[0;33m[!] Warning: $DEST_DIR is not in your PATH.\033[0m\n"
    printf "To run irosh commands directly, add this to your .bashrc or .zshrc:\n"
    printf "  \033[1mexport PATH=\"\$PATH:$DEST_DIR\"\033[0m\n\n"
    printf "Or run them using the full path:\n"
    printf "  \033[1m$DEST_DIR/irosh-server --simple\033[0m\n"
else
    printf " * To start your server, run: \033[1m irosh-server --simple \033[0m\n"
    printf " * To list saved peers:      \033[1m irosh list \033[0m\n"
    printf " * To run in background:     \033[1m irosh-server service install \033[0m\n"
fi

printf " * To uninstall:             \033[1m curl -fsSL irosh.pages.dev/uninstall | sh \033[0m\n"
printf "\n"