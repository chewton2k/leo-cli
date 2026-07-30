//! Where background diagnostics go.
//!
//! Anything below the UI layer — a git auto-commit, a keychain that will not
//! answer, a malformed config, chunk progress during a long transcription — used
//! to write straight to stderr. That is correct for the CLI and wrong for the
//! TUI, where stdout and stderr are the alternate screen: a stray line lands on
//! top of the panes and stays there until the next full repaint.
//!
//! So diagnostics go through here instead. On the CLI they print immediately. In
//! the TUI they are buffered, and the event loop drains them into the status
//! line, which is where a user can actually read them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// True while a full-screen UI owns the terminal.
static QUIET: AtomicBool = AtomicBool::new(false);

/// Messages waiting for the UI to collect them.
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Cap the buffer. A provider failing on a loop must not grow this without
/// bound while the user is away from the keyboard.
const MAX_PENDING: usize = 32;

/// Start buffering instead of printing. Called when the TUI takes the terminal.
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// Report something the user may want to know but that is not a failure of the
/// thing they asked for.
pub fn warn(message: impl Into<String>) {
    let message = message.into();
    if !is_quiet() {
        eprintln!("  {message}");
        return;
    }
    if let Ok(mut pending) = PENDING.lock() {
        if pending.len() < MAX_PENDING {
            pending.push(message);
        }
    }
}

/// Take everything buffered since the last call.
pub fn drain() -> Vec<String> {
    match PENDING.lock() {
        Ok(mut pending) => std::mem::take(&mut *pending),
        Err(_) => Vec::new(),
    }
}

/// Drop anything buffered without showing it. Used when leaving the TUI, so a
/// message queued mid-session does not surface after the screen is gone.
pub fn clear() {
    if let Ok(mut pending) = PENDING.lock() {
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// The quiet flag and the buffer are process-global, so these tests cannot
    /// run concurrently with each other.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn reset() {
        set_quiet(false);
        clear();
    }

    #[test]
    fn quiet_mode_buffers_instead_of_printing() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        set_quiet(true);
        warn("keychain unavailable");
        warn("config is invalid");

        let drained = drain();
        assert_eq!(drained, vec!["keychain unavailable", "config is invalid"]);
        // Draining takes them, so the same message is not shown twice.
        assert!(drain().is_empty());
        reset();
    }

    #[test]
    fn draining_when_nothing_is_buffered_is_empty_not_an_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();
        assert!(drain().is_empty());
    }

    /// A provider failing in a loop must not grow the buffer forever.
    #[test]
    fn the_buffer_is_bounded() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();
        set_quiet(true);
        for i in 0..(MAX_PENDING * 3) {
            warn(format!("message {i}"));
        }
        let drained = drain();
        assert_eq!(drained.len(), MAX_PENDING);
        // The earliest are kept, which is where a root cause usually is.
        assert_eq!(drained[0], "message 0");
        reset();
    }

    #[test]
    fn clearing_discards_without_showing() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();
        set_quiet(true);
        warn("something");
        clear();
        assert!(drain().is_empty());
        reset();
    }

    #[test]
    fn the_flag_is_readable_so_callers_can_choose_a_channel() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();
        assert!(!is_quiet());
        set_quiet(true);
        assert!(is_quiet());
        reset();
        assert!(!is_quiet());
    }
}
