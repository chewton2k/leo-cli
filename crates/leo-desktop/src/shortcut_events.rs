use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

use handy_keys::{Hotkey, KeyEvent, KeyboardListener};

pub const IDLE: Duration = Duration::from_secs(1);
pub const AWAY: Duration = Duration::from_secs(10);
const LINGER: Duration = Duration::from_secs(30);
const BACKPRESSURE: Duration = Duration::from_millis(5);

pub struct Binding {
    pub key: Hotkey,
    pressed: bool,
}

impl Binding {
    pub fn new(key: Hotkey) -> Self {
        Self {
            key,
            pressed: false,
        }
    }

    pub fn input(&mut self, event: KeyEvent) -> Option<bool> {
        if event.is_key_down {
            if !self.pressed
                && self.key.key == event.key
                && self.key.modifiers.matches(event.modifiers)
            {
                self.pressed = true;
                return Some(true);
            }
        } else if self.pressed
            && ((event.key.is_some() && event.key == self.key.key)
                || (event.key.is_none() && !self.key.modifiers.matches(event.modifiers)))
        {
            self.pressed = false;
            return Some(false);
        }
        None
    }
}

pub fn deliver<T>(tx: &mpsc::SyncSender<T>, mut event: T, stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::Acquire) {
        match tx.try_send(event) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
            Err(mpsc::TrySendError::Full(pending)) => event = pending,
        }
        std::thread::sleep(BACKPRESSURE);
    }
    false
}

pub struct Feed {
    stop: Arc<AtomicBool>,
    blocked: Arc<Mutex<HashSet<Hotkey>>>,
    recording: Arc<AtomicBool>,
}

impl Feed {
    pub fn start(
        send: impl Fn(KeyEvent, &AtomicBool) -> bool + Send + 'static,
    ) -> Result<Self, String> {
        let blocked = Arc::new(Mutex::new(HashSet::new()));
        let listener = KeyboardListener::new_with_blocking(blocked.clone()).map_err(|_| {
            "Allow Leo in System Settings → Privacy & Security → Accessibility, then try again."
                .to_owned()
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let recording = Arc::new(AtomicBool::new(false));
        let raw = recording.clone();
        let keys = blocked.clone();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.recv_timeout(LINGER) {
                    Ok(event) => {
                        let relevant = raw.load(Ordering::Acquire)
                            || keys
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .iter()
                                .any(|key| event.key.is_none() || event.key == key.key);
                        if relevant && !send(event, &stopped) {
                            break;
                        }
                    }
                    Err(handy_keys::Error::Timeout) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            blocked,
            recording,
        })
    }

    pub fn block(&self, keys: HashSet<Hotkey>, recording: bool) {
        self.recording.store(recording, Ordering::Release);
        let mut blocked = self.blocked.lock().unwrap_or_else(|e| e.into_inner());
        if *blocked != keys {
            *blocked = keys;
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.blocked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handy_keys::{Key, Modifiers as M};

    fn event(modifiers: M, key: Option<Key>, down: bool) -> KeyEvent {
        KeyEvent {
            modifiers,
            key,
            is_key_down: down,
            changed_modifier: None,
        }
    }

    #[test]
    fn chords_ignore_repeats_and_release_on_key_or_modifier_release() {
        for modifiers_first in [false, true] {
            let mut binding = Binding::new("option+space".parse().unwrap());
            assert_eq!(binding.input(event(M::CTRL, Some(Key::Space), true)), None);
            assert_eq!(
                binding.input(event(M::OPT, Some(Key::Space), true)),
                Some(true)
            );
            assert_eq!(binding.input(event(M::OPT, Some(Key::Space), true)), None);
            let released = if modifiers_first {
                event(M::empty(), None, false)
            } else {
                event(M::OPT, Some(Key::Space), false)
            };
            assert_eq!(binding.input(released), Some(false));
            assert_eq!(binding.input(released), None);
        }
    }

    #[test]
    fn modifier_only_fn_and_side_specific_bindings_preserve_matching() {
        for modifiers in [M::FN, M::OPT, M::OPT_LEFT] {
            let key = Hotkey::new(modifiers, None).unwrap();
            let mut binding = Binding::new(key);
            assert_eq!(binding.input(event(key.modifiers, None, true)), Some(true));
            assert_eq!(binding.input(event(key.modifiers, None, false)), None);
            assert_eq!(binding.input(event(M::empty(), None, false)), Some(false));
        }
    }

    #[test]
    fn a_full_event_queue_preserves_order_and_shutdown_does_not_wait_on_capacity() {
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        tx.send(1).unwrap();
        let flag = stop.clone();
        let worker = std::thread::spawn(move || deliver(&tx, 2, &flag));
        assert_eq!(rx.recv().unwrap(), 1);
        assert_eq!(rx.recv_timeout(IDLE).unwrap(), 2);
        assert!(worker.join().unwrap());
        let (tx, _rx) = mpsc::sync_channel(1);
        tx.send(1).unwrap();
        let (started, ready) = mpsc::sync_channel(1);
        let (done, finished) = mpsc::sync_channel(1);
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            done.send(deliver(&tx, 2, &flag)).unwrap();
        });
        ready.recv().unwrap();
        stop.store(true, Ordering::Release);
        assert!(!finished.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }
}
