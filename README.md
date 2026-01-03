# CARTA desktop app (Tauri)

This repo contains packaging/build instructions and helper scripts for creating the CARTA desktop app using Tauri.

## Contents
- [macOS](#macos)
- [Windows](#windows)
- [Build Windows installer on Linux](#build-windows-installer-on-linux)
- [Repo layout](#repo-layout)

## macOS

### Prerequisites
- Xcode Command Line Tools
  - `xcode-select --install`
- Rust
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  - After installation, log out and log back in so environment variables are updated.
- Tauri CLI
  - `cargo install tauri-cli`

### Packaging process

1. Build carta-casacore

    Build and install `carta-casacore` with the floating root flag `-DDATA_DIR="%CASAROOT%/data"`. This allows casacore to locate the measures data that is bundled with the app.

    Note: keep `%CASAROOT%` literal (do not expand it in your shell).
    ```
    git clone https://github.com/CARTAvis/carta-casacore.git --recursive
    cd carta-casacore
    mkdir -p build
    cd build
    cmake .. -DUSE_FFTW3=ON -DUSE_HDF5=ON -DUSE_THREADS=ON -DUSE_OPENMP=ON -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF -DBUILD_PYTHON=OFF -DUseCcache=1 -DHAS_CXX11=1 -DDATA_DIR="%CASAROOT%/data" -DCMAKE_INSTALL_PREFIX=/opt/casaroot-carta-casacore
    make -j 4
    sudo make install
    ```

2. Prepare carta-backend

    Build `carta-backend` with the `-DCartaUserFolderPrefix=` flag:
    - Beta releases: `-DCartaUserFolderPrefix=".carta-beta"`
    - Normal releases: `-DCartaUserFolderPrefix=".carta"`

    Also make sure to check out the correct branch/tag.
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

    Copy the backend into `src-tauri/backend/`:
    ```
    sh scripts/macOS/copy_backend.sh <path-to-carta-backend-build-folder>
    ```
    This script copies and downloads the necessary files (binary, libs, and casacore data) into `src-tauri/backend/`.


3. Prepare carta-frontend

    A production `carta-frontend` can either be built from source:
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
    OR
    a pre-built package can be downloaded from the NPM registry, for example:
    ```
    wget https://registry.npmjs.org/carta-frontend/-/carta-frontend-5.0.3.tgz
    tar xvf carta-frontend-5.0.3.tgz
    ```

    Copy the frontend into `src-tauri/frontend/`:
    ```
    sh scripts/macOS/copy_frontend.sh <path-to-carta-frontend-build-folder>
    ```

4. Set up certificate
    - Import the Developer ID certificate into Keychain Access:
      - Open Keychain Access
      - Import the Developer ID certificate (enter the certificate password)
      - Expand the certificate and double-click the private key
      - In the "Access Control" tab, set "Allow all applications to access this item"
    
5. Package
    - Modify `src-tauri/tauri.conf.json`
        - Change `version` to the version of this release
    - Modify `src-tauri/Cargo.toml`
        - Change `version` and `description` to the version of this release
        - DO NOT change `edition` because it is for Rust, not CARTA version.
    - Modify `scripts/macOS/package.sh`
        - Change `APPLE_ID` to your Apple ID
        - Change `APPLE_PASSWORD` to your Apple password (prefer an app-specific password)
    - Run `sh scripts/macOS/package.sh`
        - Enter the password of your login password when prompted to unlock the keychain.
    - The package will be generated in `src-tauri/target/release/bundle/`.


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

### Packaging process
> **The commands below must be run in PowerShell, not in WSL.**

1. Prepare frontend and backend
    - Put the Linux AppImage somewhere accessible to WSL.
    - Run `wsl.exe bash scripts/Windows/extract_appimage.sh <path-to-AppImage>`
2. Modify configuration
    - Modify `src-tauri/tauri.conf.json`
        - Change `version` to the version of this release
    - Modify `src-tauri/Cargo.toml`
        - Change `version` and `description` to the version of this release
        - DO NOT change `edition` because it is for Rust, not CARTA version.
3. Build tauri app
    ```powershell
    cd src-tauri
    # Clean previous build
    cargo clean --release
    # Build NSIS installer
    cargo tauri build --bundles nsis
    ```
4. Get the installer from `src-tauri/target/release/bundle/nsis`

## Build Windows installer on Linux
### Prerequisites
- Rust
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  - After installation, log out and log back in so environment variables are updated.
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

### Packaging process
1. Prepare frontend and backend
    - Put the Linux AppImage somewhere accessible to WSL.
    - Run `bash scripts/Windows/extract_appimage.sh <path-to-AppImage>`
2. Modify configuration
    - Modify `src-tauri/tauri.conf.json`
        - Change `version` to the version of this release
    - Modify `src-tauri/Cargo.toml`
        - Change `version` and `description` to the version of this release
        - DO NOT change `edition` because it is for Rust, not CARTA version.
3. Build tauri app
    ```bash
    cd src-tauri
    # Clean previous build
    cargo clean --release
    # Build Windows installer
    cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
    ```
4. Get the installer from `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis`

## Repo layout
- `scripts/macOS/copy_backend.sh`
    - The script to copy CARTA backend to `src-tauri/backend/`.
- `scripts/macOS/copy_frontend.sh`
    - The script to copy CARTA frontend to `src-tauri/frontend/`.
- `scripts/macOS/package.sh`
    - The script to package CARTA for macOS.
- `scripts/Windows/extract_appimage.sh`
    - The script to extract CARTA frontend and backend from Linux AppImage for Windows.
- `src-tauri/build.rs`
    - The build script for Tauri. We should not modify it.
- `src-tauri/Cargo.lock`
    - The Cargo.lock generated from `Cargo.toml`. We should not modify it.
- `src-tauri/Cargo.toml`
    - Here you can set the version and description of the package.
- `src-tauri/tauri.conf.json`
    - Here you can set the name and version of the package.
- `src-tauri/backend/`
    - `bin`
        - `carta-backend`
            - This is the packaged carta-backend executable.
    - `etc/data`
        - This should contain the `geodetic` and `ephemerides` folders required by carta-casacore. Grab the latest version from Astron when making a package:
            ```bash
            cd src-tauri/backend/etc/data
            wget https://www.astron.nl/iers/WSRT_Measures.ztar
            tar xfz WSRT_Measures.ztar
            rm WSRT_Measures.ztar
            ```
    - `libs`
        - These are the packaged library files needed by carta-backend from the packaging computer.
- `src-tauri/frontend/`
    - This contains the built frontend files.
- `src-tauri/capabilities/` & `src-tauri/gen/`
    - These are generated by Tauri. We should not modify them.
- `src-tauri/icons/`
    - These are the icons generated using `cargo tauri icon <path-to-icon>.png`. We should not modify them.
- `src-tauri/src`
    - `lib.rs`
        - This is the source code for CARTA Tauri app.
    - `main.rs`
        - This is the main Rust file for Tauri. We should not modify it.
- `src-tauri/target/`
    - This contains the built files. Can be cleaned using `cargo clean`.
