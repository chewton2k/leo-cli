use std::time::{Duration, Instant};

use super::*;

const DISK_CHECK_EVERY: Duration = Duration::from_secs(2);
const DISK_CHECK_WATCHED: Duration = Duration::from_secs(60);

impl App {
    pub(super) fn maybe_reload_from_disk(&mut self) -> bool {
        let since = self.jobs.last_disk_check.map(|at| at.elapsed());
        if since.is_some_and(|since| since < DISK_CHECK_EVERY) {
            return false;
        }
        let busy = self.mode != Mode::Normal
            || self.jobs.recording.is_some()
            || self.jobs.asking.is_some()
            || self.writing.editing.is_some();
        if busy {
            return false;
        }
        let quiet = self
            .jobs
            .watch
            .as_ref()
            .filter(|watch| !watch.broken())
            .is_some_and(|watch| !watch.take())
            && since.is_some_and(|since| since < DISK_CHECK_WATCHED);
        if quiet {
            return false;
        }
        self.jobs.last_disk_check = Some(Instant::now());
        if !self.store.changed_on_disk() || self.store.refresh().is_err() {
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
