mod links;
mod port;
mod shell_path;
mod tray;

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const MAIN: &str = "main";
const HIDDEN: &str = "--hidden";
const LAUNCHED: &str = "app-launched";

struct Engine(Mutex<Option<leo_web::Running>>);

pub fn run() {
    fix_path();
    let hidden = std::env::args().any(|a| a == HIDDEN);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN]),
        ))
        .invoke_handler(tauri::generate_handler![print_page])
        .setup(move |app| {
            let config = leo_core::paths::config_dir().ok();
            let wanted = config
                .as_deref()
                .map(port::remembered)
                .unwrap_or(port::DEFAULT_PORT);
            let started = tauri::async_runtime::block_on(leo_web::start(
                leo_web::StartOptions {
                    port: wanted,
                    token: leo_web::fresh_token(),
                },
                leo_host::powers(),
            ));
            let port = started.as_ref().map(|r| r.port()).unwrap_or(0);
            let address = match &started {
                Ok(running) => {
                    if let Some(config) = &config {
                        let _ = port::remember(config, running.port());
                    }
                    WebviewUrl::External(
                        running
                            .sign_in_url()
                            .parse()
                            .expect("the sign-in address is a URL"),
                    )
                }
                Err(e) => WebviewUrl::App(
                    format!("index.html?problem={}", url_text(&format!("{e:#}"))).into(),
                ),
            };
            app.manage(Engine(Mutex::new(started.ok())));
            let builder = WebviewWindowBuilder::new(app, MAIN, address)
                .title("leo")
                .inner_size(1200.0, 800.0)
                .min_inner_size(480.0, 480.0)
                .visible(!hidden)
                .initialization_script(PRINT)
                .on_navigation(move |url| {
                    if links::stays_inside(url, port) {
                        return true;
                    }
                    let _ = leo_core::open::link(url.as_str());
                    false
                })
                .on_new_window(move |url, _| {
                    if !links::stays_inside(&url, port) {
                        let _ = leo_core::open::link(url.as_str());
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .on_download(|_, event| {
                    match event {
                        tauri::webview::DownloadEvent::Requested { url, destination } => {
                            let suggested = destination
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .or_else(|| {
                                    url.path_segments()
                                        .and_then(|mut s| s.next_back().map(str::to_string))
                                })
                                .unwrap_or_default();
                            if let Some(folder) = downloads_folder() {
                                *destination = links::download_path(&folder, &suggested);
                                remember_download(destination.clone());
                            }
                        }
                        tauri::webview::DownloadEvent::Finished { success: true, .. } => {
                            reveal_last_download();
                        }
                        _ => {}
                    }
                    true
                });
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(18.0, 31.0))
                .initialization_script("window.__LEO_APP__ = 'mac';");
            let window = builder.build()?;
            let handle = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = handle.hide();
                    back_up(handle.app_handle());
                }
            });
            tray::build(app.handle())?;
            open_at_login_once(app.handle(), config.as_deref());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("leo could not start");
    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show(app),
        RunEvent::Exit => stop_engine(app),
        _ => {}
    });
}

const PRINT: &str = "window.print = () => window.__TAURI_INTERNALS__.invoke('print_page');";

#[tauri::command]
fn print_page(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

static LAST_DOWNLOAD: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

fn downloads_folder() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let folder = std::path::PathBuf::from(home).join("Downloads");
    folder.is_dir().then_some(folder)
}

fn remember_download(path: std::path::PathBuf) {
    if let Ok(mut last) = LAST_DOWNLOAD.lock() {
        *last = Some(path);
    }
}

fn reveal_last_download() {
    let path = LAST_DOWNLOAD.lock().ok().and_then(|mut last| last.take());
    let Some(path) = path else {
        return;
    };
    #[cfg(target_os = "macos")]
    let _ = leo_core::children::quiet_command("open")
        .arg("-R")
        .arg(&path)
        .spawn();
    #[cfg(windows)]
    let _ = leo_core::children::quiet_command("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

fn back_up(app: &AppHandle) {
    if let Ok(held) = app.state::<Engine>().0.lock() {
        if let Some(running) = held.as_ref() {
            running.back_up();
        }
    }
}

pub(crate) fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn stop_engine(app: &AppHandle) {
    let running = app
        .state::<Engine>()
        .0
        .lock()
        .ok()
        .and_then(|mut held| held.take());
    leo_host::stop_writing();
    if let Some(running) = running {
        tauri::async_runtime::block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(6), running.stop()).await;
        });
    }
}

fn open_at_login_once(app: &AppHandle, config: Option<&std::path::Path>) {
    let Some(config) = config.filter(|_| !cfg!(debug_assertions)) else {
        return;
    };
    let marker = config.join(LAUNCHED);
    if marker.exists() {
        return;
    }
    if app.autolaunch().enable().is_ok() {
        let _ = std::fs::write(marker, "");
    }
}

fn fix_path() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let shell = std::env::var_os("SHELL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/bin/zsh".into());
    let login = shell_path::login_path(&shell, Duration::from_secs(3));
    let current = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let path = shell_path::merged(
        &current,
        login.as_deref(),
        &shell_path::usual_folders(home.as_deref()),
    );
    std::env::set_var("PATH", path);
}

fn url_text(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}
