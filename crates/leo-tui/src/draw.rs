//! Painting the whole frame from App state.

use super::*;

impl App {
    pub(super) fn draw(&self, frame: &mut Frame) {
        let tabs = self.tabs();
        let f = view::layout_with_tabs(frame.area(), !tabs.is_empty(), self.focus);
        view::tabs::render(frame, f.tabs, &tabs);

        let dir_rows = self.dir_rows();
        let note_rows = self.note_rows();

        let (left_title, left_empty) = self.left_pane_labels();
        view::dirs::render(
            frame,
            f.dirs,
            &dir_rows,
            self.dir_sel,
            self.focus == Pane::Dirs,
            left_title,
            &left_empty,
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
        view::preview::render(
            frame,
            f.preview,
            &preview,
            self.preview_scroll,
            self.focus == Pane::Preview,
            self.checkbox_cursor(),
            self.filter.as_deref().filter(|q| !q.trim().is_empty()),
        );

        let ghost = self.ghost();
        // While filtering, the command row belongs to the filter: it is a lens
        // on the pane above rather than a command to run.
        match (&self.mode, &self.filter) {
            (Mode::Filter, Some(query)) => {
                view::status::render_filter(frame, f.command, query, self.note_count())
            }
            _ => view::status::render_command(
                frame,
                f.command,
                self.mode == Mode::Command,
                self.cmd.text(),
                self.cmd.cursor(),
                ghost.as_deref(),
                view::hints::for_place(self.hint_place()),
            ),
        }
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
                    view::welcome::render(
                        frame,
                        frame.area(),
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
    fn hint_place(&self) -> view::hints::Place {
        use view::hints::Place;
        if self.recording.is_some() {
            return Place::Recording;
        }
        match (self.focus, self.left) {
            (Pane::Dirs, LeftPane::Tags) => Place::Tags,
            (Pane::Dirs, LeftPane::Dirs) => Place::Dirs,
            (Pane::Notes, _) => Place::Notes,
            (Pane::Preview, _) => Place::Preview,
        }
    }

    /// The checkbox to highlight: only with the preview focused on a note that
    /// has some, since that is when j/k and x act on it.
    fn checkbox_cursor(&self) -> Option<usize> {
        let boxes = self.checkboxes().len();
        (self.focus == Pane::Preview && self.pinned.is_none() && boxes > 0)
            .then(|| self.box_index().min(boxes - 1))
    }
}
