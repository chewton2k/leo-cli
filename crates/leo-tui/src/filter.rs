//! The `/` filter: every keystroke narrows the notes pane, so the result is
//! visible while typing rather than after committing.

use super::*;

impl App {
    pub(super) fn on_filter_key(&mut self, key: event::KeyEvent) {
        use event::KeyCode;
        match key.code {
            KeyCode::Esc => {
                // Esc abandons the filter entirely, which is the only way
                // back to the full list without deleting each character.
                self.filter = None;
                self.resync();
                self.note_sel = 0;
            }
            KeyCode::Enter => {
                // Keep the filter, but hand the keyboard back to the
                // panes so j/k and D act on what is shown.
                let empty = self.filter.as_ref().is_some_and(|q| q.trim().is_empty());
                if empty {
                    self.filter = None;
                    self.resync();
                }
            }
            KeyCode::Backspace => {
                if let Some(query) = self.filter.as_mut() {
                    query.pop();
                }
                self.mode = Mode::Filter;
                self.resync();
                self.note_sel = 0;
            }
            KeyCode::Char(c) => {
                self.filter.get_or_insert_with(String::new).push(c);
                self.mode = Mode::Filter;
                self.resync();
                self.note_sel = 0;
            }
            _ => self.mode = Mode::Filter,
        }
        self.preview_scroll = 0;
        self.unpin();
    }
}
