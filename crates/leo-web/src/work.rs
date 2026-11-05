use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const MOST_FOREGROUND: usize = 4;
const MOST_FAILURES: usize = 12;
const SAME_FAILURES: u8 = 2;

pub(crate) struct Work {
    foreground: Arc<tokio::sync::Semaphore>,
}

impl Default for Work {
    fn default() -> Self {
        Self {
            foreground: Arc::new(tokio::sync::Semaphore::new(MOST_FOREGROUND)),
        }
    }
}

impl Work {
    pub(crate) fn foreground(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        Arc::clone(&self.foreground).try_acquire_owned().ok()
    }
}

#[derive(Default)]
pub(crate) struct Failures(BTreeMap<u64, u8>);

impl Failures {
    pub(crate) fn record(&mut self, name: &str, args: &str, result: &str) -> bool {
        if !result.starts_with("That did not work:") {
            return false;
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (name, args, result).hash(&mut hash);
        let key = hash.finish();
        if self.0.len() >= MOST_FAILURES && !self.0.contains_key(&key) {
            return true;
        }
        let count = self.0.entry(key).or_default();
        *count = count.saturating_add(1);
        *count >= SAME_FAILURES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_repeated_identical_failures_stop_tools() {
        let mut failed = Failures::default();
        for i in 0..MOST_FAILURES {
            assert!(!failed.record("read", &i.to_string(), "That did not work: missing"));
        }
        assert!(failed.record("read", "0", "That did not work: missing"));
        let mut failed = Failures::default();
        for _ in 0..MOST_FAILURES {
            assert!(!failed.record("read", "same", "Found."));
        }
        assert!(!failed.record("read", "same", "That did not work: missing"));
        assert!(!failed.record("read", "same", "That did not work: changed"));
        assert!(failed.record("read", "same", "That did not work: missing"));
    }

    #[test]
    fn foreground_admission_is_bounded_and_released() {
        let work = Work::default();
        let held: Vec<_> = (0..MOST_FOREGROUND)
            .map(|_| work.foreground().unwrap())
            .collect();
        assert!(work.foreground().is_none());
        drop(held);
        assert!(work.foreground().is_some());
    }
}
