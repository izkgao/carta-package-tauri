// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use std::{
    fs,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
};

use tauri::{
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
    AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window,
    WindowEvent,
};

const DEFAULT_WINDOW_WIDTH: u32 = 1920;
const DEFAULT_WINDOW_HEIGHT: u32 = 1080;
const WINDOW_OFFSET: i32 = 25;
const WINDOW_STATE_FILE: &str = "window-state.json";

#[derive(Debug)]
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

struct AppState {
    backend: Mutex<Option<Child>>,
    backend_port: u16,
    backend_token: String,
    window_url: String,
    inspect: bool,
}

fn parse_cli_args() -> CliArgs {
    let mut input_path = None;
    let mut extra_args = Vec::new();
    let mut inspect = false;
    let mut help = false;
    let mut version = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--inspect" {
            inspect = true;
            i += 1;
            continue;
        }

        if arg == "--help" || arg == "-h" {
            help = true;
            i += 1;
            continue;
        }

        if arg == "--version" || arg == "-v" {
            version = true;
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            extra_args.push(arg.clone());
            if !arg.contains('=') && i + 1 < args.len() {
                let next = &args[i + 1];
                if !next.starts_with('-') {
                    extra_args.push(next.clone());
                    i += 2;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        if input_path.is_none() {
            input_path = Some(arg.clone());
        } else {
            extra_args.push(arg.clone());
        }
        i += 1;
    }

    CliArgs {
        input_path,
        extra_args,
        inspect,
        help,
        version,
    }
}

fn resolve_base_directory(input_path: Option<&str>) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|err| err.to_string())?;

    if let Some(path) = input_path {
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            cwd.join(path)
        };
        let metadata = fs::metadata(&candidate)
            .map_err(|_| "Error: Requested file or directory does not exist".to_string())?;

        if metadata.is_file() {
            Ok(candidate
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .to_path_buf())
        } else if metadata.is_dir() {
            Ok(candidate)
        } else {
            Err("Error: Requested path is neither a file nor a directory".to_string())
        }
    } else if cfg!(target_os = "macos") && cwd == Path::new("/") {
        home_dir().ok_or_else(|| "Error: HOME directory not found".to_string())
    } else {
        Ok(cwd)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn backend_filename() -> String {
    format!("carta_backend{}", std::env::consts::EXE_SUFFIX)
}

fn resolve_backend_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("bin").join(backend_filename());
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("src-tauri").join("bin").join(backend_filename());
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Error: carta_backend binary not found".to_string())
}

fn run_backend_help(app: &AppHandle, version: bool) -> Result<(), String> {
    let backend_path = resolve_backend_path(app)?;
    let mut cmd = Command::new(backend_path);
    if version {
        cmd.arg("--version");
    } else {
        cmd.arg("--help");
    }

    let output = cmd.output().map_err(|err| err.to_string())?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    if !version {
        println!("Additional Tauri flag:");
        println!("      --inspect      Open the DevTools in the Tauri window.");
    }

    Ok(())
}

fn spawn_backend(
    app: &AppHandle,
    state: &AppState,
    base_dir: &Path,
    extra_args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let backend_path =
        resolve_backend_path(app).map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;

    let mut cmd = Command::new(&backend_path);
    cmd.arg(base_dir)
        .arg(format!("--port={}", state.backend_port))
        .arg("--no_frontend")
        .arg("--no_browser")
        .args(extra_args)
        .env("CARTA_AUTH_TOKEN", &state.backend_token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(casa_path) = resolve_casa_path(app, &backend_path) {
        cmd.env("CASAPATH", casa_path);
    }

    let mut child = cmd.spawn()?;

    if let Some(stdout) = child.stdout.take() {
        pipe_output(stdout, false);
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_output(stderr, true);
    }

    *state.backend.lock().unwrap() = Some(child);
    Ok(())
}

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

fn resolve_casa_path(app: &AppHandle, backend_path: &Path) -> Option<String> {
    let etc_path = resolve_etc_path(app, backend_path)?;
    let resolved = fs::canonicalize(&etc_path).unwrap_or(etc_path);
    Some(format!("../../../../../{} linux", resolved.display()))
}

fn resolve_etc_path(app: &AppHandle, backend_path: &Path) -> Option<PathBuf> {
    if let Some(bin_dir) = backend_path.parent() {
        let candidate = bin_dir.join("..").join("etc");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("resources").join("etc");
        if candidate.exists() {
            return Some(candidate);
        }

        let candidate = resource_dir.join("etc");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("src-tauri").join("etc");
        if candidate.exists() {
            return Some(candidate);
        }

        let candidate = cwd.join("etc");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
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

    let (pos, size) = match (window.outer_position(), window.inner_size()) {
        (Ok(pos), Ok(size)) => (pos, size),
        _ => return,
    };

    let bounds = WindowBounds {
        width: size.width,
        height: size.height,
        x: pos.x,
        y: pos.y,
    };

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
    if let Some(window) =
        focused_window(app).or_else(|| app.webview_windows().values().next().cloned())
    {
        if let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) {
            return WindowBounds {
                width: size.width,
                height: size.height,
                x: pos.x + WINDOW_OFFSET,
                y: pos.y + WINDOW_OFFSET,
            };
        }
    }

    load_window_bounds(app).unwrap_or(WindowBounds {
        width: DEFAULT_WINDOW_WIDTH,
        height: DEFAULT_WINDOW_HEIGHT,
        x: 0,
        y: 0,
    })
}

fn new_window_label() -> String {
    format!("carta-{}", uuid::Uuid::new_v4())
}

fn create_window(app: &AppHandle, state: &AppState, label: String) -> tauri::Result<WebviewWindow> {
    let bounds = next_window_bounds(app);
    let url = WebviewUrl::App(state.window_url.clone().into());
    let window = WebviewWindowBuilder::new(app, label, url)
        .title("CARTA")
        .inner_size(bounds.width as f64, bounds.height as f64)
        .position(bounds.x as f64, bounds.y as f64)
        .build()?;

    maybe_open_devtools(&window, state.inspect);
    Ok(window)
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

    let mut menu = MenuBuilder::new(app);

    if cfg!(target_os = "macos") {
        let new_window = MenuItem::with_id(
            app,
            "new_window",
            "New CARTA Window",
            true,
            Some("CmdOrCtrl+N"),
        )?;
        let toggle_devtools = MenuItem::with_id(
            app,
            "toggle_devtools",
            "Toggle DevTools",
            true,
            Some("Alt+CmdOrCtrl+I"),
        )?;

        let app_menu = SubmenuBuilder::new(app, &app.package_info().name)
            .item(&new_window)
            .separator()
            .fullscreen()
            .separator()
            .item(&toggle_devtools)
            .separator()
            .quit()
            .build()?;

        menu = menu.item(&app_menu);
    }

    menu.item(&edit_menu).build()
}

fn handle_menu_event(app: &AppHandle, state: &AppState, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "new_window" => {
            let _ = create_window(app, state, new_window_label());
        }
        "toggle_devtools" => {
            if let Some(window) = focused_window(app) {
                toggle_devtools(&window);
            }
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

fn maybe_open_devtools(window: &WebviewWindow, enabled: bool) {
    if !enabled {
        return;
    }

    #[cfg(debug_assertions)]
    {
        window.open_devtools();
    }
}

fn toggle_devtools(window: &WebviewWindow) {
    #[cfg(debug_assertions)]
    {
        if window.is_devtools_open() {
            window.close_devtools();
        } else {
            window.open_devtools();
        }
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
    let window_url = format!(
        "dist/index.html?socketUrl=ws://localhost:{}&token={}",
        backend_port, backend_token
    );

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
            create_window(app.handle(), &state, "main".to_string())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                let app = window.app_handle();
                save_window_bounds(app, window);
                if app.webview_windows().len() <= 1 {
                    app.exit(0);
                }
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
