use handy_keys::{Hotkey, HotkeyManager, HotkeyState, KeyboardListener};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn validate(raw: &str) -> Result<(), String> {
    if raw.is_empty() || raw.len() > 200 {
        return Err("Record a shortcut in Dictation settings.".into());
    }
    let hotkey = raw
        .parse::<Hotkey>()
        .map_err(|_| "This shortcut could not be read. Record another combination.".to_owned())?;
    if hotkey.modifiers.is_empty()
        && hotkey
            .key
            .is_some_and(|key| key.to_string().eq_ignore_ascii_case("escape"))
    {
        return Err("Escape cancels dictation. Record another shortcut.".into());
    }
    Ok(())
}
fn fallback(raw: &str) -> Option<String> {
    use handy_keys::Modifiers as M;
    let hotkey: Hotkey = raw.parse().ok()?;
    if hotkey.key.is_none() || hotkey.modifiers.contains(M::FN) {
        return None;
    }
    let mut widened = M::empty();
    for group in [M::CTRL, M::OPT, M::SHIFT, M::CMD] {
        if hotkey.modifiers.intersects(group) {
            widened |= group;
        }
    }
    Some(Hotkey::new(widened, hotkey.key).ok()?.to_handy_string())
}
enum Command {
    Configure(String, mpsc::SyncSender<Result<(), String>>),
    Record(mpsc::SyncSender<Result<(), String>>),
    Resume,
    Stop,
}
pub struct Shortcuts {
    tx: mpsc::SyncSender<Command>,
    status: Arc<Mutex<Value>>,
}
impl Shortcuts {
    pub fn start(
        app: AppHandle,
        shortcut: String,
        capturing: Arc<AtomicBool>,
        input: Arc<dyn Fn(bool, bool) + Send + Sync>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel(8);
        let status = Arc::new(Mutex::new(json!({})));
        let state = status.clone();
        std::thread::spawn(move || run(app, rx, shortcut, capturing, input, state));
        Self { tx, status }
    }
    pub fn view(&self) -> Value {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn configure(&self, shortcut: &str) -> Result<(), String> {
        validate(shortcut)?;
        let (done, answer) = mpsc::sync_channel(1);
        self.tx
            .try_send(Command::Configure(shortcut.into(), done))
            .map_err(|_| "The shortcut is busy. Try again.")?;
        answer
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "The shortcut did not respond. Try again.")?
    }
    pub fn record(&self) -> Result<(), String> {
        let (done, answer) = mpsc::sync_channel(1);
        self.tx
            .try_send(Command::Record(done))
            .map_err(|_| "The shortcut is busy. Try again.")?;
        answer
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "The shortcut recorder did not respond. Try again.")?
    }
    pub fn resume(&self) {
        let _ = self.tx.try_send(Command::Resume);
    }
    pub fn stop(&self) {
        let _ = self.tx.try_send(Command::Stop);
    }
}
fn run(
    app: AppHandle,
    rx: mpsc::Receiver<Command>,
    mut shortcut: String,
    capturing: Arc<AtomicBool>,
    input: Arc<dyn Fn(bool, bool) + Send + Sync>,
    status: Arc<Mutex<Value>>,
) {
    let mut manager = None;
    let mut registered = None;
    let mut cancel = None;
    let mut listener: Option<KeyboardListener> = None;
    let mut recorder_since = None;
    let mut candidate = String::new();
    let mut permission = false;
    let mut secure = false;
    let mut carbon: Option<String> = None;
    let mut carbon_cancel = false;
    let cancel_active = Arc::new(AtomicBool::new(false));
    let carbon_active = Arc::new(AtomicBool::new(false));
    let mut problem = String::new();
    let mut dirty = true;
    let mut checked = Instant::now() - Duration::from_secs(2);
    loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = app.global_shortcut().unregister_all();
                break;
            }
            Ok(Command::Configure(value, done)) => {
                shortcut = value;
                listener = None;
                recorder_since = None;
                candidate.clear();
                dirty = true;
                let _ = done.send(Ok(()));
            }
            Ok(Command::Record(done)) => {
                if crate::secure_input::enabled() {
                    let _=done.send(Err("Close the password field or quit the app holding Secure Input, then record your shortcut again.".into()));
                    continue;
                }
                drop(manager.take());
                registered = None;
                cancel = None;
                let _ = app.global_shortcut().unregister_all();
                carbon = None;
                carbon_cancel = false;
                cancel_active.store(false, Ordering::Release);
                carbon_active.store(false, Ordering::Release);
                match KeyboardListener::new() {
                    Ok(value) => {
                        listener = Some(value);
                        recorder_since = Some(Instant::now());
                        candidate.clear();
                        let _ = done.send(Ok(()));
                    }
                    Err(_) => {
                        problem="Allow Leo in System Settings → Privacy & Security → Accessibility, then try recording the shortcut again.".into();
                        let _ = done.send(Err(problem.clone()));
                        dirty = true;
                    }
                }
            }
            Ok(Command::Resume) => {
                listener = None;
                recorder_since = None;
                dirty = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(listener) = &listener {
            for _ in 0..64 {
                let Some(event) = listener.try_recv() else {
                    break;
                };
                if event.is_key_down {
                    if let Ok(hotkey) = event.as_hotkey() {
                        candidate = hotkey.to_handy_string();
                    }
                }
            }
            if recorder_since.is_some_and(|t| t.elapsed() > Duration::from_secs(30)) {
                recorder_since = None;
                dirty = true;
            }
        }
        if recorder_since.is_none() && listener.is_some() {
            listener = None;
        }
        if checked.elapsed() >= Duration::from_secs(1) {
            let next = crate::permissions::accessibility();
            let next_secure = crate::secure_input::enabled();
            if next != permission || next_secure != secure {
                permission = next;
                secure = next_secure;
                dirty = true;
            }
            checked = Instant::now();
        }
        if listener.is_none() && dirty {
            drop(manager.take());
            registered = None;
            cancel = None;
            let _ = app.global_shortcut().unregister_all();
            carbon = None;
            carbon_cancel = false;
            cancel_active.store(false, Ordering::Release);
            problem.clear();
            if permission {
                match HotkeyManager::new_with_blocking() {
                    Ok(value) => match shortcut.parse::<Hotkey>().map_err(|e|e.to_string()).and_then(|key|value.register(key).map_err(|e|e.to_string())) {
                        Ok(id) => { registered=Some(id); manager=Some(value); }
                        Err(_) => problem="This shortcut could not be registered. Choose another in Dictation settings.".into(),
                    },
                    Err(_) => problem="The shortcut is unavailable. Check Accessibility in Health, then try again.".into(),
                }
            }
            if secure || !permission {
                if let Some(shadow) = fallback(&shortcut) {
                    let use_carbon = carbon_active.clone();
                    let action = input.clone();
                    match app.global_shortcut().on_shortcut(shadow.as_str(),move |_,_,event| { if use_carbon.load(Ordering::Acquire) { action(event.state==ShortcutState::Pressed,false); } }) {
                        Ok(())=>carbon=Some(shadow),
                        Err(_)=>problem="Another app may use this shortcut. Choose another in Dictation settings.".into(),
                    }
                } else if !permission || shortcut.parse::<Hotkey>().is_ok_and(|h| h.key.is_some()) {
                    problem="This shortcut is blocked by Secure Input or missing Accessibility. Close the password field or check Health.".into();
                }
            }
            carbon_active.store(carbon.is_some(), Ordering::Release);
            dirty = false;
        }
        let wants_carbon_cancel =
            listener.is_none() && capturing.load(Ordering::Acquire) && (secure || !permission);
        if wants_carbon_cancel && !carbon_cancel {
            let active = cancel_active.clone();
            let action = input.clone();
            carbon_cancel = app
                .global_shortcut()
                .on_shortcut("escape", move |_, _, event| {
                    if active.load(Ordering::Acquire) && event.state == ShortcutState::Pressed {
                        action(true, true);
                    }
                })
                .is_ok();
            cancel_active.store(carbon_cancel, Ordering::Release);
        } else if !wants_carbon_cancel && carbon_cancel {
            cancel_active.store(false, Ordering::Release);
            let _ = app.global_shortcut().unregister("escape");
            carbon_cancel = false;
        }
        if let Some(manager) = &manager {
            if capturing.load(Ordering::Acquire) && !wants_carbon_cancel && cancel.is_none() {
                cancel = "escape"
                    .parse()
                    .ok()
                    .and_then(|key| manager.register(key).ok());
            }
            if !capturing.load(Ordering::Acquire) || wants_carbon_cancel {
                if let Some(id) = cancel.take() {
                    let _ = manager.unregister(id);
                }
            }
            for _ in 0..64 {
                let Some(event) = manager.try_recv() else {
                    break;
                };
                if Some(event.id) == cancel {
                    if event.state == HotkeyState::Pressed {
                        input(true, true);
                    }
                } else if Some(event.id) == registered && !carbon_active.load(Ordering::Acquire) {
                    input(event.state == HotkeyState::Pressed, false);
                }
            }
        }
        *status.lock().unwrap_or_else(|e| e.into_inner()) = json!({"recording":listener.is_some(),"candidate":candidate,"accessibility":permission,"secure_input":secure,"fallback":carbon,"problem":problem});
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn modifier_and_fn_shortcuts_validate_but_secure_fallback_is_explicit() {
        assert!(super::validate("option").is_ok());
        assert!(super::validate("fn").is_ok());
        assert!(super::validate("ctrl+space").is_ok());
        assert!(super::validate("").is_err());
        assert!(super::validate("escape").is_err());
        assert!(super::fallback("option+space").is_some());
        assert!(super::fallback("fn").is_none());
        assert!(super::fallback("option").is_none());
    }
}
