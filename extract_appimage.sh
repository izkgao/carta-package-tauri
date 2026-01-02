#!/bin/bash

# Extract AppImage
echo "Extracting AppImage..."
chmod +x carta-$(arch).AppImage
./carta-$(arch).AppImage --appimage-extract > /dev/null 2>&1

# Create directories
echo "Preparing directories..."
mkdir -p src-tauri/backend/bin
mkdir -p src-tauri/backend/libs
mkdir -p src-tauri/backend/etc
mkdir -p src-tauri/frontend

# Copy backend
echo "Copying backend files..."
cp squashfs-root/bin/carta_backend src-tauri/backend/bin/.
cp squashfs-root/lib/* src-tauri/backend/libs/.
cp -R squashfs-root/etc/* src-tauri/backend/etc/.

# Copy frontend
echo "Copying frontend files..."
cp -R squashfs-root/share/carta/frontend/* src-tauri/frontend/.

# Delete extracted files
echo "Cleaning up..."
rm -rf squashfs-root
echo "Done!"