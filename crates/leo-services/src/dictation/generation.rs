use std::sync::atomic::{AtomicU64, Ordering};
#[derive(Default)]
pub struct Generation(AtomicU64);
impl Generation {
    pub fn apply<T, R>(
        &self,
        id: u64,
        state: &std::sync::Mutex<T>,
        apply: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        if !self.current(id) {
            return None;
        }
        Some(apply(&mut state))
    }
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn cancel(&self) {
        self.next();
    }
    pub fn current(&self, value: u64) -> bool {
        self.0.load(Ordering::Acquire) == value
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancelled_updates_waiting_for_the_state_lock_are_ignored() {
        use std::sync::{Arc, Mutex};
        let generation = Arc::new(super::Generation::default());
        let id = generation.next();
        let state = Arc::new(Mutex::new(0));
        let held = state.lock().unwrap();
        let worker = generation.clone();
        let state_worker = state.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            started.send(()).unwrap();
            worker.apply(id, &state_worker, |value| *value = 1)
        });
        ready.recv().unwrap();
        generation.cancel();
        drop(held);
        assert_eq!(thread.join().unwrap(), None);
        assert_eq!(*state.lock().unwrap(), 0);
    }
    #[test]
    fn cancelled_and_old_completions_can_never_paste() {
        let generation = super::Generation::default();
        let first = generation.next();
        assert!(generation.current(first));
        generation.cancel();
        assert!(!generation.current(first));
        let second = generation.next();
        assert!(!generation.current(first));
        assert!(generation.current(second));
    }
}
