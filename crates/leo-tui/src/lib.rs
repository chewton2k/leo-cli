//! The full-screen shell.
//!
//! `App` owns the store and the selection state; every key press becomes an
//! [`Intent`] or a parsed [`Action`], and the handlers in [`leo_core::action`] do
//! the work. Rendering reads `App` and nothing else, so the panes stay
//! independently testable.

pub mod cmdline;
pub mod complete;
pub mod editor;
pub mod keys;
mod recent;
pub mod settings;
pub mod task;
pub mod view;

mod backup;
mod disk;
mod draw;
mod mouse;
mod profile;
mod pump;
pub mod shell;
mod welcome;
mod writing;

use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};

use cmdline::{CmdLine, CmdOutcome};
use complete::{Completion, NoteChoice, Sources};
use keys::{Intent, Pane};
use leo_core::action::{
    self, Action, ConfirmedAction, Ctx, Effect, Kind, Line, ListenRequest, Outcome, Parsed,
};
use leo_core::store::Store;
use task::{Job, TaskEvent};
use view::dirs::DirRow;
use view::notes::NoteRow;
use view::preview::Preview;
use view::settings::Row as SettingsRow;

/// The backend bound every terminal-taking method needs. `Backend` alone is not
/// enough: `?` on a draw has to convert the backend's error into `anyhow::Error`,
/// which requires it to be a `Send + Sync` std error. Both `CrosstermBackend`
/// and `TestBackend` satisfy this, so the App can be driven by either — which is
/// what makes the event handling testable without a real terminal.
trait TuiBackend: Backend<Error: std::error::Error + Send + Sync + 'static> {}

impl<B> TuiBackend for B
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
}

/// How long a status message stays before the status line goes quiet again.
const MESSAGE_TTL: Duration = Duration::from_secs(6);
/// Event-poll timeout. Short enough that a background task's progress appears
/// promptly, long enough not to spin the CPU.
const TICK: Duration = Duration::from_millis(120);

/// Which input surface is active.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    Command,
    Help,
    Confirm {
        prompt: String,
        on_yes: ConfirmedAction,
    },
    Settings,
    Welcome,
}

pub struct App {
    store: Store,
    current_dir: String,
    /// Note IDs in pane order; index+1 is the number the `:` line accepts.
    numbering: Vec<String>,
    /// The live filter, when one is set. `numbering` respects it, so the numbers
    /// the user types always mean the rows the user can see.
    filter: Option<String>,
    search_base: Option<String>,
    /// Notes recently looked at, most recent first.
    recent: recent::Recent,
    /// A running `:ask`, with the answer so far.
    asking: Option<Asking>,
    /// When the notes last changed, for the idle trigger.
    last_change: Instant,
    /// A background push, and when the last one finished.
    pushing: Option<(task::Job, view::progress::Progress, Instant)>,
    checking: Option<(task::Job, view::progress::Progress, Instant)>,
    probe: leo_services::doctor::Probe,
    gh_ready: fn() -> bool,
    local_models: fn(&leo_services::config::Config) -> leo_services::config::choice::Local,
    setup_steps: fn(&std::path::Path) -> Vec<leo_services::health::Check>,
    recordings: Option<std::path::PathBuf>,
    obsidian: fn(&std::path::Path) -> Result<leo_core::obsidian::Opened>,
    update: Option<std::sync::mpsc::Receiver<String>>,
    model_download: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    speech_model_wanted: fn() -> bool,
    last_disk_check: Option<Instant>,
    last_push: Option<Instant>,
    /// How many commits are waiting, refreshed when the notes change rather than
    /// on every frame: it costs a git process.
    unpushed: Option<usize>,
    note_sel: usize,
    editing: Option<editor::Editor>,
    edit_uncommitted: bool,
    /// Notes marked with Space. When any are, D and m act on all of them.
    marked: Vec<String>,
    dir_sel: usize,
    focus: Pane,
    mode: Mode,
    cmd: CmdLine,
    preview_scroll: u16,
    /// The last message and when it arrived, for the status line.
    message: Option<(Kind, String, Instant)>,
    /// Output shown in the preview instead of the selected note, keeping each
    /// line's styling. Cleared by Esc, by moving the selection, or by `clear`.
    pinned: Option<(String, Vec<Line>)>,
    /// An answer from all the notes, shown in the preview until dismissed like
    /// pinned output. Markdown, so it renders like a note.
    answer: Option<(String, String)>,
    /// The setup screen, while it is open.
    welcome: Option<welcome::WelcomeScreen>,
    /// The running recording, if any.
    recording: Option<Recording>,
    /// Tab-completion state, live only while cycling.
    completing: Option<Cycle>,
    /// Scroll offset for the help overlay.
    help_scroll: u16,
    /// The provider screen's rows and selection, live only while it is open.
    settings: Option<SettingsScreen>,
    /// Foreground work the user is waiting on, shown in the status line.
    busy: Option<(view::progress::Progress, Instant)>,
    /// Set when the next frame should repaint every cell rather than a diff.
    repaint: bool,
    quit: bool,
}

/// The provider screen's state. Rows are rebuilt from config after every edit,
/// so what is on screen is always what is in the file.
struct SettingsScreen {
    rows: Vec<SettingsRow>,
    selected: usize,
    status: Option<String>,
}

/// Tab cycling: the candidates for one token and how far through them the user
/// has walked. Dropped as soon as the line changes any other way, so Tab never
/// replays a stale candidate list.
struct Cycle {
    completion: Completion,
    /// What the token was before the first Tab, so cycling back is possible.
    typed: String,
    index: usize,
}

/// A recording in progress and everything it has produced so far.
struct Recording {
    job: Job,
    /// What to do with the transcript when it finishes.
    req: ListenRequest,
    /// What the worker is doing, and how far along when that is knowable.
    progress: view::progress::Progress,
    /// When the current step started, for the elapsed clock and the spinner.
    since: Instant,
    /// The rolling transcript so far, shown as it grows.
    transcript: String,
    /// When recording began, for stamping typed points.
    started: Instant,
    /// Time spent paused so far, and since when if paused now: typed points
    /// and the transcript's time markers count recording time only.
    paused_total: std::time::Duration,
    paused_since: Option<Instant>,
    /// The point being typed right now.
    jot: String,
    /// Points typed so far. They lead the finished notes, in bold.
    jotted: Vec<leo_services::ai::chat::Jotted>,
    /// When Esc was last pressed, so a second press soon after stops the
    /// recording and a lone one does not.
    stop_armed: Option<Instant>,
    scroll: view::livescroll::LiveScroll,
    session: Option<std::path::PathBuf>,
}

impl Recording {
    fn new(job: Job, req: ListenRequest) -> Recording {
        Recording {
            job,
            req,
            progress: view::progress::Progress::spinner("Starting"),
            since: Instant::now(),
            transcript: String::new(),
            started: Instant::now(),
            paused_total: std::time::Duration::ZERO,
            paused_since: None,
            jot: String::new(),
            jotted: Vec::new(),
            stop_armed: None,
            scroll: view::livescroll::LiveScroll::new(),
            session: None,
        }
    }

    /// How much has been recorded, leaving out pauses.
    fn recorded(&self) -> std::time::Duration {
        let paused = self.paused_total
            + self
                .paused_since
                .map_or(std::time::Duration::ZERO, |t| t.elapsed());
        self.started.elapsed().saturating_sub(paused)
    }

    /// Pause, or resume, the recording.
    fn toggle_pause(&mut self) {
        match self.paused_since.take() {
            Some(since) => {
                self.paused_total += since.elapsed();
                self.job.set_paused(false);
            }
            None => {
                self.paused_since = Some(Instant::now());
                self.job.set_paused(true);
            }
        }
    }

    /// Keep the point being typed, if there is one.
    fn commit_jot(&mut self) {
        let text = self.jot.trim().to_string();
        self.jot.clear();
        if !text.is_empty() {
            let point = leo_services::ai::chat::Jotted {
                at_secs: self.recorded().as_secs(),
                text,
            };
            self.job.add_point(point.clone());
            self.jotted.push(point);
        }
    }

    /// Typed points as the preview lists them, with when each was typed.
    fn point_lines(&self) -> Vec<String> {
        self.jotted
            .iter()
            .map(|p| format!("{} ({})", p.text, leo_services::ai::chat::clock(p.at_secs)))
            .collect()
    }
}

impl App {
    pub fn new(store: Store) -> App {
        let current_dir = String::new();
        let numbering = action::numbering_for(&store, &current_dir);
        App {
            filter: None,
            search_base: None,
            recent: recent::Recent::load(),
            asking: None,
            last_change: Instant::now(),
            pushing: None,
            checking: None,
            probe: leo_services::doctor::Probe::all(),
            update: None,
            model_download: None,
            speech_model_wanted: || {
                leo_services::providers::speech_model_wanted(&leo_services::config::Config::load())
            },
            last_disk_check: None,
            gh_ready: leo_core::sync::gh_ready,
            local_models: leo_services::config::choice::local_models,
            setup_steps: welcome::real_setup_steps,
            recordings: leo_services::session::root().ok(),
            obsidian: leo_core::obsidian::open,
            last_push: None,
            unpushed: None,
            store,
            current_dir,
            numbering,
            note_sel: 0,
            editing: None,
            edit_uncommitted: false,
            marked: Vec::new(),
            dir_sel: 0,
            focus: Pane::Notes,
            mode: Mode::Normal,
            cmd: CmdLine::default(),
            preview_scroll: 0,
            message: None,
            pinned: None,
            answer: None,
            welcome: None,
            recording: None,
            completing: None,
            help_scroll: 0,
            settings: None,
            busy: None,
            repaint: false,
            quit: false,
        }
    }

    // ── derived view data ───────────────────────────────────────────────────

    fn dir_rows(&self) -> Vec<DirRow> {
        view::dirs::rows(&self.current_dir, &self.store.subdirs(&self.current_dir))
    }

    fn note_rows(&self) -> Vec<NoteRow> {
        let notes: Vec<&leo_core::notes::Note> = self
            .numbering
            .iter()
            .filter_map(|id| self.store.find_note(id))
            .collect();
        let mut rows = view::notes::rows(&notes, &self.current_dir);
        let words = self
            .filter
            .as_deref()
            .map(leo_core::notes::search_words)
            .unwrap_or_default();
        for (row, note) in rows.iter_mut().zip(&notes) {
            row.marked = self.marked.contains(&row.id);
            row.pinned = self.store.find_note(&row.id).is_some_and(|n| n.pinned);
            // Only when the title does not already show why it was found.
            let title = note.title.to_lowercase();
            if !words.is_empty() && !words.iter().all(|w| title.contains(w.as_str())) {
                row.snippet = note.matching_line(self.filter.as_deref().unwrap_or(""));
            }
        }
        rows
    }

    /// The selected note's checkboxes, ticked or not.
    fn checkboxes(&self) -> Vec<bool> {
        self.selected_id()
            .and_then(|id| self.store.find_note(id))
            .map(|n| n.checkboxes())
            .unwrap_or_default()
    }

    fn selected_id(&self) -> Option<&String> {
        self.numbering.get(self.note_sel)
    }

    /// Remember the selected note as recently visited.
    ///
    /// Called when the selection settles rather than on every keystroke of j/k:
    /// scrolling past a note is not visiting it, and recording it would fill the
    /// list with notes the user never looked at.
    fn remember_visit(&mut self) {
        if let Some(id) = self.selected_id().cloned() {
            self.recent.touch(&id);
        }
    }

    /// The recent-notes strip, most recent first.
    fn tabs(&self) -> Vec<view::tabs::Tab> {
        let current = self.selected_id();
        self.recent
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
    fn on_resize(&mut self, width: u16, height: u16) {
        if view::Shape::for_width(width) == view::Shape::One {
            return;
        }
        let frames = view::layout_with_tabs(
            Rect::new(0, 0, width, height),
            !self.tabs().is_empty(),
            self.focus,
        );
        if !frames.shows(self.focus) {
            self.focus = Pane::Notes;
        }
    }

    /// The next pane in `direction` that is actually on screen.
    ///
    /// At narrow widths some panes are not drawn, and focusing one the user
    /// cannot see would make the keyboard appear to stop working. In the
    /// one-pane shape every pane is "visible" in turn, since the focused one is
    /// the one that gets drawn — which is what keeps everything reachable.
    fn next_visible_pane<B: TuiBackend>(&self, terminal: &Terminal<B>, direction: isize) -> Pane {
        let Ok(size) = terminal.size() else {
            return self.focus;
        };
        let area = Rect::new(0, 0, size.width, size.height);
        let shape = view::Shape::for_width(size.width);

        // One pane at a time: every step lands somewhere, because whichever pane
        // has focus is the one drawn.
        if shape == view::Shape::One {
            return if direction < 0 {
                self.focus.left()
            } else {
                self.focus.right()
            };
        }

        let frames = view::layout_with_tabs(area, !self.tabs().is_empty(), self.focus);
        let mut candidate = self.focus;
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
        self.focus
    }

    /// Show a note by id, wherever it lives.
    ///
    /// Selects it when the current listing contains it, and pins it into the
    /// preview when it does not — the user asked for that note, not for a place.
    fn open_note(&mut self, id: &str) {
        match self.numbering.iter().position(|n| n == id) {
            Some(position) => {
                self.note_sel = position;
                self.preview_scroll = 0;
                self.unpin();
            }
            None => {
                if let Some(note) = self.store.find_note(id) {
                    let (title, body) = (note.title.clone(), note.body.clone());
                    self.pinned = Some((title, body.lines().map(Line::plain).collect()));
                    self.preview_scroll = 0;
                }
            }
        }
        self.recent.touch(id);
    }

    /// Jump to the next entry in the recent list, wrapping.
    ///
    /// Cycling rather than presenting a menu: with five entries, pressing a key
    /// twice is faster than reading a list, and it matches how editors move
    /// between recent tabs.
    fn jump_recent(&mut self) {
        let store = &self.store;
        self.recent
            .retain_existing(|id| store.find_note(id).is_some());
        if self.recent.is_empty() {
            self.say(Kind::Dim, "No notes visited yet.");
            return;
        }

        let current = self.selected_id().cloned();
        // The next entry that is not where we already are.
        let target = self
            .recent
            .ids()
            .iter()
            .find(|id| Some(*id) != current.as_ref())
            .cloned();

        let Some(target) = target else {
            self.say(Kind::Dim, "Only this note has been visited.");
            return;
        };

        match self.numbering.iter().position(|id| id == &target) {
            Some(position) => {
                self.note_sel = position;
                self.preview_scroll = 0;
                self.unpin();
            }
            // Not in the current listing: show it anyway rather than refusing,
            // since the user asked for that note and not for a place.
            None => {
                if let Some(note) = self.store.find_note(&target) {
                    let title = note.title.clone();
                    let body = note.body.clone();
                    self.pinned = Some((title, body.lines().map(Line::plain).collect()));
                    self.preview_scroll = 0;
                }
            }
        }

        if let Some(note) = self.store.find_note(&target) {
            let title = note.title.clone();
            self.recent.touch(&target);
            self.say(Kind::Dim, format!("← {title}"));
        }
    }

    /// The 1-based number of the selection, as a `:` line argument.
    fn selected_ref(&self) -> Option<String> {
        self.selected_id().map(|_| (self.note_sel + 1).to_string())
    }

    fn note_count(&self) -> usize {
        self.numbering.len()
    }

    /// Why the notes pane is empty, and what to do about it.
    ///
    /// Only the app knows the difference between "no notes at all", "this
    /// directory is empty", and "the filter matched nothing" — and those need
    /// different advice.
    fn empty_hint(&self) -> view::empty::Hint {
        // A filter that matched nothing is the most common empty pane, and the
        // most misleading if it does not say so.
        if let Some(query) = &self.filter {
            if !query.trim().is_empty() {
                return view::empty::Hint::no_matches(query);
            }
        }
        if self.current_dir.is_empty() {
            view::empty::Hint::no_notes()
        } else {
            view::empty::Hint::empty_directory()
        }
    }

    /// What the status bar reports on the right: how much is here.
    fn counts(&self) -> view::status::Counts {
        view::status::Counts {
            notes: self.note_count(),
            words: self
                .selected_id()
                .and_then(|id| self.store.find_note(id))
                .map(|note| note.body.split_whitespace().count()),
        }
    }

    fn say(&mut self, kind: Kind, text: impl Into<String>) {
        self.message = Some((kind, text.into(), Instant::now()));
    }

    fn greet(&mut self, first_run: bool) {
        if first_run {
            self.say(
                Kind::Dim,
                "Welcome. Enter writes in a note, n makes one, / finds anything, ? shows the rest.",
            );
        }
    }

    /// Whether anything a recording needs is missing. `None` means go ahead.
    ///
    /// Reports every gap at once with its fix, so one attempt tells the user
    /// everything they need to do rather than one thing per attempt. Checked
    /// before recording rather than after: discovering there is no transcription
    /// provider once the user has already talked for twenty minutes is the worst
    /// possible time to learn it.
    fn listen_preflight(&mut self, screen: bool) -> Option<Vec<Line>> {
        let config = leo_services::config::Config::load();
        // Screen capture and the replay hook do not use the microphone, so
        // probing it would refuse a recording that would have worked.
        let uses_microphone = !screen && std::env::var("LEO_FAKE_AUDIO").is_err();
        let checks = leo_services::health::recording(
            &config,
            leo_services::config::secret::default_store().as_ref(),
            uses_microphone,
        );
        let missing: Vec<_> = checks.iter().filter(|c| !c.state.is_ready()).collect();
        if missing.is_empty() {
            return None;
        }

        let mut lines = vec![Line::bad("Not ready to record:")];
        for check in missing {
            lines.push(Line::warn(format!(
                "  {} — needed for {}",
                check.what, check.needed_for
            )));
            if let leo_services::health::State::Missing { fix } = &check.state {
                for fix_line in fix.lines() {
                    lines.push(Line::dim(format!("      {}", fix_line.trim())));
                }
            }
        }
        lines.push(Line::blank());
        lines.push(Line::dim(
            "  /settings picks the AI · /doctor checks everything",
        ));
        Some(lines)
    }

    /// Refresh the numbering after the store or directory changed, keeping the
    /// selection in range.
    /// Drop whatever output or answer is covering the preview.
    fn unpin(&mut self) {
        self.pinned = None;
        self.answer = None;
    }

    /// Recompute the numbering, keeping the selected note selected if it is
    /// still listed — an edit moves a note to the top, and the selection has to
    /// follow it rather than land on whatever slid into its old row.
    fn resync(&mut self) {
        let keep = self.selected_id().cloned();
        self.numbering = match &self.filter {
            Some(query) => action::filtered_numbering(&self.store, &self.current_dir, query),
            None => action::numbering_for(&self.store, &self.current_dir),
        };
        if let Some(pos) = keep.and_then(|id| self.numbering.iter().position(|n| *n == id)) {
            self.note_sel = pos;
        }
        if self.note_sel >= self.numbering.len() {
            self.note_sel = self.numbering.len().saturating_sub(1);
        }
        let dirs = self.dir_rows().len();
        if self.dir_sel >= dirs {
            self.dir_sel = dirs.saturating_sub(1);
        }
    }

    fn on_key<B: TuiBackend>(
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

            Mode::Command => {
                self.mode = Mode::Command;
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
                        if self.filter.take().is_some() {
                            self.resync();
                            self.note_sel = 0;
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
                        self.submit_line(&line, terminal)
                    }
                }
            }

            Mode::Normal => {
                self.mode = Mode::Normal;
                if self.editing.is_some() && self.focus == Pane::Preview {
                    return self.on_edit_key(key, terminal);
                }
                // While recording, the keyboard takes notes: typing builds a
                // point, Enter adds it, Tab switches bullets and raw text, Esc
                // stops. Once stopping, the panes get their keys back.
                if let Some(rec) = self.recording.as_mut().filter(|r| !r.job.stop_requested()) {
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
                let intent = keys::normal(key, self.focus);
                self.on_intent(intent, terminal)
            }
        }
    }

    fn on_intent<B: TuiBackend>(
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
                self.focus = self.next_visible_pane(terminal, -1);
                Ok(())
            }
            Intent::FocusRight => {
                let next = self.next_visible_pane(terminal, 1);
                if next == Pane::Preview && self.focus == Pane::Notes && self.start_editing() {
                    return Ok(());
                }
                self.focus = next;
                Ok(())
            }

            Intent::ScrollDown => {
                self.preview_scroll = self.preview_scroll.saturating_add(5);
                Ok(())
            }
            Intent::ScrollUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(5);
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
                self.search_base = self.filter.clone();
                self.cmd.open("new ");
                self.mode = Mode::Command;
                Ok(())
            }

            Intent::ToggleMark => {
                let Some(id) = self.selected_id().cloned() else {
                    return Ok(());
                };
                match self.marked.iter().position(|m| *m == id) {
                    Some(i) => {
                        self.marked.remove(i);
                    }
                    None => self.marked.push(id),
                }
                match self.marked.len() {
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
            Intent::DeleteSelected => match self.focus {
                Pane::Dirs => self.delete_selected_dir(terminal),
                _ if !self.marked.is_empty() => self.run_action(
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
                self.search_base = self.filter.clone();
                self.cmd.open(seed);
                self.mode = Mode::Command;
                Ok(())
            }

            Intent::OpenFilter => {
                self.search_base = None;
                let seed = self.filter.clone().unwrap_or_default();
                self.cmd.open(&seed);
                self.mode = Mode::Command;
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
                if !self.marked.is_empty() {
                    self.marked.clear();
                    self.say(Kind::Dim, "Marks cleared.");
                    return Ok(());
                }
                let picked = self.selected_id().cloned();
                if self.filter.take().is_some() {
                    self.resync();
                    match picked {
                        Some(id) => self.jump_to(&id),
                        None => self.note_sel = 0,
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

    fn move_selection(&mut self, intent: Intent) {
        match self.focus {
            Pane::Dirs => {
                let len = self.dir_rows().len();
                self.dir_sel = step(self.dir_sel, len, intent);
            }
            Pane::Notes => {
                let len = self.note_count();
                self.note_sel = step(self.note_sel, len, intent);
                // A new note means the old scroll position is meaningless.
                self.preview_scroll = 0;
                self.unpin();
            }
            Pane::Preview => match intent {
                Intent::Down => self.preview_scroll = self.preview_scroll.saturating_add(1),
                Intent::Up => self.preview_scroll = self.preview_scroll.saturating_sub(1),
                Intent::First => self.preview_scroll = 0,
                Intent::Last => self.preview_scroll = u16::MAX / 2,
                _ => {}
            },
        }
    }

    /// `D` in the dirs pane: delete that directory and everything in it.
    fn delete_selected_dir<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let rows = self.dir_rows();
        let Some(row) = rows.get(self.dir_sel) else {
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
    fn open<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        match self.focus {
            Pane::Dirs => {
                let rows = self.dir_rows();
                let Some(row) = rows.get(self.dir_sel) else {
                    return Ok(());
                };
                let target = row.target.clone();
                self.run_action(Action::Cd { path: target }, terminal)
            }
            Pane::Notes => {
                if !self.start_editing() {
                    self.focus = Pane::Preview;
                }
                Ok(())
            }
            Pane::Preview => Ok(()),
        }
    }

    /// Select a note by id, following it into its directory when it is not in
    /// the current listing — which is where a search result from elsewhere
    /// has to take you.
    fn jump_to(&mut self, id: &str) {
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
        self.unpin();
        self.preview_scroll = 0;
        self.focus = Pane::Notes;
    }

    // ── running actions ─────────────────────────────────────────────────────

    fn follow_search(&mut self) {
        let text = self.cmd.text().to_string();
        let wanted = if action::is_command(&text) {
            self.search_base.clone()
        } else if text.trim().is_empty() {
            None
        } else {
            Some(text)
        };
        if wanted != self.filter {
            self.filter = wanted;
            self.note_sel = 0;
            self.preview_scroll = 0;
            self.unpin();
            self.resync();
        }
    }

    fn submit_line<B: TuiBackend>(&mut self, line: &str, terminal: &mut Terminal<B>) -> Result<()> {
        if action::is_command(line) {
            return self.run_line(line, terminal);
        }
        if line.trim().is_empty() {
            if self.filter.take().is_some() {
                self.resync();
                self.note_sel = 0;
            }
            return Ok(());
        }
        self.filter = Some(line.to_string());
        self.resync();
        self.focus = Pane::Notes;
        let found = self.note_count();
        self.say(
            Kind::Dim,
            match found {
                0 => "Nothing found. Esc clears the search.".to_string(),
                1 => "1 note found. Esc clears the search.".to_string(),
                n => format!("{n} notes found. Esc clears the search."),
            },
        );
        Ok(())
    }

    fn run_line<B: TuiBackend>(&mut self, line: &str, terminal: &mut Terminal<B>) -> Result<()> {
        match action::parse(line) {
            Parsed::Empty => Ok(()),
            Parsed::Usage(usage) => {
                self.say(Kind::Warn, format!("Usage: {usage}"));
                Ok(())
            }
            Parsed::Unknown(verb) => {
                self.say(
                    Kind::Bad,
                    format!(
                        "Unknown command: {verb} — press / for the menu of commands, ? for every key"
                    ),
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

    fn run_action<B: TuiBackend>(
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
        let selected = self.numbering.get(self.note_sel).map(String::as_str);
        let action = match action::fill_selected(action, selected, &self.marked) {
            Ok(action) => action,
            Err(line) => return self.absorb(Outcome::line(line), terminal),
        };
        // Marks are spent by the command that used them.
        if matches!(&action, Action::DeleteMany { .. })
            || matches!(&action, Action::Mv { notes, .. } if !self.marked.is_empty() && *notes == self.marked)
        {
            self.marked.clear();
        }
        // `ask` is the one action that can take a minute. Run it on a worker and
        // stream the answer: inline, it froze the interface with nothing to say
        // whether the model was thinking or the request had died.
        if let Action::Ask { note } = &action {
            if self.asking.is_some() {
                self.say(Kind::Warn, "Already asking — one at a time.");
                return Ok(());
            }
            if !self.store.notes.iter().any(|n| n.id == *note) {
                return self.run_action_inline(action, terminal);
            }
            let resolved = action::resolve(note, &self.store, &self.numbering);
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
                    "Type a question after /ask, or write @leo and a question in a note.",
                );
                return Ok(());
            }
            if self.set_up_first(welcome::Need::Writing) {
                return Ok(());
            }

            self.asking = Some(Asking {
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

    fn run_action_inline<B: TuiBackend>(
        &mut self,
        action: Action,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let outcome = match action::apply(
            action,
            &mut self.store,
            Ctx {
                current_dir: &self.current_dir,
                numbering: &self.numbering,
                selected: self.numbering.get(self.note_sel).map(String::as_str),
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
    fn absorb<B: TuiBackend>(
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
            self.current_dir = dir;
            self.note_sel = 0;
            self.dir_sel = 0;
            self.unpin();
        }

        match outcome.selection {
            Some(sel) => {
                self.numbering = sel;
                self.note_sel = 0;
            }
            None if outcome.dirty => self.resync(),
            None => {}
        }

        // A note just made or added to: take the user to it, in its own
        // directory, with any search cleared so it is actually listed.
        if let Some(id) = &outcome.select {
            if self.filter.take().is_some() {
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
                self.preview_scroll = 0;
            }
        }

        match outcome.effect {
            Effect::None => Ok(()),

            Effect::Quit => {
                self.quit = true;
                Ok(())
            }

            Effect::AskNotes { question } => {
                if self.asking.is_some() {
                    self.say(Kind::Warn, "Already asking — one at a time.");
                    return Ok(());
                }
                if self.set_up_first(welcome::Need::Writing) {
                    return Ok(());
                }
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
                self.asking = Some(Asking {
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
                if let Some(pos) = self.numbering.iter().position(|n| n == &id) {
                    self.note_sel = pos;
                    self.unpin();
                } else if let Some(note) = self.store.find_note(&id) {
                    // Not in the current directory's listing, so show it
                    // directly rather than silently doing nothing.
                    let title = note.title.clone();
                    let lines = note.body.lines().map(Line::plain).collect();
                    self.pinned = Some((title, lines));
                }
                self.preview_scroll = 0;
                self.focus = Pane::Preview;
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
                        self.preview_scroll = 0;
                    }
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                Ok(())
            }

            Effect::Doctor => {
                if self.checking.is_some() {
                    self.say(Kind::Warn, "Already checking — one at a time.");
                    return Ok(());
                }
                let probe = leo_services::doctor::Probe {
                    microphone: self.probe.microphone && self.recording.is_none(),
                    ..self.probe
                };
                self.checking = Some((
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
                if self.recording.is_some() {
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
                    self.preview_scroll = 0;
                    return Ok(());
                }
                self.recording = Some(Recording::new(
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

    fn offer_backup_setup(&mut self) {
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
                "Make an empty private repository on GitHub, paste its URL, then Enter. (With GitHub's gh tool, /backup github does it for you.)",
            );
        }
    }

    /// Leave the alternate screen, run `f` on the real terminal, then come
    /// back. Everything that writes to stdout or reads stdin — `$EDITOR`, git,
    /// the no-echo key prompt, the recorder — goes through here.
    fn outside<B: TuiBackend, T>(
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
    fn suspend_with_store<B: TuiBackend>(
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

    // ── completion ──────────────────────────────────────────────────────────

    /// Candidate sources drawn from the store and config.
    fn sources(&self) -> Sources {
        Sources {
            dirs: self.store.subdirs(&self.current_dir),
            notes: self
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
            tags: self.store.tags().into_iter().map(|(t, _)| t).collect(),
        }
    }

    /// Tab: complete the token, or step to the next candidate if already
    /// cycling. With one match this completes and stops; with several, each Tab
    /// advances and wraps around to what was typed.
    fn cycle_completion(&mut self) {
        if let Some(cycle) = self.completing.take() {
            let count = cycle.completion.matches.len();
            if count == 0 {
                return;
            }
            // One past the end restores the original text, so cycling is
            // never a trap.
            let next = (cycle.index + 1) % (count + 1);
            let (line, cursor) = if next == count {
                complete::apply(self.cmd.text(), &cycle.completion, &cycle.typed)
            } else {
                complete::apply(
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
        let completion = complete::complete(self.cmd.text(), self.cmd.cursor(), &sources);
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

        let (line, cursor) = complete::apply(self.cmd.text(), &completion, &completion.matches[0]);
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
    fn menu(&self) -> Option<(Vec<view::menu::Item>, Option<usize>)> {
        if self.mode != Mode::Command {
            return None;
        }
        let (completion, selected) = match &self.completing {
            Some(cycle) => {
                let index = (cycle.index < cycle.completion.matches.len()).then_some(cycle.index);
                (cycle.completion.clone(), index)
            }
            None => (
                complete::complete(self.cmd.text(), self.cmd.cursor(), &self.sources()),
                None,
            ),
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
    fn ghost(&self) -> Option<String> {
        if self.mode != Mode::Command || self.completing.is_some() {
            return None;
        }
        let text = self.cmd.text();
        if text.is_empty() {
            return None;
        }
        let completion = complete::complete(text, self.cmd.cursor(), &self.sources());
        let typed: String = text
            .chars()
            .skip(completion.start)
            .take(completion.end.saturating_sub(completion.start))
            .collect();
        completion.ghost(&typed)
    }
}

/// Hand the terminal back to the shell: leave raw mode and the alternate
/// screen, but keep the same `Terminal` instance. Calling `ratatui::init()`
/// again instead would stack a second panic hook and build a second terminal
/// over the live one.
fn suspend<B: TuiBackend>(terminal: &mut Terminal<B>) -> Result<()> {
    // The shell owns the terminal from here, so diagnostics may print again —
    // and anything already queued is worth showing alongside whatever the
    // suspended command prints.
    leo_core::diag::set_quiet(false);
    unhush_stderr();
    for message in leo_core::diag::drain() {
        eprintln!("  {message}");
    }
    disable_raw_mode()?;
    execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Take the terminal back. The screen we left is gone, so blank both of
/// ratatui's buffers to force a full repaint on the next draw.
///
/// Deliberately not `Terminal::clear()`: that snapshots the cursor first, which
/// makes `CrosstermBackend` emit a Device Status Report (`ESC[6n`) and block
/// reading the terminal's reply. Anything that does not answer — a pty harness,
/// a dumb pipe, a terminal that swallowed the query while we were suspended —
/// hangs or errors the app out on resume. Resetting the buffers needs no
/// round trip.
fn resume<B: TuiBackend>(terminal: &mut Terminal<B>) -> Result<()> {
    leo_core::diag::set_quiet(true);
    hush_stderr();
    enable_raw_mode()?;
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        Clear(ClearType::All)
    )?;
    // Two swaps reset both buffers, so the next diff has nothing to compare
    // against and repaints every cell.
    terminal.swap_buffers();
    terminal.swap_buffers();
    terminal.hide_cursor()?;
    Ok(())
}

/// An [`action::Ai`] whose answer is already known.
///
/// Structuring happens on a worker thread, but the note is written on the main
/// thread — and the writing logic (titles, tags, appending, saving) already
/// lives behind the `Ai` seam in `action`. Feeding the finished text back
/// through that seam reuses all of it instead of duplicating it here.
struct ReadyNote {
    title: Option<String>,
    body: String,
}

impl action::Ai for ReadyNote {
    fn expand_prompts(&self, body: &str, _title: &str) -> Result<(String, usize)> {
        Ok((body.to_string(), 0))
    }

    fn structure(&self, _transcript: &str) -> Result<(String, String)> {
        Ok((
            self.title
                .clone()
                .unwrap_or_else(|| "Untitled Notes".to_string()),
            self.body.clone(),
        ))
    }

    fn structure_append(&self, _transcript: &str, _existing: &str) -> Result<String> {
        Ok(self.body.clone())
    }
}

/// An answer already in hand, for writing back through the ordinary handler.
///
/// Distinct from [`ReadyNote`], whose `expand_prompts` deliberately echoes its
/// input: that one exists for the listen path, where nothing was expanded. Using
/// it here wrote the note back unchanged, which is the bug this type fixes.
struct PreExpanded {
    body: String,
    count: usize,
}

impl action::Ai for PreExpanded {
    fn expand_prompts(&self, _body: &str, _title: &str) -> Result<(String, usize)> {
        Ok((self.body.clone(), self.count))
    }

    fn structure(&self, _transcript: &str) -> Result<(String, String)> {
        anyhow::bail!("structuring is not this type's job")
    }

    fn structure_append(&self, _transcript: &str, _existing: &str) -> Result<String> {
        anyhow::bail!("structuring is not this type's job")
    }
}

/// A running `:ask`.
struct Asking {
    job: task::Job,
    /// The question, when this is a question across all the notes rather than
    /// a note's @leo lines.
    question: Option<String>,
    progress: view::progress::Progress,
    since: Instant,
    /// The answer so far, shown while it arrives.
    text: String,
}

/// Move a list selection, saturating at both ends rather than wrapping — a
/// wrap makes `j` on the last item feel like a jump.
fn step(current: usize, len: usize, intent: Intent) -> usize {
    if len == 0 {
        return 0;
    }
    match intent {
        Intent::Down => (current + 1).min(len - 1),
        Intent::Up => current.saturating_sub(1),
        Intent::First => 0,
        Intent::Last => len - 1,
        _ => current,
    }
}

/// Run the TUI. `ratatui::init` installs a panic hook that restores the
/// terminal, so a panic cannot leave the user in raw mode.
const STOP_CONFIRM_WITHIN: Duration = Duration::from_secs(3);

const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1006h";

const MOUSE_OFF: &str = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l";

fn disable_mouse() {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(MOUSE_OFF.as_bytes());
    let _ = out.flush();
}

fn is_main_thread(name: Option<&str>) -> bool {
    name == Some("main")
}

static HUSHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SAVED_STDERR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

fn hush_stderr() {
    if !HUSHING.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    #[cfg(unix)]
    unsafe {
        if SAVED_STDERR.load(std::sync::atomic::Ordering::SeqCst) >= 0 {
            return;
        }
        let saved = libc::dup(2);
        if saved < 0 {
            return;
        }
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
        if null < 0 {
            libc::close(saved);
            return;
        }
        libc::dup2(null, 2);
        libc::close(null);
        SAVED_STDERR.store(saved, std::sync::atomic::Ordering::SeqCst);
    }
}

fn unhush_stderr() {
    #[cfg(unix)]
    unsafe {
        let saved = SAVED_STDERR.swap(-1, std::sync::atomic::Ordering::SeqCst);
        if saved >= 0 {
            libc::dup2(saved, 2);
            libc::close(saved);
        }
    }
}

fn install_panic_hook() {
    let restoring = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if is_main_thread(std::thread::current().name()) {
            unhush_stderr();
            disable_mouse();
            restoring(info);
        } else {
            leo_core::diag::warn(format!("a background task crashed: {info}"));
        }
    }));
}

fn enable_mouse() -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout();
    out.write_all(MOUSE_ON.as_bytes())?;
    out.flush()
}

pub fn run() -> Result<()> {
    // Nothing below the UI may write to the terminal while the panes own it:
    // a stray line lands on top of them and stays until the next full repaint.
    leo_core::diag::set_quiet(true);
    // Before the first frame, so nothing is painted in the wrong colours.
    view::theme::init(leo_services::config::Config::load().theme.palette());
    let mut store = Store::load()?;
    // A first run explains itself: the manual is a real note the user can
    // search, scroll, and delete. A failure here must not stop the app.
    let installed_manual = leo_core::manual::install_if_absent(&mut store)
        .unwrap_or(None)
        .is_some();
    let mut terminal = ratatui::init();
    HUSHING.store(true, std::sync::atomic::Ordering::SeqCst);
    hush_stderr();
    install_panic_hook();
    // Mouse reporting is opt-in per terminal. Failing to enable it is not fatal:
    // every key still works, which is how leo is mostly driven.
    let mouse = enable_mouse().is_ok();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    let mut app = App::new(store);
    // The note on screen at startup has been looked at, so it belongs in the
    // recent list. Without this the first Tab has only one entry — the note the
    // user is already on — and answers "only this note has been visited".
    app.remember_visit();
    app.greet(installed_manual);
    app.resume_interrupted();
    app.update = Some(task::start_update_check());
    app.fetch_speech_model();
    let result = event_loop(&mut terminal, &mut app);
    app.flush_edit();
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    // Persist the recent list so the strip survives a restart, which is the
    // difference between a convenience and a novelty.
    app.recent.save();
    if mouse {
        disable_mouse();
    }
    ratatui::restore();
    unhush_stderr();
    HUSHING.store(false, std::sync::atomic::Ordering::SeqCst);
    leo_core::diag::set_quiet(false);
    // After the screen is handed back, so the push can say what it is doing on
    // an ordinary terminal rather than painting over the panes on the way out.
    app.push_on_quit();
    // Anything queued but never shown dies with the screen it belonged to.
    leo_core::diag::clear();
    result
}

fn event_loop<B: TuiBackend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    while !app.quit {
        if std::mem::take(&mut app.repaint) {
            // Blank both buffers so the next draw writes every cell.
            terminal.swap_buffers();
            terminal.swap_buffers();
            // Clear through the backend rather than `Terminal::clear`, which
            // queries the cursor position and blocks on the terminal's reply.
            use ratatui::backend::ClearType;
            terminal.backend_mut().clear_region(ClearType::All)?;
        }
        terminal.draw(|frame| app.draw(frame))?;

        if !event::poll(TICK)? {
            // No input: give the worker a chance to report progress, and pick up
            // anything the lower layers queued.
            app.pump_editor();
            app.pump_tasks(terminal)?;
            app.pump_doctor();
            app.pump_update();
            app.pump_model_download();
            app.maybe_reload_from_disk();
            app.pump_diagnostics();
            // Only when there is no input to handle: an automatic backup must
            // never compete with the user's typing.
            app.pump_push();
            app.maybe_auto_push();
            continue;
        }
        match event::read()? {
            // Only key *presses*: on Windows a release would double every key.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                app.on_key(key, terminal)?;
            }
            Event::Mouse(mouse) => app.on_mouse(mouse, terminal)?,
            Event::Paste(text) => app.on_paste(&text),
            // A resize can take away the pane that had focus, leaving j and k
            // moving a selection the user cannot see.
            Event::Resize(width, height) => app.on_resize(width, height),
            _ => {}
        }
        app.pump_tasks(terminal)?;
        app.pump_doctor();
        app.pump_diagnostics();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
