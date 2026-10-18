use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MOST_EVENTS: usize = 32;
const MOST_EVENT_BYTES: usize = 512 * 1024;
const WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);

#[derive(Clone)]
pub struct Sender {
    tx: tokio::sync::mpsc::Sender<String>,
    stopped: Arc<AtomicBool>,
}

pub(crate) fn channel() -> (Sender, tokio::sync::mpsc::Receiver<String>) {
    let (tx, rx) = tokio::sync::mpsc::channel(MOST_EVENTS);
    (
        Sender {
            tx,
            stopped: Default::default(),
        },
        rx,
    )
}

impl Sender {
    pub fn is_closed(&self) -> bool {
        self.stopped.load(Ordering::Relaxed) || self.tx.is_closed()
    }

    pub(crate) fn try_send(&self, text: String) -> bool {
        if text.len() > MOST_EVENT_BYTES || self.is_closed() {
            self.stopped.store(true, Ordering::Relaxed);
            return false;
        }
        self.tx.try_send(text).is_ok()
    }

    pub(crate) fn send(&self, mut text: String) -> bool {
        let started = Instant::now();
        loop {
            if text.len() > MOST_EVENT_BYTES || self.is_closed() {
                break;
            }
            match self.tx.try_send(text) {
                Ok(()) => return true,
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                Err(tokio::sync::mpsc::error::TrySendError::Full(again)) => text = again,
            }
            if started.elapsed() >= WAIT {
                break;
            }
            std::thread::sleep(POLL);
        }
        self.stopped.store(true, Ordering::Relaxed);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reply_queue_is_bounded_and_disconnects_are_shared() {
        let (tx, rx) = channel();
        for _ in 0..MOST_EVENTS {
            assert!(tx.try_send("piece".into()));
        }
        assert!(!tx.try_send("full".into()));
        assert!(!tx.is_closed());
        let other = tx.clone();
        drop(rx);
        assert!(tx.is_closed());
        assert!(!other.send("closed".into()));
    }

    #[test]
    fn oversized_events_stop_the_reply_without_being_queued() {
        let (tx, mut rx) = channel();
        assert!(!tx.send("x".repeat(MOST_EVENT_BYTES + 1)));
        assert!(tx.is_closed());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_slow_reader_cannot_hold_the_worker_indefinitely() {
        let (tx, _rx) = channel();
        for _ in 0..MOST_EVENTS {
            assert!(tx.try_send("piece".into()));
        }
        let started = Instant::now();
        assert!(!tx.send("full".into()));
        assert!(started.elapsed() >= WAIT);
        assert!(started.elapsed() < WAIT + Duration::from_secs(1));
        assert!(tx.is_closed());
    }

    #[test]
    fn a_reader_can_drain_the_queue_while_the_worker_waits() {
        let (tx, mut rx) = channel();
        for _ in 0..MOST_EVENTS {
            assert!(tx.try_send("piece".into()));
        }
        let worker = std::thread::spawn(move || tx.send("last".into()));
        assert_eq!(rx.blocking_recv().as_deref(), Some("piece"));
        assert!(worker.join().unwrap());
        for _ in 1..MOST_EVENTS {
            assert_eq!(rx.blocking_recv().as_deref(), Some("piece"));
        }
        assert_eq!(rx.blocking_recv().as_deref(), Some("last"));
    }
}
