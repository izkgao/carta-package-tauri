#!/bin/bash
set -eu
# Enable pipefail when running under bash; dash (POSIX sh) does not support it.
if [ -n "${BASH_VERSION:-}" ]; then
  set -o pipefail
fi

CALL_DIR=$(pwd)
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)

if [ "${1:-}" = "" ]; then
  echo "Usage: $0 <path-to-AppImage>" >&2
  exit 1
fi

APPIMAGE_ARG="$1"
if command -v readlink >/dev/null 2>&1; then
  if ! APPIMAGE=$(cd "$CALL_DIR" && readlink -f "$APPIMAGE_ARG"); then
    case "$APPIMAGE_ARG" in
      /*) APPIMAGE="$APPIMAGE_ARG" ;;
      *) APPIMAGE="$CALL_DIR/$APPIMAGE_ARG" ;;
    esac
  fi
else
  case "$APPIMAGE_ARG" in
    /*) APPIMAGE="$APPIMAGE_ARG" ;;
    *) APPIMAGE="$CALL_DIR/$APPIMAGE_ARG" ;;
  esac
fi

EXTRACT_WORKDIR="$PROJECT_ROOT/.tmp/appimage-extract"
cleanup() {
  rm -rf "$EXTRACT_WORKDIR"
}
trap cleanup EXIT

# Clean up previous extracted files (only extraction workdir; project files cleaned after successful extraction)
echo "Cleaning up previous extracted files..."
rm -rf "$SCRIPT_DIR/squashfs-root"
rm -rf "$EXTRACT_WORKDIR"

# Extract AppImage
echo "Extracting AppImage..."
if [ ! -f "$APPIMAGE" ]; then
  echo "Error: AppImage not found: $APPIMAGE" >&2
  exit 1
fi

mkdir -p "$PROJECT_ROOT/.tmp"
mkdir -p "$EXTRACT_WORKDIR"
cd "$EXTRACT_WORKDIR"

chmod +x "$APPIMAGE" 2>/dev/null || echo "Warning: unable to chmod +x: $APPIMAGE" >&2
extract_with_7z() {
  if command -v 7z >/dev/null 2>&1; then
    SEVEN_Z=7z
  elif command -v 7zz >/dev/null 2>&1; then
    SEVEN_Z=7zz
  elif command -v 7za >/dev/null 2>&1; then
    SEVEN_Z=7za
  else
    echo "Error: 7z not found (required to extract AppImage on this platform)" >&2
    exit 1
  fi

  echo "Using $SEVEN_Z to extract (may take a few minutes)..."
  "$SEVEN_Z" x -y "$APPIMAGE"
}

case "$(uname -s)" in
  Darwin)
    extract_with_7z
    ;;
  *)
    if ! "$APPIMAGE" --appimage-extract > /dev/null 2>&1; then
      echo "Built-in AppImage extraction failed; falling back to 7z..." >&2
      extract_with_7z
    fi
    ;;
esac

EXTRACT_DIR=""
for candidate in \
  "$EXTRACT_WORKDIR/squashfs-root" \
  "$EXTRACT_WORKDIR/squashfs-root/squashfs-root" \
  "$EXTRACT_WORKDIR"
do
  if [ -f "$candidate/bin/carta_backend" ]; then
    EXTRACT_DIR="$candidate"
    break
  fi
done

if [ "$EXTRACT_DIR" = "" ]; then
  echo "Error: Extracted content not found (missing bin/carta_backend)" >&2
  echo "Tried: $EXTRACT_WORKDIR/squashfs-root, $EXTRACT_WORKDIR" >&2
  exit 1
fi

# Clean project directories after successful extraction (avoids leaving the repo empty if extraction fails)
rm -rf "$PROJECT_ROOT/src-tauri/backend/bin"
rm -rf "$PROJECT_ROOT/src-tauri/backend/libs"
rm -rf "$PROJECT_ROOT/src-tauri/backend/etc"
rm -rf "$PROJECT_ROOT/src-tauri/frontend"

# Create directories
echo "Preparing directories..."
mkdir -p "$PROJECT_ROOT/src-tauri/backend/bin"
mkdir -p "$PROJECT_ROOT/src-tauri/backend/libs"
mkdir -p "$PROJECT_ROOT/src-tauri/backend/etc"
mkdir -p "$PROJECT_ROOT/src-tauri/frontend"
touch "$PROJECT_ROOT/src-tauri/backend/bin/.gitkeep"
touch "$PROJECT_ROOT/src-tauri/backend/libs/.gitkeep"
touch "$PROJECT_ROOT/src-tauri/backend/etc/.gitkeep"
touch "$PROJECT_ROOT/src-tauri/frontend/.gitkeep"

# Copy backend
echo "Copying backend files..."
cp "$EXTRACT_DIR/bin/carta_backend" "$PROJECT_ROOT/src-tauri/backend/bin/."
cp "$EXTRACT_DIR/lib/"* "$PROJECT_ROOT/src-tauri/backend/libs/."
cp -R "$EXTRACT_DIR/etc/"* "$PROJECT_ROOT/src-tauri/backend/etc/."

# Copy frontend
echo "Copying frontend files..."
cp -R "$EXTRACT_DIR/share/carta/frontend/"* "$PROJECT_ROOT/src-tauri/frontend/."

# Delete extracted files
echo "Cleaning up..."
rm -rf "$EXTRACT_WORKDIR"
rm -rf "$PROJECT_ROOT/.tmp"
echo "Done!"
