#!/bin/bash

BACKEND_BUILD_PATH=$1
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
PROJECT_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)

BINDIR="$PROJECT_ROOT/src-tauri/backend/bin"
LIBDIR="$PROJECT_ROOT/src-tauri/backend/libs"
ETCDIR="$PROJECT_ROOT/src-tauri/backend/etc"

# Clean up previous copied files
rm -rf "$BINDIR"
rm -rf "$LIBDIR"
rm -rf "$ETCDIR"

mkdir -p "$BINDIR"
mkdir -p "$LIBDIR"
mkdir -p "$ETCDIR"

touch "$BINDIR/.gitkeep"
touch "$LIBDIR/.gitkeep"
touch "$ETCDIR/.gitkeep"

# Get the executable's directory
EXEC_DIR=$(cd "$BACKEND_BUILD_PATH" && pwd)
echo "Source executable directory: $EXEC_DIR"

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
    local rpaths=$(otool -l "$binary" | grep -A2 LC_RPATH | grep "path" | awk '{print $2}')
    
    # Extract path from the binary itself if it's a library
    local extracted_path=$(extract_path "$binary")
    if [ -n "$extracted_path" ]; then
        rpaths="$rpaths $extracted_path"
    fi

    # If no RPATHs found, try looking in common locations
    if [ -z "$rpaths" ]; then
        rpaths="$(common_search_paths)"
    else
        rpaths="$rpaths $(common_search_paths)"
    fi

    # Try each RPATH to find the library
    for rpath in $rpaths; do
        # Replace @executable_path if present
        rpath="${rpath/@executable_path/$EXEC_DIR}"
        rpath="${rpath/@loader_path/$bin_dir}"
        
        # Check if library exists at this rpath
        if [ -f "$rpath/$lib_subpath" ]; then
            echo "$rpath/$lib_subpath"
            return 0
        fi
        if [ -f "$rpath/$lib_name" ]; then
            echo "$rpath/$lib_name"
            return 0
        fi
    done
    
    # Library not found
    echo ""
    return 1
}

find_common_lib() {
    local lib_name="$1"
    for path in $(common_search_paths); do
        if [ -f "$path/$lib_name" ]; then
            echo "$path/$lib_name"
            return 0
        fi
    done

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
PROCESSED_FILE=$(mktemp)
MISSING_FILE=$(mktemp)

# Function to check if a library has been processed
is_processed() {
    grep -q "^$1$" "$PROCESSED_FILE" 2>/dev/null
    return $?
}

# Function to mark a library as processed
mark_processed() {
    echo "$1" >> "$PROCESSED_FILE"
}

record_missing() {
    local dep="$1"
    if ! grep -q "^$dep$" "$MISSING_FILE" 2>/dev/null; then
        echo "$dep" >> "$MISSING_FILE"
    fi
}

# Function to recursively copy dependencies
copy_dependencies() {
    local binary=$1
    
    # Mark this binary as processed
    local bin_path=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")
    mark_processed "$bin_path"
    
    local deps=$(otool -L "$binary" | tail -n +2 | awk '{print $1}' | grep -v "^/System" | grep -v "^/usr/lib")
    
    for dep in $deps; do
        local depname=$(basename "$dep")
        
        # Skip if we've already copied this dependency
        if [ ! -f "$LIBDIR/$depname" ]; then
            resolved_path=$(resolve_dep_path "$binary" "$dep")
            if [ -n "$resolved_path" ]; then
                dep=$resolved_path
                cp "$dep" "$LIBDIR/"
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
    done
}

# Copy the main binary to the bin directory
echo "1. Copy carta_backend..."
cp "$BACKEND_BUILD_PATH/carta_backend" "$BINDIR/"
TARGET_EXEC="$BINDIR/carta_backend"

# Start the recursive copy process with the main binary
echo "--------------------------------------------------------"
echo "2. Copy libs..."
echo "--------------------------------------------------------"
copy_dependencies "$BACKEND_BUILD_PATH/carta_backend"

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
    # Clean up temp file
    rm -f "$PROCESSED_FILE"
    rm -f "$MISSING_FILE"
    exit 1
fi

echo "All libs copied."

# Clean up temp file
rm -f "$PROCESSED_FILE"
rm -f "$MISSING_FILE"

echo "--------------------------------------------------------"
echo "Updating library paths..."

# Update main executable to point to libraries in ../libs
for lib in "$LIBDIR"/*; do
    [ -e "$lib" ] || continue
    libname=$(basename "$lib")
    old_id=$(otool -D "$lib" | tail -n +2)
    if [ -z "$old_id" ]; then
        # If it's not a dylib with an ID, it might still be a dependency path in the executable
        # Try to find if this lib name is in the otool -L output
        match=$(otool -L "$TARGET_EXEC" | grep "$libname" | awk '{print $1}' | head -n 1)
        if [ -n "$match" ]; then
            run_install_name_tool install_name_tool -change "$match" "@executable_path/../libs/$libname" "$TARGET_EXEC"
        fi
    else
        run_install_name_tool install_name_tool -change "$old_id" "@executable_path/../libs/$libname" "$TARGET_EXEC"
    fi
done

# Update libraries to point to each other using @loader_path
for lib in "$LIBDIR"/*; do
    [ -e "$lib" ] || continue
    echo "Processing $lib"
    otool -L "$lib" | awk 'NR>1 {print $1}' | while read dep; do
        if [[ "$dep" == /usr/lib/* || "$dep" == /System/* || "$dep" == @loader_path* ]]; then
            continue
        fi
        depname=$(basename "$dep")
        if [ -f "$LIBDIR/$depname" ]; then
            run_install_name_tool install_name_tool -change "$dep" "@loader_path/$depname" "$lib"
        fi
    done
    # Also fix the ID of the library itself
    libname=$(basename "$lib")
    run_install_name_tool install_name_tool -id "@loader_path/$libname" "$lib"
done

# Download measures data to etc/data
echo "--------------------------------------------------------"
echo "3. Download etc/data..."
cd "$ETCDIR"
mkdir -p data
cd data
wget https://www.astron.nl/iers/WSRT_Measures.ztar
tar xfz WSRT_Measures.ztar
rm WSRT_Measures.ztar

echo "--------------------------------------------------------"
echo "4. Sign binary and libraries..."
codesign --force --sign - "$TARGET_EXEC"
for lib in "$LIBDIR"/*; do
    [ -e "$lib" ] || continue
    codesign --force --sign - "$lib"
done
echo "Done!"