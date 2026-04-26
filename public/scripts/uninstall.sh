#!/bin/sh
# irosh - Uninstall Script
# Supports: Linux, macOS, Android (Termux)

set -e

# --- Help Function ---
show_help() {
    printf "irosh uninstaller - Remove irosh binaries and services\n\n"
    printf "Usage:\n"
    printf "  curl -fsSL irosh.pages.dev/uninstall | sh [OPTIONS]\n\n"
    printf "Options:\n"
    printf "  --yes      Skip confirmation prompts and remove everything\n"
    printf "  help       Show this help message\n\n"
    printf "What this removes:\n"
    printf "  - irosh unified binary\n"
    printf "  - systemd service (if installed)\n"
    printf "  - macOS LaunchAgent (if installed)\n"
    printf "  - optionally, your state directory (~/.irosh)\n"
    exit 0
}

# --- Parse Arguments ---
FORCE=false
for arg in "$@"; do
    case "$arg" in
        --yes) FORCE=true ;;
        help|--help|-h) show_help ;;
    esac
done

printf "\n\033[1;31m🗑️ Uninstalling irosh...\033[0m\n"
printf "\033[0;34m--------------------------------------------------\033[0m\n"

# --- Determine where irosh might be installed ---
DEST_DIRS="/usr/local/bin $HOME/.local/bin"

# --- Remove binaries ---
FOUND=false
for dir in $DEST_DIRS; do
    if [ -f "$dir/irosh" ]; then
        rm -f "$dir/irosh"
        printf "✅ Removed irosh from $dir\n"
        FOUND=true
    fi
    # Cleanup old legacy binaries if they exist
    if [ -f "$dir/irosh-server" ]; then
        rm -f "$dir/irosh-server"
        printf "✅ Removed legacy irosh-server from $dir\n"
        FOUND=true
    fi
    if [ -f "$dir/irosh-client" ]; then
        rm -f "$dir/irosh-client"
        printf "✅ Removed legacy irosh-client from $dir\n"
        FOUND=true
    fi
done

# --- Remove systemd service (Linux) ---
# New service name
if [ -f "$HOME/.config/systemd/user/irosh.service" ]; then
    printf "🛑 Stopping irosh service...\n"
    systemctl --user stop irosh 2>/dev/null || true
    systemctl --user disable irosh 2>/dev/null || true
    rm -f "$HOME/.config/systemd/user/irosh.service"
    printf "✅ Removed systemd service\n"
    FOUND=true
fi
# Legacy service name
if [ -f "$HOME/.config/systemd/user/irosh-server.service" ]; then
    printf "🛑 Stopping legacy irosh-server service...\n"
    systemctl --user stop irosh-server 2>/dev/null || true
    systemctl --user disable irosh-server 2>/dev/null || true
    rm -f "$HOME/.config/systemd/user/irosh-server.service"
    printf "✅ Removed legacy systemd service\n"
    FOUND=true
fi

# --- Remove macOS LaunchAgent ---
# New label
LABEL="dev.irosh.server"
if [ -f "$HOME/Library/LaunchAgents/$LABEL.plist" ]; then
    printf "🛑 Stopping irosh service...\n"
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
    rm -f "$HOME/Library/LaunchAgents/$LABEL.plist"
    printf "✅ Removed macOS LaunchAgent\n"
    FOUND=true
fi
# Legacy label
LEGACY_PLIST="/Library/LaunchDaemons/ai.irosh.server.plist"
if [ -f "$LEGACY_PLIST" ]; then
    printf "🛑 Stopping legacy irosh service...\n"
    sudo launchctl unload "$LEGACY_PLIST" 2>/dev/null || true
    sudo rm -f "$LEGACY_PLIST"
    printf "✅ Removed legacy macOS LaunchDaemon\n"
    FOUND=true
fi

# --- Ask about state directory ---
if [ -d "$HOME/.irosh" ]; then
    if [ "$FORCE" = true ]; then
        rm -rf "$HOME/.irosh"
        printf "✅ Removed state directory\n"
    else
        printf "\n⚠️  Found state directory: $HOME/.irosh\n"
        printf "   This contains your keys, trust records, and saved peers.\n"
        printf "   Do you want to remove it? (y/N): "
        read -r answer
        if [ "$answer" = "y" ] || [ "$answer" = "Y" ]; then
            rm -rf "$HOME/.irosh"
            printf "✅ Removed state directory\n"
        else
            printf "   Preserved state directory at $HOME/.irosh\n"
        fi
    fi
fi

if [ "$FOUND" = false ]; then
    printf "\n\033[0;33m⚠️  No irosh binaries found in standard locations.\033[0m\n"
    printf "   You may need to manually remove them.\n"
fi

printf "\n\033[1;32m✅ Uninstall complete!\033[0m\n"
printf "\033[0;34m--------------------------------------------------\033[0m\n"
printf "👉 To reinstall: \033[1m curl -fsSL irosh.pages.dev/install | sh \033[0m\n"
printf "\n"