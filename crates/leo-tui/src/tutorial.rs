use super::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub(super) enum Tour {
    Invite,
    Create,
    Write(String),
    Find(String),
    Complete,
}

fn marker(store: &Store) -> std::path::PathBuf {
    store
        .notes_dir
        .parent()
        .unwrap_or(&store.notes_dir)
        .join(".tour-completed")
}

pub(super) fn render(frame: &mut Frame, screen: Rect, tour: Option<&Tour>) {
    let (title, text) = if matches!(tour, Some(Tour::Complete)) {
        (" Ready to go ", "You created, wrote and found a note.\n\nF2 opens Actions: New, Search, Record and Settings.\n: opens commands. ? shows every shortcut.\n\nEnter or Esc closes this tour.")
    } else {
        (" Welcome to leo ", "Learn the basics by creating, writing and finding your own note. It takes about a minute.\n\nEnter starts the tour. Esc skips it.\nYou can replay it from Actions or :tutorial.")
    };
    let text = if screen.width < 44 || screen.height < 11 {
        if matches!(tour, Some(Tour::Complete)) {
            "Tour complete.
Enter: close
Esc: close"
        } else {
            "Enter: start
Esc: skip
:tutorial replays"
        }
    } else {
        text
    };
    let area = view::help::centered(screen, 62, 11);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

impl App {
    pub(super) fn offer_tour(&mut self) {
        if !marker(&self.store).exists()
            && self.jobs.recording.is_none()
            && self.jobs.asking.is_none()
        {
            self.open_tour();
        }
    }

    pub(super) fn open_tour(&mut self) {
        self.tour = Some(Tour::Invite);
        self.mode = Mode::Tour;
    }

    pub(super) fn on_tour_key(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            event::KeyCode::Enter if matches!(self.tour, Some(Tour::Invite)) => {
                self.tour = Some(Tour::Create);
                self.message = None;
            }
            event::KeyCode::Enter | event::KeyCode::Esc => {
                self.end_tour();
            }
            _ => self.mode = Mode::Tour,
        }
        Ok(())
    }

    pub(super) fn end_tour(&mut self) {
        self.tour = None;
        if let Err(error) = std::fs::write(marker(&self.store), "done") {
            self.say(Kind::Warn, format!("Could not remember the tour: {error}"));
        }
    }

    pub(super) fn advance_tour(&mut self) {
        let next = match &self.tour {
            Some(Tour::Create) => self
                .writing
                .editing
                .as_ref()
                .map(|ed| Tour::Write(ed.id.clone())),
            Some(Tour::Write(id))
                if self.writing.editing.is_none()
                    && self
                        .store
                        .find_note(id)
                        .is_some_and(|n| !n.body.trim().is_empty()) =>
            {
                Some(Tour::Find(id.clone()))
            }
            Some(Tour::Find(id))
                if self.mode == Mode::Normal
                    && self.nav.filter.is_some()
                    && self.nav.numbering.contains(id) =>
            {
                Some(Tour::Complete)
            }
            _ => None,
        };
        if let Some(next) = next {
            if matches!(next, Tour::Complete) {
                self.mode = Mode::Tour;
            }
            self.tour = Some(next);
            self.message = None;
        }
    }

    pub(super) fn tour_hint(&self) -> Option<(Kind, &str)> {
        let hint = match &self.tour {
            Some(Tour::Create) => {
                "Tour 1/3 · n creates a note: type a title, then Enter. Ctrl-G skips."
            }
            Some(Tour::Write(_)) => {
                "Tour 2/3 · Write a sentence, then Esc to return. Ctrl-G skips."
            }
            Some(Tour::Find(_)) => {
                "Tour 3/3 · / searches: type a word from your note, then Enter. Ctrl-G skips."
            }
            _ => return None,
        };
        Some((Kind::Dim, hint))
    }
}
