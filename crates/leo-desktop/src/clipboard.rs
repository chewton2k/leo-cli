use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

pub fn copy(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|_| "The clipboard could not be opened. Try Copy again.".into())
}
pub fn paste(
    app: &AppHandle,
    text: &str,
    current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Option<String>, String> {
    if !current() {
        return Ok(None);
    }
    if !crate::permissions::accessibility() || crate::permissions::elevated_target() {
        copy(app, text)?;
        return Ok(Some(if cfg!(target_os="macos") { "Press ⌘V. Allow Leo in System Settings → Privacy & Security → Accessibility for automatic paste." } else { "Press Ctrl+V. Windows cannot paste into this elevated window automatically." }.into()));
    }
    let app = app.clone();
    let caller = app.clone();
    let text = text.to_owned();
    let (done, finished) = std::sync::mpsc::sync_channel(1);
    caller
        .run_on_main_thread(move || {
            if !current() { let _=done.send(Ok(None)); return; }
            let result = (|| {
                if app.try_state::<crate::input::EnigoState>().is_none() {
                    app.manage(crate::input::EnigoState::new()?);
                }
                let input = app.state::<crate::input::EnigoState>();
                input.0.with(|enigo| crate::paste_tx::try_reliable_paste(
                    &text, &app, enigo, current.clone()
                )).ok_or("Keyboard input is busy. Try again.")?

            })();
            let result = match result {
                Ok(()) => Ok(None),
                Err(_) => Err("Automatic paste could not preserve your clipboard. Copy your text from Recent dictations.".into()),
            };
            let _ = done.send(result);
        })
        .map_err(|e| e.to_string())?;
    finished
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| "Paste is taking too long. Copy the text from Recent dictations.".to_owned())?
}
