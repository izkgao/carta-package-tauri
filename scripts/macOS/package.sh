#!/bin/bash

# Set paths
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
BIN="$PROJECT_ROOT/src-tauri/backend/bin/carta_backend"
LIBDIR="$PROJECT_ROOT/src-tauri/backend/libs"

# Unlock keychain
security unlock-keychain ~/Library/Keychains/login.keychain-db

# Get signing identity
CODESIGN_LINE=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -n 1)
ID=$(echo "$CODESIGN_LINE" | awk '{print $2}')
IDENTITY=$(echo "$CODESIGN_LINE" | sed -n 's/.*(\([A-Z0-9]\{10\}\)).*/\1/p')

# Set environment variables for notarization
export APPLE_SIGNING_IDENTITY="$IDENTITY"
export APPLE_ID=""
export APPLE_PASSWORD=""
export APPLE_TEAM_ID="$IDENTITY"

# Sign binary
codesign --force --deep --options runtime --timestamp --sign "$ID" "$BIN"

# Sign libraries
for lib in "$LIBDIR"/*; do
    codesign --force --deep --options runtime --timestamp --sign "$ID" "$lib"
done

# Package
cd $PROJECT_ROOT/src-tauri
cargo clean --release
cargo tauri build --bundles dmg