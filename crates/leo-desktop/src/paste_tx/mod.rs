#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub(crate) const QUIET_PERIOD: Duration = Duration::from_millis(200);

pub(crate) const RESTORE_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) const FAILED_INJECTION_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(crate) struct TxState {
    pub published_at: Instant,
    pub injected_at: Option<Instant>,
    pub injection_failed: bool,
    pub receipts: Vec<Instant>,
    pub ownership_lost: bool,
    pub cancelled: bool,
    #[allow(dead_code)]
    pub auto_submit_sent: bool,
    pub logged_receipt: bool,
}

impl TxState {
    pub fn new() -> Self {
        Self {
            published_at: Instant::now(),
            injected_at: None,
            injection_failed: false,
            receipts: Vec::new(),
            ownership_lost: false,
            cancelled: false,
            auto_submit_sent: false,
            logged_receipt: false,
        }
    }

    pub fn record_receipt(&mut self, at: Instant) {
        if self.receipts.len() == 128 {
            self.receipts.remove(0);
        }
        self.receipts.push(at);
        if !self.logged_receipt {
            if let Some(injected) = self.injected_at {
                if at >= injected {
                    self.logged_receipt = true;
                    log::info!(
                        "[reliable-paste] clipboard read {}ms after chord",
                        at.duration_since(injected).as_millis()
                    );
                }
            }
        }
    }

    pub fn last_receipt_after_injection(&self) -> Option<Instant> {
        let injected = self.injected_at?;
        self.receipts.iter().copied().rev().find(|t| *t >= injected)
    }

    pub fn any_receipt_after_injection(&self) -> bool {
        self.last_receipt_after_injection().is_some()
    }
}

pub(crate) enum WaitDecision {
    KeepWaiting,
    Finish,
}

pub(crate) fn evaluate(state: &TxState, now: Instant) -> WaitDecision {
    if state.ownership_lost || state.cancelled {
        return WaitDecision::Finish;
    }
    if let Some(last) = state.last_receipt_after_injection() {
        if now.duration_since(last) >= QUIET_PERIOD {
            return WaitDecision::Finish;
        }
    }
    let deadline = if state.injection_failed {
        FAILED_INJECTION_TIMEOUT
    } else {
        RESTORE_TIMEOUT
    };
    if now.duration_since(state.published_at) >= deadline {
        return WaitDecision::Finish;
    }
    WaitDecision::KeepWaiting
}

const CHORD_HOLD_MS: u64 = 100;

pub(crate) fn send_chord(
    enigo: &mut enigo::Enigo,
    paste_method: &crate::settings::PasteMethod,
) -> Result<(), String> {
    use crate::settings::PasteMethod;
    match paste_method {
        PasteMethod::CtrlV => crate::input::send_paste_ctrl_v(enigo, CHORD_HOLD_MS),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn try_reliable_paste(
    text: &str,
    app_handle: &tauri::AppHandle,
    enigo: &mut enigo::Enigo,
    auto_submit: bool,
    current: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), String> {
    platform::run(text, app_handle, enigo, auto_submit, current)
}

pub(crate) struct Snapshot(pub Vec<Vec<(String, Vec<u8>)>>);
impl Snapshot {
    pub fn new(items: Vec<Vec<(String, Vec<u8>)>>) -> Result<Self, String> {
        if items.len() > 256
            || items.iter().any(|i| i.len() > 256)
            || items
                .iter()
                .flatten()
                .map(|(_, data)| data.len())
                .sum::<usize>()
                > 64 * 1024 * 1024
        {
            return Err("The clipboard is too large to preserve. Copy its contents elsewhere, then dictate again.".into());
        }
        Ok(Self(items))
    }
}

pub(crate) fn should_submit(state: &TxState) -> bool {
    state.any_receipt_after_injection()
        && !state.ownership_lost
        && !state.cancelled
        && !state.injection_failed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_after_publish(published_ago: Duration) -> TxState {
        let mut s = TxState::new();
        s.published_at = Instant::now() - published_ago;
        s
    }

    #[test]
    fn keeps_waiting_without_receipt_within_timeout() {
        let s = state_after_publish(Duration::from_millis(100));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn finishes_after_quiet_period_once_read() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(250));
        s.receipts.push(Instant::now() - QUIET_PERIOD);
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn waits_through_quiet_period_after_recent_read() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        s.receipts.push(Instant::now() - Duration::from_millis(50));
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn pre_injection_receipt_does_not_count() {
        let mut s = state_after_publish(Duration::from_millis(300));
        s.receipts.push(Instant::now() - Duration::from_millis(200));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        assert!(!s.any_receipt_after_injection());
        assert!(matches!(
            evaluate(&s, Instant::now()),
            WaitDecision::KeepWaiting
        ));
    }

    #[test]
    fn finishes_on_timeout_without_receipt() {
        let s = state_after_publish(RESTORE_TIMEOUT);
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn failed_injection_uses_short_timeout() {
        let mut s = state_after_publish(FAILED_INJECTION_TIMEOUT);
        s.injection_failed = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }

    #[test]
    fn ownership_loss_finishes_immediately() {
        let mut s = state_after_publish(Duration::from_millis(10));
        s.ownership_lost = true;
        assert!(matches!(evaluate(&s, Instant::now()), WaitDecision::Finish));
    }
}

#[cfg(test)]
mod snapshot_tests {
    #[test]
    fn a_clipboard_snapshot_keeps_every_item_and_format() {
        let snapshot = super::Snapshot::new(vec![
            vec![
                ("text/plain".into(), b"hello".to_vec()),
                ("text/rtf".into(), b"{rtf}".to_vec()),
            ],
            vec![("public.file-url".into(), b"file:///tmp/a".to_vec())],
        ])
        .unwrap();
        assert_eq!(snapshot.0.len(), 2);
        assert_eq!(snapshot.0[0].len(), 2);
        assert!(super::Snapshot::new(vec![vec![(
            "text/plain".into(),
            vec![0; 64 * 1024 * 1024 + 1]
        )]])
        .is_err());
    }
}

#[cfg(test)]
mod submit_tests {
    use super::*;
    #[test]
    fn enter_is_not_sent_after_a_new_copy_or_cancel() {
        let mut state = TxState::new();
        state.injected_at = Some(Instant::now());
        state.record_receipt(Instant::now());
        assert!(should_submit(&state));
        state.ownership_lost = true;
        assert!(!should_submit(&state));
        state.ownership_lost = false;
        state.cancelled = true;
        assert!(!should_submit(&state));
    }
}
