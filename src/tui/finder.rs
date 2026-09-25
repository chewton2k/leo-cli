//! The Ctrl-P fuzzy finder over every note in every directory.

use super::*;

impl App {
    /// Every note in every directory, labelled with its directory so two notes
    /// sharing a title stay distinguishable.
    pub(super) fn all_note_choices(&self) -> Vec<Choice> {
        self.store
            .list_notes(None, usize::MAX)
            .iter()
            .map(|n| Choice {
                id: n.id.clone(),
                label: if n.directory.is_empty() {
                    n.title.clone()
                } else {
                    format!("{}/{}", n.directory, n.title)
                },
            })
            .collect()
    }

    pub(super) fn on_find_key(&mut self, key: event::KeyEvent) -> Result<()> {
        let Some(finder) = self.finder.as_mut() else {
            self.mode = Mode::Normal;
            return Ok(());
        };

        match key.code {
            event::KeyCode::Esc => {
                self.finder = None;
                self.mode = Mode::Normal;
            }
            event::KeyCode::Enter => {
                let chosen = finder.selected().cloned();
                self.finder = None;
                self.mode = Mode::Normal;
                if let Some(choice) = chosen {
                    self.jump_to(&choice.id);
                }
            }
            event::KeyCode::Down => finder.down(),
            event::KeyCode::Up => finder.up(),
            event::KeyCode::Backspace => finder.backspace(),
            event::KeyCode::Char(c) if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                match c {
                    'n' => finder.down(),
                    'p' => finder.up(),
                    'c' => {
                        self.finder = None;
                        self.mode = Mode::Normal;
                    }
                    _ => {}
                }
            }
            event::KeyCode::Char(c) => finder.push(c),
            _ => {}
        }
        Ok(())
    }

    /// Select a note by id, following it into its directory when it is not in
    /// the current listing — otherwise Enter in the finder would appear to do
    /// nothing for a note stored elsewhere.
    pub(super) fn jump_to(&mut self, id: &str) {
        let Some(dir) = self.store.find_note(id).map(|n| n.directory.clone()) else {
            return;
        };
        if dir != self.current_dir {
            self.current_dir = dir;
            self.dir_sel = 0;
            self.numbering = action::numbering_for(&self.store, &self.current_dir);
        }
        if let Some(pos) = self.numbering.iter().position(|n| n == id) {
            self.note_sel = pos;
        }
        self.pinned = None;
        self.preview_scroll = 0;
        self.focus = Pane::Notes;
    }
}
