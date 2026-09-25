//! The full-screen shell.
//!
//! `App` owns the store and the selection state; every key press becomes an
//! [`Intent`] or a parsed [`Action`], and the handlers in [`crate::action`] do
//! the work. Rendering reads `App` and nothing else, so the panes stay
//! independently testable.

pub mod cmdline;
pub mod complete;
pub mod keys;
mod recent;
pub mod settings;
pub mod task;
pub mod view;

use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};

use crate::action::{
    self, Action, ConfirmedAction, Ctx, Effect, Kind, Line, ListenRequest, Outcome, Parsed,
    RealAi,
};
use crate::store::Store;
use cmdline::{CmdLine, CmdOutcome};
use crate::config::edit::Task;
use complete::{Completion, NoteChoice, Sources};
use task::{Job, TaskEvent};
use view::overlay::{Choice, Finder};
use view::settings::Row as SettingsRow;
use keys::{Intent, Pane};
use view::dirs::DirRow;
use view::notes::NoteRow;
use view::preview::Preview;

/// The backend bound every terminal-taking method needs. `Backend` alone is not
/// enough: `?` on a draw has to convert the backend's error into `anyhow::Error`,
/// which requires it to be a `Send + Sync` std error. Both `CrosstermBackend`
/// and `TestBackend` satisfy this, so the App can be driven by either — which is
/// what makes the event handling testable without a real terminal.
trait TuiBackend: Backend<Error: std::error::Error + Send + Sync + 'static> {}

impl<B> TuiBackend for B where B: Backend, B::Error: std::error::Error + Send + Sync + 'static {}

/// How long a status message stays before the status line goes quiet again.
const MESSAGE_TTL: Duration = Duration::from_secs(6);
/// Event-poll timeout. Short enough that a background task's progress appears
/// promptly, long enough not to spin the CPU.
const TICK: Duration = Duration::from_millis(120);

/// What the left pane lists. Directories and tags are two ways to slice the same
/// notes, and both deserve to be navigable rather than only typeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftPane {
    Dirs,
    Tags,
}

/// Which input surface is active.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    /// Typing a filter. Every keystroke narrows the notes pane.
    Filter,
    Command,
    Help,
    Confirm { prompt: String, on_yes: ConfirmedAction },
    Find,
    Settings,
}

pub struct App {
    store: Store,
    current_dir: String,
    /// Note IDs in pane order; index+1 is the number the `:` line accepts.
    numbering: Vec<String>,
    /// The live filter, when one is set. `numbering` respects it, so the numbers
    /// the user types always mean the rows the user can see.
    filter: Option<String>,
    /// Whether the left pane lists directories or tags.
    left: LeftPane,
    /// Notes recently looked at, most recent first.
    recent: recent::Recent,
    /// A running `:ask`, with the answer so far.
    asking: Option<Asking>,
    /// When the notes last changed, for the idle trigger.
    last_change: Instant,
    /// A background push, and when the last one finished.
    pushing: Option<(task::Job, view::progress::Progress, Instant)>,
    last_push: Option<Instant>,
    /// How many commits are waiting, refreshed when the notes change rather than
    /// on every frame: it costs a git process.
    unpushed: Option<usize>,
    note_sel: usize,
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
    /// The running recording, if any.
    recording: Option<Recording>,
    /// Tab-completion state, live only while cycling.
    completing: Option<Cycle>,
    /// Scroll offset for the help overlay.
    help_scroll: u16,
    finder: Option<Finder>,
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
    /// The condensed bullet stream, which is what the preview shows: a raw
    /// transcript is not readable while you are still listening.
    condensed: String,
    /// The raw rolling transcript, behind a toggle.
    raw: String,
    show_raw: bool,
}

impl App {
    pub fn new(store: Store) -> App {
        let current_dir = String::new();
        let numbering = action::numbering_for(&store, &current_dir);
        App {
            filter: None,
            left: LeftPane::Dirs,
            recent: recent::Recent::load(),
            asking: None,
            last_change: Instant::now(),
            pushing: None,
            last_push: None,
            unpushed: None,
            store,
            current_dir,
            numbering,
            note_sel: 0,
            dir_sel: 0,
            focus: Pane::Notes,
            mode: Mode::Normal,
            cmd: CmdLine::default(),
            preview_scroll: 0,
            message: None,
            pinned: None,
            recording: None,
            completing: None,
            help_scroll: 0,
            finder: None,
            settings: None,
            busy: None,
            repaint: false,
            quit: false,
        }
    }

    // ── derived view data ───────────────────────────────────────────────────

    fn dir_rows(&self) -> Vec<DirRow> {
        match self.left {
            LeftPane::Dirs => {
                view::dirs::rows(&self.current_dir, &self.store.subdirs(&self.current_dir))
            }
            LeftPane::Tags => view::dirs::tag_rows(&self.store.tags()),
        }
    }

    /// The left pane's title and empty state, which differ by what it lists.
    fn left_pane_labels(&self) -> (&'static str, view::empty::Hint) {
        match self.left {
            LeftPane::Dirs => ("dirs", view::empty::Hint::no_directories()),
            LeftPane::Tags => ("tags", view::empty::Hint::no_tags()),
        }
    }

    fn note_rows(&self) -> Vec<NoteRow> {
        let notes: Vec<&crate::notes::Note> = self
            .numbering
            .iter()
            .filter_map(|id| self.store.find_note(id))
            .collect();
        view::notes::rows(&notes)
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

    // ── automatic backup ────────────────────────────────────────────────────

    /// Record that the notes changed, restarting the quiet period.
    fn note_changed(&mut self) {
        self.last_change = Instant::now();
        // Asked once per change rather than once per frame: it is a git process.
        self.unpushed = crate::sync::unpushed(&self.store.notes_dir);
    }

    /// Start a background push when the policy says to.
    ///
    /// Called from the idle branch of the event loop, so it only ever runs when
    /// the user is not typing.
    fn maybe_auto_push(&mut self) {
        let config = crate::config::Config::load().sync;
        let when = crate::config::sync::PushWhen {
            unpushed: self.unpushed,
            quiet_for: self.last_change.elapsed(),
            since_last_push: self.last_push.map(|at| at.elapsed()),
            in_flight: self.pushing.is_some(),
        };
        if !crate::config::sync::should_push_now(&config, when) {
            return;
        }
        self.pushing = Some((
            task::start_push(self.store.notes_dir.clone()),
            view::progress::Progress::spinner("Backing up"),
            Instant::now(),
        ));
    }

    /// Push on the way out, if the policy says to and anything is waiting.
    ///
    /// Synchronous and after the alternate screen is gone: quitting should not
    /// return the prompt and then keep working invisibly, and the user is owed a
    /// line saying whether their notes made it.
    fn push_on_quit(&mut self) {
        let config = crate::config::Config::load().sync;
        // Asked fresh: the cached count is from the last change, and a background
        // push may have cleared it since.
        let unpushed = crate::sync::unpushed(&self.store.notes_dir);
        if !crate::config::sync::should_push_on_quit(&config, unpushed) {
            return;
        }

        let waiting = unpushed.unwrap_or(0);
        println!(
            "  backing up {waiting} change{}…",
            if waiting == 1 { "" } else { "s" }
        );
        match crate::sync::push(&self.store.notes_dir) {
            Ok(()) => println!("  backed up."),
            Err(e) => {
                println!("  backup failed: {e}");
                println!("  your notes are committed locally; `leo sync push` retries.");
            }
        }
    }

    /// Drain a running background push.
    fn pump_push(&mut self) {
        let Some((job, _, _)) = self.pushing.as_mut() else {
            return;
        };
        let events = job.drain();
        if events.is_empty() && !job.is_done() {
            return;
        }

        let mut done = false;
        let mut failure = None;
        for event in events {
            match event {
                TaskEvent::Pushed => done = true,
                TaskEvent::Failed(e) => failure = Some(e),
                _ => {}
            }
        }

        if done {
            self.pushing = None;
            self.last_push = Some(Instant::now());
            self.unpushed = crate::sync::unpushed(&self.store.notes_dir);
            self.say(Kind::Dim, "Backed up.");
        } else if let Some(e) = failure {
            self.pushing = None;
            // Recorded so the floor applies to failures too, or a broken remote
            // means a git process every time the loop goes quiet.
            self.last_push = Some(Instant::now());
            self.say(
                Kind::Warn,
                format!("Backup failed: {e}. Try `:sync pull` then `:sync push`."),
            );
        }
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
    fn next_visible_pane<B: TuiBackend>(
        &self,
        terminal: &Terminal<B>,
        direction: isize,
    ) -> Pane {
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
                self.pinned = None;
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
        self.recent.retain_existing(|id| store.find_note(id).is_some());
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
                self.pinned = None;
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

    /// Say the one useful thing on a first run, and nothing on every run after.
    ///
    /// One instruction is actionable where a list of seven is a chore, so this
    /// names the first gap only and stays quiet when there is nothing to fix.
    fn greet(&mut self, first_run: bool) {
        if !first_run {
            return;
        }
        let config = crate::config::Config::load();
        match crate::health::next_step(&config, crate::config::secret::default_store().as_ref()) {
            Some(step) => self.say(Kind::Warn, step),
            None => self.say(
                Kind::Good,
                "Everything is set up. Press ? for help, or : to run a command.",
            ),
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
        let config = crate::config::Config::load();
        // Screen capture and the replay hook do not use the microphone, so
        // probing it would refuse a recording that would have worked.
        let uses_microphone = !screen && std::env::var("LEO_FAKE_AUDIO").is_err();
        let checks = crate::health::recording(
            &config,
            crate::config::secret::default_store().as_ref(),
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
            if let crate::health::State::Missing { fix } = &check.state {
                for fix_line in fix.lines() {
                    lines.push(Line::dim(format!("      {}", fix_line.trim())));
                }
            }
        }
        lines.push(Line::blank());
        lines.push(Line::dim(
            "  Ctrl-S manages providers · `leo doctor` checks everything",
        ));
        Some(lines)
    }

    /// Refresh the numbering after the store or directory changed, keeping the
    /// selection in range.
    fn resync(&mut self) {
        self.numbering = match &self.filter {
            Some(query) => {
                action::filtered_numbering(&self.store, &self.current_dir, query)
            }
            None => action::numbering_for(&self.store, &self.current_dir),
        };
        if self.note_sel >= self.numbering.len() {
            self.note_sel = self.numbering.len().saturating_sub(1);
        }
        let dirs = self.dir_rows().len();
        if self.dir_sel >= dirs {
            self.dir_sel = dirs.saturating_sub(1);
        }
    }

    // ── input ───────────────────────────────────────────────────────────────

    /// Clicks and the scroll wheel.
    ///
    /// Deliberately limited to selecting and scrolling. A click cannot delete,
    /// edit or open anything: mouse input has no modifier discipline and no
    /// confirmation habit, so the safe half is the useful half. Everything here
    /// has a keyboard equivalent, and nothing here is the only way to do it.
    fn on_mouse<B: TuiBackend>(
        &mut self,
        mouse: MouseEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let area = terminal.size().map(|s| Rect::new(0, 0, s.width, s.height))?;

        // The profile page owns the whole screen when it is open, so clicks
        // belong to it. Anything else with an overlay up ignores them: a click
        // behind one would act on something the user cannot see.
        if matches!(self.mode, Mode::Settings) {
            return self.on_settings_mouse(mouse, area);
        }
        if !matches!(self.mode, Mode::Normal) {
            return Ok(());
        }

        // The same geometry that was painted: `layout` alone omits the tab row,
        // so every pane would be one line out whenever the strip is showing.
        let tabs = self.tabs();
        let frames = view::layout_with_tabs(area, !tabs.is_empty(), self.focus);
        let column = mouse.column;
        let row = mouse.row;

        let in_pane = |rect: Rect| {
            column >= rect.x
                && column < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // The tab strip: clicking a tab opens that note, which is what a
                // row of tabs is for.
                if frames.tabs.height > 0 && row == frames.tabs.y {
                    if let Some(index) = view::tabs::tab_at(&tabs, column) {
                        // `tabs` and the recent list are in the same order, and
                        // both skip notes that no longer exist.
                        let ids: Vec<String> = self
                            .recent
                            .ids()
                            .iter()
                            .filter(|id| self.store.find_note(id).is_some())
                            .cloned()
                            .collect();
                        if let Some(target) = ids.get(index).cloned() {
                            self.open_note(&target);
                        }
                    }
                    return Ok(());
                }

                if in_pane(frames.dirs) {
                    self.focus = Pane::Dirs;
                    let rows = self.dir_rows();
                    if let Some(index) =
                        view::notes::row_at(frames.dirs, row, self.dir_sel, rows.len())
                    {
                        self.dir_sel = index;
                    }
                } else if in_pane(frames.notes) {
                    self.focus = Pane::Notes;
                    let total = self.note_count();
                    if let Some(index) =
                        view::notes::row_at(frames.notes, row, self.note_sel, total)
                    {
                        self.note_sel = index;
                        // Clicking a note is opening it, as far as the recent
                        // list is concerned.
                        self.pinned = None;
                    }
                } else if in_pane(frames.preview) {
                    self.focus = Pane::Preview;
                }
                Ok(())
            }
            // The wheel acts on whatever is under the pointer, not on what has
            // focus: that is what every other application does.
            MouseEventKind::ScrollDown => {
                self.wheel(&frames, column, row, Intent::Down);
                Ok(())
            }
            MouseEventKind::ScrollUp => {
                self.wheel(&frames, column, row, Intent::Up);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Clicks and the wheel on the profile page.
    ///
    /// Selection only, like the panes: choosing a row still takes Enter, so a
    /// stray click cannot rewrite a chain or start a git repo.
    fn on_settings_mouse(&mut self, mouse: MouseEvent, area: Rect) -> Result<()> {
        let Some(screen) = self.settings.as_mut() else {
            return Ok(());
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let list = view::settings::list_area(area);
                let Some(index) = view::settings::row_at(
                    list,
                    mouse.row,
                    screen.selected,
                    screen.rows.len(),
                ) else {
                    return Ok(());
                };
                // Land on something actionable: clicking a heading should move to
                // the nearest row that does something rather than nothing.
                if screen.rows.get(index).is_some_and(|r| r.selectable()) {
                    screen.selected = index;
                }
                Ok(())
            }
            MouseEventKind::ScrollDown => {
                screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                Ok(())
            }
            MouseEventKind::ScrollUp => {
                screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Scroll whatever is under the pointer, which is not necessarily what has
    /// focus — that is what every other application does.
    fn wheel(&mut self, frames: &view::Frames, column: u16, row: u16, direction: Intent) {
        let inside = |rect: Rect| {
            column >= rect.x
                && column < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
        };

        if inside(frames.preview) {
            self.preview_scroll = match direction {
                Intent::Down => self.preview_scroll.saturating_add(1),
                _ => self.preview_scroll.saturating_sub(1),
            };
        } else if inside(frames.notes) {
            self.note_sel = step(self.note_sel, self.note_count(), direction);
            self.preview_scroll = 0;
            self.pinned = None;
        } else if inside(frames.dirs) {
            self.dir_sel = step(self.dir_sel, self.dir_rows().len(), direction);
        }
    }

    fn on_key<B: TuiBackend>(&mut self, key: event::KeyEvent, terminal: &mut Terminal<B>) -> Result<()> {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            // Filtering: every keystroke narrows the pane, so the result is
            // visible while typing rather than after committing.
            Mode::Filter => {
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
                        let empty = self
                            .filter
                            .as_ref()
                            .is_some_and(|q| q.trim().is_empty());
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
                self.pinned = None;
                Ok(())
            }

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

            Mode::Find => {
                self.mode = Mode::Find;
                self.on_find_key(key)
            }

            Mode::Settings => {
                self.mode = Mode::Settings;
                self.on_settings_key(key, terminal)
            }

            Mode::Command => {
                self.mode = Mode::Command;
                let outcome = self.cmd.key(key);
                // Any key other than Tab invalidates the candidate list.
                if outcome != CmdOutcome::Complete {
                    self.completing = None;
                }
                match outcome {
                    CmdOutcome::Editing => Ok(()),
                    CmdOutcome::Cancel => {
                        self.mode = Mode::Normal;
                        Ok(())
                    }
                    CmdOutcome::Complete => {
                        self.cycle_completion();
                        Ok(())
                    }
                    CmdOutcome::Submit(line) => {
                        self.mode = Mode::Normal;
                        self.run_line(&line, terminal)
                    }
                }
            }

            Mode::Normal => {
                self.mode = Mode::Normal;
                // While recording, a few keys mean something else: Enter and
                // Esc stop, `t` switches between the bullets and the raw text.
                if let Some(rec) = self.recording.as_mut() {
                    match key.code {
                        event::KeyCode::Enter | event::KeyCode::Esc => {
                            // Idempotent: pressing Enter again while the worker
                            // finishes must not look like a second command.
                            if !rec.job.stop_requested() {
                                rec.job.request_stop();
                                rec.progress =
                                    view::progress::Progress::spinner("Finishing the recording");
                                rec.since = Instant::now();
                                self.say(Kind::Dim, "Stopping...");
                            }
                            return Ok(());
                        }
                        event::KeyCode::Char('t') => {
                            rec.show_raw = !rec.show_raw;
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

    fn on_intent<B: TuiBackend>(&mut self, intent: Intent, terminal: &mut Terminal<B>) -> Result<()> {
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
                self.focus = self.next_visible_pane(terminal, 1);
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

            Intent::ToggleLeftPane => {
                self.left = match self.left {
                    LeftPane::Dirs => LeftPane::Tags,
                    LeftPane::Tags => LeftPane::Dirs,
                };
                self.dir_sel = 0;
                self.focus = Pane::Dirs;
                Ok(())
            }

            // Undo goes through the same handler the `:` line uses, so there is
            // one stack and one set of semantics rather than two.
            Intent::Undo => self.run_action(Action::Undo, terminal),

            Intent::ToggleCheckbox => {
                let Some(note_ref) = self.selected_ref() else {
                    return Ok(());
                };
                // Toggle the first open box, which is what `x` means with no
                // number available from a single key press.
                let index = self
                    .selected_id()
                    .and_then(|id| self.store.find_note(id))
                    .and_then(|n| first_open_checkbox(&n.body))
                    .unwrap_or(1);
                self.run_action(Action::Check { note: note_ref, index }, terminal)
            }

            Intent::EditSelected => match self.selected_ref() {
                Some(note) => self.run_action(Action::Edit { note }, terminal),
                None => Ok(()),
            },

            // `D` deletes whatever is selected, which depends on the focused
            // pane: a note in the notes pane, a whole directory in the dirs
            // pane. Both confirm first.
            Intent::DeleteSelected => match self.focus {
                Pane::Dirs => self.delete_selected_dir(terminal),
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
                self.filter = Some(String::new());
                self.mode = Mode::Filter;
                self.note_sel = 0;
                self.resync();
                Ok(())
            }

            Intent::OpenFinder => {
                self.finder = Some(Finder::open(self.all_note_choices()));
                self.mode = Mode::Find;
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

            Intent::Cancel => {
                self.pinned = None;
                self.mode = Mode::Normal;
                Ok(())
            }

            // Reload also forces a full repaint. Anything that wrote to the
            // terminal behind ratatui's back leaves its cell diff out of step
            // with the screen, and this is the one key a user will try when the
            // display looks wrong.
            Intent::Reload => {
                self.store = Store::load_from(&self.store.notes_dir.clone())?;
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
                self.pinned = None;
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
            self.say(Kind::Dim, "Move into a directory to delete it, or press h then D.");
            return Ok(());
        }
        self.run_action(
            Action::Rmdir { name: row.target.clone(), recursive: true },
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

                match self.left {
                    LeftPane::Dirs => self.run_action(Action::Cd { path: target }, terminal),
                    // Opening a tag narrows the notes pane to it, reusing the
                    // filter rather than inventing a second kind of narrowing —
                    // so Esc clears a tag the same way it clears a search.
                    LeftPane::Tags => {
                        self.filter = Some(target.clone());
                        self.note_sel = 0;
                        self.resync();
                        self.focus = Pane::Notes;
                        self.say(Kind::Dim, format!("Showing #{target}. Esc clears it."));
                        Ok(())
                    }
                }
            }
            Pane::Notes => {
                self.focus = Pane::Preview;
                Ok(())
            }
            Pane::Preview => Ok(()),
        }
    }

    // ── running actions ─────────────────────────────────────────────────────

    fn run_line<B: TuiBackend>(&mut self, line: &str, terminal: &mut Terminal<B>) -> Result<()> {
        match action::parse(line) {
            Parsed::Empty => Ok(()),
            Parsed::Usage(usage) => {
                self.say(Kind::Warn, format!("Usage: {usage}"));
                Ok(())
            }
            Parsed::Unknown(verb) => {
                self.say(Kind::Bad, format!("Unknown command: {verb}"));
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

    fn run_action<B: TuiBackend>(&mut self, action: Action, terminal: &mut Terminal<B>) -> Result<()> {
        // `ask` is the one action that can take a minute. Run it on a worker and
        // stream the answer: inline, it froze the interface with nothing to say
        // whether the model was thinking or the request had died.
        if let Action::Ask { note } = &action {
            if self.asking.is_some() {
                self.say(Kind::Warn, "Already asking — one at a time.");
                return Ok(());
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
                self.say(Kind::Dim, "No @leo prompts found in this note.");
                return Ok(());
            }

            self.asking = Some(Asking {
                job: task::start_ask(note.clone(), title, body),
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
            Ctx { current_dir: &self.current_dir, numbering: &self.numbering },
            &RealAi,
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
    fn absorb<B: TuiBackend>(&mut self, outcome: Outcome, terminal: &mut Terminal<B>) -> Result<()> {
        // Anything that changed the notes restarts the quiet period, and makes
        // the waiting-commit count worth asking for again.
        if outcome.dirty {
            self.note_changed();
        }

        if let Some(dir) = outcome.new_dir {
            self.current_dir = dir;
            self.note_sel = 0;
            self.dir_sel = 0;
            self.pinned = None;
        }

        match outcome.selection {
            Some(sel) => {
                self.numbering = sel;
                self.note_sel = 0;
            }
            None if outcome.dirty => self.resync(),
            None => {}
        }

        // Multi-line output goes to the preview; a single line is a status.
        let printable: Vec<&Line> =
            outcome.lines.iter().filter(|l| l.kind != Kind::Blank).collect();
        match printable.as_slice() {
            [] => {}
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

            Effect::ShowNote { id } => {
                // Select it in the pane if it is visible, and focus the body.
                if let Some(pos) = self.numbering.iter().position(|n| n == &id) {
                    self.note_sel = pos;
                    self.pinned = None;
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
                crate::shell::run_editor(store, req, &RealAi)
            }),

            Effect::Listen(req) => {
                if self.recording.is_some() {
                    self.say(Kind::Warn, "Already recording — press Enter to stop.");
                    return Ok(());
                }
                // Check the whole path to a finished note before recording, not
                // just the recorder. Discovering there is no transcription
                // provider *after* talking for twenty minutes is the worst way
                // to learn it.
                if let Some(lines) = self.listen_preflight(req.screen) {
                    self.pinned = Some(("not ready to record".to_string(), lines));
                    self.preview_scroll = 0;
                    return Ok(());
                }
                self.recording = Some(Recording {
                    job: task::start_listen(req.screen),
                    req,
                    progress: view::progress::Progress::spinner("Starting"),
                    since: Instant::now(),
                    condensed: String::new(),
                    raw: String::new(),
                    show_raw: false,
                });
                self.pinned = None;
                self.say(Kind::Dim, "Recording — Enter to stop, t toggles raw text.");
                Ok(())
            }

            Effect::Sync(a) => {
                let notes_dir = self.store.notes_dir.clone();
                let out = self.outside(terminal, || {
                    use crate::action::SyncAction;
                    match &a {
                        SyncAction::Init => crate::sync::init(&notes_dir),
                        SyncAction::Connect { url } => crate::sync::connect(&notes_dir, url),
                        SyncAction::Push => crate::sync::push(&notes_dir),
                        SyncAction::Pull => crate::sync::pull(&notes_dir),
                        SyncAction::Status => crate::sync::status(&notes_dir),
                    }
                })?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                } else {
                    // Pull rewrites files underneath us.
                    self.store = Store::load_from(&self.store.notes_dir.clone())?;
                    self.resync();
                    self.say(Kind::Good, "sync done.");
                }
                Ok(())
            }

            Effect::Model(a) => {
                let out = self.outside(terminal, || crate::run_model(a.clone()))?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                Ok(())
            }

            Effect::Config(a) => {
                let out = self.outside(terminal, || crate::run_config(a.clone()))?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                Ok(())
            }
        }
    }

    /// Leave the alternate screen, run `f` on the real terminal, then come
    /// back. Everything that writes to stdout or reads stdin — `$EDITOR`, git,
    /// the no-echo key prompt, the recorder — goes through here.
    fn outside<B: TuiBackend, T>(&mut self, terminal: &mut Terminal<B>, f: impl FnOnce() -> T) -> Result<T> {
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


    // ── the provider screen ─────────────────────────────────────────────────

    /// Open or rebuild the provider screen. Rows come from the config file and
    /// the keychain every time, so an edit made here or in `$EDITOR` shows up
    /// immediately rather than going stale.
    fn open_settings(&mut self, status: Option<String>) {
        let keep = self.settings.as_ref().map(|s| s.selected).unwrap_or(0);
        let cfg = crate::config::Config::load();
        let rows = settings::rows(
            &cfg,
            crate::config::secret::default_store().as_ref(),
            &self.store.notes_dir,
        );
        let selected = if keep == 0 || keep >= rows.len() {
            view::settings::first_selectable(&rows)
        } else {
            keep
        };
        self.settings = Some(SettingsScreen { rows, selected, status });
        self.mode = Mode::Settings;
    }

    fn selected_provider(&self) -> Option<(String, Task, bool)> {
        let screen = self.settings.as_ref()?;
        let row = screen.rows.get(screen.selected)?;
        let name = row.provider_name()?.to_string();
        let task = row.task()?;
        let in_chain = matches!(row, SettingsRow::Member { .. });
        Some((name, task, in_chain))
    }

    /// Perform a settings row's action.
    ///
    /// Each of these writes to the config or shells out to git, so they go
    /// through the same code the `:` line uses rather than a parallel path.
    fn run_setting<B: TuiBackend>(
        &mut self,
        action: view::settings::SettingAction,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        use view::settings::SettingAction as A;
        match action {
            A::NextAutoPush => {
                let changed = settings::cycle_auto_push()?;
                self.after_settings_change(changed);
                Ok(())
            }

            A::NextTheme => {
                let changed = settings::cycle_theme()?;
                self.after_settings_change(changed);
                // A new palette only shows after a repaint with it installed;
                // the process-wide palette is set once, so say what happened
                // rather than pretending it took effect.
                Ok(())
            }
            A::EditConfig => {
                let out = self
                    .outside(terminal, || crate::run_config(action::ConfigAction::Edit))?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                self.refresh_settings();
                Ok(())
            }
            A::SyncInit => {
                let dir = self.store.notes_dir.clone();
                match crate::sync::init(&dir) {
                    Ok(()) => {
                        self.say(Kind::Good, "Git backup started. Connect a remote next.");
                        self.refresh_settings();
                    }
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                Ok(())
            }
            A::SyncConnect { current } => {
                // The URL has to be typed, so hand over to the `:` line rather
                // than inventing a second text input on this screen. The existing
                // URL is prefilled so changing one character does not mean
                // retyping the whole thing.
                self.settings = None;
                self.mode = Mode::Command;
                match &current {
                    Some(url) => {
                        self.cmd.open(&format!("sync connect {url}"));
                        self.say(Kind::Dim, "Edit the URL, then Enter.");
                    }
                    None => {
                        self.cmd.open("sync connect ");
                        self.say(Kind::Dim, "Paste the repository URL, then Enter.");
                    }
                }
                Ok(())
            }
            A::SyncPush | A::SyncPull => {
                let notes_dir = self.store.notes_dir.clone();
                let push = matches!(action, A::SyncPush);
                let out = self.outside(terminal, || {
                    if push {
                        crate::sync::push(&notes_dir)
                    } else {
                        crate::sync::pull(&notes_dir)
                    }
                })?;
                match out {
                    Ok(()) => {
                        // A pull rewrites the notes on disk.
                        self.store = Store::load_from(&self.store.notes_dir.clone())?;
                        self.resync();
                        self.refresh_settings();
                    }
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                Ok(())
            }
        }
    }

    /// Rebuild the rows after something on the page changed.
    fn refresh_settings(&mut self) {
        if self.settings.is_some() {
            let cfg = crate::config::Config::load();
            let rows = settings::rows(
                &cfg,
                crate::config::secret::default_store().as_ref(),
                &self.store.notes_dir,
            );
            if let Some(screen) = self.settings.as_mut() {
                screen.selected = screen.selected.min(rows.len().saturating_sub(1));
                screen.rows = rows;
            }
        }
    }

    fn on_settings_key<B: TuiBackend>(
        &mut self,
        key: event::KeyEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        // Esc and Ctrl-S both close, so the key that opened it also closes it.
        let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
        if key.code == event::KeyCode::Esc || (ctrl && key.code == event::KeyCode::Char('s')) {
            self.settings = None;
            self.mode = Mode::Normal;
            return Ok(());
        }

        // A settings row: appearance, backup, or where things live.
        let selected_action = self
            .settings
            .as_ref()
            .and_then(|s| s.rows.get(s.selected))
            .and_then(|row| row.action().cloned());
        if let Some(action) = selected_action {
            if let Some(screen) = self.settings.as_mut() {
                match key.code {
                    event::KeyCode::Char('j') | event::KeyCode::Down => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                        return Ok(());
                    }
                    event::KeyCode::Char('k') | event::KeyCode::Up => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if matches!(key.code, event::KeyCode::Enter) {
                return self.run_setting(action, terminal);
            }
            return Ok(());
        }

        let Some((name, task, in_chain)) = self.selected_provider() else {
            // Nothing actionable is selected; only movement and closing apply.
            if let Some(screen) = self.settings.as_mut() {
                match key.code {
                    event::KeyCode::Char('j') | event::KeyCode::Down => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                    }
                    event::KeyCode::Char('k') | event::KeyCode::Up => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                    }
                    _ => {}
                }
            }
            return Ok(());
        };

        match key.code {
            event::KeyCode::Char('j') | event::KeyCode::Down => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                }
            }
            event::KeyCode::Char('k') | event::KeyCode::Up => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                }
            }

            // Reorder. Capital J/K, so a mistyped movement key cannot silently
            // rewrite the user's config.
            event::KeyCode::Char('J') => {
                let changed = settings::reorder(task, &name, 1)?;
                self.after_settings_change(changed);
            }
            event::KeyCode::Char('K') => {
                let changed = settings::reorder(task, &name, -1)?;
                self.after_settings_change(changed);
            }

            event::KeyCode::Char('a') if !in_chain => {
                let changed = settings::add_to_chain(task, &name)?;
                self.after_settings_change(changed);
            }
            event::KeyCode::Char('d') if in_chain => {
                let changed = settings::remove_from_chain(task, &name)?;
                self.after_settings_change(changed);
            }

            // Storing a key needs a prompt with echo disabled, which needs the
            // real terminal, so drop out of the TUI for it. `l` rather than `k`
            // because `k` moves the selection.
            event::KeyCode::Char('l') => {
                let target = name.clone();
                let out = self.outside(terminal, || {
                    crate::run_model(crate::action::ModelAction::Login { name: target })
                })?;
                let status = match out {
                    Ok(()) => format!("stored a key for {name}"),
                    Err(e) => e.to_string(),
                };
                self.open_settings(Some(status));
            }

            // Removing a key needs no prompt, so it happens in place.
            event::KeyCode::Char('x') => {
                let status = match crate::run_model(crate::action::ModelAction::Logout {
                    name: name.clone(),
                }) {
                    Ok(()) => format!("removed the key for {name}"),
                    Err(e) => e.to_string(),
                };
                self.open_settings(Some(status));
            }

            // One small request. Blocking, so say what is happening first.
            event::KeyCode::Char('t') => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.status = Some(format!("testing {name}..."));
                }
                terminal.draw(|frame| self.draw(frame))?;
                let status = match crate::test_provider(&name) {
                    Ok(report) => report,
                    Err(e) => format!("{name}: {e}"),
                };
                self.open_settings(Some(status));
            }

            event::KeyCode::Char('e') => {
                let out = self.outside(terminal, || {
                    crate::run_config(crate::action::ConfigAction::Edit)
                })?;
                let status = match out {
                    Ok(()) => None,
                    Err(e) => Some(e.to_string()),
                };
                self.open_settings(status);
            }

            _ => {}
        }
        Ok(())
    }

    /// Reload the screen after an edit, or report that nothing changed.
    fn after_settings_change(&mut self, changed: settings::Changed) {
        match changed {
            settings::Changed::Yes(message) => self.open_settings(Some(message)),
            settings::Changed::No => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.status = Some("nothing to change".to_string());
                }
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
                    self.store
                        .find_note(id)
                        .map(|n| NoteChoice { number: i + 1, title: n.title.clone() })
                })
                .collect(),
            tags: self.store.tags().into_iter().map(|(t, _)| t).collect(),
            providers: crate::config::Config::load()
                .providers
                .keys()
                .cloned()
                .collect(),
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
            self.completing = Some(Cycle { completion, typed: cycle.typed, index: next });
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
        let completion = Completion { start: completion.start, end: cursor, matches: completion.matches };
        self.completing = Some(Cycle { completion, typed, index: 0 });
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

    // ── finder ──────────────────────────────────────────────────────────────

    /// Every note in every directory, labelled with its directory so two notes
    /// sharing a title stay distinguishable.
    fn all_note_choices(&self) -> Vec<Choice> {
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

    fn on_find_key(&mut self, key: event::KeyEvent) -> Result<()> {
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
        self.pinned = None;
        self.preview_scroll = 0;
        self.focus = Pane::Notes;
    }

    /// Surface anything the layers below queued while they had no terminal.
    /// Returns true when a message arrived, so the caller can redraw.
    fn pump_diagnostics(&mut self) -> bool {
        let messages = crate::diag::drain();
        let last = messages.into_iter().next_back();
        match last {
            Some(message) => {
                self.say(Kind::Warn, message);
                true
            }
            None => false,
        }
    }

    /// Absorb whatever the worker has sent since the last tick. Returns true
    /// when something changed and a redraw is warranted.
    /// Drain the streaming `:ask` job, if one is running.
    fn pump_ask<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<bool> {
        let Some(ask) = self.asking.as_mut() else {
            return Ok(false);
        };

        let events = ask.job.drain();
        if events.is_empty() && !ask.job.is_done() {
            return Ok(false);
        }

        let mut expanded: Option<(String, String, usize)> = None;
        let mut failure: Option<String> = None;
        let mut fallbacks: Vec<String> = Vec::new();

        for event in events {
            match event {
                TaskEvent::Started { label } => {
                    ask.progress = view::progress::Progress::spinner(label);
                    ask.since = Instant::now();
                }
                TaskEvent::Streaming(text) => ask.text = text,
                TaskEvent::Expanded { note, body, count } => {
                    expanded = Some((note, body, count))
                }
                TaskEvent::ProviderFallback { from, to } => {
                    fallbacks.push(format!("{from} → {to}"))
                }
                TaskEvent::Failed(e) => failure = Some(e),
                _ => {}
            }
        }

        for note in fallbacks {
            self.say(Kind::Warn, note);
        }

        if let Some(e) = failure {
            self.asking = None;
            self.say(Kind::Bad, e);
            return Ok(true);
        }

        if let Some((note, body, count)) = expanded {
            self.asking = None;
            if count == 0 {
                self.say(Kind::Dim, "Nothing could be expanded.");
                return Ok(true);
            }
            // Written through the ordinary handler, with the answer already in
            // hand, so saving and the message are identical to the CLI path.
            let answered = PreExpanded {
                body: body.clone(),
                count,
            };
            let outcome = action::apply(
                Action::Ask { note },
                &mut self.store,
                Ctx {
                    current_dir: &self.current_dir,
                    numbering: &self.numbering,
                },
                &answered,
            )?;
            self.absorb(outcome, terminal)?;
            return Ok(true);
        }

        Ok(true)
    }

    fn pump_tasks<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<bool> {
        if self.pump_ask(terminal)? {
            return Ok(true);
        }
        let Some(rec) = self.recording.as_mut() else {
            return Ok(false);
        };

        let events = rec.job.drain();
        if events.is_empty() && !rec.job.is_done() {
            return Ok(false);
        }

        let mut finished: Option<String> = None;
        let mut structured: Option<(Option<String>, String)> = None;
        let mut failure: Option<String> = None;
        let mut fallbacks: Vec<String> = Vec::new();

        for event in events {
            match event {
                TaskEvent::Started { label } => {
                    rec.progress = view::progress::Progress::spinner(label);
                    rec.since = Instant::now();
                }
                TaskEvent::Progress { label, steps } => {
                    // Restart the clock when the kind of work changes, so the
                    // elapsed time answers "how long has this step taken".
                    if rec.progress.label != label {
                        rec.since = Instant::now();
                    }
                    rec.progress = match steps {
                        Some((done, total)) => {
                            view::progress::Progress::steps(label, done, total)
                        }
                        None => view::progress::Progress::spinner(label),
                    };
                }
                TaskEvent::Transcript(text) => rec.raw = text,
                TaskEvent::LiveNote(text) => rec.condensed = text,
                TaskEvent::ProviderFallback { from, to } => {
                    fallbacks.push(format!("{from} unavailable, using {to}"))
                }
                TaskEvent::Finished { transcript } => finished = Some(transcript),
                TaskEvent::Structured { title, body } => structured = Some((title, body)),
                TaskEvent::Failed(e) => failure = Some(e),
                // Other jobs' events; not this one's business.
                TaskEvent::Streaming(_)
                | TaskEvent::Expanded { .. }
                | TaskEvent::Pushed => {}
            }
        }

        for f in fallbacks {
            self.say(Kind::Warn, f);
        }

        if let Some(e) = failure {
            self.recording = None;
            self.say(Kind::Bad, e);
            return Ok(true);
        }

        // The recording is done; structuring is another request, so it runs on
        // its own thread and the UI keeps animating.
        if let Some(transcript) = finished {
            let rec = self.recording.take().expect("checked above");
            if transcript.trim().is_empty() {
                self.say(Kind::Dim, "No speech detected.");
                return Ok(true);
            }
            let existing = rec
                .req
                .append_to
                .as_deref()
                .and_then(|target| self.store.find_by_index_or_prefix(target))
                .map(|n| n.body.clone());
            self.recording = Some(Recording {
                job: task::start_structuring(transcript, existing),
                req: rec.req,
                progress: view::progress::Progress::spinner("Structuring notes"),
                since: Instant::now(),
                condensed: rec.condensed,
                raw: rec.raw,
                show_raw: rec.show_raw,
            });
            return Ok(true);
        }

        // Structuring finished: write the note here, on the thread that owns the
        // store.
        if let Some((title, body)) = structured {
            let rec = self.recording.take().expect("checked above");
            let ready = ReadyNote { title, body };
            match action::apply_transcript(&mut self.store, &rec.req, "ready", &ready) {
                Ok(outcome) => self.absorb(outcome, terminal)?,
                Err(e) => self.say(Kind::Bad, e.to_string()),
            }
            return Ok(true);
        }

        // The worker ended without a terminal event.
        if self.recording.as_ref().map(|r| r.job.is_done()).unwrap_or(false) {
            self.recording = None;
            self.say(Kind::Warn, "Recording ended unexpectedly.");
        }
        Ok(true)
    }

    // ── rendering ───────────────────────────────────────────────────────────

    fn draw(&self, frame: &mut Frame) {
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
                title: "answering…".to_string(),
                body: a.text.clone(),
            });
        let preview = match (streaming, &self.recording, &self.pinned, selected_note) {
            (Some(live), ..) => live,
            // A live recording owns the preview: that stream is the reason the
            // feature exists.
            (None, Some(rec), _, _) => {
                let (title, body) = if rec.show_raw {
                    ("live transcript (t for notes)", rec.raw.clone())
                } else if rec.condensed.is_empty() {
                    ("live notes (t for raw text)", "  listening...".to_string())
                } else {
                    ("live notes (t for raw text)", rec.condensed.clone())
                };
                Preview::Text { title: title.to_string(), body }
            }
            (None, None, Some((title, lines)), _) => {
                Preview::Lines { title: title.clone(), lines }
            }
            (None, None, None, Some(note)) => Preview::Note(note),
            (None, None, None, None) => Preview::Empty,
        };
        view::preview::render(
            frame,
            f.preview,
            &preview,
            self.preview_scroll,
            self.focus == Pane::Preview,
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

        match &self.mode {
            Mode::Help => view::help::render_help(frame, frame.area(), self.help_scroll),
            Mode::Confirm { prompt, .. } => {
                view::help::render_confirm(frame, frame.area(), prompt)
            }
            Mode::Find => {
                if let Some(finder) = &self.finder {
                    view::overlay::render(frame, frame.area(), finder);
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
    fn live_message(&self) -> Option<(Kind, &str)> {
        self.message.as_ref().and_then(|(kind, text, at)| {
            (at.elapsed() < MESSAGE_TTL).then_some((*kind, text.as_str()))
        })
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
    crate::diag::set_quiet(false);
    for message in crate::diag::drain() {
        eprintln!("  {message}");
    }
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
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
    crate::diag::set_quiet(true);
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen, Clear(ClearType::All))?;
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
            self.title.clone().unwrap_or_else(|| "Untitled Notes".to_string()),
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

/// The 1-based index of the first unchecked checkbox, counting every checkbox.
fn first_open_checkbox(body: &str) -> Option<usize> {
    let mut n = 0;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]") {
            n += 1;
            if t.starts_with("- [ ]") {
                return Some(n);
            }
        }
    }
    None
}

/// Run the TUI. `ratatui::init` installs a panic hook that restores the
/// terminal, so a panic cannot leave the user in raw mode.
pub fn run() -> Result<()> {
    // Nothing below the UI may write to the terminal while the panes own it:
    // a stray line lands on top of them and stays until the next full repaint.
    crate::diag::set_quiet(true);
    // Before the first frame, so nothing is painted in the wrong colours.
    view::theme::init(crate::config::Config::load().theme.palette());
    let mut store = Store::load()?;
    // A first run explains itself: the manual is a real note the user can
    // search, scroll, and delete. A failure here must not stop the app.
    let installed_manual = crate::manual::install_if_absent(&mut store)
        .unwrap_or(None)
        .is_some();
    let mut terminal = ratatui::init();
    // Mouse reporting is opt-in per terminal. Failing to enable it is not fatal:
    // every key still works, which is how leo is mostly driven.
    let mouse = execute!(std::io::stdout(), EnableMouseCapture).is_ok();
    let mut app = App::new(store);
    // The note on screen at startup has been looked at, so it belongs in the
    // recent list. Without this the first Tab has only one entry — the note the
    // user is already on — and answers "only this note has been visited".
    app.remember_visit();
    app.greet(installed_manual);
    let result = event_loop(&mut terminal, &mut app);
    // Persist the recent list so the strip survives a restart, which is the
    // difference between a convenience and a novelty.
    app.recent.save();
    if mouse {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    crate::diag::set_quiet(false);
    // After the screen is handed back, so the push can say what it is doing on
    // an ordinary terminal rather than painting over the panes on the way out.
    app.push_on_quit();
    // Anything queued but never shown dies with the screen it belonged to.
    crate::diag::clear();
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
            app.pump_tasks(terminal)?;
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
            // A resize can take away the pane that had focus, leaving j and k
            // moving a selection the user cannot see.
            Event::Resize(width, height) => app.on_resize(width, height),
            _ => {}
        }
        app.pump_tasks(terminal)?;
        app.pump_diagnostics();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        store.create_dir("cs130");
        store
            .create_note("Rust ownership", "- [ ] read\n- [x] done", vec!["rust".to_string()], "")
            .unwrap();
        store.create_note("Graph traversals", "- BFS", vec![], "").unwrap();
        store.create_note("Nested note", "body", vec![], "cs130").unwrap();
        store.save().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (App::new(store), dir)
    }

    /// Notes are sorted newest-first, so find one by title rather than index.
    fn select_titled(app: &mut App, title: &str) {
        let pos = app
            .numbering
            .iter()
            .position(|id| {
                app.store.find_note(id).map(|n| n.title == title).unwrap_or(false)
            })
            .expect("note is in the current listing");
        app.note_sel = pos;
    }

    #[test]
    fn tab_completes_a_verb_on_the_command_line() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");

        app.cycle_completion();
        assert_eq!(app.cmd.text(), "view");
        assert_eq!(app.cmd.cursor(), 4);
    }

    #[test]
    fn tab_completes_a_note_reference_to_its_number() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("view owner");

        // Notes list newest-first, so derive the expected number rather than
        // assuming creation order.
        let expected = app
            .numbering
            .iter()
            .position(|id| {
                app.store.find_note(id).map(|n| n.title == "Rust ownership").unwrap_or(false)
            })
            .map(|i| i + 1)
            .unwrap();

        app.cycle_completion();
        // Only the number is a valid argument; the title was just for matching.
        assert_eq!(app.cmd.text(), format!("view {expected}"));
    }

    #[test]
    fn tab_cycles_through_candidates_and_back_to_what_was_typed() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("s");

        app.cycle_completion();
        let first = app.cmd.text().to_string();
        app.cycle_completion();
        let second = app.cmd.text().to_string();
        assert_ne!(first, second, "a second Tab must advance");

        // Walking off the end restores the original text rather than trapping
        // the user in the candidate list.
        let mut guard = 0;
        while app.cmd.text() != "s" && guard < 50 {
            app.cycle_completion();
            guard += 1;
        }
        assert_eq!(app.cmd.text(), "s");
    }

    #[test]
    fn a_keystroke_after_tab_abandons_the_candidate_list() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");
        app.cycle_completion();
        assert!(app.completing.is_some());

        // Feeding any non-Tab key through the command-mode path clears the
        // cycle, so a later Tab re-derives candidates from the new text.
        let key = event::KeyEvent::new(event::KeyCode::Char('x'), event::KeyModifiers::NONE);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        app.mode = Mode::Command;
        app.on_key(key, &mut terminal).unwrap();
        assert!(app.completing.is_none());
        assert_eq!(app.cmd.text(), "viewx");
    }

    #[test]
    fn the_ghost_hint_shows_the_rest_of_the_top_match() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");
        assert_eq!(app.ghost().as_deref(), Some("w"));

        // Not shown once cycling has started: the line already holds the match.
        app.cycle_completion();
        assert_eq!(app.ghost(), None);
    }

    #[test]
    fn no_ghost_hint_outside_the_command_line() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Normal;
        app.cmd.open("vie");
        assert_eq!(app.ghost(), None);
    }

    #[test]
    fn the_frame_renders_the_completed_command_with_its_hint() {
        let (mut app, _d) = temp_app();
        app.mode = Mode::Command;
        app.cmd.open("vie");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains(":view"), "ghost hint is not rendered:\n{out}");
    }

    #[test]
    fn the_finder_lists_notes_from_every_directory() {
        let (app, _d) = temp_app();
        let choices = app.all_note_choices();
        let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Rust ownership"), "{labels:?}");
        // A note outside the current directory is labelled with its path.
        assert!(labels.contains(&"cs130/Nested note"), "{labels:?}");
    }

    #[test]
    fn jumping_to_a_note_follows_it_into_its_directory() {
        let (mut app, _d) = temp_app();
        let nested = app
            .store
            .list_notes(None, 100)
            .iter()
            .find(|n| n.title == "Nested note")
            .map(|n| n.id.clone())
            .unwrap();

        assert_eq!(app.current_dir, "");
        app.jump_to(&nested);

        assert_eq!(app.current_dir, "cs130");
        assert_eq!(app.selected_id(), Some(&nested), "the note is selected");
        assert_eq!(app.focus, Pane::Notes);
    }

    #[test]
    fn jumping_to_a_note_in_the_current_directory_only_moves_the_selection() {
        let (mut app, _d) = temp_app();
        let id = app
            .store
            .list_notes(None, 100)
            .iter()
            .find(|n| n.title == "Rust ownership")
            .map(|n| n.id.clone())
            .unwrap();
        app.jump_to(&id);
        assert_eq!(app.current_dir, "");
        assert_eq!(app.selected_id(), Some(&id));
    }

    #[test]
    fn completion_sources_come_from_the_current_directory_and_store() {
        let (app, _d) = temp_app();
        let s = app.sources();
        assert!(s.dirs.contains(&"cs130".to_string()));
        assert!(s.tags.contains(&"rust".to_string()));
        // Only notes in the current listing are numbered.
        assert_eq!(s.notes.len(), 2);
        assert!(s.notes.iter().any(|n| n.title == "Rust ownership"));
    }

    #[test]
    fn x_toggles_the_first_open_checkbox_of_the_selected_note() {
        let (mut app, _d) = temp_app();
        select_titled(&mut app, "Rust ownership");
        let id = app.selected_id().cloned().unwrap();

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        app.on_intent(Intent::ToggleCheckbox, &mut terminal).unwrap();

        assert!(
            app.store.find_note(&id).unwrap().body.contains("- [x] read"),
            "body: {}",
            app.store.find_note(&id).unwrap().body
        );
    }

    /// The bug this guards: work below the UI printed to stdout while the panes
    /// owned the screen, so git's commit summary and config warnings landed on
    /// top of the notes list. They now arrive as status-line messages instead.
    #[test]
    fn a_background_warning_becomes_a_status_message_not_terminal_output() {
        let (mut app, _d) = temp_app();
        crate::diag::set_quiet(true);
        crate::diag::clear();

        crate::diag::warn("could not read the stored credential for \"groq\"");
        assert!(app.pump_diagnostics(), "the warning was not picked up");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 12)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let out = terminal.backend().to_string();
        assert!(
            out.contains("could not read the stored credential"),
            "the warning never reached the status line:\n{out}"
        );

        crate::diag::set_quiet(false);
        crate::diag::clear();
    }

    #[test]
    fn pumping_with_nothing_queued_reports_no_change() {
        let (mut app, _d) = temp_app();
        crate::diag::set_quiet(true);
        crate::diag::clear();
        assert!(!app.pump_diagnostics());
        crate::diag::set_quiet(false);
    }

    /// `D` means "delete what is selected", so which pane has focus decides
    /// whether that is a note or a whole directory.
    #[test]
    fn d_in_the_dirs_pane_asks_to_delete_the_directory() {
        let (mut app, _d) = temp_app();
        app.focus = Pane::Dirs;
        // temp_app builds cs130 with one note in it.
        app.dir_sel = 0;

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
        app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();

        match &app.mode {
            Mode::Confirm { prompt, on_yes } => {
                assert!(prompt.contains("cs130/"), "prompt: {prompt}");
                assert!(prompt.contains("1 note"), "prompt: {prompt}");
                assert_eq!(
                    *on_yes,
                    crate::action::ConfirmedAction::DeleteDir { path: "cs130".to_string() }
                );
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
        // Still there until confirmed.
        assert!(app.store.dir_exists("cs130"));
    }

    #[test]
    fn confirming_in_the_dirs_pane_removes_the_directory_and_its_notes() {
        let (mut app, _d) = temp_app();
        app.focus = Pane::Dirs;
        app.dir_sel = 0;
        let notes_before = app.store.notes.len();

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
        app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();
        let yes = event::KeyEvent::new(event::KeyCode::Char('y'), event::KeyModifiers::NONE);
        app.on_key(yes, &mut terminal).unwrap();

        assert!(!app.store.dir_exists("cs130"));
        assert_eq!(app.store.notes.len(), notes_before - 1);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn declining_the_confirmation_keeps_the_directory() {
        let (mut app, _d) = temp_app();
        app.focus = Pane::Dirs;
        app.dir_sel = 0;
        let notes_before = app.store.notes.len();

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
        app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();
        let no = event::KeyEvent::new(event::KeyCode::Char('n'), event::KeyModifiers::NONE);
        app.on_key(no, &mut terminal).unwrap();

        assert!(app.store.dir_exists("cs130"));
        assert_eq!(app.store.notes.len(), notes_before);
    }

    /// `..` is navigation, not a directory to destroy.
    #[test]
    fn d_on_the_parent_entry_deletes_nothing() {
        let (mut app, _d) = temp_app();
        app.current_dir = "cs130".to_string();
        app.focus = Pane::Dirs;
        app.dir_sel = 0; // the ".." row
        assert_eq!(app.dir_rows()[0].target, "..");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
        app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();

        assert_eq!(app.mode, Mode::Normal, "no confirmation was raised");
        assert!(app.store.dir_exists("cs130"));
    }

    /// The notes pane keeps its old meaning.
    #[test]
    fn d_in_the_notes_pane_still_targets_a_note() {
        let (mut app, _d) = temp_app();
        app.focus = Pane::Notes;
        select_titled(&mut app, "Rust ownership");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
        app.on_intent(Intent::DeleteSelected, &mut terminal).unwrap();

        match &app.mode {
            Mode::Confirm { on_yes, .. } => assert!(matches!(
                on_yes,
                crate::action::ConfirmedAction::DeleteNote { .. }
            )),
            other => panic!("expected a note confirmation, got {other:?}"),
        }
    }

    /// The finished text is written through the same seam a live model would
    /// use, so titles, tags and appending behave identically.
    #[test]
    fn a_ready_note_is_written_through_the_normal_path() {
        let (mut app, _d) = temp_app();
        let before = app.store.notes.len();
        let req = crate::action::ListenRequest {
            screen: false,
            title: None,
            append_to: None,
            dir: String::new(),
        };
        let ready = ReadyNote {
            title: Some("Lecture 4".to_string()),
            body: "- a point".to_string(),
        };

        let outcome =
            action::apply_transcript(&mut app.store, &req, "ready", &ready).unwrap();
        assert!(outcome.dirty);
        assert_eq!(app.store.notes.len(), before + 1);
        let note = app.store.find_by_title("Lecture 4").first().copied().unwrap();
        assert_eq!(note.body, "- a point");
        assert_eq!(note.tags, vec!["listen"]);
    }

    #[test]
    fn a_ready_note_without_a_title_still_saves() {
        let (mut app, _d) = temp_app();
        let req = crate::action::ListenRequest {
            screen: false,
            title: None,
            append_to: None,
            dir: String::new(),
        };
        let ready = ReadyNote { title: None, body: "- body".to_string() };
        action::apply_transcript(&mut app.store, &req, "ready", &ready).unwrap();
        assert_eq!(app.store.find_by_title("Untitled Notes").len(), 1);
    }

    /// A user-supplied title still wins over whatever the model produced.
    #[test]
    fn a_ready_note_respects_a_title_the_user_chose() {
        let (mut app, _d) = temp_app();
        let req = crate::action::ListenRequest {
            screen: false,
            title: Some("My Title".to_string()),
            append_to: None,
            dir: String::new(),
        };
        let ready = ReadyNote {
            title: Some("Model Title".to_string()),
            body: "- body".to_string(),
        };
        action::apply_transcript(&mut app.store, &req, "ready", &ready).unwrap();
        assert_eq!(app.store.find_by_title("My Title").len(), 1);
        assert!(app.store.find_by_title("Model Title").is_empty());
    }

    /// Waiting must look like waiting: a spinner and a clock for unknown work,
    /// a real bar when the step count is known.
    /// A retired name must explain itself in ONE message: the status line holds
    /// a single one, so a two-part explanation loses its first half.
    #[test]
    fn a_retired_command_explains_itself_in_one_message() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 14)).unwrap();

        app.run_line("d 1", &mut terminal).unwrap();
        let (kind, text, _) = app.message.as_ref().expect("a message");
        assert_eq!(*kind, Kind::Warn);
        assert!(text.contains("`d`"), "does not name the old command: {text}");
        assert!(text.contains(":delete"), "does not name the replacement: {text}");

        app.run_line("env", &mut terminal).unwrap();
        let (_, text, _) = app.message.as_ref().expect("a message");
        assert!(text.contains("model login"), "{text}");
        assert!(text.contains("keychain"), "does not say why: {text}");
    }

    // ── streaming ask ───────────────────────────────────────────────────────

    /// The bug this guards: the write-back used ReadyNote, whose expand_prompts
    /// echoes its input, so the answer was streamed to the screen and then thrown
    /// away when the note was saved.
    #[test]
    fn an_answer_is_written_back_and_not_echoed() {
        let expanded = PreExpanded {
            body: "the answer".to_string(),
            count: 1,
        };
        let (body, count) = action::Ai::expand_prompts(&expanded, "@leo question", "T").unwrap();
        assert_eq!(body, "the answer", "the original body was returned instead");
        assert_eq!(count, 1);

        // And the listen path's type still echoes, which is what it is for.
        let ready = ReadyNote {
            title: None,
            body: "structured".to_string(),
        };
        let (body, _) = action::Ai::expand_prompts(&ready, "unchanged", "T").unwrap();
        assert_eq!(body, "unchanged");
    }

    /// A note with no prompts must not start a job at all.
    #[test]
    fn asking_a_note_without_prompts_starts_nothing() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        let id = app.selected_id().cloned().unwrap();
        assert!(
            !app.store.find_note(&id).unwrap().body.contains("@leo"),
            "fixture note should have no prompts"
        );

        app.run_action(Action::Ask { note: "1".to_string() }, &mut terminal)
            .unwrap();
        assert!(app.asking.is_none(), "a job was started with nothing to ask");
        let (_, message, _) = app.message.as_ref().expect("a message");
        assert!(message.contains("No @leo prompts"), "{message}");
    }

    /// Two asks at once would race to write the same note.
    #[test]
    fn a_second_ask_is_refused_while_one_is_running() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        // Stand in for a running job without making a request.
        app.asking = Some(Asking {
            job: task::start_ask(String::new(), String::new(), String::new()),
            progress: view::progress::Progress::spinner("Asking"),
            since: Instant::now(),
            text: String::new(),
        });

        app.run_action(Action::Ask { note: "1".to_string() }, &mut terminal)
            .unwrap();
        let (_, message, _) = app.message.as_ref().expect("a message");
        assert!(message.contains("one at a time"), "{message}");
    }

    /// Text arriving must show in the preview, or streaming is invisible.
    #[test]
    fn a_streaming_answer_appears_in_the_preview() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.asking = Some(Asking {
            job: task::start_ask(String::new(), String::new(), String::new()),
            progress: view::progress::Progress::spinner("Asking"),
            since: Instant::now(),
            text: "ownership means".to_string(),
        });

        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("ownership means"), "{out}");
        assert!(out.contains("answering"), "no indication it is still arriving: {out}");
    }

    // ── recent notes ────────────────────────────────────────────────────────

    /// Moving the selection is visiting a note, and the strip must show it.
    #[test]
    fn visiting_notes_builds_the_recent_strip() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        assert!(app.note_count() >= 2);

        app.on_intent(Intent::Down, &mut terminal).unwrap();
        app.on_intent(Intent::Up, &mut terminal).unwrap();

        let tabs = app.tabs();
        assert_eq!(tabs.len(), 2, "both visited notes should be listed");
        // The note on screen is the current tab, and it is first.
        assert!(tabs[0].current, "the current note is not marked");

        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        let title = app
            .store
            .find_note(app.selected_id().unwrap())
            .unwrap()
            .title
            .clone();
        assert!(out.contains(title.split(' ').next().unwrap()), "{out}");
    }

    /// Tab returns to the previous note, which is the whole point of the list.
    #[test]
    fn tab_jumps_back_to_the_previous_note() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.remember_visit();
        let first = app.selected_id().cloned().unwrap();
        app.on_intent(Intent::Down, &mut terminal).unwrap();
        let second = app.selected_id().cloned().unwrap();
        assert_ne!(first, second);

        app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
        assert_eq!(app.selected_id(), Some(&first), "Tab did not go back");

        // And again returns to where we came from.
        app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
        assert_eq!(app.selected_id(), Some(&second));
    }

    /// The note on screen at startup counts as visited, or the first Tab has
    /// only the current note to offer and refuses.
    #[test]
    fn the_note_on_screen_at_startup_is_recorded_as_visited() {
        let (mut app, _d) = temp_app();
        app.recent = crate::tui::recent::Recent::default();

        app.remember_visit();
        assert_eq!(app.recent.ids().len(), 1);

        // So after moving once, Tab has somewhere to go back to.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        let first = app.selected_id().cloned().unwrap();
        app.on_intent(Intent::Down, &mut terminal).unwrap();
        app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
        assert_eq!(app.selected_id(), Some(&first));
    }

    #[test]
    fn tab_with_nothing_visited_says_so_rather_than_doing_nothing() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        app.recent = crate::tui::recent::Recent::default();

        app.on_intent(Intent::JumpRecent, &mut terminal).unwrap();
        let (_, message, _) = app.message.as_ref().expect("a message");
        assert!(message.contains("No notes visited"), "{message}");
    }

    /// A deleted note must not linger in the strip as a row that does nothing.
    #[test]
    fn a_deleted_note_leaves_the_recent_strip() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.remember_visit();
        let id = app.selected_id().cloned().unwrap();
        app.on_intent(Intent::Down, &mut terminal).unwrap();
        assert_eq!(app.tabs().len(), 2);

        app.store.delete_note(&id);
        app.resync();
        assert_eq!(app.tabs().len(), 1, "the deleted note is still listed");
    }

    /// The strip must not take a row when it is empty.
    #[test]
    fn the_strip_costs_no_space_until_a_note_is_visited() {
        let (mut app, _d) = temp_app();
        app.recent = crate::tui::recent::Recent::default();

        let with_none = view::layout_with_tabs(Rect::new(0, 0, 80, 20), false, Pane::Notes);
        let with_some = view::layout_with_tabs(Rect::new(0, 0, 80, 20), true, Pane::Notes);
        assert_eq!(with_none.tabs.height, 0);
        assert_eq!(with_some.tabs.height, 1);
        // And the panes get the row back.
        assert!(with_none.dirs.height > with_some.dirs.height);
    }

    // ── tags ────────────────────────────────────────────────────────────────

    /// The left pane has one column and two things to show, so it toggles.
    #[test]
    fn t_switches_the_left_pane_between_directories_and_tags() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        terminal.draw(|f| app.draw(f)).unwrap();
        assert!(terminal.backend().to_string().contains("dirs"));

        app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("tags"), "{out}");
        // The fixture tags a note "rust", so the tag and its count are listed.
        assert!(out.contains("#rust"), "{out}");

        app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        assert!(terminal.backend().to_string().contains("dirs"));
    }

    /// Opening a tag narrows the notes pane, through the same filter a search
    /// uses — so Esc clears a tag the same way it clears a search.
    #[test]
    fn opening_a_tag_filters_the_notes_pane() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        let all = app.note_count();

        app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
        app.on_intent(Intent::Open, &mut terminal).unwrap();

        assert_eq!(app.filter.as_deref(), Some("rust"));
        assert!(app.note_count() < all, "the tag did not narrow anything");
        assert_eq!(app.focus, Pane::Notes, "focus should follow the notes");

        // Every listed note actually carries the tag.
        for id in &app.numbering {
            let note = app.store.find_note(id).unwrap();
            assert!(note.tags.iter().any(|t| t == "rust"), "{:?}", note.tags);
        }
    }

    #[test]
    fn toggling_to_tags_resets_the_selection_so_it_cannot_dangle() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.dir_sel = 5;
        app.on_intent(Intent::ToggleLeftPane, &mut terminal).unwrap();
        assert_eq!(app.dir_sel, 0);
        // And drawing with the new listing does not panic.
        terminal.draw(|f| app.draw(f)).unwrap();
    }

    // ── filtering ───────────────────────────────────────────────────────────

    fn press(c: char) -> event::KeyEvent {
        event::KeyEvent::new(event::KeyCode::Char(c), event::KeyModifiers::NONE)
    }

    fn press_code(code: event::KeyCode) -> event::KeyEvent {
        event::KeyEvent::new(code, event::KeyModifiers::NONE)
    }

    /// The pane must narrow while typing, not after committing.
    #[test]
    fn typing_a_filter_narrows_the_pane_on_every_keystroke() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        let all = app.note_count();
        assert!(all >= 2, "fixture needs several notes");

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        assert_eq!(app.note_count(), all, "an empty filter hides nothing");

        // "Rust ownership" is in the fixture; "Graph traversals" is not a match.
        for c in "own".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        assert_eq!(app.note_count(), 1, "filter did not narrow the pane");
        let id = app.selected_id().cloned().unwrap();
        assert!(app.store.find_note(&id).unwrap().title.contains("ownership"));
    }

    /// The numbers the user types must mean the rows the user sees. If numbering
    /// ignored the filter, `:delete 1` would delete something else.
    #[test]
    fn numbering_follows_the_filter() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        for c in "own".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        assert_eq!(app.numbering.len(), 1);

        let visible = app.store.find_note(&app.numbering[0]).unwrap().title.clone();
        assert!(visible.contains("ownership"), "{visible}");
    }

    #[test]
    fn backspace_widens_the_filter_again() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        let all = app.note_count();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        for c in "own".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        assert_eq!(app.note_count(), 1);

        for _ in 0..3 {
            app.on_key(press_code(event::KeyCode::Backspace), &mut terminal)
                .unwrap();
        }
        assert_eq!(app.note_count(), all, "backspacing did not restore the list");
    }

    /// Esc is the only way back to the full list without deleting each character.
    #[test]
    fn esc_abandons_the_filter() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        let all = app.note_count();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        for c in "own".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        app.on_key(press_code(event::KeyCode::Esc), &mut terminal).unwrap();

        assert!(app.filter.is_none(), "the filter survived Esc");
        assert_eq!(app.note_count(), all);
        assert!(matches!(app.mode, Mode::Normal));
    }

    /// Enter keeps the filter but hands the keyboard back, so j/k and D act on
    /// what is shown.
    #[test]
    fn enter_keeps_the_filter_and_returns_to_the_panes() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        for c in "own".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();

        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.filter.as_deref(), Some("own"));
        assert_eq!(app.note_count(), 1);
    }

    /// Committing an empty filter should leave no filter at all, rather than an
    /// invisible one that quietly changes the pane title.
    #[test]
    fn committing_an_empty_filter_clears_it() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        app.on_key(press_code(event::KeyCode::Enter), &mut terminal).unwrap();
        assert!(app.filter.is_none());
    }

    /// A filter matching nothing must say so, and say how to get out.
    #[test]
    fn a_filter_that_matches_nothing_says_so() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        for c in "zzzz".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        assert_eq!(app.note_count(), 0);

        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("zzzz"), "the query is not shown: {out}");
        assert!(out.contains("Esc to clear"), "{out}");
    }

    /// Case must not matter, or the filter is a guessing game.
    #[test]
    fn filtering_ignores_case() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();

        app.on_intent(Intent::OpenFilter, &mut terminal).unwrap();
        for c in "OWNER".chars() {
            app.on_key(press(c), &mut terminal).unwrap();
        }
        assert_eq!(app.note_count(), 1);
    }

    // ── mouse ───────────────────────────────────────────────────────────────

    /// The geometry the app is actually painting, which depends on whether the
    /// tab strip is showing. Computing it any other way in a test is how the
    /// off-by-one row bug went unnoticed.
    fn frames_for(app: &App, width: u16, height: u16) -> view::Frames {
        view::layout_with_tabs(
            Rect::new(0, 0, width, height),
            !app.tabs().is_empty(),
            app.focus,
        )
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        }
    }

    fn wheel_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: ratatui::crossterm::event::KeyModifiers::NONE,
        }
    }

    /// Clicking a pane focuses it, so the keyboard picks up where the mouse left
    /// off rather than acting on a different pane than the one just clicked.
    #[test]
    fn clicking_a_pane_focuses_it() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = frames_for(&app, 100, 20);

        app.on_mouse(click(frames.dirs.x + 2, frames.dirs.y + 1), &mut terminal)
            .unwrap();
        assert_eq!(app.focus, Pane::Dirs);

        app.on_mouse(click(frames.preview.x + 2, frames.preview.y + 1), &mut terminal)
            .unwrap();
        assert_eq!(app.focus, Pane::Preview);

        app.on_mouse(click(frames.notes.x + 2, frames.notes.y + 1), &mut terminal)
            .unwrap();
        assert_eq!(app.focus, Pane::Notes);
    }

    #[test]
    fn clicking_a_note_selects_that_note() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = frames_for(&app, 100, 20);
        assert!(app.note_count() >= 2, "fixture needs two notes");

        // The second row inside the pane is the second note.
        app.on_mouse(click(frames.notes.x + 3, frames.notes.y + 2), &mut terminal)
            .unwrap();
        assert_eq!(app.note_sel, 1);

        // And back to the first.
        app.on_mouse(click(frames.notes.x + 3, frames.notes.y + 1), &mut terminal)
            .unwrap();
        assert_eq!(app.note_sel, 0);
    }

    /// A click on a border must not move the selection.
    #[test]
    fn clicking_a_border_leaves_the_selection_alone() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = frames_for(&app, 100, 20);

        app.note_sel = 1;
        app.on_mouse(click(frames.notes.x + 3, frames.notes.y), &mut terminal)
            .unwrap();
        assert_eq!(app.note_sel, 1, "the border moved the selection");
    }

    /// The wheel acts on what is under the pointer, not on what has focus.
    #[test]
    fn the_wheel_scrolls_the_pane_under_the_pointer() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = frames_for(&app, 100, 20);

        // Focus is on the notes pane; the pointer is over the preview.
        app.focus = Pane::Notes;
        let before = app.note_sel;
        app.on_mouse(
            wheel_event(MouseEventKind::ScrollDown, frames.preview.x + 2, frames.preview.y + 2),
            &mut terminal,
        )
        .unwrap();
        assert_eq!(app.preview_scroll, 1, "the preview did not scroll");
        assert_eq!(app.note_sel, before, "the wheel moved the wrong pane");

        // Over the notes pane, it moves the selection.
        app.on_mouse(
            wheel_event(MouseEventKind::ScrollDown, frames.notes.x + 2, frames.notes.y + 2),
            &mut terminal,
        )
        .unwrap();
        assert_eq!(app.note_sel, before + 1);
    }

    #[test]
    fn scrolling_up_at_the_top_stays_put() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = frames_for(&app, 100, 20);

        app.on_mouse(
            wheel_event(MouseEventKind::ScrollUp, frames.preview.x + 2, frames.preview.y + 2),
            &mut terminal,
        )
        .unwrap();
        assert_eq!(app.preview_scroll, 0);
    }

    /// The profile page owns the screen when it is open, so clicks belong to it.
    /// Ignoring them made the page look broken to anyone who reached for the
    /// mouse.
    #[test]
    fn clicking_a_row_on_the_profile_page_selects_it() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

        app.on_intent(Intent::OpenSettings, &mut terminal).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let before = app.settings.as_ref().unwrap().selected;

        // Aim at the next row that does something, wherever that is.
        let area = Rect::new(0, 0, 100, 30);
        let list = view::settings::list_area(area);
        let target = view::settings::step(&app.settings.as_ref().unwrap().rows, before, 1);
        assert_ne!(target, before, "fixture has only one selectable row");
        app.on_mouse(click(list.x + 4, list.y + target as u16), &mut terminal)
            .unwrap();

        let after = app.settings.as_ref().unwrap().selected;
        assert_ne!(after, before, "the click did not move the selection");
        assert!(
            app.settings.as_ref().unwrap().rows[after].selectable(),
            "the click landed on a row that does nothing"
        );
    }

    #[test]
    fn the_wheel_moves_the_profile_selection() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

        app.on_intent(Intent::OpenSettings, &mut terminal).unwrap();
        let before = app.settings.as_ref().unwrap().selected;
        app.on_mouse(wheel_event(MouseEventKind::ScrollDown, 50, 10), &mut terminal)
            .unwrap();
        assert!(app.settings.as_ref().unwrap().selected > before);
    }

    /// A row of tabs the user cannot click is not really a row of tabs.
    #[test]
    fn clicking_a_tab_opens_that_note() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();

        // Visit two notes so the strip has two tabs.
        app.remember_visit();
        let first = app.selected_id().cloned().unwrap();
        app.on_intent(Intent::Down, &mut terminal).unwrap();
        let second = app.selected_id().cloned().unwrap();
        assert_eq!(app.tabs().len(), 2);

        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = view::layout_with_tabs(Rect::new(0, 0, 100, 20), true, Pane::Notes);

        // The second tab is the note we came from; click it.
        let tabs = app.tabs();
        let column = {
            let first_label = tabs[0].title.chars().count().min(18) + 2;
            (first_label + 2) as u16
        };
        app.on_mouse(click(column, frames.tabs.y), &mut terminal).unwrap();

        assert_eq!(
            app.selected_id(),
            Some(&first),
            "clicking the second tab did not open that note"
        );
        assert_ne!(app.selected_id(), Some(&second));
    }

    // ── automatic backup ────────────────────────────────────────────────────

    /// Editing must restart the quiet period, or an idle push could fire in the
    /// middle of a burst of writing.
    #[test]
    fn changing_a_note_restarts_the_quiet_period() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();

        // Pretend the notes have been quiet for a while.
        app.last_change = Instant::now() - std::time::Duration::from_secs(600);
        assert!(app.last_change.elapsed() > std::time::Duration::from_secs(300));

        // Any change resets it.
        app.run_action(Action::Undo, &mut terminal).unwrap();
        let id = app.selected_id().cloned().unwrap();
        app.store.toggle_checkbox(&id, 1);
        app.note_changed();
        assert!(app.last_change.elapsed() < std::time::Duration::from_secs(5));
    }

    /// Nothing waiting must not start a push, or a quiet session runs git every
    /// time the loop idles.
    #[test]
    fn no_push_is_started_when_nothing_is_waiting() {
        let (mut app, _d) = temp_app();
        app.unpushed = Some(0);
        app.last_change = Instant::now() - std::time::Duration::from_secs(600);
        app.maybe_auto_push();
        assert!(app.pushing.is_none());

        // Nor when there is no upstream at all, where a push would only fail.
        app.unpushed = None;
        app.maybe_auto_push();
        assert!(app.pushing.is_none());
    }

    /// The store this fixture uses has no git repo, so the default policy must
    /// leave it alone entirely.
    #[test]
    fn a_store_without_a_repo_is_never_pushed() {
        let (mut app, _d) = temp_app();
        app.note_changed();
        assert_eq!(app.unpushed, None, "a store with no repo reported a count");
        app.last_change = Instant::now() - std::time::Duration::from_secs(600);
        app.maybe_auto_push();
        assert!(app.pushing.is_none());

        // And quitting does nothing rather than erroring.
        app.push_on_quit();
    }

    // ── responsive layout ───────────────────────────────────────────────────

    /// Narrowing the terminal must not leave focus on a pane that is gone: j and
    /// k would move a selection the user cannot see.
    #[test]
    fn a_resize_moves_focus_off_a_pane_that_disappeared() {
        let (mut app, _d) = temp_app();
        app.focus = Pane::Dirs;

        // Wide enough for three panes: the directories pane is real.
        app.on_resize(120, 30);
        assert_eq!(app.focus, Pane::Dirs);

        // Narrow enough to drop it.
        app.on_resize(70, 24);
        assert_eq!(app.focus, Pane::Notes, "focus stayed on a hidden pane");
    }

    /// In the one-pane shape every pane is reachable, so focus is never moved out
    /// from under the user.
    #[test]
    fn a_resize_to_one_pane_leaves_focus_alone() {
        let (mut app, _d) = temp_app();
        app.focus = Pane::Preview;
        app.on_resize(40, 20);
        assert_eq!(app.focus, Pane::Preview);
    }

    /// `h` and `l` must not stop on a pane that is not drawn.
    #[test]
    fn focus_movement_skips_hidden_panes() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 24)).unwrap();

        // Two-pane shape: notes and preview only.
        app.focus = Pane::Notes;
        app.on_intent(Intent::FocusLeft, &mut terminal).unwrap();
        assert_eq!(app.focus, Pane::Notes, "focus moved onto the hidden dirs pane");

        app.on_intent(Intent::FocusRight, &mut terminal).unwrap();
        assert_eq!(app.focus, Pane::Preview);
    }

    /// At the narrowest size every pane is still reachable, which is what makes
    /// the one-pane shape usable rather than merely small.
    #[test]
    fn every_pane_is_reachable_on_a_narrow_terminal() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 20)).unwrap();

        app.focus = Pane::Notes;
        app.on_intent(Intent::FocusLeft, &mut terminal).unwrap();
        assert_eq!(app.focus, Pane::Dirs);
        app.on_intent(Intent::FocusRight, &mut terminal).unwrap();
        assert_eq!(app.focus, Pane::Notes);
        app.on_intent(Intent::FocusRight, &mut terminal).unwrap();
        assert_eq!(app.focus, Pane::Preview);

        // And each one draws without panicking at that size.
        for focus in [Pane::Dirs, Pane::Notes, Pane::Preview] {
            app.focus = focus;
            terminal.draw(|f| app.draw(f)).unwrap();
        }
    }

    /// Every size must draw. Terminals report odd geometry mid-resize.
    #[test]
    fn the_whole_app_draws_at_any_size() {
        let (mut app, _d) = temp_app();
        for (w, h) in [(200, 60), (120, 30), (90, 24), (70, 20), (50, 16), (40, 10), (20, 6), (10, 3), (4, 2)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| app.draw(f)).unwrap();

            // And with every overlay up, since those size themselves too.
            for mode in [Mode::Help, Mode::Settings] {
                app.mode = mode;
                if matches!(app.mode, Mode::Settings) {
                    app.on_intent(Intent::OpenSettings, &mut terminal).unwrap();
                }
                terminal.draw(|f| app.draw(f)).unwrap();
            }
            app.mode = Mode::Normal;
        }
    }

    /// A click behind an overlay would act on something the user cannot see.
    #[test]
    fn a_click_is_ignored_while_an_overlay_is_open() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let frames = frames_for(&app, 100, 20);

        app.mode = Mode::Help;
        app.focus = Pane::Notes;
        app.on_mouse(click(frames.dirs.x + 2, frames.dirs.y + 1), &mut terminal)
            .unwrap();
        assert_eq!(app.focus, Pane::Notes, "a click reached through the help screen");
    }

    /// `u` must reverse the key that did the damage, through the same stack the
    /// `:` line uses.
    #[test]
    fn u_takes_back_a_delete_from_the_pane() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();

        let before = app.note_count();
        let id = app.selected_id().cloned().expect("a selection");
        let title = app.store.find_note(&id).unwrap().title.clone();

        // Delete without the prompt, the way the confirmed path does.
        app.store.delete_note(&id);
        app.resync();
        assert_eq!(app.note_count(), before - 1);

        app.on_intent(Intent::Undo, &mut terminal).unwrap();
        assert_eq!(app.note_count(), before, "u did not restore the note");
        assert!(app.store.find_note(&id).is_some());

        // And it says what came back, rather than doing it silently.
        let (_, message, _) = app.message.as_ref().expect("a message");
        assert!(message.contains(&title), "{message}");
    }

    #[test]
    fn u_with_nothing_to_undo_says_so() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();
        app.on_intent(Intent::Undo, &mut terminal).unwrap();
        let (_, message, _) = app.message.as_ref().expect("a message");
        assert!(message.contains("Nothing to undo"), "{message}");
    }

    /// An empty pane must explain itself, and the explanation depends on where
    /// the user is: "no notes yet" at the root is different advice from "this
    /// directory is empty", which also needs the way out.
    #[test]
    fn an_empty_pane_explains_itself_differently_by_place() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();

        // An empty root: the fixture ships notes, so clear them.
        for id in app.numbering.clone() {
            app.store.delete_note(&id);
        }
        app.resync();
        assert_eq!(app.note_count(), 0);
        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("No notes yet"), "{out}");
        assert!(out.contains(":new"), "{out}");

        // Inside a directory, where leaving matters as much as writing. A fresh
        // one, since the fixture's directories have notes in them.
        app.store.create_dir("scratch");
        app.current_dir = "scratch".to_string();
        app.resync();
        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("Nothing in this directory"), "{out}");
        assert!(out.contains("cd .."), "{out}");
    }

    /// And the preview says so rather than showing an empty box.
    #[test]
    fn an_empty_preview_says_nothing_is_selected() {
        let (mut app, _d) = temp_app();
        for id in app.numbering.clone() {
            app.store.delete_note(&id);
        }
        app.resync();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 14)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("No note selected"), "{out}");
    }

    /// A first run must say one thing, and later runs nothing: a greeting the
    /// user has to dismiss on every launch is worse than no greeting.
    #[test]
    fn only_a_first_run_is_greeted() {
        let (mut app, _d) = temp_app();
        app.greet(false);
        assert!(app.message.is_none(), "a later run should say nothing");

        app.greet(true);
        let (_, text, _) = app.message.as_ref().expect("a first run should say something");
        assert!(!text.trim().is_empty());
        assert_eq!(text.lines().count(), 1, "more than one instruction: {text}");
    }

    /// The user must learn about every gap before speaking, not one per attempt.
    #[test]
    fn listen_refuses_with_the_fixes_when_nothing_is_set_up() {
        let (mut app, _d) = temp_app();
        // A config with no usable providers at all.
        let lines = {
            let config = crate::config::Config {
                chat: crate::config::provider::TaskChain { chain: vec![] },
                transcribe: crate::config::provider::TaskChain { chain: vec![] },
                providers: Default::default(),
                theme: Default::default(),
                sync: Default::default(),
            };
            let checks =
                crate::health::recording(&config, &crate::config::secret::MemoryStore::default(), true);
            checks
                .iter()
                .filter(|c| !c.state.is_ready())
                .count()
        };
        assert!(lines >= 2, "expected several gaps, got {lines}");

        // And the App path renders them into the preview rather than a status
        // line, since a one-line status cannot hold install commands.
        app.pinned = Some((
            "not ready to record".to_string(),
            vec![Line::bad("Not ready to record:")],
        ));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 14)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(
            terminal.backend().to_string().contains("Not ready to record"),
            "{}",
            terminal.backend().to_string()
        );
    }

    #[test]
    fn foreground_work_renders_a_progress_indicator() {
        let (mut app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 14)).unwrap();

        app.busy = Some((
            view::progress::Progress::spinner("Structuring notes"),
            Instant::now(),
        ));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("Structuring notes"), "{out}");
        assert!(out.contains("00:00"), "no clock: {out}");

        app.busy = Some((
            view::progress::Progress::steps("Transcribing", 3, 8),
            Instant::now(),
        ));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("3/8"), "no step count: {out}");
        assert!(out.contains('█'), "no bar: {out}");
    }

    #[test]
    fn with_nothing_running_the_status_line_has_no_indicator() {
        let (app, _d) = temp_app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 14)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let out = terminal.backend().to_string();
        assert!(!out.contains('█'), "a bar with no work: {out}");
    }

    #[test]
    fn stepping_saturates_instead_of_wrapping() {
        assert_eq!(step(0, 3, Intent::Up), 0);
        assert_eq!(step(2, 3, Intent::Down), 2);
        assert_eq!(step(1, 3, Intent::Down), 2);
        assert_eq!(step(1, 3, Intent::Up), 0);
        assert_eq!(step(1, 3, Intent::First), 0);
        assert_eq!(step(0, 3, Intent::Last), 2);
    }

    #[test]
    fn stepping_an_empty_list_stays_at_zero() {
        for intent in [Intent::Down, Intent::Up, Intent::First, Intent::Last] {
            assert_eq!(step(0, 0, intent), 0);
        }
    }

    #[test]
    fn the_first_open_checkbox_is_found_by_overall_position() {
        // Checked boxes still count, because `check <note> <N>` numbers them all.
        assert_eq!(first_open_checkbox("- [x] done\n- [ ] next"), Some(2));
        assert_eq!(first_open_checkbox("- [ ] first"), Some(1));
        assert_eq!(first_open_checkbox("  - [ ] indented"), Some(1));
        assert_eq!(first_open_checkbox("- [x] all\n- [X] done"), None);
        assert_eq!(first_open_checkbox("no checkboxes"), None);
        assert_eq!(first_open_checkbox(""), None);
    }
}
