//! Automatic backup: when to push, and draining the push worker.
//!
//! The policy itself is pure and lives in `config/sync.rs`; this gathers its
//! inputs from the App and hands the push to a worker.

use super::*;

impl App {
    /// Record that the notes changed, restarting the quiet period.
    pub(super) fn note_changed(&mut self) {
        self.last_change = Instant::now();
        // Asked once per change rather than once per frame: it is a git process.
        self.unpushed = leo_core::sync::unpushed(&self.store.notes_dir);
    }

    /// Start a background push when the policy says to.
    ///
    /// Called from the idle branch of the event loop, so it only ever runs when
    /// the user is not typing.
    pub(super) fn maybe_auto_push(&mut self) {
        let config = leo_services::config::Config::load().sync;
        let when = leo_services::config::sync::PushWhen {
            unpushed: self.unpushed,
            quiet_for: self.last_change.elapsed(),
            since_last_push: self.last_push.map(|at| at.elapsed()),
            in_flight: self.pushing.is_some(),
        };
        if !leo_services::config::sync::should_push_now(&config, when) {
            return;
        }
        self.pushing = Some((
            task::start_push(self.store.notes_dir.clone()),
            view::progress::Progress::spinner("Backing up"),
            Instant::now(),
        ));
    }

    /// Push on the way out, if the policy says to and anything is waiting.
    ///
    /// Synchronous and after the alternate screen is gone: quitting should not
    /// return the prompt and then keep working invisibly, and the user is owed a
    /// line saying whether their notes made it.
    pub(super) fn push_on_quit(&mut self) {
        let config = leo_services::config::Config::load().sync;
        // Asked fresh: the cached count is from the last change, and a background
        // push may have cleared it since.
        let unpushed = leo_core::sync::unpushed(&self.store.notes_dir);
        if !leo_services::config::sync::should_push_on_quit(&config, unpushed) {
            return;
        }

        let waiting = unpushed.unwrap_or(0);
        println!(
            "  backing up {waiting} change{}…",
            if waiting == 1 { "" } else { "s" }
        );
        match leo_core::sync::push(&self.store.notes_dir) {
            Ok(()) => println!("  backed up."),
            Err(e) => {
                println!("  backup failed: {e}");
                println!("  your notes are committed locally; `leo sync` retries.");
            }
        }
    }

    /// Drain a running background push.
    pub(super) fn pump_push(&mut self) {
        let Some((job, _, _)) = self.pushing.as_mut() else {
            return;
        };
        let events = job.drain();
        if events.is_empty() && !job.is_done() {
            return;
        }

        let mut done = false;
        let mut failure = None;
        for event in events {
            match event {
                TaskEvent::Pushed => done = true,
                TaskEvent::Failed(e) => failure = Some(e),
                _ => {}
            }
        }

        if done {
            self.pushing = None;
            self.last_push = Some(Instant::now());
            self.unpushed = leo_core::sync::unpushed(&self.store.notes_dir);
            self.say(Kind::Dim, "Backed up.");
        } else if let Some(e) = failure {
            self.pushing = None;
            // Recorded so the floor applies to failures too, or a broken remote
            // means a git process every time the loop goes quiet.
            self.last_push = Some(Instant::now());
            self.say(
                Kind::Warn,
                format!("Backup failed: {e}. Try `/sync`, which pulls first."),
            );
        }
    }
}
