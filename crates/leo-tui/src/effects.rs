use super::*;

impl App {
    pub(super) fn run_line<B: TuiBackend>(
        &mut self,
        line: &str,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        match action::parse(line) {
            Parsed::Empty => Ok(()),
            Parsed::Usage(usage) => {
                self.say(Kind::Warn, format!("Usage: {usage}"));
                Ok(())
            }
            Parsed::Unknown(verb) => {
                self.say(
                    Kind::Bad,
                    format!("Unknown command: {verb} — press : for commands, ? for every key"),
                );
                Ok(())
            }
            // One line, not two: the status line holds a single message, so a
            // second call would silently replace the first and the user would
            // see the replacement without ever learning what happened.
            Parsed::Retired {
                verb,
                replacement,
                why,
            } => {
                self.say(
                    Kind::Warn,
                    format!("`{verb}` is gone — use `{replacement}` ({why})."),
                );
                Ok(())
            }
            Parsed::Action(action) => self.run_action(action, terminal),
        }
    }

    pub(super) fn run_action<B: TuiBackend>(
        &mut self,
        action: Action,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        self.flush_edit();
        if let Action::New { title } = action {
            return match self.create_and_edit(title) {
                Ok(()) => Ok(()),
                Err(e) => {
                    self.say(Kind::Bad, e.to_string());
                    Ok(())
                }
            };
        }
        let selected = self
            .nav
            .numbering
            .get(self.nav.note_sel)
            .map(String::as_str);
        let action = match action::fill_selected(action, selected, &self.nav.marked) {
            Ok(action) => action,
            Err(line) => return self.absorb(Outcome::line(line), terminal),
        };
        // Marks are spent by the command that used them.
        if matches!(&action, Action::DeleteMany { .. })
            || matches!(&action, Action::Mv { notes, .. } if !self.nav.marked.is_empty() && *notes == self.nav.marked)
        {
            self.nav.marked.clear();
        }
        // `ask` is the one action that can take a minute. Run it on a worker and
        // stream the answer: inline, it froze the interface with nothing to say
        // whether the model was thinking or the request had died.
        if let Action::Ask { note } = &action {
            if self.jobs.asking.is_some() {
                self.say(Kind::Warn, "Already asking — one at a time.");
                return Ok(());
            }
            if !self.store.notes.iter().any(|n| n.id == *note) {
                return self.run_action_inline(action, terminal);
            }
            let resolved = action::resolve(note, &self.store, &self.nav.numbering);
            let action::Resolved::One(id) = resolved else {
                // Ambiguous or missing: let the ordinary handler explain, since
                // it already words those cases well.
                return self.run_action_inline(action, terminal);
            };
            let Some(target) = self.store.find_note(&id) else {
                return self.run_action_inline(action, terminal);
            };
            let (title, body) = (target.title.clone(), target.body.clone());
            if !body.lines().any(|l| action::is_leo_prompt(l).is_some()) {
                self.say(
                    Kind::Dim,
                    "Type a question after :ask, or write @leo and a question in a note.",
                );
                return Ok(());
            }
            if self.set_up_first(welcome::Need::Writing) {
                return Ok(());
            }

            self.jobs.asking = Some(Asking {
                job: task::start_ask(note.clone(), title, body),
                question: None,
                progress: view::progress::Progress::spinner("Asking"),
                since: Instant::now(),
                text: String::new(),
            });
            return Ok(());
        }
        self.run_action_inline(action, terminal)
    }

    pub(super) fn run_action_inline<B: TuiBackend>(
        &mut self,
        action: Action,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let outcome = match action::apply(
            action,
            &mut self.store,
            Ctx {
                current_dir: &self.nav.current_dir,
                numbering: &self.nav.numbering,
                selected: self
                    .nav
                    .numbering
                    .get(self.nav.note_sel)
                    .map(String::as_str),
                marked: &[],
            },
            &leo_services::ai::RealAi,
        ) {
            Ok(o) => o,
            // A handler failure is a status-line message, never a crash.
            Err(e) => {
                self.say(Kind::Bad, e.to_string());
                return Ok(());
            }
        };
        self.absorb(outcome, terminal)
    }

    /// Apply an outcome's state changes, show its lines, and perform its effect.
    pub(super) fn absorb<B: TuiBackend>(
        &mut self,
        outcome: Outcome,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        // Anything that changed the notes restarts the quiet period, and makes
        // the waiting-commit count worth asking for again.
        if outcome.dirty {
            self.note_changed();
        }

        if let Some(dir) = outcome.new_dir {
            self.nav.current_dir = dir;
            self.nav.note_sel = 0;
            self.nav.dir_sel = 0;
            self.unpin();
        }

        match outcome.selection {
            Some(sel) => {
                self.nav.numbering = sel;
                self.nav.note_sel = 0;
            }
            None if outcome.dirty => self.resync(),
            None => {}
        }

        // A note just made or added to: take the user to it, in its own
        // directory, with any search cleared so it is actually listed.
        if let Some(id) = &outcome.select {
            if self.nav.filter.take().is_some() {
                self.resync();
            }
            self.jump_to(id);
        }

        // Multi-line output goes to the preview; a single line is a status.
        let printable: Vec<&Line> = outcome
            .lines
            .iter()
            .filter(|l| l.kind != Kind::Blank)
            .collect();
        match printable.as_slice() {
            [] => {}
            [one] if outcome.undoable => {
                self.say(one.kind, format!("{} u brings it back.", one.text))
            }
            [one] => self.say(one.kind, one.text.clone()),
            many => {
                let lines = many.iter().map(|l| (*l).clone()).collect();
                self.pinned = Some(("output".to_string(), lines));
                self.nav.preview_scroll = 0;
            }
        }

        match outcome.effect {
            Effect::None => Ok(()),

            Effect::Quit => {
                self.quit = true;
                Ok(())
            }

            Effect::AskNotes { question } => {
                if self.jobs.asking.is_some() {
                    self.say(Kind::Warn, "Already asking — one at a time.");
                    return Ok(());
                }
                if self.set_up_first(welcome::Need::Writing) {
                    return Ok(());
                }
                self.nav.answer_sources = self
                    .store
                    .relevant(&question, 6)
                    .iter()
                    .map(|n| n.id.clone())
                    .collect();
                let notes: Vec<(String, String, String)> = self
                    .store
                    .relevant(&question, 6)
                    .into_iter()
                    .map(|n| (n.title.clone(), n.directory.clone(), n.body.clone()))
                    .collect();
                if notes.is_empty() {
                    self.say(Kind::Dim, "None of your notes mention that.");
                    return Ok(());
                }
                self.jobs.asking = Some(Asking {
                    job: task::start_question(question.clone(), notes),
                    question: Some(question),
                    progress: view::progress::Progress::spinner("Asking your notes"),
                    since: Instant::now(),
                    text: String::new(),
                });
                self.unpin();
                Ok(())
            }

            Effect::ShowNote { id } => {
                // Select it in the pane if it is visible, and focus the body.
                if let Some(pos) = self.nav.numbering.iter().position(|n| n == &id) {
                    self.nav.note_sel = pos;
                    self.unpin();
                } else if let Some(note) = self.store.find_note(&id) {
                    // Not in the current directory's listing, so show it
                    // directly rather than silently doing nothing.
                    let title = note.title.clone();
                    let lines = note.body.lines().map(Line::plain).collect();
                    self.pinned = Some((title, lines));
                }
                self.nav.preview_scroll = 0;
                self.nav.focus = Pane::Preview;
                Ok(())
            }

            Effect::Tutorial => {
                self.open_tour();
                Ok(())
            }
            Effect::Settings => {
                self.open_settings(None);
                Ok(())
            }

            Effect::Obsidian => {
                match (self.obsidian)(&self.store.notes_dir) {
                    Ok(opened) => {
                        let lines = opened.describe().into_iter().map(Line::plain).collect();
                        self.unpin();
                        self.pinned = Some(("obsidian (Esc closes)".to_string(), lines));
                        self.nav.preview_scroll = 0;
                    }
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                Ok(())
            }

            Effect::Doctor => {
                if self.jobs.checking.is_some() {
                    self.say(Kind::Warn, "Already checking — one at a time.");
                    return Ok(());
                }
                let probe = leo_services::doctor::Probe {
                    microphone: self.probe.microphone && self.jobs.recording.is_none(),
                    ..self.probe
                };
                self.jobs.checking = Some((
                    task::start_doctor(self.store.notes_dir.clone(), probe),
                    view::progress::Progress::spinner("Checking everything"),
                    Instant::now(),
                ));
                Ok(())
            }

            Effect::ShowHelp => {
                self.mode = Mode::Help;
                self.help_scroll = 0;
                Ok(())
            }

            Effect::Confirm { prompt, on_yes } => {
                self.mode = Mode::Confirm { prompt, on_yes };
                Ok(())
            }

            Effect::Edit(req) => self.suspend_with_store(terminal, |store| {
                crate::shell::run_editor(store, req, &leo_services::ai::RealAi)
            }),

            Effect::Listen(req) => {
                if self.jobs.recording.is_some() {
                    self.say(Kind::Warn, "Already recording — press Esc twice to stop.");
                    return Ok(());
                }
                // Check the whole path to a finished note before recording, not
                // just the recorder. Discovering there is no transcription
                // provider *after* talking for twenty minutes is the worst way
                // to learn it.
                if self.set_up_first(welcome::Need::Recording) {
                    return Ok(());
                }
                if let Some(lines) = self.listen_preflight(req.screen) {
                    self.pinned = Some(("not ready to record".to_string(), lines));
                    self.nav.preview_scroll = 0;
                    return Ok(());
                }
                self.jobs.recording = Some(Recording::new(
                    task::start_listen(
                        req.title.clone(),
                        req.append_to.clone(),
                        req.dir.clone(),
                        req.screen,
                    ),
                    req,
                ));
                self.unpin();
                self.say(
                    Kind::Dim,
                    "Recording — type a point and Enter to add it; Esc twice stops.",
                );
                Ok(())
            }

            Effect::Sync(leo_core::action::SyncAction::Now)
                if leo_core::sync::remote_url(&self.store.notes_dir).is_none() =>
            {
                self.offer_backup_setup();
                Ok(())
            }

            Effect::Sync(a) => {
                let notes_dir = self.store.notes_dir.clone();
                let out = self.outside(terminal, || {
                    use leo_core::action::SyncAction;
                    let done = |r: Result<()>| r.map(|()| "Backed up.".to_string());
                    match &a {
                        SyncAction::Now => done(leo_core::sync::now(&notes_dir)),
                        SyncAction::Init => done(leo_core::sync::init(&notes_dir)),
                        // Connecting is the moment to back up: bring down any notes
                        // already there, then send these.
                        SyncAction::Connect { url } => done(
                            leo_core::sync::connect(&notes_dir, url)
                                .and_then(|()| leo_core::sync::now(&notes_dir)),
                        ),
                        SyncAction::Push => done(leo_core::sync::push(&notes_dir)),
                        SyncAction::Pull => done(leo_core::sync::pull(&notes_dir)),
                        SyncAction::Status => done(leo_core::sync::status(&notes_dir)),
                        SyncAction::GitHub { name } => leo_core::sync::github(
                            &notes_dir,
                            name.as_deref().unwrap_or(leo_core::sync::GITHUB_REPO),
                        )
                        .map(|backup| backup.describe()),
                    }
                })?;
                match out {
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                    Ok(said) => {
                        self.store = Store::load_from(&self.store.notes_dir.clone())?;
                        self.resync();
                        self.say(Kind::Good, said);
                    }
                }
                Ok(())
            }
        }
    }

    pub(super) fn offer_backup_setup(&mut self) {
        self.mode = Mode::Command;
        if (self.gh_ready)() {
            self.cmd.open("backup github");
            self.say(
                Kind::Dim,
                "Enter makes a private repository, leo-notes, on your GitHub (or joins yours) and backs up.",
            );
        } else {
            self.cmd.open("backup connect ");
            self.say(
                Kind::Dim,
                "Make an empty private repository on GitHub, paste its URL, then Enter. (With GitHub's gh tool, :backup github does it for you.)",
            );
        }
    }

    /// Leave the alternate screen, run `f` on the real terminal, then come
    /// back. Everything that writes to stdout or reads stdin — `$EDITOR`, git,
    /// the no-echo key prompt, the recorder — goes through here.
    pub(super) fn outside<B: TuiBackend, T>(
        &mut self,
        terminal: &mut Terminal<B>,
        f: impl FnOnce() -> T,
    ) -> Result<T> {
        suspend(terminal)?;
        let result = f();
        resume(terminal)?;
        Ok(result)
    }

    /// Run a store-mutating job outside the TUI, then absorb its outcome.
    /// `self.store` is borrowed for the call, so this cannot go through
    /// [`Self::outside`]'s closure.
    pub(super) fn suspend_with_store<B: TuiBackend>(
        &mut self,
        terminal: &mut Terminal<B>,
        job: impl FnOnce(&mut Store) -> Result<Outcome>,
    ) -> Result<()> {
        suspend(terminal)?;
        let result = job(&mut self.store);
        resume(terminal)?;

        match result {
            Ok(outcome) => self.absorb(outcome, terminal),
            // A failed editor or recording is a status message, not a crash.
            Err(e) => {
                self.say(Kind::Bad, e.to_string());
                Ok(())
            }
        }
    }
}
