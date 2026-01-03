#!/bin/bash

FRONTEND_BUILD_PATH=$1
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)

FRONTENDDIR="$PROJECT_ROOT/src-tauri/frontend"

# Clean up previous copied files
rm -rf "$FRONTENDDIR"

mkdir -p "$FRONTENDDIR"

touch "$FRONTENDDIR/.gitkeep"

# Copy the frontend files
echo "Copy frontend files..."
cp -r "$FRONTEND_BUILD_PATH"/* "$FRONTENDDIR/"
echo "Done!"
