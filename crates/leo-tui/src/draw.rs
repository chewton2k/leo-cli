//! Painting the whole frame from App state.

use super::*;

impl App {
    pub(super) fn draw(&self, frame: &mut Frame) {
        let tabs = self.tabs();
        let f = view::layout_with_tabs(frame.area(), !tabs.is_empty(), self.focus);
        view::tabs::render(frame, f.tabs, &tabs);

        let dir_rows = self.dir_rows();
        let note_rows = self.note_rows();

        view::dirs::render(
            frame,
            f.dirs,
            &dir_rows,
            self.dir_sel,
            self.focus == Pane::Dirs,
            "dirs",
            &view::empty::Hint::no_directories(),
        );
        let empty_hint = self.empty_hint();
        view::notes::render(
            frame,
            f.notes,
            &note_rows,
            self.note_sel,
            self.focus == Pane::Notes,
            &empty_hint,
            self.filter.as_deref(),
        );

        let selected_note = self.selected_id().and_then(|id| self.store.find_note(id));
        // An answer arriving owns the preview: watching it appear is the point of
        // streaming, and it replaces the note only until it is saved into it.
        let streaming = self
            .asking
            .as_ref()
            .filter(|a| !a.text.trim().is_empty())
            .map(|a| Preview::Text {
                title: match &a.question {
                    Some(q) => format!("answering from your notes: {q}"),
                    None => "answering…".to_string(),
                },
                body: a.text.clone(),
            });
        let preview = match (streaming, &self.recording, &self.pinned, selected_note) {
            (Some(live), ..) => live,
            // A live recording owns the preview: that stream is the reason the
            // feature exists.
            (None, Some(rec), _, _) => Preview::Live {
                paused: rec.job.paused(),
                points: rec.point_lines(),
                transcript: &rec.transcript,
                // The box is for typing, so it goes once the recording stops.
                jot: (!rec.job.stop_requested()).then_some(rec.jot.as_str()),
                scroll: &rec.scroll,
            },
            (None, None, _, _) if self.answer.is_some() => {
                let (question, text) = self.answer.as_ref().expect("checked");
                Preview::Text {
                    title: format!("from your notes: {question} (Esc closes)"),
                    body: text.clone(),
                }
            }
            (None, None, Some((title, lines)), _) => Preview::Lines {
                title: title.clone(),
                lines,
            },
            (None, None, None, Some(note)) => Preview::Note(note),
            (None, None, None, None) => Preview::Empty,
        };
        let now = chrono::Utc::now();
        let written = self
            .editing
            .as_ref()
            .filter(|_| !matches!(preview, Preview::Text { .. } | Preview::Live { .. }))
            .and_then(|ed| self.store.find_note(&ed.id).map(|note| (ed, note)));
        match written {
            Some((ed, note)) => view::editing::render(
                frame,
                f.preview,
                &format!(
                    "{}  ·  {}",
                    note.title,
                    view::when::long(note.updated_at, now)
                ),
                ed,
                self.focus == Pane::Preview,
            ),
            None => view::preview::render(
                frame,
                f.preview,
                &preview,
                self.preview_scroll,
                self.focus == Pane::Preview,
                self.filter.as_deref().filter(|q| !q.trim().is_empty()),
            ),
        }

        let ghost = self.ghost();
        let found = (self.mode == Mode::Command && !action::is_command(self.cmd.text()))
            .then(|| self.filter.as_ref().map(|_| self.note_count()))
            .flatten();
        view::status::render_command(
            frame,
            f.command,
            (self.mode == Mode::Command).then(|| view::status::Typing {
                text: self.cmd.text(),
                cursor: self.cmd.cursor(),
                ghost: ghost.as_deref(),
                found,
            }),
            view::hints::for_place(self.hint_place()),
        );
        // A job's progress replaces the plain busy label, so the user can see
        // both that something is happening and how far along it is.
        let busy = self
            .asking
            .as_ref()
            .map(|a| view::progress::render(&a.progress, a.since.elapsed()))
            .or_else(|| {
                self.recording
                    .as_ref()
                    .map(|r| view::progress::render(&r.progress, r.since.elapsed()))
            })
            .or_else(|| {
                self.checking
                    .as_ref()
                    .map(|(_, p, since)| view::progress::render(p, since.elapsed()))
            })
            .or_else(|| {
                self.busy
                    .as_ref()
                    .map(|(p, since)| view::progress::render(p, since.elapsed()))
            });
        view::status::render_status(
            frame,
            f.status,
            &self.current_dir,
            self.live_message(),
            busy.as_deref(),
            self.counts(),
        );

        if let Some((items, selected)) = self.menu() {
            view::menu::render(frame, frame.area(), f.command, &items, selected);
        }

        match &self.mode {
            Mode::Help => view::help::render_help(frame, frame.area(), self.help_scroll),
            Mode::Confirm { prompt, .. } => view::help::render_confirm(frame, frame.area(), prompt),
            Mode::Welcome => {
                if let Some(screen) = &self.welcome {
                    let heading = match screen.need {
                        welcome::Need::Recording => view::welcome::Heading {
                            title: "Before you record",
                            why: "Recording turns speech into a note, which needs these.",
                        },
                        welcome::Need::Writing => view::welcome::Heading {
                            title: "Before you ask",
                            why: "Answers come from an AI for writing, which is not set up yet.",
                        },
                    };
                    view::welcome::render(
                        frame,
                        frame.area(),
                        heading,
                        &screen.steps,
                        screen.selected,
                        screen.status.as_deref(),
                    );
                }
            }
            Mode::Settings => {
                if let Some(screen) = &self.settings {
                    view::settings::render(
                        frame,
                        frame.area(),
                        &screen.rows,
                        screen.selected,
                        screen.status.as_deref(),
                    );
                }
            }
            _ => {}
        }
    }

    /// The status message, if it has not aged out.
    pub(super) fn live_message(&self) -> Option<(Kind, &str)> {
        self.message.as_ref().and_then(|(kind, text, at)| {
            (at.elapsed() < MESSAGE_TTL).then_some((*kind, text.as_str()))
        })
    }

    /// Which set of key hints the idle command line shows.
    pub(super) fn hint_place(&self) -> view::hints::Place {
        use view::hints::Place;
        if self.recording.is_some() {
            return Place::Recording;
        }
        match self.focus {
            Pane::Dirs => Place::Dirs,
            Pane::Preview if self.editing.is_some() => Place::Preview,
            _ => Place::Notes,
        }
    }
}
