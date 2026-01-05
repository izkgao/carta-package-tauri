#!/bin/bash
set -u

if [ -z "${BASH_VERSION:-}" ]; then
    exec /bin/bash "$0" "$@"
fi

fail() {
    echo "Error: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"
}

BACKEND_BUILD_PATH=${1:-}
if [ -z "$BACKEND_BUILD_PATH" ]; then
    fail "Usage: $0 <backend_build_path>"
fi
if [ ! -d "$BACKEND_BUILD_PATH" ]; then
    fail "Backend build path not found: $BACKEND_BUILD_PATH"
fi

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd) || fail "Unable to resolve script directory"
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd) || fail "Unable to resolve project root"
if [ ! -d "$PROJECT_ROOT/src-tauri" ]; then
    fail "Invalid project root: $PROJECT_ROOT"
fi

BINDIR="$PROJECT_ROOT/src-tauri/backend/bin"
LIBDIR="$PROJECT_ROOT/src-tauri/backend/libs"
ETCDIR="$PROJECT_ROOT/src-tauri/backend/etc"

require_command otool
require_command install_name_tool
require_command tar

# Get the executable's directory
EXEC_DIR=$(cd "$BACKEND_BUILD_PATH" && pwd) || fail "Unable to resolve backend build path: $BACKEND_BUILD_PATH"
BACKEND_EXEC="$EXEC_DIR/carta_backend"
if [ ! -f "$BACKEND_EXEC" ]; then
    fail "Missing carta_backend at $BACKEND_EXEC"
fi

echo "Source executable directory: $EXEC_DIR"

# Clean up previous copied files
rm -rf "$BINDIR" || fail "Failed to remove $BINDIR"
rm -rf "$LIBDIR" || fail "Failed to remove $LIBDIR"
rm -rf "$ETCDIR" || fail "Failed to remove $ETCDIR"

mkdir -p "$BINDIR" "$LIBDIR" "$ETCDIR" || fail "Failed to create backend directories"

touch "$BINDIR/.gitkeep" "$LIBDIR/.gitkeep" "$ETCDIR/.gitkeep" || fail "Failed to create .gitkeep files"

# Function to extract library path from otool output
extract_path() {
    local lib="$1"
    local path=$(otool -L "$lib" | head -n 2 | tail -n 1 | awk '{print $1}')
    
    # Remove the library name to get just the directory
    if [[ "$path" == /* ]]; then
        # It's a full path
        dirname "$path"
    else
        # It's either @rpath or relative - just return empty
        echo ""
    fi
}

common_search_paths() {
    for path in \
        "$EXEC_DIR" \
        "$EXEC_DIR/../Frameworks" \
        "$EXEC_DIR/lib" \
        "$EXEC_DIR/../lib" \
        "$EXEC_DIR/../libs" \
        /opt/homebrew/opt/*/lib \
        "/opt/homebrew/lib" \
        "/opt/carta-casacore/lib" \
        "/opt/carta-casacore/libs" \
        "/opt/casaroot-carta-casacore/lib" \
        "/usr/local/lib" \
        "/opt/local/lib" \
        "/usr/lib"; do
        if [ -d "$path" ]; then
            echo "$path"
        fi
    done
}

# Function to find the actual path for an @rpath reference
resolve_rpath() {
    local binary=$1
    local rpath_lib=$2
    local lib_name=$(basename "$rpath_lib")
    local lib_subpath="${rpath_lib#@rpath/}"
    local bin_dir=$(cd "$(dirname "$binary")" && pwd)
    
    # Get the RPATHs from the binary
    local rpaths=""
    rpaths=$(otool -l "$binary" 2>/dev/null | grep -A2 LC_RPATH | grep "path" | awk '{print $2}' || true)
    
    # Extract path from the binary itself if it's a library
    local extracted_path=$(extract_path "$binary")
    local rpaths_file=""
    rpaths_file=$(mktemp) || fail "mktemp failed"
    if [ -n "$rpaths" ]; then
        printf '%s\n' "$rpaths" >> "$rpaths_file"
    fi
    if [ -n "$extracted_path" ]; then
        printf '%s\n' "$extracted_path" >> "$rpaths_file"
    fi
    common_search_paths >> "$rpaths_file"

    local found=""
    while IFS= read -r rpath; do
        [ -n "$rpath" ] || continue
        # Replace @executable_path if present
        rpath="${rpath/@executable_path/$EXEC_DIR}"
        rpath="${rpath/@loader_path/$bin_dir}"
        
        # Check if library exists at this rpath
        if [ -f "$rpath/$lib_subpath" ]; then
            found="$rpath/$lib_subpath"
            break
        fi
        if [ -f "$rpath/$lib_name" ]; then
            found="$rpath/$lib_name"
            break
        fi
    done < "$rpaths_file"
    rm -f "$rpaths_file"

    if [ -n "$found" ]; then
        echo "$found"
        return 0
    fi
    
    # Library not found
    echo ""
    return 1
}

find_common_lib() {
    local lib_name="$1"
    local paths_file=""
    paths_file=$(mktemp) || fail "mktemp failed"
    common_search_paths >> "$paths_file"
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        if [ -f "$path/$lib_name" ]; then
            rm -f "$paths_file"
            echo "$path/$lib_name"
            return 0
        fi
    done < "$paths_file"
    rm -f "$paths_file"

    echo ""
    return 1
}

resolve_dep_path() {
    local binary=$1
    local dep=$2
    local depname=$(basename "$dep")
    local resolved=""
    local bin_dir=$(cd "$(dirname "$binary")" && pwd)

    if [[ "$dep" == @rpath/* ]]; then
        resolved=$(resolve_rpath "$binary" "$dep")
    elif [[ "$dep" == @executable_path/* ]]; then
        if [[ "$dep" != @executable_path/../libs/* ]]; then
            resolved="${dep/@executable_path/$EXEC_DIR}"
        fi
        if [ ! -f "$resolved" ]; then
            resolved=$(find_common_lib "$depname")
        fi
    elif [[ "$dep" == @loader_path/* ]]; then
        resolved="${dep/@loader_path/$bin_dir}"
        if [ ! -f "$resolved" ]; then
            resolved=$(find_common_lib "$depname")
        fi
    elif [[ "$dep" == /* ]]; then
        resolved="$dep"
    else
        resolved="$bin_dir/$dep"
        if [ ! -f "$resolved" ]; then
            resolved=$(find_common_lib "$depname")
        fi
    fi

    if [ -n "$resolved" ] && [ -f "$resolved" ]; then
        echo "$resolved"
        return 0
    fi

    echo ""
    return 1
}

run_install_name_tool() {
    local output=""
    output=$("$@" 2>&1)
    local status=$?
    if [ -n "$output" ]; then
        echo "$output" | grep -v "warning: changes being made to the file will invalidate the code signature" >&2
    fi
    return $status
}

# Instead of an associative array, we'll use simple files to track processed/missing libraries
PROCESSED_FILE=$(mktemp) || fail "mktemp failed"
MISSING_FILE=$(mktemp) || fail "mktemp failed"

cleanup() {
    [ -n "${PROCESSED_FILE:-}" ] && rm -f "$PROCESSED_FILE"
    [ -n "${MISSING_FILE:-}" ] && rm -f "$MISSING_FILE"
}
trap cleanup EXIT

# Function to check if a library has been processed
is_processed() {
    grep -Fqx "$1" "$PROCESSED_FILE" 2>/dev/null
    return $?
}

# Function to mark a library as processed
mark_processed() {
    echo "$1" >> "$PROCESSED_FILE"
}

record_missing() {
    local dep="$1"
    if ! grep -Fqx "$dep" "$MISSING_FILE" 2>/dev/null; then
        echo "$dep" >> "$MISSING_FILE"
    fi
}

# Function to recursively copy dependencies
copy_dependencies() {
    local binary=$1
    
    # Mark this binary as processed
    local bin_path=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")
    mark_processed "$bin_path"
    
    local otool_output=""
    if ! otool_output=$(otool -L "$binary"); then
        fail "otool -L failed for $binary"
    fi
    local deps=""
    deps=$(printf '%s\n' "$otool_output" | awk 'NR>1 {print $1}' | awk '!/^\/System/ && !/^\/usr\/lib/')
    
    while IFS= read -r dep; do
        [ -n "$dep" ] || continue
        local depname=$(basename "$dep")
        local resolved_path=""
        
        # Skip if we've already copied this dependency
        if [ ! -f "$LIBDIR/$depname" ]; then
            resolved_path=$(resolve_dep_path "$binary" "$dep")
            if [ -n "$resolved_path" ]; then
                dep=$resolved_path
                cp "$dep" "$LIBDIR/" || fail "Failed to copy $dep to $LIBDIR"
                # Process dependencies of this dependency if not already processed
                if ! is_processed "$dep"; then
                    copy_dependencies "$dep"
                fi
            else
                record_missing "$dep"
            fi
        elif ! is_processed "$LIBDIR/$depname"; then
            # We've already copied this lib but haven't processed its dependencies
            # Process dependencies of this library
            mark_processed "$LIBDIR/$depname"
            copy_dependencies "$LIBDIR/$depname"
        fi
    done <<< "$deps"
}

# Copy the main binary to the bin directory
echo "1. Copy carta_backend..."
cp "$BACKEND_EXEC" "$BINDIR/" || fail "Failed to copy carta_backend to $BINDIR"
TARGET_EXEC="$BINDIR/carta_backend"
echo "  Done!"

# Start the recursive copy process with the main binary
echo "--------------------------------------------------------"
echo "2. Copy libraries..."
copy_dependencies "$BACKEND_EXEC"

# Process all libraries in the libs directory to ensure complete dependency resolution
for lib in "$LIBDIR"/*; do
    if [ -f "$lib" ]; then
        if ! is_processed "$lib"; then
            copy_dependencies "$lib"
        fi
    fi
done

# Summarize missing dependencies only
if [ -s "$MISSING_FILE" ]; then
    echo "  Missing dependencies:"
    sort -u "$MISSING_FILE" | while read -r dep; do
        echo "    $(basename "$dep")"
    done
    exit 1
fi

echo "  Ensuring copied files are owner-writable..."
# Some upstream libraries are shipped as read-only (e.g. mode 444). Make sure the
# copied artifacts are writable so install_name_tool/codesign (and subsequent
# builds that overwrite resources) won't fail with permission denied.
find "$BINDIR" "$LIBDIR" -type f -exec chmod u+w {} + || fail "Failed to chmod u+w on copied backend files"

echo "  Updating library paths..."

# Update main executable to point to libraries in ../libs
for lib in "$LIBDIR"/*; do
    [ -e "$lib" ] || continue
    libname=$(basename "$lib")
    if ! old_id=$(otool -D "$lib" 2>/dev/null | tail -n +2); then
        fail "otool -D failed for $lib"
    fi
    if [ -z "$old_id" ]; then
        # If it's not a dylib with an ID, it might still be a dependency path in the executable
        # Try to find if this lib name is in the otool -L output
        match=$(otool -L "$TARGET_EXEC" 2>/dev/null | grep "$libname" | awk '{print $1}' | head -n 1)
        if [ -n "$match" ]; then
            run_install_name_tool install_name_tool -change "$match" "@executable_path/../libs/$libname" "$TARGET_EXEC" \
                || fail "install_name_tool failed for $TARGET_EXEC"
        fi
    else
        run_install_name_tool install_name_tool -change "$old_id" "@executable_path/../libs/$libname" "$TARGET_EXEC" \
            || fail "install_name_tool failed for $TARGET_EXEC"
    fi
done

# Update libraries to point to each other using @loader_path
for lib in "$LIBDIR"/*; do
    [ -e "$lib" ] || continue
    otool -L "$lib" | awk 'NR>1 {print $1}' | while read dep; do
        if [[ "$dep" == /usr/lib/* || "$dep" == /System/* || "$dep" == @loader_path* ]]; then
            continue
        fi
        depname=$(basename "$dep")
        if [ -f "$LIBDIR/$depname" ]; then
            run_install_name_tool install_name_tool -change "$dep" "@loader_path/$depname" "$lib" \
                || fail "install_name_tool failed for $lib"
        fi
    done
    # Also fix the ID of the library itself
    libname=$(basename "$lib")
    run_install_name_tool install_name_tool -id "@loader_path/$libname" "$lib" \
        || fail "install_name_tool failed for $lib"
done

echo "  Done!"

echo "--------------------------------------------------------"
echo "3. Sign files..."

# Sign the executable
codesign --force --sign - "$TARGET_EXEC" || fail "codesign failed for $TARGET_EXEC"
# Sign libraries
codesign --force --sign - "$LIBDIR"/* || fail "codesign failed for $LIBDIR"
echo "  Done!"

# Download measures data to etc/data
echo "--------------------------------------------------------"
echo "4. Download etc/data..."
cd "$ETCDIR" || fail "Failed to cd to $ETCDIR"
mkdir -p data || fail "Failed to create $ETCDIR/data"
cd data || fail "Failed to cd to $ETCDIR/data"

MEASURES_URL="https://www.astron.nl/iers/WSRT_Measures.ztar"
MEASURES_ARCHIVE="WSRT_Measures.ztar"
if command -v curl >/dev/null 2>&1; then
    curl -fL "$MEASURES_URL" -o "$MEASURES_ARCHIVE" || fail "Failed to download measures data"
elif command -v wget >/dev/null 2>&1; then
    wget -O "$MEASURES_ARCHIVE" "$MEASURES_URL" || fail "Failed to download measures data"
else
    fail "curl or wget is required to download measures data"
fi
tar xfz "$MEASURES_ARCHIVE" || fail "Failed to extract measures data"
rm -f "$MEASURES_ARCHIVE" || fail "Failed to remove $MEASURES_ARCHIVE"

echo "  Done!"
echo "--------------------------------------------------------"
echo "All done!"
