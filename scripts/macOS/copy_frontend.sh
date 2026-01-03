#!/bin/bash
set -u

fail() {
    echo "Error: $*" >&2
    exit 1
}

FRONTEND_BUILD_PATH=${1:-}
if [ -z "$FRONTEND_BUILD_PATH" ]; then
    fail "Usage: $0 <frontend_build_path>"
fi
if [ ! -d "$FRONTEND_BUILD_PATH" ]; then
    fail "Frontend build path not found: $FRONTEND_BUILD_PATH"
fi
if ! find "$FRONTEND_BUILD_PATH" -mindepth 1 -maxdepth 1 ! -name '.*' -print -quit | grep -q .; then
    fail "Frontend build path is empty: $FRONTEND_BUILD_PATH"
fi

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd) || fail "Unable to resolve script directory"
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd) || fail "Unable to resolve project root"

FRONTENDDIR="$PROJECT_ROOT/src-tauri/frontend"

# Clean up previous copied files
rm -rf "$FRONTENDDIR" || fail "Failed to remove $FRONTENDDIR"

mkdir -p "$FRONTENDDIR" || fail "Failed to create $FRONTENDDIR"

touch "$FRONTENDDIR/.gitkeep" || fail "Failed to create $FRONTENDDIR/.gitkeep"

# Copy the frontend files
echo "Copy frontend files..."
cp -r "$FRONTEND_BUILD_PATH"/* "$FRONTENDDIR/" || fail "Failed to copy frontend files"
echo "Done!"
