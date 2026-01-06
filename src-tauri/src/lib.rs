use std::{
    error::Error,
    fmt, fs,
    io::{self, BufRead, BufReader},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, RunEvent, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
};

const DEFAULT_WINDOW_WIDTH: u32 = 1920;
const DEFAULT_WINDOW_HEIGHT: u32 = 1080;
const WINDOW_OFFSET: i32 = 25;
const WINDOW_STATE_FILE: &str = "window-state.json";
const WINDOW_TITLE: &str = "CARTA";
const MAIN_WINDOW_LABEL: &str = "main";

const BACKEND_DIR: &str = "backend";
const FRONTEND_DIR: &str = "frontend";
const SYMLINK_BASE: &str = "/tmp";
const SYMLINK_NAME: &str = "carta-etc";

const ENV_AUTH_TOKEN: &str = "CARTA_AUTH_TOKEN";
const ENV_CASAPATH: &str = "CASAPATH";
const BACKEND_FILENAME: &str = "carta_backend";
#[cfg(target_os = "windows")]
const ENV_WSL_DISTRO: &str = "CARTA_WSL_DISTRO";

const BACKEND_TIMEOUT_SECS: u64 = 60;
const CONNECT_TIMEOUT_MS: u64 = 250;
const CONNECT_RETRY_MS: u64 = 100;

const MENU_NEW_WINDOW: &str = "new_window";
const MENU_TOGGLE_DEVTOOLS: &str = "toggle_devtools";
const MENU_TOGGLE_FULLSCREEN: &str = "toggle_fullscreen";
const MENU_QUIT: &str = "quit";

#[derive(Debug, Default)]
struct CliArgs {
    input_path: Option<String>,
    extra_args: Vec<String>,
    inspect: bool,
    help: bool,
    version: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct WindowBounds {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}

impl WindowBounds {
    fn new(pos: PhysicalPosition<i32>, size: PhysicalSize<u32>, scale: f64) -> Self {
        Self {
            width: (size.width as f64 / scale) as u32,
            height: (size.height as f64 / scale) as u32,
            x: (pos.x as f64 / scale) as i32,
            y: (pos.y as f64 / scale) as i32,
        }
    }

    fn with_offset(mut self, offset: i32) -> Self {
        self.x += offset;
        self.y += offset;
        self
    }
}

#[derive(Debug)]
struct AppError(String);

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for AppError {}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        Self(err.to_string())
    }
}

type AppResult<T> = Result<T, AppError>;

struct AppState {
    backend: Mutex<Option<Child>>,
    backend_port: u16,
    backend_token: String,
    window_url: String,
    inspect: bool,
}

fn parse_cli_args_from<I>(args: I) -> CliArgs
where
    I: IntoIterator<Item = String>,
{
    let mut result = CliArgs::default();
    let mut iter = args.into_iter().peekable();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--" => {
                for rest in iter {
                    if result.input_path.is_none() {
                        result.input_path = Some(rest);
                    } else {
                        result.extra_args.push(rest);
                    }
                }
                break;
            }
            "--inspect" => result.inspect = true,
            "--help" | "-h" => result.help = true,
            "--version" | "-v" => result.version = true,
            s if s.starts_with('-') => {
                result.extra_args.push(arg.clone());
                if !s.contains('=')
                    && let Some(next) = iter.peek()
                    && !next.starts_with('-')
                {
                    // Safe: peek() succeeded above, so next() will return Some
                    result.extra_args.push(iter.next().unwrap());
                }
            }
            _ if result.input_path.is_none() => result.input_path = Some(arg.clone()),
            _ => result.extra_args.push(arg.clone()),
        }
    }

    result
}

fn parse_cli_args() -> CliArgs {
    parse_cli_args_from(std::env::args().skip(1))
}

fn resolve_base_directory(input_path: Option<&str>) -> AppResult<PathBuf> {
    let cwd = std::env::current_dir()?;

    if let Some(path) = input_path {
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            cwd.join(path)
        };
        let metadata = fs::metadata(&candidate)
            .map_err(|_| AppError::from("Requested file or directory does not exist"))?;

        if metadata.is_file() {
            Ok(candidate
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .to_path_buf())
        } else if metadata.is_dir() {
            Ok(candidate)
        } else {
            Err("Requested path is neither a file nor a directory".into())
        }
    } else if should_default_to_home(&cwd) {
        home_dir().ok_or_else(|| "HOME directory not found".into())
    } else {
        Ok(cwd)
    }
}

fn should_default_to_home(cwd: &Path) -> bool {
    if cwd == Path::new("/") {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        // When launched from Finder, macOS apps may start in the app bundle's executable directory.
        if cwd.to_string_lossy().contains(".app/Contents/MacOS") {
            return true;
        }
    }

    #[cfg(target_os = "linux")]
    {
        if is_linux_appimage_mount_dir(cwd) {
            return true;
        }
    }

    false
}

#[cfg(target_os = "linux")]
fn is_linux_appimage_mount_dir(cwd: &Path) -> bool {
    if !cwd.starts_with("/tmp") {
        return false;
    }

    let cwd_str = cwd.to_string_lossy();
    if cwd_str.contains("/.mount_") {
        return true;
    }

    let has_appimage_env =
        std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some();
    if !has_appimage_env {
        return false;
    }

    matches!(
        cwd.file_name(),
        Some(name) if name == std::ffi::OsStr::new("usr")
    )
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn win_to_wsl_path(win_path: &str) -> Option<String> {
    // Convert C:\path\to\file to /mnt/c/path/to/file
    // Also strips Windows extended-length path prefix (\\?\) for WSL compatibility
    let path = win_path.trim();
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    if path.len() < 2 {
        return None;
    }
    let chars: Vec<char> = path.chars().collect();
    if chars.get(1) != Some(&':') {
        return None;
    }
    let drive = chars[0].to_ascii_lowercase();
    let rest = &path[2..].replace('\\', "/");
    Some(format!("/mnt/{}{}", drive, rest))
}

#[cfg(target_os = "windows")]
fn wsl_distro() -> Option<String> {
    std::env::var(ENV_WSL_DISTRO)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "windows")]
fn add_wsl_distro(cmd: &mut Command) {
    if let Some(distro) = wsl_distro() {
        cmd.arg("-d").arg(distro);
    }
}

#[cfg(target_os = "windows")]
fn wsl_bash_command(command: &str) -> Command {
    let mut cmd = Command::new("wsl.exe");
    add_wsl_distro(&mut cmd);
    cmd.arg("--").arg("bash").arg("-lc").arg(command);
    cmd
}

#[cfg(target_os = "windows")]
fn wsl_bash_output(command: &str) -> AppResult<std::process::Output> {
    let output = wsl_bash_command(command)
        .output()
        .map_err(|err| AppError(format!("Failed to run wsl.exe bash command: {}", err)))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(AppError(format!("WSL command failed: {}", detail.trim())));
    }
    Ok(output)
}

#[cfg(target_os = "windows")]
fn bash_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let mut escaped = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            escaped.push_str("'\"'\"'");
        } else {
            escaped.push(ch);
        }
    }
    escaped.push('\'');
    escaped
}

fn resolve_backend_path(app: &AppHandle) -> AppResult<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir
            .join(BACKEND_DIR)
            .join("bin")
            .join(BACKEND_FILENAME);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("backend/bin/carta_backend binary not found".into())
}

fn resolve_frontend_path(app: &AppHandle) -> AppResult<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join(FRONTEND_DIR);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("frontend directory not found".into())
}

fn resolve_etc_path(backend_path: &Path) -> AppResult<String> {
    let etc_path = backend_path
        .parent()
        .map(|bin| bin.join("..").join("etc"))
        .filter(|p| p.exists())
        .ok_or(AppError::from("backend/etc directory not found"))?;

    let resolved = fs::canonicalize(&etc_path).unwrap_or(etc_path);

    #[cfg(target_os = "windows")]
    {
        let wsl_path = win_to_wsl_path(&resolved.to_string_lossy())
            .ok_or_else(|| AppError::from("Failed to convert etc path to WSL format"))?;

        // If path doesn't contain spaces, use it directly
        if !wsl_path.contains(' ') {
            return Ok(wsl_path);
        }

        // Path contains spaces, need to create symlink in WSL
        let link_path = format!("{}/{}", SYMLINK_BASE, SYMLINK_NAME);

        // Check if symlink already exists and points to correct target
        let mut check_cmd = Command::new("wsl.exe");
        add_wsl_distro(&mut check_cmd);
        check_cmd
            .args(["--", "readlink", "-f", &link_path])
            .creation_flags(CREATE_NO_WINDOW);

        if let Ok(output) = check_cmd.output() {
            let existing = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if existing == wsl_path {
                return Ok(link_path);
            }
        }

        // Check if something exists at link path
        let mut stat_cmd = Command::new("wsl.exe");
        add_wsl_distro(&mut stat_cmd);
        stat_cmd
            .args(["--", "test", "-e", &link_path])
            .creation_flags(CREATE_NO_WINDOW);

        let exists = stat_cmd.status().map(|s| s.success()).unwrap_or(false);

        // Check if it's a symlink
        let mut is_link_cmd = Command::new("wsl.exe");
        add_wsl_distro(&mut is_link_cmd);
        is_link_cmd
            .args(["--", "test", "-L", &link_path])
            .creation_flags(CREATE_NO_WINDOW);

        let is_symlink = is_link_cmd.status().map(|s| s.success()).unwrap_or(false);

        if exists && !is_symlink {
            return Err("symlink path already exists".into());
        }

        // Remove existing symlink if it points to wrong target
        if is_symlink {
            let mut rm_cmd = Command::new("wsl.exe");
            add_wsl_distro(&mut rm_cmd);
            rm_cmd
                .args(["--", "rm", "-f", &link_path])
                .creation_flags(CREATE_NO_WINDOW);
            let _ = rm_cmd.status();
        }

        // Create new symlink
        let mut ln_cmd = Command::new("wsl.exe");
        add_wsl_distro(&mut ln_cmd);
        ln_cmd
            .args(["--", "ln", "-s", &wsl_path, &link_path])
            .creation_flags(CREATE_NO_WINDOW);

        if ln_cmd.status().map(|s| s.success()).unwrap_or(false) {
            Ok(link_path)
        } else {
            Err("failed to create symlink for etc path with spaces".into())
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        // If the etc path itself does not contain spaces, use the real path directly
        if !resolved.to_string_lossy().contains(' ') {
            return Ok(resolved.to_string_lossy().into_owned());
        }

        // If the etc path contains spaces, try to create a symlink in /tmp
        let base_dir = PathBuf::from(SYMLINK_BASE);
        let _ = fs::create_dir_all(&base_dir);
        let link_path = base_dir.join(SYMLINK_NAME);

        if let Ok(metadata) = fs::symlink_metadata(&link_path) {
            if !metadata.file_type().is_symlink() {
                return Err("symlink path already exists".into());
            }

            if let Ok(existing) = fs::read_link(&link_path)
                && existing == resolved
            {
                return Ok(link_path.to_string_lossy().into_owned());
            }

            let _ = fs::remove_file(&link_path);
        }

        if std::os::unix::fs::symlink(&resolved, &link_path).is_ok() {
            Ok(link_path.to_string_lossy().into_owned())
        } else {
            Err("failed to create symlink for etc path with spaces".into())
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = resolved;
        Err("unsupported platform".into())
    }
}

fn resolve_casa_path(backend_path: &Path) -> AppResult<String> {
    let etc_path = resolve_etc_path(backend_path)?;
    // The "../../../../../" prefix clears the hardcoded absolute path from the build machine
    // embedded in carta_backend, allowing us to specify the correct etc directory path.
    Ok(format!("../../../../../{} linux", etc_path))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn run_backend_help(_app: &AppHandle, _version: bool) -> AppResult<()> {
    Err("unsupported platform".into())
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn run_backend_help(app: &AppHandle, version: bool) -> AppResult<()> {
    let backend_path = resolve_backend_path(app)?;
    let flag = if version { "--version" } else { "--help" };

    let output = {
        #[cfg(target_os = "windows")]
        {
            let backend_win = backend_path.to_string_lossy();
            let script = format!(
                "set -e\nbackend_win={}\nbackend=$(wslpath -a -u \"$backend_win\")\nexec \"$backend\" {}\n",
                bash_escape(&backend_win),
                bash_escape(flag)
            );
            wsl_bash_output(&script)?
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            Command::new(backend_path).arg(flag).output()?
        }
    };

    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    if !version {
        println!("Additional Tauri flag:");
        println!("      --inspect      Open the DevTools in the Tauri window.");
    }
    Ok(())
}

/// Spawns a thread to pipe backend output to stdout/stderr.
/// The thread exits naturally when the pipe closes. JoinHandle is intentionally
/// discarded as waiting for it adds complexity with minimal benefit.
fn pipe_output<T: std::io::Read + Send + 'static>(reader: T, is_stderr: bool) {
    std::thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines().map_while(Result::ok) {
            if is_stderr {
                eprintln!("{}", line);
            } else {
                println!("{}", line);
            }
        }
    });
}

fn spawn_backend(
    app: &AppHandle,
    state: &AppState,
    base_dir: &Path,
    extra_args: &[String],
) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        let backend_path = resolve_backend_path(app)?;
        let frontend_path = resolve_frontend_path(app)?;

        // Convert Windows paths to WSL paths directly in Rust
        let backend = win_to_wsl_path(&backend_path.to_string_lossy())
            .ok_or_else(|| AppError::from("Failed to convert backend path to WSL format"))?;
        let frontend = win_to_wsl_path(&frontend_path.to_string_lossy())
            .ok_or_else(|| AppError::from("Failed to convert frontend path to WSL format"))?;
        let base = win_to_wsl_path(&base_dir.to_string_lossy())
            .ok_or_else(|| AppError::from("Failed to convert base path to WSL format"))?;
        let casa_path = resolve_casa_path(&backend_path)?;

        // Libs directory for LD_LIBRARY_PATH
        let libs_path = backend_path
            .parent()
            .map(|bin| bin.join("..").join("libs"))
            .filter(|p| p.exists())
            .and_then(|p| win_to_wsl_path(&p.to_string_lossy()))
            .unwrap_or_default();
        let extra = extra_args
            .iter()
            .map(|arg| bash_escape(arg))
            .collect::<Vec<_>>()
            .join(" ");

        let auth_token = &state.backend_token;
        let port = state.backend_port;
        let backend_escaped = bash_escape(&backend);
        let frontend_escaped = bash_escape(&frontend);
        let base_escaped = bash_escape(&base);
        let auth_token_escaped = bash_escape(auth_token);
        let libs_escaped = bash_escape(&libs_path);
        let casa_path_escaped = bash_escape(&casa_path);

        let command = format!(
            "export LD_LIBRARY_PATH={libs_escaped}:$LD_LIBRARY_PATH; export {ENV_AUTH_TOKEN}={auth_token_escaped}; export {ENV_CASAPATH}={casa_path_escaped}; exec {backend_escaped} {base_escaped} --port={port} --frontend_folder={frontend_escaped} --no_browser {extra}"
        );

        let mut cmd = wsl_bash_command(&command);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);

        let mut child = cmd.spawn().map_err(AppError::from)?;

        if let Some(stdout) = child.stdout.take() {
            pipe_output(stdout, false);
        }
        if let Some(stderr) = child.stderr.take() {
            pipe_output(stderr, true);
        }

        *state.backend.lock().unwrap() = Some(child);
        Ok(())
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let backend_path = resolve_backend_path(app)?;
        let frontend_path = resolve_frontend_path(app)?;

        let mut cmd = Command::new(&backend_path);
        cmd.arg(base_dir)
            .arg(format!("--port={}", state.backend_port))
            .arg(format!("--frontend_folder={}", frontend_path.display()))
            .arg("--no_browser")
            .args(extra_args)
            .env(ENV_AUTH_TOKEN, &state.backend_token)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let casa_path = resolve_casa_path(&backend_path)?;
        cmd.env(ENV_CASAPATH, casa_path);

        #[cfg(target_os = "linux")]
        {
            let libs_dir = backend_path
                .parent()
                .map(|bin| bin.join("..").join("libs"))
                .filter(|p| p.exists());
            if let Some(libs_dir) = libs_dir {
                let mut ld_library_path = libs_dir.to_string_lossy().into_owned();
                if let Ok(existing) = std::env::var("LD_LIBRARY_PATH")
                    && !existing.trim().is_empty()
                {
                    ld_library_path.push(':');
                    ld_library_path.push_str(existing.trim());
                }
                cmd.env("LD_LIBRARY_PATH", ld_library_path);
            }
        }

        let mut child = cmd.spawn().map_err(AppError::from)?;

        if let Some(stdout) = child.stdout.take() {
            pipe_output(stdout, false);
        }
        if let Some(stderr) = child.stderr.take() {
            pipe_output(stderr, true);
        }

        *state.backend.lock().unwrap() = Some(child);
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, state, base_dir, extra_args);
        Err("unsupported platform".into())
    }
}

fn wait_for_backend(state: &AppState, timeout: Duration) -> AppResult<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], state.backend_port));
    let start = Instant::now();
    let mut last_error: Option<io::Error> = None;

    while start.elapsed() < timeout {
        // Check if backend process is still running.
        // On Windows, this checks wsl.exe which exits when the inner carta_backend exits.
        if let Some(ref mut child) = *state.backend.lock().unwrap()
            && let Ok(Some(status)) = child.try_wait()
        {
            return Err(AppError(format!(
                "Backend process exited unexpectedly with status: {}",
                status
            )));
        }

        match TcpStream::connect_timeout(&addr, Duration::from_millis(CONNECT_TIMEOUT_MS)) {
            Ok(_) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
        std::thread::sleep(Duration::from_millis(CONNECT_RETRY_MS));
    }

    let detail = last_error
        .map(|err| format!(" ({})", err))
        .unwrap_or_default();
    Err(AppError(format!(
        "Backend not ready on port {} after {}s{}",
        state.backend_port,
        timeout.as_secs(),
        detail
    )))
}

fn window_state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(WINDOW_STATE_FILE))
}

fn load_window_bounds(app: &AppHandle) -> Option<WindowBounds> {
    let path = window_state_path(app)?;
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_window_bounds(app: &AppHandle, window: &Window) {
    let Some(path) = window_state_path(app) else {
        return;
    };
    let (Ok(pos), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) else {
        return;
    };

    let bounds = WindowBounds::new(pos, size, scale);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(payload) = serde_json::to_string(&bounds) {
        let _ = fs::write(path, payload);
    }
}

fn focused_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.webview_windows()
        .values()
        .find(|window| window.is_focused().unwrap_or(false))
        .cloned()
}

fn next_window_bounds(app: &AppHandle) -> WindowBounds {
    focused_window(app)
        .or_else(|| app.webview_windows().values().next().cloned())
        .and_then(|w| {
            let pos = w.outer_position().ok()?;
            let size = w.inner_size().ok()?;
            let scale = w.scale_factor().ok()?;
            Some(WindowBounds::new(pos, size, scale).with_offset(WINDOW_OFFSET))
        })
        .or_else(|| load_window_bounds(app))
        .unwrap_or(WindowBounds {
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            x: 0,
            y: 0,
        })
}

fn new_window_label() -> String {
    format!("carta-{}", uuid::Uuid::new_v4())
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .separator()
        .select_all()
        .build()?;

    let new_window = MenuItem::with_id(
        app,
        MENU_NEW_WINDOW,
        "New CARTA Window",
        true,
        Some("CmdOrCtrl+N"),
    )?;
    let toggle_devtools = MenuItem::with_id(
        app,
        MENU_TOGGLE_DEVTOOLS,
        "Toggle DevTools",
        true,
        Some("Alt+CmdOrCtrl+I"),
    )?;
    let toggle_fullscreen = MenuItem::with_id(
        app,
        MENU_TOGGLE_FULLSCREEN,
        "Toggle Fullscreen",
        true,
        Some("F11"),
    )?;

    let app_menu_builder = SubmenuBuilder::new(app, &app.package_info().name)
        .item(&new_window)
        .separator()
        .item(&toggle_fullscreen)
        .separator()
        .item(&toggle_devtools)
        .separator();

    #[cfg(any(target_os = "linux"))]
    let app_menu = {
        let quit = MenuItem::with_id(app, MENU_QUIT, "Quit CARTA", true, Some("Ctrl+Q"))?;
        app_menu_builder.item(&quit).build()?
    };

    #[cfg(not(any(target_os = "linux")))]
    let app_menu = app_menu_builder.quit().build()?;

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .build()
}

fn toggle_devtools(window: &WebviewWindow) {
    if window.is_devtools_open() {
        window.close_devtools();
    } else {
        window.open_devtools();
    }
}

fn toggle_fullscreen(window: &WebviewWindow) {
    let next_state = !window.is_fullscreen().unwrap_or(false);
    let _ = window.set_fullscreen(next_state);
}

fn create_window(app: &AppHandle, state: &AppState, label: String) -> tauri::Result<WebviewWindow> {
    let bounds = next_window_bounds(app);
    let url = WebviewUrl::App(state.window_url.clone().into());
    let menu = build_menu(app)?;
    let window = WebviewWindowBuilder::new(app, label, url)
        .title(WINDOW_TITLE)
        .menu(menu)
        .inner_size(bounds.width as f64, bounds.height as f64)
        .position(bounds.x as f64, bounds.y as f64)
        .build()?;

    if state.inspect {
        window.open_devtools();
    }
    Ok(window)
}

fn handle_menu_event(app: &AppHandle, state: &AppState, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        MENU_NEW_WINDOW => {
            let _ = create_window(app, state, new_window_label());
        }
        MENU_TOGGLE_DEVTOOLS => {
            if let Some(window) = focused_window(app)
                .or_else(|| app.webview_windows().values().next().cloned())
            {
                let _ = window.set_focus();
                toggle_devtools(&window);
            }
        }
        MENU_TOGGLE_FULLSCREEN => {
            if let Some(window) = focused_window(app)
                .or_else(|| app.webview_windows().values().next().cloned())
            {
                let _ = window.set_focus();
                toggle_fullscreen(&window);
            }
        }
        MENU_QUIT => {
            shutdown_backend(state);
            app.exit(0);
        }
        _ => {}
    }
}

fn shutdown_backend(state: &AppState) {
    if let Some(mut child) = state.backend.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli = parse_cli_args();
    let base_dir = match resolve_base_directory(cli.input_path.as_deref()) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(1);
        }
    };

    let backend_port = match portpicker::pick_unused_port() {
        Some(port) => port,
        None => {
            eprintln!("Error: No free port available.");
            std::process::exit(1);
        }
    };
    let backend_token = uuid::Uuid::new_v4().to_string();
    let window_url = format!("http://localhost:{}/?token={}", backend_port, backend_token);

    let state = AppState {
        backend: Mutex::new(None),
        backend_port,
        backend_token,
        window_url,
        inspect: cli.inspect,
    };

    let extra_args = cli.extra_args.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![])
        .menu(build_menu)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            handle_menu_event(app, &state, event);
        })
        .setup(move |app| {
            if cli.help || cli.version {
                if let Err(err) = run_backend_help(app.handle(), cli.version) {
                    eprintln!("{}", err);
                }
                std::process::exit(0);
            }

            let state = app.state::<AppState>();
            spawn_backend(app.handle(), &state, &base_dir, &extra_args)?;
            if let Err(err) = wait_for_backend(&state, Duration::from_secs(BACKEND_TIMEOUT_SECS)) {
                shutdown_backend(&state);
                return Err(err.into());
            }
            create_window(app.handle(), &state, MAIN_WINDOW_LABEL.to_string())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                    save_window_bounds(window.app_handle(), window);
                }
                WindowEvent::CloseRequested { .. } => {
                    let app = window.app_handle();
                    save_window_bounds(app, window);
                    if app.webview_windows().len() <= 1 {
                        app.exit(0);
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            let state = app_handle.state::<AppState>();
            shutdown_backend(&state);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> CliArgs {
        parse_cli_args_from(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn parse_cli_args_respects_double_dash() {
        let parsed = parse_args(&["--", "--inspect", "-weird"]);
        assert_eq!(parsed.input_path.as_deref(), Some("--inspect"));
        assert_eq!(parsed.extra_args, vec!["-weird"]);
        assert!(!parsed.inspect);
    }

    #[test]
    fn parse_cli_args_collects_unknown_flags_with_values() {
        let parsed = parse_args(&["--foo", "bar", "file"]);
        assert_eq!(parsed.input_path.as_deref(), Some("file"));
        assert_eq!(parsed.extra_args, vec!["--foo", "bar"]);
    }

    #[test]
    fn parse_cli_args_handles_positional_args() {
        let parsed = parse_args(&["file1", "file2", "file3"]);
        assert_eq!(parsed.input_path.as_deref(), Some("file1"));
        assert_eq!(parsed.extra_args, vec!["file2", "file3"]);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn resolve_casa_path_uses_space_free_path() {
        let base_dir = std::env::temp_dir().join(format!("carta test {}", uuid::Uuid::new_v4()));
        let backend_dir = base_dir.join("backend");
        let bin_dir = backend_dir.join("bin");
        let etc_dir = backend_dir.join("etc");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&etc_dir).unwrap();
        let backend_path = bin_dir.join(BACKEND_FILENAME);
        fs::write(&backend_path, b"").unwrap();

        let resolved = fs::canonicalize(&etc_dir).unwrap_or_else(|_| etc_dir.clone());
        let link_path = PathBuf::from(SYMLINK_BASE).join(SYMLINK_NAME);
        let mut cleanup_link = false;

        match fs::symlink_metadata(&link_path) {
            Ok(metadata) => {
                if !metadata.file_type().is_symlink() {
                    let _ = fs::remove_dir_all(&base_dir);
                    return;
                }
                let Ok(existing) = fs::read_link(&link_path) else {
                    let _ = fs::remove_dir_all(&base_dir);
                    return;
                };
                if existing != resolved {
                    let _ = fs::remove_dir_all(&base_dir);
                    return;
                }
            }
            Err(_) => cleanup_link = true,
        }

        let casa_path = resolve_casa_path(&backend_path).unwrap();
        let parts: Vec<_> = casa_path.split(' ').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], "linux");
        assert!(parts[0].contains(SYMLINK_NAME));

        if cleanup_link {
            let _ = fs::remove_file(&link_path);
        }
        let _ = fs::remove_dir_all(&base_dir);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolve_casa_path_uses_space_free_path() {
        assert!(
            wsl_bash_output("true").is_ok(),
            "WSL is required to run this test"
        );

        let base_dir = std::env::temp_dir().join(format!("carta test {}", uuid::Uuid::new_v4()));
        let backend_dir = base_dir.join("backend");
        let bin_dir = backend_dir.join("bin");
        let etc_dir = backend_dir.join("etc");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&etc_dir).unwrap();
        let backend_path = bin_dir.join(BACKEND_FILENAME);
        fs::write(&backend_path, b"").unwrap();

        let resolved = fs::canonicalize(&etc_dir).unwrap_or_else(|_| etc_dir.clone());
        let Some(wsl_path) = win_to_wsl_path(&resolved.to_string_lossy()) else {
            let _ = fs::remove_dir_all(&base_dir);
            return;
        };

        let link_path = format!("{}/{}", SYMLINK_BASE, SYMLINK_NAME);
        let link_probe = format!(
            "link={}\nif [ -L \"$link\" ]; then readlink -f \"$link\" || echo __BROKEN__; \
elif [ -e \"$link\" ]; then echo __NONLINK__; else echo __MISSING__; fi\n",
            bash_escape(&link_path)
        );
        let Ok(output) = wsl_bash_output(&link_probe) else {
            let _ = fs::remove_dir_all(&base_dir);
            return;
        };
        let link_state = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let mut cleanup_link = false;
        match link_state.as_str() {
            "__NONLINK__" | "__BROKEN__" => {
                let _ = fs::remove_dir_all(&base_dir);
                return;
            }
            "__MISSING__" => cleanup_link = true,
            existing => {
                if existing != wsl_path {
                    let _ = fs::remove_dir_all(&base_dir);
                    return;
                }
            }
        }

        let casa_path = resolve_casa_path(&backend_path).unwrap();
        let parts: Vec<_> = casa_path.split(' ').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], "linux");
        assert!(parts[0].contains(SYMLINK_NAME));

        if cleanup_link {
            let cleanup = format!(
                "link={}\nexpected={}\nif [ -L \"$link\" ]; then \
target=$(readlink -f \"$link\" || true); \
if [ \"$target\" = \"$expected\" ]; then rm -f \"$link\"; fi; fi\n",
                bash_escape(&link_path),
                bash_escape(&wsl_path)
            );
            let _ = wsl_bash_output(&cleanup);
        }
        let _ = fs::remove_dir_all(&base_dir);
    }
}
