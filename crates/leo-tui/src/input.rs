use super::*;

impl App {
    pub(super) fn on_key<B: TuiBackend>(
        &mut self,
        key: event::KeyEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        if self.tour.is_some()
            && key.code == event::KeyCode::Char('g')
            && key.modifiers.contains(event::KeyModifiers::CONTROL)
        {
            self.end_tour();
            if self.mode == Mode::Tour {
                self.mode = Mode::Normal;
            }
            return Ok(());
        }
        let result = self.handle_key(key, terminal);
        if result.is_ok() {
            self.advance_tour();
        }
        result
    }

    pub(super) fn handle_key<B: TuiBackend>(
        &mut self,
        key: event::KeyEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Confirm { prompt, on_yes } => {
                let yes = matches!(key.code, event::KeyCode::Char('y' | 'Y'));
                if yes {
                    let outcome = action::apply_confirmed(&mut self.store, &on_yes)?;
                    self.absorb(outcome, terminal)?;
                } else {
                    self.say(Kind::Dim, "Cancelled.");
                }
                // Mode was already reset to Normal by the take above.
                let _ = prompt;
                Ok(())
            }

            // Help scrolls with the same keys as everything else; any other
            // key closes it, `?` included, since it is a toggle.
            Mode::Help => {
                let page = 10;
                match key.code {
                    event::KeyCode::Char('j') | event::KeyCode::Down => {
                        self.mode = Mode::Help;
                        self.help_scroll = self.help_scroll.saturating_add(1);
                    }
                    event::KeyCode::Char('k') | event::KeyCode::Up => {
                        self.mode = Mode::Help;
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                    }
                    event::KeyCode::PageDown | event::KeyCode::Char(' ') => {
                        self.mode = Mode::Help;
                        self.help_scroll = self.help_scroll.saturating_add(page);
                    }
                    event::KeyCode::PageUp => {
                        self.mode = Mode::Help;
                        self.help_scroll = self.help_scroll.saturating_sub(page);
                    }
                    event::KeyCode::Char('g') => {
                        self.mode = Mode::Help;
                        self.help_scroll = 0;
                    }
                    event::KeyCode::Char('G') => {
                        self.mode = Mode::Help;
                        self.help_scroll = view::help::line_count() as u16;
                    }
                    _ => {
                        self.mode = Mode::Normal;
                        self.help_scroll = 0;
                    }
                }
                Ok(())
            }

            Mode::Settings => {
                self.mode = Mode::Settings;
                self.on_settings_key(key, terminal)
            }

            Mode::Welcome => {
                self.mode = Mode::Welcome;
                self.on_welcome_key(key)
            }

            Mode::Actions { selected } => self.on_actions_key(key, selected, terminal),
            Mode::Sources { selected } => self.on_sources_key(key, selected),
            Mode::Tour => self.on_tour_key(key),
            mode @ (Mode::Command | Mode::Search) => {
                let searching = mode == Mode::Search;
                self.mode = mode;
                let outcome = self.cmd.key(key);
                // Any key other than Tab invalidates the candidate list.
                if outcome != CmdOutcome::Complete {
                    self.completing = None;
                }
                match outcome {
                    CmdOutcome::Editing => {
                        self.follow_search();
                        Ok(())
                    }
                    CmdOutcome::Cancel => {
                        self.mode = Mode::Normal;
                        if searching && self.nav.filter.take().is_some() {
                            self.resync();
                            self.nav.note_sel = 0;
                        }
                        Ok(())
                    }
                    CmdOutcome::Complete => {
                        self.cycle_completion();
                        self.follow_search();
                        Ok(())
                    }
                    CmdOutcome::Submit(line) => {
                        self.mode = Mode::Normal;
                        if searching {
                            self.submit_search(&line)
                        } else {
                            self.run_line(&line, terminal)
                        }
                    }
                }
            }

            Mode::Normal => {
                self.mode = Mode::Normal;
                if self.writing.editing.is_some() && self.nav.focus == Pane::Preview {
                    return self.on_edit_key(key, terminal);
                }
                // While recording, the keyboard takes notes: typing builds a
                // point, Enter adds it, Tab switches bullets and raw text, Esc
                // stops. Once stopping, the panes get their keys back.
                if let Some(rec) = self
                    .jobs
                    .recording
                    .as_mut()
                    .filter(|r| !r.job.stop_requested())
                {
                    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
                    match key.code {
                        event::KeyCode::Esc => {
                            let armed = rec
                                .stop_armed
                                .is_some_and(|at| at.elapsed() < STOP_CONFIRM_WITHIN);
                            if !armed {
                                rec.stop_armed = Some(Instant::now());
                                self.say(
                                    Kind::Warn,
                                    "Press Esc again to stop recording, or keep talking.",
                                );
                                return Ok(());
                            }
                            rec.commit_jot();
                            rec.job.request_stop();
                            rec.progress =
                                view::progress::Progress::spinner("Finishing the recording");
                            rec.since = Instant::now();
                            self.say(Kind::Dim, "Stopping...");
                            return Ok(());
                        }
                        event::KeyCode::Enter => {
                            rec.commit_jot();
                            return Ok(());
                        }
                        event::KeyCode::Char('p') if ctrl => {
                            rec.toggle_pause();
                            let word = if rec.job.paused() {
                                "Paused — Ctrl-P resumes."
                            } else {
                                "Recording again."
                            };
                            self.say(Kind::Dim, word);
                            return Ok(());
                        }
                        event::KeyCode::Up => {
                            rec.scroll.scroll_by(-1, Instant::now());
                            return Ok(());
                        }
                        event::KeyCode::Down => {
                            rec.scroll.scroll_by(1, Instant::now());
                            return Ok(());
                        }
                        event::KeyCode::PageUp => {
                            let page = rec.scroll.page() as isize;
                            rec.scroll.scroll_by(-page, Instant::now());
                            return Ok(());
                        }
                        event::KeyCode::PageDown => {
                            let page = rec.scroll.page() as isize;
                            rec.scroll.scroll_by(page, Instant::now());
                            return Ok(());
                        }
                        event::KeyCode::Home => {
                            rec.scroll.to_top(Instant::now());
                            return Ok(());
                        }
                        event::KeyCode::End => {
                            rec.scroll.follow();
                            return Ok(());
                        }
                        event::KeyCode::Tab => return Ok(()),
                        event::KeyCode::Backspace => {
                            rec.jot.pop();
                            return Ok(());
                        }
                        event::KeyCode::Char(c) if !ctrl => {
                            rec.jot.push(c);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                let intent = keys::normal(key, self.nav.focus);
                self.on_intent(intent, terminal)
            }
        }
    }

    pub(super) fn on_intent<B: TuiBackend>(
        &mut self,
        intent: Intent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        match intent {
            Intent::Nothing => Ok(()),
            Intent::Quit => {
                self.quit = true;
                Ok(())
            }

            Intent::Down | Intent::Up | Intent::First | Intent::Last => {
                self.move_selection(intent);
                self.remember_visit();
                Ok(())
            }

            Intent::FocusLeft => {
                self.nav.focus = self.next_visible_pane(terminal, -1);
                Ok(())
            }
            Intent::FocusRight => {
                let next = self.next_visible_pane(terminal, 1);
                if next == Pane::Preview && self.nav.focus == Pane::Notes && self.start_editing() {
                    return Ok(());
                }
                self.nav.focus = next;
                Ok(())
            }

            Intent::ScrollDown => {
                self.nav.preview_scroll = self.nav.preview_scroll.saturating_add(5);
                Ok(())
            }
            Intent::ScrollUp => {
                self.nav.preview_scroll = self.nav.preview_scroll.saturating_sub(5);
                Ok(())
            }

            Intent::Open => {
                let opened = self.open(terminal);
                self.remember_visit();
                opened
            }

            // The left pane has two things to show and one column to show them
            // in, so it toggles rather than taking a fourth pane.
            Intent::JumpRecent => {
                self.jump_recent();
                Ok(())
            }

            // Undo goes through the same handler the `:` line uses, so there is
            // one stack and one set of semantics rather than two.
            Intent::Undo => self.run_action(Action::Undo, terminal),

            Intent::ToggleCheckbox => {
                let Some(note_ref) = self.selected_ref() else {
                    return Ok(());
                };
                let boxes = self.checkboxes();
                let index = boxes.iter().position(|ticked| !ticked).map_or(1, |i| i + 1);
                self.run_action(
                    Action::Check {
                        note: note_ref,
                        index,
                    },
                    terminal,
                )
            }

            Intent::EditSelected => match self.selected_ref() {
                Some(note) => self.run_action(Action::Edit { note }, terminal),
                None => Ok(()),
            },

            Intent::NewNote => {
                self.cmd.open("new ");
                self.mode = Mode::Command;
                Ok(())
            }

            Intent::ToggleMark => {
                let Some(id) = self.selected_id().cloned() else {
                    return Ok(());
                };
                match self.nav.marked.iter().position(|m| *m == id) {
                    Some(i) => {
                        self.nav.marked.remove(i);
                    }
                    None => self.nav.marked.push(id),
                }
                match self.nav.marked.len() {
                    0 => self.say(Kind::Dim, "No notes marked."),
                    n => self.say(
                        Kind::Dim,
                        format!("{n} marked — D deletes them, m moves them, Esc clears."),
                    ),
                }
                Ok(())
            }

            // Pre-filled rather than asked for from scratch: the usual rename is
            // a small change to the title that is already there.
            Intent::RenameSelected => {
                let title = self
                    .selected_id()
                    .and_then(|id| self.store.find_note(id))
                    .map(|n| n.title.clone());
                match title {
                    Some(title) => {
                        self.cmd.open(&format!("rename {title}"));
                        self.mode = Mode::Command;
                    }
                    None => self.say(Kind::Dim, "No note selected."),
                }
                Ok(())
            }

            Intent::PinSelected => self.run_action(
                Action::Pin {
                    note: String::new(),
                },
                terminal,
            ),

            Intent::Record => self.run_action(
                Action::Listen {
                    title: None,
                    append_to: None,
                    screen: false,
                },
                terminal,
            ),

            // `D` deletes whatever is selected, which depends on the focused
            // pane: a note in the notes pane, a whole directory in the dirs
            // pane. Both confirm first.
            Intent::DeleteSelected => match self.nav.focus {
                Pane::Dirs => self.delete_selected_dir(terminal),
                _ if !self.nav.marked.is_empty() => self.run_action(
                    Action::Delete {
                        note: String::new(),
                    },
                    terminal,
                ),
                _ => match self.selected_ref() {
                    Some(note) => self.run_action(Action::Delete { note }, terminal),
                    None => Ok(()),
                },
            },

            Intent::OpenCommand { seed } => {
                self.cmd.open(seed);
                self.mode = Mode::Command;
                Ok(())
            }

            Intent::OpenFilter => {
                let seed = self.nav.filter.clone().unwrap_or_default();
                self.cmd.open(&seed);
                self.mode = Mode::Search;
                Ok(())
            }

            Intent::OpenSources => {
                if !self.nav.answer_sources.is_empty()
                    && (self.answer.is_some() || self.jobs.asking.is_some())
                {
                    self.mode = Mode::Sources { selected: 0 };
                }
                Ok(())
            }
            Intent::OpenActions => {
                self.mode = Mode::Actions { selected: 0 };
                Ok(())
            }

            Intent::OpenSettings => {
                self.open_settings(None);
                Ok(())
            }

            Intent::ToggleHelp => {
                self.mode = Mode::Help;
                self.help_scroll = 0;
                Ok(())
            }

            // Esc also ends a search or a tag, leaving you on the note you
            // picked — in its own directory, since results come from anywhere.
            Intent::Cancel => {
                self.unpin();
                self.mode = Mode::Normal;
                if !self.nav.marked.is_empty() {
                    self.nav.marked.clear();
                    self.say(Kind::Dim, "Marks cleared.");
                    return Ok(());
                }
                let picked = self.selected_id().cloned();
                if self.nav.filter.take().is_some() {
                    self.resync();
                    match picked {
                        Some(id) => self.jump_to(&id),
                        None => self.nav.note_sel = 0,
                    }
                }
                Ok(())
            }

            // Reload also forces a full repaint. Anything that wrote to the
            // terminal behind ratatui's back leaves its cell diff out of step
            // with the screen, and this is the one key a user will try when the
            // display looks wrong.
            Intent::Reload => {
                self.store.refresh()?;
                self.resync();
                self.repaint = true;
                self.say(Kind::Dim, "Reloaded.");
                Ok(())
            }
        }
    }

    // ── running actions ─────────────────────────────────────────────────────

    pub(super) fn follow_search(&mut self) {
        if self.mode != Mode::Search {
            return;
        }
        let text = self.cmd.text().to_string();
        let wanted = (!text.trim().is_empty()).then_some(text);
        if wanted != self.nav.filter {
            self.nav.filter = wanted;
            self.nav.note_sel = 0;
            self.nav.preview_scroll = 0;
            self.unpin();
            self.resync();
        }
    }

    pub(super) fn submit_search(&mut self, line: &str) -> Result<()> {
        self.nav.filter = (!line.trim().is_empty()).then(|| line.to_string());
        self.resync();
        self.nav.focus = Pane::Notes;
        let found = self.note_count();
        self.say(
            Kind::Dim,
            format!("{found} notes found. Esc clears the search."),
        );
        Ok(())
    }

    // ── completion ──────────────────────────────────────────────────────────

    /// Candidate sources drawn from the store and config.
    pub(super) fn sources(&self) -> Sources {
        Sources {
            dirs: self.store.subdirs(&self.nav.current_dir),
            notes: self
                .nav
                .numbering
                .iter()
                .enumerate()
                .filter_map(|(i, id)| {
                    self.store.find_note(id).map(|n| NoteChoice {
                        number: i + 1,
                        title: n.title.clone(),
                    })
                })
                .collect(),
        }
    }

    /// Tab: complete the token, or step to the next candidate if already
    /// cycling. With one match this completes and stops; with several, each Tab
    /// advances and wraps around to what was typed.
    pub(super) fn input_completion(&self, sources: &Sources) -> Completion {
        if self.mode == Mode::Search {
            complete::search(self.cmd.text(), self.cmd.cursor(), sources)
        } else {
            complete::complete(self.cmd.text(), self.cmd.cursor(), sources)
        }
    }

    pub(super) fn apply_completion(
        &self,
        line: &str,
        completion: &Completion,
        choice: &str,
    ) -> (String, usize) {
        if self.mode == Mode::Search {
            complete::apply_literal(line, completion, choice)
        } else {
            complete::apply(line, completion, choice)
        }
    }

    pub(super) fn cycle_completion(&mut self) {
        if let Some(cycle) = self.completing.take() {
            let count = cycle.completion.matches.len();
            if count == 0 {
                return;
            }
            // One past the end restores the original text, so cycling is
            // never a trap.
            let next = (cycle.index + 1) % (count + 1);
            let (line, cursor) = if next == count {
                self.apply_completion(self.cmd.text(), &cycle.completion, &cycle.typed)
            } else {
                self.apply_completion(
                    self.cmd.text(),
                    &cycle.completion,
                    &cycle.completion.matches[next],
                )
            };
            self.cmd.set_with_cursor(&line, cursor);
            // The span to replace moved with the new text.
            let completion = Completion {
                start: cycle.completion.start,
                end: cursor,
                matches: cycle.completion.matches,
            };
            self.completing = Some(Cycle {
                completion,
                typed: cycle.typed,
                index: next,
            });
            return;
        }

        let sources = self.sources();
        let completion = self.input_completion(&sources);
        if completion.matches.is_empty() {
            return;
        }
        let typed: String = self
            .cmd
            .text()
            .chars()
            .skip(completion.start)
            .take(completion.end.saturating_sub(completion.start))
            .collect();

        let (line, cursor) =
            self.apply_completion(self.cmd.text(), &completion, &completion.matches[0]);
        self.cmd.set_with_cursor(&line, cursor);
        let completion = Completion {
            start: completion.start,
            end: cursor,
            matches: completion.matches,
        };
        self.completing = Some(Cycle {
            completion,
            typed,
            index: 0,
        });
    }

    /// What the menu above the `:` line lists, and which one Tab has chosen.
    pub(super) fn menu(&self) -> Option<(Vec<view::menu::Item>, Option<usize>)> {
        if self.mode != Mode::Command {
            return None;
        }
        let (completion, selected) = match &self.completing {
            Some(cycle) => {
                let index = (cycle.index < cycle.completion.matches.len()).then_some(cycle.index);
                (cycle.completion.clone(), index)
            }
            None => (self.input_completion(&self.sources()), None),
        };
        if completion.matches.is_empty() {
            return None;
        }
        let first_word = self
            .cmd
            .text()
            .chars()
            .take(completion.start)
            .all(char::is_whitespace);
        let items = completion
            .matches
            .into_iter()
            .map(|label| view::menu::Item {
                detail: first_word
                    .then(|| action::verb(&label).map(|v| v.summary))
                    .flatten(),
                label,
            })
            .collect();
        Some((items, selected))
    }

    /// The ghost hint: what the top candidate would add, shown ahead of the
    /// cursor. Only computed while the `:` line is open and idle.
    pub(super) fn ghost(&self) -> Option<String> {
        if self.mode != Mode::Command || self.completing.is_some() {
            return None;
        }
        let text = self.cmd.text();
        if text.is_empty() {
            return None;
        }
        let completion = self.input_completion(&self.sources());
        let typed: String = text
            .chars()
            .skip(completion.start)
            .take(completion.end.saturating_sub(completion.start))
            .collect();
        completion.ghost(&typed)
    }
}
