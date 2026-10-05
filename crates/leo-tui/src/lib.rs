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
mod discovery;
mod disk;
mod draw;
mod effects;
mod input;
mod mouse;
mod navigation;
mod profile;
mod pump;
pub mod shell;
mod state;
mod tutorial;
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
    Search,
    Actions {
        selected: usize,
    },
    Sources {
        selected: usize,
    },
    Tour,
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
    nav: state::Navigation,
    writing: state::Writing,
    jobs: state::Jobs,
    probe: leo_services::doctor::Probe,
    gh_ready: fn() -> bool,
    local_models: fn(&leo_services::config::Config) -> leo_services::config::choice::Local,
    setup_steps: fn(&std::path::Path) -> Vec<leo_services::health::Check>,
    recordings: Option<std::path::PathBuf>,
    obsidian: fn(&std::path::Path) -> Result<leo_core::obsidian::Opened>,
    speech_model_wanted: fn() -> bool,
    check_usage: fn(),
    mode: Mode,
    cmd: CmdLine,
    message: Option<(Kind, String, Instant)>,
    pinned: Option<(String, Vec<Line>)>,
    answer: Option<(String, String)>,
    welcome: Option<welcome::WelcomeScreen>,
    completing: Option<Cycle>,
    help_scroll: u16,
    settings: Option<SettingsScreen>,
    repaint: bool,
    tour: Option<tutorial::Tour>,
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

    /// Typed points as the preview lists them.
    fn point_lines(&self) -> Vec<String> {
        self.jotted.iter().map(|p| p.text.clone()).collect()
    }
}

impl App {
    pub fn new(store: Store) -> App {
        App {
            nav: state::Navigation::new(&store),
            writing: state::Writing::default(),
            jobs: state::Jobs::default(),
            probe: leo_services::doctor::Probe::all(),
            speech_model_wanted: || {
                leo_services::providers::speech_model_wanted(&leo_services::config::Config::load())
            },
            check_usage: || {
                leo_services::usage::refresh_codex(&leo_services::config::Config::load())
            },
            gh_ready: leo_core::sync::gh_ready,
            local_models: leo_services::config::choice::local_models,
            setup_steps: welcome::real_setup_steps,
            recordings: leo_services::session::root().ok(),
            obsidian: leo_core::obsidian::open,
            store,
            mode: Mode::Normal,
            cmd: CmdLine::default(),
            message: None,
            pinned: None,
            answer: None,
            welcome: None,
            completing: None,
            help_scroll: 0,
            settings: None,
            repaint: false,
            tour: None,
            quit: false,
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
            "  :settings picks the AI · :doctor checks everything",
        ));
        Some(lines)
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
    leo_services::session::mic::warm_up();
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
    app.jobs.update = Some(task::start_update_check());
    app.fetch_speech_model();
    app.offer_tour();
    let result = event_loop(&mut terminal, &mut app);
    app.flush_edit();
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    // Persist the recent list so the strip survives a restart, which is the
    // difference between a convenience and a novelty.
    app.nav.recent.save();
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
            app.pump_usage();
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
