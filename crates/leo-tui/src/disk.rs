use std::time::{Duration, Instant};

use super::*;

const DISK_CHECK_EVERY: Duration = Duration::from_secs(2);

impl App {
    pub(super) fn maybe_reload_from_disk(&mut self) -> bool {
        if self
            .jobs
            .last_disk_check
            .is_some_and(|at| at.elapsed() < DISK_CHECK_EVERY)
        {
            return false;
        }
        self.jobs.last_disk_check = Some(Instant::now());
        let busy = self.mode != Mode::Normal
            || self.jobs.recording.is_some()
            || self.jobs.asking.is_some()
            || self.writing.editing.is_some();
        if busy || !self.store.changed_on_disk() || self.store.refresh().is_err() {
            return false;
        }
        self.resync();
        self.repaint = true;
        if leo_core::sync::is_initialized(&self.store.notes_dir) {
            let _ = leo_core::sync::auto_commit(&self.store.notes_dir);
            self.jobs.unpushed = leo_core::sync::unpushed(&self.store.notes_dir);
            self.jobs.last_change = Instant::now();
        }
        self.say(Kind::Dim, "Notes changed on disk; reloaded.");
        true
    }
}
