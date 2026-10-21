#!/bin/bash
set -e

# Configuration
#
# The musl cross toolchain location is machine-specific. Override it with the
# IROSH_MUSL_TOOLCHAIN_DIR environment variable, or by setting TOOLCHAIN_DIR
# before invoking this script. If neither is set, fall back to whatever
# `aarch64-linux-musl-gcc` is on PATH (installed by e.g. `musl-tools` or
# `cargo install cargo-zigbuild` setups).
TARGET="aarch64-unknown-linux-musl"
if [ -n "${IROSH_MUSL_TOOLCHAIN_DIR:-}" ]; then
    TOOLCHAIN_DIR="$IROSH_MUSL_TOOLCHAIN_DIR"
elif [ -n "${TOOLCHAIN_DIR:-}" ]; then
    : # honour a pre-set TOOLCHAIN_DIR
else
    TOOLCHAIN_DIR=""
fi
if [ -n "$TOOLCHAIN_DIR" ]; then
    BIN_DIR="$TOOLCHAIN_DIR/bin"
else
    BIN_DIR=""
fi
OUTPUT_DIR="target/$TARGET/release"
BINARY_NAME="irosh"
REMOTE_PATH="/sdcard/$BINARY_NAME"
TERMUX_HOME="/data/data/com.termux/files/home"
TERMUX_PATH="$TERMUX_HOME/$BINARY_NAME"

echo "Checking environment..."
if [ -n "$BIN_DIR" ]; then
    if [ ! -d "$TOOLCHAIN_DIR" ]; then
        echo "Error: musl toolchain not found at $TOOLCHAIN_DIR"
        echo "Set IROSH_MUSL_TOOLCHAIN_DIR to the toolchain directory, or leave it unset to use the toolchain on PATH."
        exit 1
    fi
    CC_BIN="$BIN_DIR/aarch64-linux-musl-gcc"
    AR_BIN="$BIN_DIR/aarch64-linux-musl-ar"
else
    if ! command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
        echo "Error: aarch64-linux-musl-gcc not found."
        echo "Either install a musl cross toolchain and set IROSH_MUSL_TOOLCHAIN_DIR,"
        echo "or put aarch64-linux-musl-gcc on PATH."
        exit 1
    fi
    CC_BIN="$(command -v aarch64-linux-musl-gcc)"
    AR_BIN="$(command -v aarch64-linux-musl-ar || true)"
fi

echo "Building static binary for $TARGET..."
# Set up environment for cross-compilation
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$CC_BIN"
export CC_aarch64_unknown_linux_musl="$CC_BIN"
if [ -n "$AR_BIN" ]; then
    export AR_aarch64_unknown_linux_musl="$AR_BIN"
fi

cargo build --target $TARGET --release -p irosh-cli

echo "Build successful. Output: $OUTPUT_DIR/$BINARY_NAME"

# Check for adb devices
DEVICES=$(adb devices | grep -v "List" | grep "device" | wc -l)
if [ "$DEVICES" -eq 0 ]; then
    echo "Warning: No Android devices connected via adb."
    echo "To deploy manually:"
    echo "  adb push $OUTPUT_DIR/$BINARY_NAME $REMOTE_PATH"
    echo "  adb shell \"run-as com.termux cp $REMOTE_PATH $TERMUX_PATH && chmod +x $TERMUX_PATH\""
else
    echo "Pushing to device..."
    adb push "$OUTPUT_DIR/$BINARY_NAME" "$REMOTE_PATH"
    adb shell "run-as com.termux cp $REMOTE_PATH $TERMUX_PATH"
    adb shell "run-as com.termux chmod +x $TERMUX_PATH"
    echo "Done! You can run it with: adb shell run-as com.termux $TERMUX_PATH <target>"
fi

echo ""
echo "To run in Termux:"
echo "---------------------------"
echo "Inside Termux, run:"
echo "chmod +x $TERMUX_PATH"
echo "$TERMUX_PATH <target>"
