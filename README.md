# Desktop application of CARTA

## macOS

### Prerequisites
- Rust
    - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
    - After installation, logout and login again to make sure the environment variables are updated.
- Tauri
    - `cargo install tauri-cli`

### Packaging process

1. Build carta-casacore

    It is essential that carta-casacore is built and installed with a floating root flag: `-DDATA_DIR="%CASAROOT%/data"`. This ensures casacore will be able to look for the measures data that we bundle with the package:
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

    Build the carta-backend with the `-DCartaUserFolderPrefix=` flag. If it is a beta-release, use `.carta-beta`, if it is a normal release, use `.carta`. Also, make sure to ‘checkout’ the correct branch/tag.
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

    Copy CARTA backend to `src-tauri/backend/`
    ```
    sh scripts/macOS/copy_backend.sh <path-to-carta-backend-build-folder>
    ```
    This script will copy and download the necessary files (binary, libs, and casacore data) to the `src-tauri/backend/` folder.


3. Prepare carta-frontend

    A production carta-frontend can either be built from source:
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
    A pre-built package can be download from the NPM repository: e.g.
    ```
    wget https://registry.npmjs.org/carta-frontend/-/carta-frontend-5.0.3.tgz
    tar xvf carta-frontend-5.0.3.tgz
    ```

    Copy CARTA frontend to `src-tauri/frontend/`
    ```
    sh scripts/macOS/copy_frontend.sh <path-to-carta-frontend-build-folder>
    ```

4. Set up certificate
    - Import developer ID certificate to Keychain Access
        - Open Keychain Access
        - Import the developer ID certificate and enter the password of the certificate
        - Unfold the certificate and double-click the private key
        - In the "Access Control" tab, set the keychain to "Allow all applications to access this item"
    
5. Package
    - Modify `src-tauri/tauri.conf.json`
        - Change `version` to the version of this release
    - Modify `src-tauri/Cargo.toml`
        - Change `version` and `description` to the version of this release
        - DO NOT change `edition` because it is for Rust, not CARTA version.
    - Modify `scripts/macOS/package.sh`
        - Change `APPLE_ID` to your Apple ID
        - Change `APPLE_PASSWORD` to your Apple password
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
    - Put Linux AppImage to `scripts/Windows/`
    - Run `wsl.exe bash scripts/Windows/extract_appimage.sh`
2. Modify configuration
    - Modify `src-tauri/tauri.conf.json`
        - Change `version` to the version of this release
    - Modify `src-tauri/Cargo.toml`
        - Change `version` and `description` to the version of this release
        - DO NOT change `edition` because it is for Rust, not CARTA version.
3. Build tauri app
    ```PowerShell
    cd src-tauri
    # Clean previous build
    cargo clean
    # Build nsis installer
    cargo tauri build --bundles nsis
    ```
4. Get the installer from `src-tauri/target/release/bundle/`

## File and folder description
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