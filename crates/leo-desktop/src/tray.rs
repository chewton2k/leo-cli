//! The menu bar icon: quick actions (record, copy the last dictation, Settings,
//! updates) and what Leo is using right now (speech model, writing AI and its
//! plan limits, AI use since launch, version). The status rows are disabled
//! items whose text a background thread refreshes, so opening the menu never
//! waits on the disk or an agent.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::app_settings::UpdateApi;

/// How often the status rows are read again.
const REFRESH: Duration = Duration::from_secs(20);
/// Codex's limits need an app-server round trip, so they are asked every five
/// minutes (`usage::STALE_AFTER`), not every refresh.
const WRITING_EVERY: u32 = 15;

/// A menu command is offered to the page this many times, this far apart.
const ASK_TRIES: u32 = 10;
const ASK_GAP: Duration = Duration::from_millis(500);

struct Status {
    speech: MenuItem<Wry>,
    writing: MenuItem<Wry>,
    limits: MenuItem<Wry>,
    used: MenuItem<Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let info = |id: &str, text: &str| MenuItem::with_id(app, id, text, false, None::<&str>);
    let open = item("open", "Open Leo")?;
    let lecture = item("record-microphone", "Record a lecture")?;
    let call = item("record-call", "Record a call")?;
    let copy = item("copy-dictation", "Copy last dictation")?;
    let status = Arc::new(Status {
        speech: info("speech", "Speech: …")?,
        writing: info("writing", "Writing: …")?,
        limits: info("limits", "Plan use: unknown")?,
        used: info("used", "No AI requests since Leo started")?,
    });
    let settings = item("settings", "Settings…")?;
    let updates = item("updates", "Check for updates…")?;
    let version = info("version", &format!("Leo {}", env!("CARGO_PKG_VERSION")))?;
    let quit = item("quit", "Quit Leo")?;
    let line = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &line()?,
            &lecture,
            &call,
            &copy,
            &line()?,
            &status.speech,
            &status.writing,
            &status.limits,
            &status.used,
            &line()?,
            &settings,
            &updates,
            &version,
            &line()?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("leo")
        .tooltip("Leo")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show(app),
            "record-microphone" => page(app, "record", "microphone"),
            "record-call" => page(app, "record", "call"),
            "copy-dictation" => copy_last_dictation(app),
            "settings" => page(app, "settings", "general"),
            "updates" => {
                if let Some(updates) = app.try_state::<Arc<crate::updates::Updates>>() {
                    // A check already running is fine: the page shows its result.
                    let _ = updates.change("check_updates");
                }
                page(app, "settings", "updates");
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    let handle = app.clone();
    std::thread::Builder::new()
        .name("leo-tray-status".into())
        .spawn(move || refresh(handle, status))?;
    Ok(())
}

/// Shows the window and asks the page to go somewhere (`leoFromApp` in
/// app/events.js). The arguments are fixed words, never user text. The page may
/// still be loading (first launch, a reload), so the request is tried for a few
/// seconds and runs at most once (`__leoAsked` remembers each click's number).
fn page(app: &AppHandle, what: &'static str, which: &'static str) {
    static CLICKS: AtomicU64 = AtomicU64::new(0);
    crate::show(app);
    let click = CLICKS.fetch_add(1, Ordering::Relaxed);
    let app = app.clone();
    let script = format!(
        "(() => {{ const done = window.__leoAsked = window.__leoAsked || {{}}; \
         if (done[{click}] || !window.leoFromApp) return; done[{click}] = true; \
         window.leoFromApp('{what}', '{which}'); }})()"
    );
    let _ = std::thread::Builder::new()
        .name("leo-tray-page".into())
        .spawn(move || {
            for _ in 0..ASK_TRIES {
                if let Some(window) = app.get_webview_window(crate::MAIN) {
                    let _ = window.eval(&script);
                }
                std::thread::sleep(ASK_GAP);
            }
        });
}

/// Reads the newest dictation off the main thread, so a busy database never
/// holds up the menu.
fn copy_last_dictation(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("leo-tray-copy".into())
        .spawn(move || {
            let last = leo_core::paths::data_dir()
                .ok()
                .and_then(|data| leo_web::dictation::recent(&data).ok())
                .and_then(|rows| rows.into_iter().next())
                .and_then(|row| row["text"].as_str().map(str::to_string));
            if let Some(text) = last {
                let _ = app.clipboard().write_text(text);
            }
        });
}

fn refresh(app: AppHandle, status: Arc<Status>) {
    let mut round = 0u32;
    loop {
        let speech = leo_services::speech::library::library()
            .ok()
            .and_then(|library| library.selected_name())
            .unwrap_or_else(|| "no model chosen".into());
        let _ = status.speech.set_text(format!("Speech: {speech}"));
        let writing = leo_services::web_settings::writing_status(round % WRITING_EVERY == 0);
        let _ = status.writing.set_text(format!("Writing: {}", writing.0));
        let _ = status.limits.set_text(match &writing.1 {
            Some(limits) => format!("Plan use: {limits}"),
            None if writing.0 == "No writing AI chosen" => {
                "Plan use: choose a writing AI in Settings".into()
            }
            None => "Plan use: shown after the first answer".into(),
        });
        let used = app.try_state::<crate::Engine>().and_then(|engine| {
            engine
                .0
                .lock()
                .ok()
                .and_then(|held| held.as_ref().map(|running| running.usage_line()))
        });
        if let Some(used) = used {
            let _ = status.used.set_text(used);
        }
        round = round.wrapping_add(1);
        std::thread::sleep(REFRESH);
    }
}
