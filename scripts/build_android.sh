#!/bin/bash
set -e

# Configuration
TARGET="aarch64-unknown-linux-musl"
TOOLCHAIN_DIR="/home/kristency/Tools/aarch64-linux-musl-cross"
BIN_DIR="$TOOLCHAIN_DIR/bin"
OUTPUT_DIR="target/$TARGET/release"
BINARY_NAME="irosh"
REMOTE_PATH="/data/local/tmp/$BINARY_NAME"

echo "Checking environment..."
if [ ! -d "$TOOLCHAIN_DIR" ]; then
    echo "Error: musl toolchain not found at $TOOLCHAIN_DIR"
    exit 1
fi

echo "Building static binary for $TARGET..."
# Set up environment for cross-compilation
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$BIN_DIR/aarch64-linux-musl-gcc"
export CC_aarch64_unknown_linux_musl="$BIN_DIR/aarch64-linux-musl-gcc"
export AR_aarch64_unknown_linux_musl="$BIN_DIR/aarch64-linux-musl-ar"

cargo build --target $TARGET --release -p irosh-cli

echo "Build successful. Output: $OUTPUT_DIR/$BINARY_NAME"

# Check for adb devices
DEVICES=$(adb devices | grep -v "List" | grep "device" | wc -l)
if [ "$DEVICES" -eq 0 ]; then
    echo "Warning: No Android devices connected via adb."
    echo "To deploy manually: adb push $OUTPUT_DIR/$BINARY_NAME $REMOTE_PATH"
else
    echo "Pushing to device..."
    adb push "$OUTPUT_DIR/$BINARY_NAME" "$REMOTE_PATH"
    adb shell "chmod +x $REMOTE_PATH"
    echo "Done! You can run it with: adb shell $REMOTE_PATH"
fi

echo ""
echo "To move into Termux and run:"
echo "---------------------------"
echo "Inside Termux, run:"
echo "cp $REMOTE_PATH ~/irosh && chmod +x ~/irosh"
echo "./irosh <target>"
