use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, Weak};

const MOST_FOREGROUND: usize = 4;
const MOST_GRADES: usize = 8;
const MOST_FAILURES: usize = 12;
const SAME_FAILURES: u8 = 2;

pub(crate) struct Work {
    foreground: Arc<tokio::sync::Semaphore>,
    grades: Mutex<BTreeMap<String, Weak<GradeSignal>>>,
}

impl Default for Work {
    fn default() -> Self {
        Self {
            foreground: Arc::new(tokio::sync::Semaphore::new(MOST_FOREGROUND)),
            grades: Default::default(),
        }
    }
}

impl Work {
    pub(crate) fn foreground(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        Arc::clone(&self.foreground).try_acquire_owned().ok()
    }

    pub(crate) fn grade(&self, key: String) -> Option<GradeJob> {
        let mut grades = self.grades.lock().ok()?;
        grades.retain(|_, job| job.strong_count() > 0);
        if let Some(signal) = grades.get(&key).and_then(Weak::upgrade) {
            return Some(GradeJob {
                signal,
                leader: false,
            });
        }
        if grades.len() >= MOST_GRADES {
            return None;
        }
        let signal = Arc::new(GradeSignal {
            finished: tokio::sync::watch::channel(false).0,
        });
        grades.insert(key, Arc::downgrade(&signal));
        Some(GradeJob {
            signal,
            leader: true,
        })
    }
}

struct GradeSignal {
    finished: tokio::sync::watch::Sender<bool>,
}

pub(crate) struct GradeJob {
    signal: Arc<GradeSignal>,
    pub(crate) leader: bool,
}

impl GradeJob {
    pub(crate) async fn wait(&self) {
        let mut finished = self.signal.finished.subscribe();
        let _ = finished.wait_for(|done| *done).await;
    }
}

impl Drop for GradeJob {
    fn drop(&mut self) {
        if self.leader {
            self.signal.finished.send_replace(true);
        }
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

    #[test]
    fn grade_admission_shares_only_the_same_key_and_releases_slots() {
        let work = Work::default();
        let first = work.grade("practice:attempt".into()).unwrap();
        assert!(Arc::ptr_eq(
            &first.signal,
            &work.grade("practice:attempt".into()).unwrap().signal
        ));
        let mut held = vec![first];
        for i in 1..MOST_GRADES {
            held.push(work.grade(i.to_string()).unwrap());
        }
        assert!(work.grade("full".into()).is_none());
        assert!(!Arc::ptr_eq(&held[0].signal, &held[1].signal));
        drop(held);
        assert!(work.grade("released".into()).is_some());
    }
}
