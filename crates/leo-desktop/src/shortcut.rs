use crate::shortcut_events::{self, Binding, Feed};
use handy_keys::{Hotkey, Key, KeyEvent, Modifiers};
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
    if hotkey.key.is_none()
        && (Modifiers::CMD.contains(hotkey.modifiers) || Modifiers::CTRL.contains(hotkey.modifiers))
    {
        return Err("This key is used to paste. Record another shortcut.".into());
    }
    Ok(())
}
fn loaded(saved: String) -> (String, String) {
    match validate(&saved) {
        Ok(()) => (saved, String::new()),
        Err(error) => {
            let default = leo_services::dictation::settings::Settings::default().shortcut;
            let note = format!(
                "The saved shortcut cannot be used, so Leo uses {default} for now. {error}"
            );
            (default, note)
        }
    }
}
fn release_held(pressed: bool) -> Option<(bool, bool)> {
    pressed.then_some((false, false))
}
fn use_system(secure: bool, permission: bool, feed_ok: bool) -> bool {
    secure || !permission || !feed_ok
}
struct Chord {
    most: Modifiers,
    key: Option<Key>,
    down: Option<Key>,
}
const NO_CHORD: Chord = Chord {
    most: Modifiers::empty(),
    key: None,
    down: None,
};
#[derive(Debug, PartialEq)]
enum Step {
    Held(String),
    Captured(String),
    Cancel,
}
fn chord(modifiers: Modifiers, key: Option<Key>) -> String {
    Hotkey::new(modifiers, key)
        .map(|hotkey| hotkey.to_handy_string())
        .unwrap_or_default()
}
fn recorder_step(state: &mut Chord, event: KeyEvent) -> Step {
    if state.most.is_empty() && state.key.is_none() {
        if !event.is_key_down {
            return Step::Held(String::new());
        }
        if event.key == Some(Key::Escape) && event.modifiers.is_empty() {
            return Step::Cancel;
        }
    }
    state.most |= event.modifiers;
    if event.is_key_down && event.key.is_some() {
        state.key = event.key;
        state.down = event.key;
    } else if event.key == state.down {
        state.down = None;
    }
    if !event.is_key_down && event.modifiers.is_empty() && state.down.is_none() {
        return Step::Captured(chord(state.most, state.key));
    }
    Step::Held(chord(event.modifiers, state.down))
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
const TAKEN: &str = "Another app may use this shortcut. Choose another in Dictation settings.";
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
    Key(u64, KeyEvent, Instant),
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
        let (shortcut, note) = loaded(shortcut);
        let status = Arc::new(Mutex::new(json!({})));
        let state = status.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let sender = tx.clone();
        std::thread::spawn(move || {
            run(
                app, rx, sender, shortcut, note, capturing, input, state, stop,
            )
        });
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
    mut note: String,
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
    let mut chord_state = NO_CHORD;
    let mut pressing = String::new();
    let mut captured = String::new();
    let mut configure_done = None;
    let mut permission = false;
    let mut secure = false;
    let mut system = false;
    let mut carbon: Option<String> = None;
    let mut carbon_cancel = false;
    let cancel_active = Arc::new(AtomicBool::new(false));
    let carbon_active = Arc::new(AtomicBool::new(false));
    let carbon_down = Arc::new(AtomicBool::new(false));
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
                configure_done = Some((done, std::mem::replace(&mut shortcut, value)));
                recorder_since = None;
                note.clear();
                dirty = true;
            }
            Some(Command::Record(done)) => {
                if crate::secure_input::enabled() {
                    let _ = done.send(Err("Close the password field or quit the app holding Secure Input, then record your shortcut again.".into()));
                } else if !permission {
                    let _ = done.send(Err("Allow Leo in System Settings → Privacy & Security → Accessibility, then try again.".into()));
                } else {
                    recorder_since = Some(Instant::now());
                    chord_state = NO_CHORD;
                    pressing.clear();
                    captured.clear();
                    note.clear();
                    dirty = true;
                    record_done = Some(done);
                }
            }
            Some(Command::Resume) => {
                recorder_since = None;
                dirty = true;
            }
            Some(Command::Key(source, event, _at)) => {
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
            note = "No keys were pressed. Click Change to try again.".into();
            dirty = true;
        }
        let mut taken = false;
        if dirty {
            if let Some((pressed, cancel)) = release_held(
                binding
                    .as_ref()
                    .is_some_and(|binding: &Binding| binding.pressed)
                    | carbon_down.swap(false, Ordering::AcqRel),
            ) {
                input(pressed, cancel);
            }
            drop(feed.take());
            key = None;
            epoch = epoch.wrapping_add(1);
            binding = shortcut.parse().ok().map(Binding::new);
            carbon_active.store(false, Ordering::Release);
            cancel_active.store(false, Ordering::Release);
            let _ = app.global_shortcut().unregister_all();
            carbon = None;
            carbon_cancel = false;
            problem.clone_from(&note);
            if tapped(
                &shortcut,
                permission,
                recorder_since.is_some(),
                cfg!(windows),
            ) {
                let sender = tx.clone();
                let source = epoch;
                match Feed::start(move |event, at, stop| {
                    shortcut_events::deliver(&sender, Command::Key(source, event, at), stop)
                }) {
                    Ok(value) => feed = Some(value),
                    Err(error) => problem = error,
                }
            }
            system = use_system(secure, permission, feed.is_some());
            if recorder_since.is_none() && system {
                if let Some(shadow) = fallback(&shortcut) {
                    let active = carbon_active.clone();
                    let action = input.clone();
                    let down = carbon_down.clone();
                    match app
                        .global_shortcut()
                        .on_shortcut(shadow.as_str(), move |_, _, event| {
                            if active.load(Ordering::Acquire) {
                                let pressed = event.state == ShortcutState::Pressed;
                                down.store(pressed, Ordering::Release);
                                action(pressed, false);
                            }
                        }) {
                        Ok(()) => carbon = Some(shadow),
                        Err(_) => {
                            taken = true;
                            problem = TAKEN.into();
                        }
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
        if let Some((done, previous)) = configure_done.take() {
            let refused = taken && previous != shortcut;
            if refused {
                shortcut = previous;
                dirty = true;
                let _ = tx.try_send(Command::Wake);
            }
            let _ = done.send(if refused { Err(TAKEN.into()) } else { Ok(()) });
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
                match recorder_step(&mut chord_state, event) {
                    Step::Held(keys) => pressing = keys,
                    Step::Captured(keys) => {
                        captured = keys;
                        recorder_since = None;
                    }
                    Step::Cancel => recorder_since = None,
                }
                if recorder_since.is_none() {
                    dirty = true;
                    let _ = tx.try_send(Command::Wake);
                }
            } else if wants_escape && !wants_carbon_cancel && escape.input(event) == Some(true) {
                input(true, true);
            } else if !carbon_active.load(Ordering::Acquire) {
                if let Some(pressed) = binding.as_mut().and_then(|binding| binding.input(event)) {
                    input(pressed, false);
                }
            }
        }
        let view = json!({"recording":recorder_since.is_some(),"held":if recorder_since.is_some() { pressing.as_str() } else { "" },"captured":captured,"accessibility":permission,"secure_input":secure,"fallback":carbon,"problem":problem});
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

    #[test]
    fn paste_modifiers_alone_are_refused_and_a_bad_saved_shortcut_loads_the_default() {
        use super::{loaded, validate};
        for raw in ["cmd", "cmd_left", "ctrl", "ctrl_right"] {
            assert_eq!(
                validate(raw),
                Err("This key is used to paste. Record another shortcut.".into()),
                "{raw}"
            );
        }
        for raw in ["cmd+k", "option", "ctrl+shift", "fn"] {
            assert!(validate(raw).is_ok(), "{raw}");
        }
        assert_eq!(loaded("fn".into()), ("fn".into(), String::new()));
        let (shortcut, note) = loaded("cmd".into());
        assert_eq!(
            shortcut,
            leo_services::dictation::settings::Settings::default().shortcut
        );
        assert!(note.contains("This key is used to paste."), "{note}");
    }

    #[test]
    fn a_held_binding_is_released_and_a_failed_feed_uses_the_system_shortcut() {
        use super::{release_held, use_system};
        assert_eq!(release_held(true), Some((false, false)));
        assert_eq!(release_held(false), None);
        assert!(!use_system(false, true, true));
        assert!(use_system(false, true, false), "feed failed");
        assert!(use_system(true, true, true), "secure input");
        assert!(use_system(false, false, true), "no permission");
    }

    #[test]
    fn the_recorder_keeps_the_largest_chord_until_every_key_is_up() {
        use super::{recorder_step, Step, NO_CHORD};
        use handy_keys::{Hotkey, Key, KeyEvent, Modifiers as M};
        let event = |modifiers, key, is_key_down| KeyEvent {
            modifiers,
            key,
            is_key_down,
            changed_modifier: None,
        };
        let name = |raw: &str| raw.parse::<Hotkey>().unwrap().to_handy_string();
        for keys_first in [false, true] {
            let mut state = NO_CHORD;
            let mut step = |e| recorder_step(&mut state, e);
            assert_eq!(
                step(event(M::empty(), Some(Key::Return), false)),
                Step::Held(String::new())
            );
            assert_eq!(step(event(M::CMD, None, true)), Step::Held(name("cmd")));
            assert_eq!(
                step(event(M::CMD | M::SHIFT, None, true)),
                Step::Held(name("cmd+shift"))
            );
            let k = event(M::CMD | M::SHIFT, Some(Key::K), true);
            assert_eq!(step(k), Step::Held(name("cmd+shift+k")));
            assert_eq!(step(k), Step::Held(name("cmd+shift+k")), "repeat");
            if keys_first {
                assert_eq!(
                    step(event(M::CMD | M::SHIFT, Some(Key::K), false)),
                    Step::Held(name("cmd+shift"))
                );
                assert_eq!(
                    step(event(M::SHIFT, None, false)),
                    Step::Held(name("shift"))
                );
                assert_eq!(
                    step(event(M::empty(), None, false)),
                    Step::Captured(name("cmd+shift+k"))
                );
            } else {
                assert_eq!(step(event(M::CMD, None, false)), Step::Held(name("cmd+k")));
                assert_eq!(step(event(M::empty(), None, false)), Step::Held(name("k")));
                assert_eq!(
                    step(event(M::empty(), Some(Key::K), false)),
                    Step::Captured(name("cmd+shift+k"))
                );
            }
        }
        for (raw, modifier) in [("fn", M::FN), ("option", M::OPT)] {
            let mut state = NO_CHORD;
            recorder_step(&mut state, event(modifier, None, true));
            assert_eq!(
                recorder_step(&mut state, event(M::empty(), None, false)),
                Step::Captured(name(raw))
            );
        }
        let mut state = NO_CHORD;
        assert_eq!(
            recorder_step(&mut state, event(M::empty(), Some(Key::Escape), true)),
            Step::Cancel
        );
        recorder_step(&mut state, event(M::SHIFT, None, true));
        assert_eq!(
            recorder_step(&mut state, event(M::SHIFT, Some(Key::Escape), true)),
            Step::Held(name("shift+escape"))
        );
    }
}
