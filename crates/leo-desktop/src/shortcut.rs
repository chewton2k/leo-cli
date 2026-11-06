use crate::shortcut_events::{self, Binding, Feed};
use handy_keys::{Hotkey, KeyEvent};
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
fn tapped(shortcut: &str, permission: bool, recording: bool, windows: bool) -> bool {
    let exact = shortcut
        .parse::<Hotkey>()
        .ok()
        .zip(fallback(shortcut))
        .is_some_and(|(key, shadow)| key.to_handy_string() == shadow);
    permission && (!windows || recording || !exact)
}
enum Command {
    Configure(String, mpsc::SyncSender<Result<(), String>>),
    Record(mpsc::SyncSender<Result<(), String>>),
    Resume,
    Wake,
    Key(u64, KeyEvent),
}
pub struct Shortcuts {
    tx: mpsc::SyncSender<Command>,
    stopped: Arc<AtomicBool>,
    status: Arc<Mutex<Value>>,
}
impl Shortcuts {
    pub fn start(
        app: AppHandle,
        shortcut: String,
        capturing: Arc<AtomicBool>,
        input: Arc<dyn Fn(bool, bool) + Send + Sync>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel(64);
        let status = Arc::new(Mutex::new(json!({})));
        let state = status.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let sender = tx.clone();
        std::thread::spawn(move || run(app, rx, sender, shortcut, capturing, input, state, stop));
        Self {
            tx,
            stopped,
            status,
        }
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
    pub fn wake(&self) {
        let _ = self.tx.try_send(Command::Wake);
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake();
    }
}
impl Drop for Shortcuts {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    app: AppHandle,
    rx: mpsc::Receiver<Command>,
    tx: mpsc::SyncSender<Command>,
    mut shortcut: String,
    capturing: Arc<AtomicBool>,
    input: Arc<dyn Fn(bool, bool) + Send + Sync>,
    status: Arc<Mutex<Value>>,
    stopped: Arc<AtomicBool>,
) {
    let mut feed: Option<Feed> = None;
    let mut epoch = 0u64;
    let mut binding = None;
    let mut escape = Binding::new("escape".parse().unwrap());
    let mut recorder_since: Option<Instant> = None;
    let mut candidate = String::new();
    let mut permission = false;
    let mut secure = false;
    let mut system = false;
    let mut carbon: Option<String> = None;
    let mut carbon_cancel = false;
    let cancel_active = Arc::new(AtomicBool::new(false));
    let carbon_active = Arc::new(AtomicBool::new(false));
    let mut problem = String::new();
    let mut dirty = true;
    let mut checked = Instant::now() - shortcut_events::IDLE;
    let mut first = true;
    let mut record_done = None;
    while !stopped.load(Ordering::Acquire) {
        let command = if first {
            first = false;
            Some(Command::Wake)
        } else {
            let wait = if recorder_since.is_some() {
                shortcut_events::IDLE
            } else {
                crate::secure_input::user_idle().clamp(shortcut_events::IDLE, shortcut_events::AWAY)
            };
            rx.recv_timeout(wait).ok()
        };
        let mut key = None;
        match command {
            Some(Command::Configure(value, done)) => {
                shortcut = value;
                recorder_since = None;
                candidate.clear();
                dirty = true;
                let _ = done.send(Ok(()));
            }
            Some(Command::Record(done)) => {
                if crate::secure_input::enabled() {
                    let _ = done.send(Err("Close the password field or quit the app holding Secure Input, then record your shortcut again.".into()));
                } else if !permission {
                    let _ = done.send(Err("Allow Leo in System Settings → Privacy & Security → Accessibility, then try again.".into()));
                } else {
                    recorder_since = Some(Instant::now());
                    candidate.clear();
                    dirty = true;
                    record_done = Some(done);
                }
            }
            Some(Command::Resume) => {
                recorder_since = None;
                dirty = true;
            }
            Some(Command::Key(source, event)) => {
                if source == epoch {
                    key = Some(event);
                }
            }
            Some(Command::Wake) | None => {}
        }
        if checked.elapsed() >= shortcut_events::IDLE {
            let next = crate::permissions::accessibility();
            let next_secure = crate::secure_input::enabled();
            if next != permission || next_secure != secure {
                permission = next;
                secure = next_secure;
                dirty = true;
            }
            checked = Instant::now();
        }
        if recorder_since.is_some_and(|at| at.elapsed() >= Duration::from_secs(30)) {
            recorder_since = None;
            dirty = true;
        }
        if dirty {
            drop(feed.take());
            key = None;
            epoch = epoch.wrapping_add(1);
            binding = shortcut.parse().ok().map(Binding::new);
            carbon_active.store(false, Ordering::Release);
            cancel_active.store(false, Ordering::Release);
            let _ = app.global_shortcut().unregister_all();
            carbon = None;
            carbon_cancel = false;
            problem.clear();
            if tapped(
                &shortcut,
                permission,
                recorder_since.is_some(),
                cfg!(windows),
            ) {
                let sender = tx.clone();
                let source = epoch;
                match Feed::start(move |event, stop| {
                    shortcut_events::deliver(&sender, Command::Key(source, event), stop)
                }) {
                    Ok(value) => feed = Some(value),
                    Err(error) => problem = error,
                }
            }
            system = secure || !permission || (cfg!(windows) && feed.is_none());
            if recorder_since.is_none() && system {
                if let Some(shadow) = fallback(&shortcut) {
                    let active = carbon_active.clone();
                    let action = input.clone();
                    match app.global_shortcut().on_shortcut(shadow.as_str(),move |_,_,event| { if active.load(Ordering::Acquire) { action(event.state==ShortcutState::Pressed,false); } }) {
                        Ok(())=>carbon=Some(shadow),
                        Err(_)=>problem="Another app may use this shortcut. Choose another in Dictation settings.".into(),
                    }
                } else if !permission
                    || shortcut
                        .parse::<Hotkey>()
                        .is_ok_and(|key| key.key.is_some())
                {
                    problem="This shortcut is blocked by Secure Input or missing Accessibility. Close the password field or check Health.".into();
                }
            }
            carbon_active.store(carbon.is_some(), Ordering::Release);
            dirty = false;
        }
        if let Some(done) = record_done.take() {
            let result = if feed.is_some() {
                Ok(())
            } else {
                recorder_since = None;
                Err(problem.clone())
            };
            let _ = done.send(result);
        }
        let wants_escape = recorder_since.is_none() && capturing.load(Ordering::Acquire);
        let wants_carbon_cancel = wants_escape && system;
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
        if let Some(feed) = &feed {
            let mut blocked = std::collections::HashSet::new();
            if recorder_since.is_none() {
                if let Some(binding) = &binding {
                    blocked.insert(binding.key);
                }
                if wants_escape && !wants_carbon_cancel {
                    blocked.insert(escape.key);
                }
            }
            feed.block(blocked, recorder_since.is_some());
        }
        if !wants_escape {
            escape = Binding::new("escape".parse().unwrap());
        }
        if let Some(event) = key {
            if recorder_since.is_some() {
                if event.is_key_down {
                    if let Ok(hotkey) = event.as_hotkey() {
                        candidate = hotkey.to_handy_string();
                    }
                }
            } else if wants_escape && !wants_carbon_cancel && escape.input(event) == Some(true) {
                input(true, true);
            } else if !carbon_active.load(Ordering::Acquire) {
                if let Some(pressed) = binding.as_mut().and_then(|binding| binding.input(event)) {
                    input(pressed, false);
                }
            }
        }
        let view = json!({"recording":recorder_since.is_some(),"candidate":candidate,"accessibility":permission,"secure_input":secure,"fallback":carbon,"problem":problem});
        let mut current = status.lock().unwrap_or_else(|e| e.into_inner());
        if *current != view {
            *current = view;
        }
    }
    cancel_active.store(false, Ordering::Release);
    carbon_active.store(false, Ordering::Release);
    let _ = app.global_shortcut().unregister_all();
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

    #[test]
    fn windows_taps_keys_only_when_the_system_hotkey_cannot_do_the_job() {
        use super::tapped;
        assert!(!tapped("ctrl+space", true, false, true));
        assert!(
            tapped("ctrl+space", true, true, true),
            "recording a shortcut"
        );
        assert!(tapped("option", true, false, true), "modifier only");
        assert!(tapped("fn", true, false, true));
        assert!(
            tapped("ctrl_left+space", true, false, true),
            "one side only"
        );
        assert!(
            tapped("ctrl+space", true, false, false),
            "macOS keeps the tap"
        );
        assert!(!tapped("ctrl+space", false, false, false), "no permission");
    }
}
