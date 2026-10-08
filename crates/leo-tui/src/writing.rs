use super::*;

impl App {
    pub(super) fn can_write(&self) -> bool {
        self.jobs.recording.is_none()
            && self.jobs.asking.is_none()
            && self.pinned.is_none()
            && self.answer.is_none()
            && self.selected_id().is_some()
    }

    pub(super) fn start_editing(&mut self) -> bool {
        if !self.can_write() {
            return false;
        }
        let Some(note) = self.selected_id().and_then(|id| self.store.find_note(id)) else {
            return false;
        };
        let already = self
            .writing
            .editing
            .as_ref()
            .is_some_and(|ed| ed.id == note.id);
        if !already {
            let (id, body) = (note.id.clone(), note.body.clone());
            self.flush_edit();
            self.writing.editing = Some(editor::Editor::open(&id, &body));
        }
        self.nav.focus = Pane::Preview;
        true
    }

    pub(super) fn flush_edit(&mut self) {
        self.write_edit(true);
    }

    fn write_edit(&mut self, commit: bool) {
        let Some(ed) = self.writing.editing.as_ref() else {
            return;
        };
        let unsaved = ed.dirty;
        let committed_later = self.writing.edit_uncommitted;
        if !unsaved && !(commit && committed_later) {
            return;
        }
        let (id, text) = (ed.id.clone(), ed.text());
        if unsaved {
            let Some(note) = self.store.find_note_mut(&id) else {
                return;
            };
            if note.body != text {
                note.body = text;
                note.updated_at = chrono::Utc::now();
            }
        }
        let saved = if commit {
            self.store.save()
        } else {
            self.store.save_files()
        };
        if let Err(e) = saved {
            self.say(
                Kind::Bad,
                format!("Could not save: {e}. It stays here until it can."),
            );
            return;
        }
        if let Some(ed) = self.writing.editing.as_mut() {
            ed.saved();
        }
        self.writing.edit_uncommitted = !commit;
        self.note_changed();
        self.resync();
    }

    pub(super) fn finish_editing<B: TuiBackend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        self.flush_edit();
        let Some(ed) = self.writing.editing.take() else {
            return Ok(());
        };
        self.nav.focus = Pane::Notes;
        let asks = ed.lines.iter().any(|l| action::is_leo_prompt(l).is_some());
        if asks && self.store.find_note(&ed.id).is_some() {
            return self.run_action(Action::Ask { note: ed.id }, terminal);
        }
        Ok(())
    }

    pub(super) fn pump_editor(&mut self) {
        if self
            .writing
            .editing
            .as_ref()
            .is_some_and(|ed| ed.wants_saving())
        {
            self.write_edit(false);
        }
    }

    pub(super) fn on_edit_key<B: TuiBackend>(
        &mut self,
        key: event::KeyEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        use event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(ed) = self.writing.editing.as_mut() else {
            return Ok(());
        };
        if ctrl {
            match key.code {
                KeyCode::Char('z') => {
                    if !ed.undo() {
                        self.say(Kind::Dim, "Nothing to undo in this note.");
                    }
                }
                KeyCode::Char('s') => {
                    self.flush_edit();
                    self.say(Kind::Dim, "Saved.");
                }
                KeyCode::Char('a') => ed.home(),
                KeyCode::Char('e') => ed.end(),
                KeyCode::Char('c') => {
                    self.finish_editing(terminal)?;
                    self.quit = true;
                }
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => return self.finish_editing(terminal),
            KeyCode::Enter => ed.enter(),
            KeyCode::Backspace => ed.backspace(),
            KeyCode::Delete => ed.delete(),
            KeyCode::Left => ed.left(),
            KeyCode::Right => ed.right(),
            KeyCode::Up => ed.up(),
            KeyCode::Down => ed.down(),
            KeyCode::Home => ed.home(),
            KeyCode::End => ed.end(),
            KeyCode::PageUp => ed.page(false),
            KeyCode::PageDown => ed.page(true),
            KeyCode::Tab => ed.indent(),
            KeyCode::BackTab => ed.outdent(),
            KeyCode::Char(c) => ed.insert(c),
            _ => {}
        }
        Ok(())
    }

    pub(super) fn on_paste(&mut self, text: &str) {
        if let Some(ed) = self
            .writing
            .editing
            .as_mut()
            .filter(|_| self.nav.focus == Pane::Preview)
        {
            ed.paste(text);
        } else if matches!(self.mode, Mode::Command | Mode::Search) {
            for c in text.chars().filter(|c| *c != '\n' && *c != '\r') {
                self.cmd.insert(c);
            }
            self.follow_search();
        }
    }

    pub(super) fn click_in_note(&mut self, column: u16, row: u16) {
        if self.writing.editing.is_none() && !self.start_editing() {
            self.nav.focus = Pane::Preview;
            return;
        }
        self.nav.focus = Pane::Preview;
        let Some(ed) = self.writing.editing.as_mut() else {
            return;
        };
        match view::editing::hit(ed, column, row) {
            Some(view::editing::Hit::Box { line }) => {
                ed.toggle_box(line);
            }
            Some(view::editing::Hit::Place { line, col }) => ed.place(line, col),
            None => {}
        }
    }

    pub(super) fn create_and_edit(&mut self, title: Option<String>) -> Result<()> {
        let typed = title.unwrap_or_default();
        let (dir, title) = action::split_new(&self.store, &typed, &self.nav.current_dir);
        let title = if title.trim().is_empty() {
            "Untitled".to_string()
        } else {
            title
        };
        if !self.store.dir_exists(&dir) {
            self.store.create_dir(&dir);
        }
        let id = self
            .store
            .create_note(title, String::new(), vec![], &dir)?
            .id
            .clone();
        self.store.save()?;
        self.note_changed();
        if self.nav.filter.take().is_some() {
            self.resync();
        }
        self.resync();
        self.jump_to(&id);
        self.unpin();
        self.start_editing();
        self.say(
            Kind::Dim,
            "Write away. Esc when you are done; it saves as you go.",
        );
        Ok(())
    }
}
