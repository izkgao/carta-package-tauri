#!/bin/bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)

cd "$SCRIPT_DIR"

if [ "${1:-}" = "" ]; then
  echo "Usage: $0 <path-to-AppImage>" >&2
  exit 1
fi

# Clean up previous extracted files
echo "Cleaning up previous extracted files..."
rm -rf squashfs-root
rm -rf "$PROJECT_ROOT/src-tauri/backend/bin"
rm -rf "$PROJECT_ROOT/src-tauri/backend/libs"
rm -rf "$PROJECT_ROOT/src-tauri/backend/etc"
rm -rf "$PROJECT_ROOT/src-tauri/frontend"

# Extract AppImage
echo "Extracting AppImage..."
APPIMAGE="$1"
if [ ! -f "$APPIMAGE" ]; then
  echo "Error: AppImage not found: $APPIMAGE" >&2
  exit 1
fi

chmod +x "$APPIMAGE"
./"$APPIMAGE" --appimage-extract > /dev/null 2>&1

# Create directories
echo "Preparing directories..."
mkdir -p "$PROJECT_ROOT/src-tauri/backend/bin"
mkdir -p "$PROJECT_ROOT/src-tauri/backend/libs"
mkdir -p "$PROJECT_ROOT/src-tauri/backend/etc"
mkdir -p "$PROJECT_ROOT/src-tauri/frontend"

# Copy backend
echo "Copying backend files..."
cp squashfs-root/bin/carta_backend "$PROJECT_ROOT/src-tauri/backend/bin/."
cp squashfs-root/lib/* "$PROJECT_ROOT/src-tauri/backend/libs/."
cp -R squashfs-root/etc/* "$PROJECT_ROOT/src-tauri/backend/etc/."

# Copy frontend
echo "Copying frontend files..."
cp -R squashfs-root/share/carta/frontend/* "$PROJECT_ROOT/src-tauri/frontend/."

# Delete extracted files
echo "Cleaning up..."
rm -rf squashfs-root
echo "Done!"
