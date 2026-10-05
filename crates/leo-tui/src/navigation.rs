use super::*;

impl App {
    // ── derived view data ───────────────────────────────────────────────────

    pub(super) fn dir_rows(&self) -> Vec<DirRow> {
        view::dirs::rows(
            &self.nav.current_dir,
            &self.store.subdirs(&self.nav.current_dir),
        )
    }

    pub(super) fn note_rows(&self) -> Vec<NoteRow> {
        let notes: Vec<&leo_core::notes::Note> = self
            .nav
            .numbering
            .iter()
            .filter_map(|id| self.store.find_note(id))
            .collect();
        let mut rows = view::notes::rows(&notes, &self.nav.current_dir);
        let words = self
            .nav
            .filter
            .as_deref()
            .map(leo_core::notes::search_words)
            .unwrap_or_default();
        for (row, note) in rows.iter_mut().zip(&notes) {
            row.marked = self.nav.marked.contains(&row.id);
            row.pinned = self.store.find_note(&row.id).is_some_and(|n| n.pinned);
            // Only when the title does not already show why it was found.
            let title = note.title.to_lowercase();
            if !words.is_empty() && !words.iter().all(|w| title.contains(w.as_str())) {
                row.snippet = note.matching_line(self.nav.filter.as_deref().unwrap_or(""));
            }
        }
        rows
    }

    /// The selected note's checkboxes, ticked or not.
    pub(super) fn checkboxes(&self) -> Vec<bool> {
        self.selected_id()
            .and_then(|id| self.store.find_note(id))
            .map(|n| n.checkboxes())
            .unwrap_or_default()
    }

    pub(super) fn selected_id(&self) -> Option<&String> {
        self.nav.numbering.get(self.nav.note_sel)
    }

    /// Remember the selected note as recently visited.
    ///
    /// Called when the selection settles rather than on every keystroke of j/k:
    /// scrolling past a note is not visiting it, and recording it would fill the
    /// list with notes the user never looked at.
    pub(super) fn remember_visit(&mut self) {
        if let Some(id) = self.selected_id().cloned() {
            self.nav.recent.touch(&id);
        }
    }

    /// The recent-notes strip, most recent first.
    pub(super) fn tabs(&self) -> Vec<view::tabs::Tab> {
        let current = self.selected_id();
        self.nav
            .recent
            .ids()
            .iter()
            .filter_map(|id| {
                self.store.find_note(id).map(|note| view::tabs::Tab {
                    title: note.title.clone(),
                    current: Some(id) == current,
                })
            })
            .collect()
    }

    /// Keep focus on something visible after a resize.
    ///
    /// Narrowing the terminal can drop the pane that had focus. In the one-pane
    /// shape the focused pane is the one drawn, so any focus is valid; otherwise
    /// focus falls back to the notes pane, which is the one always shown.
    pub(super) fn on_resize(&mut self, width: u16, height: u16) {
        if view::Shape::for_width(width) == view::Shape::One {
            return;
        }
        let frames = view::layout_with_tabs(
            Rect::new(0, 0, width, height),
            !self.tabs().is_empty(),
            self.nav.focus,
        );
        if !frames.shows(self.nav.focus) {
            self.nav.focus = Pane::Notes;
        }
    }

    /// The next pane in `direction` that is actually on screen.
    ///
    /// At narrow widths some panes are not drawn, and focusing one the user
    /// cannot see would make the keyboard appear to stop working. In the
    /// one-pane shape every pane is "visible" in turn, since the focused one is
    /// the one that gets drawn — which is what keeps everything reachable.
    pub(super) fn next_visible_pane<B: TuiBackend>(
        &self,
        terminal: &Terminal<B>,
        direction: isize,
    ) -> Pane {
        let Ok(size) = terminal.size() else {
            return self.nav.focus;
        };
        let area = Rect::new(0, 0, size.width, size.height);
        let shape = view::Shape::for_width(size.width);

        // One pane at a time: every step lands somewhere, because whichever pane
        // has focus is the one drawn.
        if shape == view::Shape::One {
            return if direction < 0 {
                self.nav.focus.left()
            } else {
                self.nav.focus.right()
            };
        }

        let frames = view::layout_with_tabs(area, !self.tabs().is_empty(), self.nav.focus);
        let mut candidate = self.nav.focus;
        // At most three steps: past that we are back where we started.
        for _ in 0..3 {
            candidate = if direction < 0 {
                candidate.left()
            } else {
                candidate.right()
            };
            if frames.shows(candidate) {
                return candidate;
            }
        }
        self.nav.focus
    }

    /// Show a note by id, wherever it lives.
    ///
    /// Selects it when the current listing contains it, and pins it into the
    /// preview when it does not — the user asked for that note, not for a place.
    pub(super) fn open_note(&mut self, id: &str) {
        match self.nav.numbering.iter().position(|n| n == id) {
            Some(position) => {
                self.nav.note_sel = position;
                self.nav.preview_scroll = 0;
                self.unpin();
            }
            None => {
                if let Some(note) = self.store.find_note(id) {
                    let (title, body) = (note.title.clone(), note.body.clone());
                    self.pinned = Some((title, body.lines().map(Line::plain).collect()));
                    self.nav.preview_scroll = 0;
                }
            }
        }
        self.nav.recent.touch(id);
    }

    /// Jump to the next entry in the recent list, wrapping.
    ///
    /// Cycling rather than presenting a menu: with five entries, pressing a key
    /// twice is faster than reading a list, and it matches how editors move
    /// between recent tabs.
    pub(super) fn jump_recent(&mut self) {
        let store = &self.store;
        self.nav
            .recent
            .retain_existing(|id| store.find_note(id).is_some());
        if self.nav.recent.is_empty() {
            self.say(Kind::Dim, "No notes visited yet.");
            return;
        }

        let current = self.selected_id().cloned();
        // The next entry that is not where we already are.
        let target = self
            .nav
            .recent
            .ids()
            .iter()
            .find(|id| Some(*id) != current.as_ref())
            .cloned();

        let Some(target) = target else {
            self.say(Kind::Dim, "Only this note has been visited.");
            return;
        };

        match self.nav.numbering.iter().position(|id| id == &target) {
            Some(position) => {
                self.nav.note_sel = position;
                self.nav.preview_scroll = 0;
                self.unpin();
            }
            // Not in the current listing: show it anyway rather than refusing,
            // since the user asked for that note and not for a place.
            None => {
                if let Some(note) = self.store.find_note(&target) {
                    let title = note.title.clone();
                    let body = note.body.clone();
                    self.pinned = Some((title, body.lines().map(Line::plain).collect()));
                    self.nav.preview_scroll = 0;
                }
            }
        }

        if let Some(note) = self.store.find_note(&target) {
            let title = note.title.clone();
            self.nav.recent.touch(&target);
            self.say(Kind::Dim, format!("← {title}"));
        }
    }

    /// The 1-based number of the selection, as a `:` line argument.
    pub(super) fn selected_ref(&self) -> Option<String> {
        self.selected_id()
            .map(|_| (self.nav.note_sel + 1).to_string())
    }

    pub(super) fn note_count(&self) -> usize {
        self.nav.numbering.len()
    }

    /// Why the notes pane is empty, and what to do about it.
    ///
    /// Only the app knows the difference between "no notes at all", "this
    /// directory is empty", and "the filter matched nothing" — and those need
    /// different advice.
    pub(super) fn empty_hint(&self) -> view::empty::Hint {
        // A filter that matched nothing is the most common empty pane, and the
        // most misleading if it does not say so.
        if let Some(query) = &self.nav.filter {
            if !query.trim().is_empty() {
                return view::empty::Hint::no_matches(query);
            }
        }
        if self.nav.current_dir.is_empty() {
            view::empty::Hint::no_notes()
        } else {
            view::empty::Hint::empty_directory()
        }
    }

    /// What the status bar reports on the right: how much is here.
    pub(super) fn counts(&self) -> view::status::Counts {
        view::status::Counts {
            notes: self.note_count(),
            words: self
                .selected_id()
                .and_then(|id| self.store.find_note(id))
                .map(|note| note.body.split_whitespace().count()),
        }
    }

    /// Refresh the numbering after the store or directory changed, keeping the
    /// selection in range.
    /// Drop whatever output or answer is covering the preview.
    pub(super) fn unpin(&mut self) {
        self.pinned = None;
        self.answer = None;
    }

    /// Recompute the numbering, keeping the selected note selected if it is
    /// still listed — an edit moves a note to the top, and the selection has to
    /// follow it rather than land on whatever slid into its old row.
    pub(super) fn resync(&mut self) {
        let keep = self.selected_id().cloned();
        self.nav.numbering = match &self.nav.filter {
            Some(query) => action::filtered_numbering(&self.store, &self.nav.current_dir, query),
            None => action::numbering_for(&self.store, &self.nav.current_dir),
        };
        if let Some(pos) = keep.and_then(|id| self.nav.numbering.iter().position(|n| *n == id)) {
            self.nav.note_sel = pos;
        }
        if self.nav.note_sel >= self.nav.numbering.len() {
            self.nav.note_sel = self.nav.numbering.len().saturating_sub(1);
        }
        let dirs = self.dir_rows().len();
        if self.nav.dir_sel >= dirs {
            self.nav.dir_sel = dirs.saturating_sub(1);
        }
    }

    pub(super) fn move_selection(&mut self, intent: Intent) {
        match self.nav.focus {
            Pane::Dirs => {
                let len = self.dir_rows().len();
                self.nav.dir_sel = step(self.nav.dir_sel, len, intent);
            }
            Pane::Notes => {
                let len = self.note_count();
                self.nav.note_sel = step(self.nav.note_sel, len, intent);
                // A new note means the old scroll position is meaningless.
                self.nav.preview_scroll = 0;
                self.unpin();
            }
            Pane::Preview => match intent {
                Intent::Down => self.nav.preview_scroll = self.nav.preview_scroll.saturating_add(1),
                Intent::Up => self.nav.preview_scroll = self.nav.preview_scroll.saturating_sub(1),
                Intent::First => self.nav.preview_scroll = 0,
                Intent::Last => self.nav.preview_scroll = u16::MAX / 2,
                _ => {}
            },
        }
    }

    /// `D` in the dirs pane: delete that directory and everything in it.
    pub(super) fn delete_selected_dir<B: TuiBackend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let rows = self.dir_rows();
        let Some(row) = rows.get(self.nav.dir_sel) else {
            return Ok(());
        };
        // ".." is a way to navigate, not a directory of its own; deleting the
        // parent from inside it would be a surprising thing for `D` to do.
        if row.target == ".." {
            self.say(
                Kind::Dim,
                "Move into a directory to delete it, or press h then D.",
            );
            return Ok(());
        }
        self.run_action(
            Action::Rmdir {
                name: row.target.clone(),
                recursive: true,
            },
            terminal,
        )
    }

    /// Enter: open the selected directory, or move focus onto the body.
    pub(super) fn open<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        match self.nav.focus {
            Pane::Dirs => {
                let rows = self.dir_rows();
                let Some(row) = rows.get(self.nav.dir_sel) else {
                    return Ok(());
                };
                let target = row.target.clone();
                self.run_action(Action::Cd { path: target }, terminal)
            }
            Pane::Notes => {
                if !self.start_editing() {
                    self.nav.focus = Pane::Preview;
                }
                Ok(())
            }
            Pane::Preview => Ok(()),
        }
    }

    /// Select a note by id, following it into its directory when it is not in
    /// the current listing — which is where a search result from elsewhere
    /// has to take you.
    pub(super) fn jump_to(&mut self, id: &str) {
        let Some(dir) = self.store.find_note(id).map(|n| n.directory.clone()) else {
            return;
        };
        if dir != self.nav.current_dir {
            self.nav.current_dir = dir;
            self.nav.dir_sel = 0;
            self.nav.numbering = action::numbering_for(&self.store, &self.nav.current_dir);
        }
        if let Some(pos) = self.nav.numbering.iter().position(|n| n == id) {
            self.nav.note_sel = pos;
        }
        self.unpin();
        self.nav.preview_scroll = 0;
        self.nav.focus = Pane::Notes;
    }
}
