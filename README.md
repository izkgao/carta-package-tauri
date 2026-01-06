# CARTA desktop app (Tauri)

This repository contains build instructions and helper scripts for packaging the CARTA desktop application using Tauri.

## Contents
- [Troubleshooting](#troubleshooting)
- [macOS](#macos)
- [Windows](#windows)
    - [Build Windows installer on Windows](#windows)
    - [Build Windows installer on Linux](#build-windows-installer-on-linux)
    - [Build Windows installer on macOS](#build-windows-installer-on-macos)
- [Linux](#linux)
- [Project Structure](#project-structure)

## Troubleshooting

### macOS: Rust toolchain selection

If `cargo tauri build` fails on macOS due to picking up the wrong Rust toolchain (e.g. Homebrew Rust instead of rustup), ensure the intended `cargo` is first in `PATH`, then verify the active binaries:

```bash
# Check the current Rust toolchain
which cargo rustc rustup

# Add the rustup toolchain to the PATH or put in at the bottom of your .zshrc
export PATH="$HOME/.cargo/bin:$PATH"
which cargo rustc rustup
```

## macOS

### Prerequisites
- Xcode Command Line Tools
  - `xcode-select --install`
- Rust
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  - After installation, log out and log back in so environment variables are updated.
- Tauri CLI
  - `cargo install tauri-cli`

### Packaging Process

#### 1. Build and Install carta-casacore

Build and install `carta-casacore` using the floating root flag `-DDATA_DIR="%CASAROOT%/data"`. This configuration allows `casacore` to locate bundled measures data.

**Note:** Keep `%CASAROOT%` literal in the command (do not expand it in your shell).
```
git clone https://github.com/CARTAvis/carta-casacore.git --recursive
cd carta-casacore
mkdir -p build
cd build
cmake .. -DUSE_FFTW3=ON -DUSE_HDF5=ON -DUSE_THREADS=ON -DUSE_OPENMP=ON -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF -DBUILD_PYTHON=OFF -DUseCcache=1 -DHAS_CXX11=1 -DDATA_DIR="%CASAROOT%/data" -DCMAKE_INSTALL_PREFIX=/opt/casaroot-carta-casacore
make -j 4
sudo make install
```

#### 2. Prepare carta-backend

Build `carta-backend` with the appropriate `-DCartaUserFolderPrefix` flag:
    - Beta releases: `-DCartaUserFolderPrefix=".carta-beta"`
    - Normal releases: `-DCartaUserFolderPrefix=".carta"`

    Ensure the correct branch or tag is checked out:
    ```
    git clone https://github.com/CARTAvis/carta-backend.git
    cd carta-backend
    git checkout release/4.0
    git submodule update --init --recursive
    mkdir build
    cd build
    cmake .. -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCartaUserFolderPrefix=".carta" -DDEPLOYMENT_TYPE=tauri
    make -j 4
    ```

    Copy the backend to `src-tauri/backend/`:
    ```bash
    bash scripts/macOS/copy_backend.sh <path-to-backend-build-folder>
    ```
    This script automates the transfer of binaries, libraries, and casacore data into the Tauri source tree.


#### 3. Prepare carta-frontend

The `carta-frontend` production build can be prepared from source or downloaded as a pre-built package.

#### Option A: Build from Source
    ```
    # Install and activate emscripten
    git clone https://github.com/emscripten-core/emsdk.git
    cd emsdk
    git pull
    ./emsdk install 4.0.3
    ./emsdk activate 4.0.3
    source ./emsdk_env.sh
    cd ..

    # Build carta-frontend
    git clone https://github.com/CARTAvis/carta-frontend.git
    cd carta-frontend
    git submodule update --init --recursive
    npm install
    npm run build-libs
    npm run build
    ```
#### Option B: Download Pre-built Package
    A pre-built package is available via the NPM registry:
    ```
    wget https://registry.npmjs.org/carta-frontend/-/carta-frontend-5.0.3.tgz
    tar xvf carta-frontend-5.0.3.tgz
    ```

Copy the frontend to `src-tauri/frontend/`:
```bash
bash scripts/macOS/copy_frontend.sh <path-to-frontend-build-folder>
```

#### 4. Configure Certificate
1. Import the **Developer ID** certificate into **Keychain Access**.
2. Expand the certificate and double-click the private key.
3. In the **Access Control** tab, select **"Allow all applications to access this item"**.
    
#### 5. Package Application
1. Update versioning:
    - `src-tauri/tauri.conf.json`: Update the `version` field.
    - `src-tauri/Cargo.toml`: Update `version` and `description`. (Note: Do not modify the Rust `edition`).
2. (Optional) Test the application.
    - `cd src-tauri`
    - `cargo clean`
    - `cargo tauri dev`
    - After testing, run `cd ..` to return to the root directory.
3. Configure packaging script:
    - `scripts/macOS/package.sh`: Set `APPLE_ID` and `APPLE_PASSWORD` (use an app-specific password).
    - If `cargo tauri build` fails due to a toolchain conflict, see [macOS: Rust toolchain selection](#macos-rust-toolchain-selection).
4. Execute build:
    - Run `bash scripts/macOS/package.sh`.
    - Provide the login password when prompted to unlock the keychain.

The generated package will be located in `src-tauri/target/release/bundle/`.


## Windows
### Prerequisites
- WSL
    - Must be Windows 10 version 2004 and higher (Build 19041 and higher) or Windows 11
    - https://learn.microsoft.com/windows/wsl/install
- Microsoft C++ Build Tools
    - Download and install Microsoft C++ Build Tools https://visualstudio.microsoft.com/visual-cpp-build-tools/
        - Check "Desktop development with C++"
- Rust
    - `winget install --id Rustlang.Rustup`
    - `rustup default stable-msvc`
- Tauri
    - `cargo install tauri-cli`

### Packaging Process
> **Note:** The following commands must be executed in PowerShell, not in WSL.

#### 1. Prepare Frontend and Backend
- Place the Linux AppImage in a location accessible to WSL.
- Execute: `wsl.exe bash scripts/WindowsLinux/extract_appimage.sh <path-to-AppImage>`

#### 2. Update Configuration
- `src-tauri/tauri.conf.json`: Update the `version` field.
- `src-tauri/Cargo.toml`: Update `version` and `description`. (Note: Do not modify the Rust `edition`).

#### 3. Build Tauri Application
```powershell
cd src-tauri
# Clean previous build artifacts
cargo clean --release
# Generate NSIS installer
cargo tauri build --bundles nsis
```
The installer will be generated in `src-tauri/target/release/bundle/nsis`.

## Build Windows installer on Linux
### Prerequisites
- Rust
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  - After installation, log out and log back to your terminal so environment variables are updated.
- Tauri 
  - Tauri CLI
    - `cargo install tauri-cli`
  - NSIS
    - `sudo apt install nsis`
  - LLVM and the LLD Linker
    - `sudo apt install lld llvm`
  - Windows Rust target
    - `rustup target add x86_64-pc-windows-msvc`
  - cargo-xwin
    - `cargo install --locked cargo-xwin`

### Packaging Process
#### 1. Prepare Frontend and Backend
- Place the Linux AppImage in an accessible location.
- Execute: `bash scripts/WindowsLinux/extract_appimage.sh <path-to-AppImage>`

#### 2. Update Configuration
- `src-tauri/tauri.conf.json`: Update the `version` field.
- `src-tauri/Cargo.toml`: Update `version` and `description`. (Note: Do not modify the Rust `edition`).
- Linux AppImage builds automatically merge `src-tauri/tauri.linux.conf.json`, which overrides `bundle.resources` to avoid bundling `backend/libs` twice (AppImage tooling will collect required `.so` dependencies).

#### 3. Build Tauri Application
```bash
cd src-tauri
# Clean previous build artifacts
cargo clean
# (Optional) Test the application
cargo tauri dev
# Generate Windows installer
cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```
The installer will be generated in `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis`.

## Build Windows installer on macOS
### Prerequisites
- Rust
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  - After installation, log out and log back to your terminal so environment variables are updated.
- Tauri 
  - Tauri CLI
    - `cargo install tauri-cli`
  - NSIS
    - `brew install nsis`
  - LLVM and the LLD Linker
    - `brew install llvm`
  - Windows Rust target
    - `rustup target add x86_64-pc-windows-msvc`
  - cargo-xwin
    - `cargo install --locked cargo-xwin`

### Packaging Process
#### 1. Prepare Frontend and Backend
- Place the Linux AppImage in an accessible location.
- Execute: `bash scripts/WindowsLinux/extract_appimage.sh <path-to-AppImage>`

#### 2. Update Configuration
- `src-tauri/tauri.conf.json`: Update the `version` field.
- `src-tauri/Cargo.toml`: Update `version` and `description`. (Note: Do not modify the Rust `edition`).

#### 3. Build Tauri Application
```bash
cd src-tauri
# Clean previous build artifacts
cargo clean
# Generate Windows installer
cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```
The installer will be generated in `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis`.

> If `cargo tauri build` fails due to a toolchain conflict, see [macOS: Rust toolchain selection](#macos-rust-toolchain-selection).

## Linux
### Prerequisites
- Rust
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  - After installation, log out and log back to your terminal so environment variables are updated.
- Tauri 
  - Tauri CLI
    - `cargo install tauri-cli`
  - Other dependencies
    ```
    sudo apt update
    sudo apt install libwebkit2gtk-4.1-dev \
      build-essential \
      curl \
      wget \
      file \
      libxdo-dev \
      libssl-dev \
      libayatana-appindicator3-dev \
      librsvg2-dev
    ```

### Packaging Process
#### 1. Prepare Frontend and Backend
- Place the Linux AppImage in an accessible location.
- Execute: `bash scripts/WindowsLinux/extract_appimage.sh <path-to-AppImage>`

#### 2. Update Configuration
- `src-tauri/tauri.conf.json`: Update the `version` field.
- `src-tauri/Cargo.toml`: Update `version` and `description`. (Note: Do not modify the Rust `edition`).

#### 3. Build Tauri Application
```bash
cd src-tauri
# Clean previous build artifacts
cargo clean
# Set library path
export LD_LIBRARY_PATH="$(pwd)/backend/libs"
# (Optional) Test the application
cargo tauri dev
# Generate AppImage
cargo tauri build --bundles appimage
```
The AppImage will be generated in `src-tauri/target/release/bundle/appimage`.


## Project Structure
- `scripts/macOS/copy_backend.sh`: Copies the CARTA backend into `src-tauri/backend/`.
- `scripts/macOS/copy_frontend.sh`: Copies the CARTA frontend into `src-tauri/frontend/`.
- `scripts/macOS/package.sh`: Automates the macOS packaging and notarization process.
- `scripts/WindowsLinux/extract_appimage.sh`: Extracts the CARTA frontend and backend from a Linux AppImage for Windows and Linux builds.
- `src-tauri/build.rs`: Tauri build script (internal; do not modify).
- `src-tauri/Cargo.lock`: Cargo dependency lockfile (auto-generated).
- `src-tauri/Cargo.toml`: Package configuration for versioning and dependencies.
- `src-tauri/tauri.conf.json`: Core Tauri configuration (application name, version, etc.).
- `src-tauri/tauri.linux.conf.json`: Linux AppImage configuration (overrides `bundle.resources` to avoid bundling `backend/libs` twice).
- `src-tauri/backend/`:
    - `bin/carta-backend`: The packaged CARTA backend executable.
    - `etc/data/`: Contains `geodetic` and `ephemerides` data required by `carta-casacore`.
        - **Note:** Retrieve the latest version from Astron during packaging:
            ```bash
            cd src-tauri/backend/etc/data
            wget https://www.astron.nl/iers/WSRT_Measures.ztar
            tar xfz WSRT_Measures.ztar
            rm WSRT_Measures.ztar
            ```
    - `libs/`: Shared libraries required by the `carta-backend`.
- `src-tauri/frontend/`: Compiled frontend assets.
- `src-tauri/capabilities/`: Internal Tauri files (do not modify).
- `src-tauri/gen/`: Internal Tauri files, generated during build and not included in version control (do not modify).
- `src-tauri/icons/`: Application icons (generated via `cargo tauri icon`).
- `src-tauri/src/`:
    - `lib.rs`: Core Rust logic for the CARTA Tauri application.
    - `main.rs`: Application entry point.
- `src-tauri/target/`: Build artifacts, generated during build and not included in version control. Use `cargo clean` to remove.
