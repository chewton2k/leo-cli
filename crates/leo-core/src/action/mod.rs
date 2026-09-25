//! The single command vocabulary for the whole program.
//!
//! Every input surface — the keymap, the `:` command line, and the CLI
//! subcommands — parses into an [`Action`], and one set of handlers applies
//! them to a [`Store`]. Handlers contain no terminal or rendering code: they
//! return an [`Outcome`] describing what to show and, when a step genuinely
//! needs the terminal (spawning `$EDITOR`, recording audio, asking for
//! confirmation), an [`Effect`] for the shell to perform. That split is what
//! makes them unit-testable without a terminal.

use anyhow::Result;

use crate::store::Store;

// ── Vocabulary ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    New {
        title: Option<String>,
    },
    List {
        tag: Option<String>,
        limit: usize,
    },
    View {
        note: String,
    },
    Edit {
        note: String,
    },
    Delete {
        note: String,
    },
    /// Delete the marked notes. Never typed: `fill_selected` makes it.
    DeleteMany {
        ids: Vec<String>,
    },
    /// Give a note a new title. Only the title changes.
    Rename {
        note: String,
        title: String,
    },
    Check {
        note: String,
        index: usize,
    },
    /// The CLI's search. In the panes, `/` does the same thing live.
    Search {
        query: String,
    },
    Listen {
        title: Option<String>,
        append_to: Option<String>,
        screen: bool,
    },
    Ask {
        note: String,
    },
    Mkdir {
        name: String,
    },
    Cd {
        path: String,
    },
    Mv {
        notes: Vec<String>,
        dir: String,
    },
    Rmdir {
        name: String,
        /// Remove the directory's notes and subdirectories too. Always asks
        /// first, since nothing else in leo destroys more than one note at once.
        recursive: bool,
    },
    Sync(SyncAction),
    /// Take back the most recent destructive change.
    Undo,
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// Back up now: pull, then push.
    Now,
    Init,
    Connect { url: String },
    Push,
    Pull,
    Status,
}

/// What parsing one input line produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Nothing to do — the line was blank, or only a stripped `hey leo` prefix.
    Empty,
    Action(Action),
    /// Recognized verb, wrong arguments. Carries the usage text to show.
    Usage(String),
    /// Unrecognized verb.
    Unknown(String),
    /// A verb that used to exist. Named so the answer is "here is the
    /// replacement" rather than "unknown command", which reads like a typo.
    Retired {
        verb: &'static str,
        replacement: &'static str,
        why: &'static str,
    },
}

// ── Output ──────────────────────────────────────────────────────────────────

/// How one output line should be presented. Naming the intent rather than a
/// color lets the CLI pick `colored` styles and the TUI pick ratatui ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Ordinary text.
    Plain,
    /// Secondary text: "Cancelled.", "No changes."
    Dim,
    /// A completed mutation.
    Good,
    /// Something the user should notice but that is not a failure.
    Warn,
    /// A failure.
    Bad,
    /// A directory name.
    Dir,
    /// A blank separator line.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

impl Line {
    pub fn plain(text: impl Into<String>) -> Line {
        Line { kind: Kind::Plain, text: text.into() }
    }
    pub fn dim(text: impl Into<String>) -> Line {
        Line { kind: Kind::Dim, text: text.into() }
    }
    pub fn good(text: impl Into<String>) -> Line {
        Line { kind: Kind::Good, text: text.into() }
    }
    pub fn warn(text: impl Into<String>) -> Line {
        Line { kind: Kind::Warn, text: text.into() }
    }
    pub fn bad(text: impl Into<String>) -> Line {
        Line { kind: Kind::Bad, text: text.into() }
    }
    pub fn dir(text: impl Into<String>) -> Line {
        Line { kind: Kind::Dir, text: text.into() }
    }
    pub fn blank() -> Line {
        Line { kind: Kind::Blank, text: String::new() }
    }
}

/// Work that requires the terminal or a long-running subprocess, so a handler
/// describes it instead of doing it. The CLI performs these inline; the TUI
/// suspends itself or hands them to its worker thread.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Effect {
    #[default]
    None,
    /// Spawn `$EDITOR` on `path`, then feed the result back through
    /// [`apply_edit`].
    Edit(EditRequest),
    /// Ask the user to confirm, then apply `on_yes`.
    Confirm { prompt: String, on_yes: ConfirmedAction },
    /// Record audio, transcribe it, then feed the result back through
    /// [`apply_transcript`].
    Listen(ListenRequest),
    /// Render a note in full.
    ShowNote { id: String },
    ShowHelp,
    Quit,
    /// Shell out to git. Streams its own output.
    Sync(SyncAction),
}

/// A pending editor session. `seed` is written to `path` before `$EDITOR` opens
/// so the user sees a frontmatter template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    pub path: std::path::PathBuf,
    pub seed: String,
    pub target: EditTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditTarget {
    /// A note that does not exist yet.
    NewNote { fallback_title: String, dir: String },
    /// An existing note, with the values to diff the result against.
    Existing { id: String, old_title: String, old_tags: Vec<String>, old_body: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum ConfirmedAction {
    DeleteNote { id: String, title: String },
    /// Delete every one of these notes, as one undo.
    DeleteNotes { ids: Vec<String> },
    /// Delete a directory and everything inside it.
    DeleteDir { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenRequest {
    pub screen: bool,
    pub title: Option<String>,
    pub append_to: Option<String>,
    pub dir: String,
}

/// Everything a handler produces. `Default` is "nothing happened", so handlers
/// only set the fields they mean.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    pub lines: Vec<Line>,
    /// Replaces the caller's note-reference numbering when `Some`.
    pub selection: Option<Vec<String>>,
    /// Replaces the caller's current directory when `Some`.
    pub new_dir: Option<String>,
    pub effect: Effect,
    /// The store changed, so any cached view of it is stale.
    pub dirty: bool,
}

impl Outcome {
    pub fn empty() -> Outcome {
        Outcome::default()
    }

    pub fn line(line: Line) -> Outcome {
        Outcome { lines: vec![line], ..Outcome::default() }
    }

    pub fn lines(lines: Vec<Line>) -> Outcome {
        Outcome { lines, ..Outcome::default() }
    }

    pub fn effect(effect: Effect) -> Outcome {
        Outcome { effect, ..Outcome::default() }
    }

    /// Convenience for tests and callers that only care about the text.
    /// The shells render `lines` with styling instead of using this.
    #[allow(dead_code)]
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── The AI seam ─────────────────────────────────────────────────────────────

/// The AI operations handlers need, behind a trait so tests never make a
/// network call and the TUI can route them onto its worker thread.
pub trait Ai {
    /// Expand every `@leo` line in `body`. Returns the new body and how many
    /// prompts were expanded.
    fn expand_prompts(&self, body: &str, title: &str) -> Result<(String, usize)>;
    /// Turn a transcript into (title, body).
    fn structure(&self, transcript: &str) -> Result<(String, String)>;
    /// Turn a transcript into a body fragment to append to `existing`.
    fn structure_append(&self, transcript: &str, existing: &str) -> Result<String>;
}

mod frontmatter;
mod handlers;
mod parse;
mod resolve;

pub use frontmatter::*;
pub use handlers::*;
pub use parse::*;
pub use resolve::*;
