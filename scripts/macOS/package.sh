#!/bin/bash

# Get signing identity
CODESIGN_LINE=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -n 1)
ID=$(echo "$CODESIGN_LINE" | awk '{print $2}')
IDENTITY=$(echo "$CODESIGN_LINE" | sed -n 's/.*(\([A-Z0-9]\{10\}\)).*/\1/p')

# Set environment variables for notarization
export APPLE_SIGNING_IDENTITY="$IDENTITY"
export APPLE_ID=""
export APPLE_PASSWORD=""
export APPLE_TEAM_ID="$IDENTITY"

# Check for required credentials
if [ -z "$APPLE_ID" ] || [ -z "$APPLE_PASSWORD" ]; then
    echo "Error: APPLE_ID and APPLE_PASSWORD must be set in the script."
    exit 1
fi

# Set paths
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
BIN="$PROJECT_ROOT/src-tauri/backend/bin/carta_backend"
LIBDIR="$PROJECT_ROOT/src-tauri/backend/libs"

# Unlock keychain
ENCRYPTED_PW_FILE="$HOME/encrypted_password.enc"
OPENSSL_PASS="pass:Qi0GBwUgrLFC1gAcp6di"

if [ -f "$ENCRYPTED_PW_FILE" ]; then
    password=$(/opt/homebrew/bin/openssl enc -aes-256-cbc -d -a -salt -iter 100 -pass "$OPENSSL_PASS" -in "$ENCRYPTED_PW_FILE")
else
    echo "Encrypted password file not found."
    read -rs -p "Enter keychain password: " user_password
    echo
    echo -n "$user_password" | /opt/homebrew/bin/openssl enc -aes-256-cbc -a -salt -iter 100 -pass "$OPENSSL_PASS" -out "$ENCRYPTED_PW_FILE"
    password="$user_password"
    echo "Encrypted password file created at $ENCRYPTED_PW_FILE"
fi

security unlock-keychain -p "$password" ~/Library/Keychains/login.keychain-db

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