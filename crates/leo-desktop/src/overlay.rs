use leo_services::dictation::settings::{Popup, Settings};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};
#[cfg(target_os = "macos")]
tauri_panel! { panel!(DictationPanel { config: { can_become_key_window: false, is_floating_panel: true } }) }
const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 132.0;
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let panel = PanelBuilder::<_, DictationPanel>::new(app, "dictation")
            .url(tauri::WebviewUrl::App("dictation.html".into()))
            .title("Dictation")
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize::new(WIDTH, HEIGHT)))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .with_window(|w| {
                w.decorations(false)
                    .transparent(true)
                    .focusable(false)
                    .visible(false)
            })
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()?;
        panel.hide();
    }
    #[cfg(not(target_os = "macos"))]
    tauri::WebviewWindowBuilder::new(
        app,
        "dictation",
        tauri::WebviewUrl::App("dictation.html".into()),
    )
    .title("Dictation")
    .inner_size(WIDTH, HEIGHT)
    .decorations(false)
    .transparent(true)
    .focused(false)
    .focusable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;
    Ok(())
}
pub fn show(
    app: &AppHandle,
    settings: &Settings,
    current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) {
    if settings.popup == Popup::Off {
        return;
    }
    let app = app.clone();
    let top = settings.top;
    let caller = app.clone();
    let _ = caller.run_on_main_thread(move || {
        if !current() {
            return;
        }
        let Some(window) = app.get_webview_window("dictation") else {
            return;
        };
        let point = crate::permissions::focused_point();
        let monitor = app
            .available_monitors()
            .ok()
            .and_then(|all| {
                all.into_iter().find(|m| {
                    let Some((x, y)) = point else {
                        return false;
                    };
                    let scale = if cfg!(target_os = "macos") {
                        m.scale_factor()
                    } else {
                        1.0
                    };
                    let p = m.position();
                    let s = m.size();
                    x >= p.x as f64 / scale
                        && y >= p.y as f64 / scale
                        && x < (p.x as f64 + s.width as f64) / scale
                        && y < (p.y as f64 + s.height as f64) / scale
                })
            })
            .or_else(|| {
                app.cursor_position()
                    .ok()
                    .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
            })
            .or_else(|| app.primary_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            let scale = monitor.scale_factor();
            let p = monitor.position();
            let s = monitor.size();
            let x = p.x as f64 / scale + (s.width as f64 / scale - WIDTH) / 2.0;
            let y = p.y as f64 / scale
                + if top {
                    46.0
                } else {
                    s.height as f64 / scale - HEIGHT - 40.0
                };
            let _ = window.set_position(tauri::LogicalPosition::new(x, y));
        }
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(panel) = app.get_webview_panel("dictation") {
                panel.show();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window.show();
    });
}
pub fn hide(app: &AppHandle) {
    let app = app.clone();
    let caller = app.clone();
    let _ = caller.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        {
            use tauri_nspanel::ManagerExt;
            if let Ok(panel) = app.get_webview_panel("dictation") {
                panel.hide();
            }
        }
        #[cfg(not(target_os = "macos"))]
        if let Some(window) = app.get_webview_window("dictation") {
            let _ = window.hide();
        }
    });
}
pub fn tokens() -> &'static str {
    let page = include_str!("../../leo-web/src/web/index.html");
    page.split_once("<style>")
        .and_then(|(_, s)| s.split_once("* { box-sizing").map(|(s, _)| s))
        .unwrap_or_default()
}
