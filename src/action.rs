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
    /// Give a note a new title. Only the title changes.
    Rename {
        note: String,
        title: String,
    },
    Check {
        note: String,
        index: usize,
    },
    Search {
        query: String,
        full_text: bool,
    },
    Remind {
        text: String,
    },
    Listen {
        title: Option<String>,
        append_to: Option<String>,
        screen: bool,
    },
    Export {
        note: String,
        format: String,
    },
    Ask {
        note: String,
    },
    Tags,
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
    Model(ModelAction),
    Config(ConfigAction),
    /// Take back the most recent destructive change.
    Undo,
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    Init,
    Connect { url: String },
    Push,
    Pull,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAction {
    List,
    Test { name: String },
    Login { name: String },
    Logout { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAction {
    Edit,
    Path,
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
    /// Provider management, which prompts for a key with echo disabled.
    Model(ModelAction),
    Config(ConfigAction),
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
pub enum ConfirmedAction {
    DeleteNote { id: String, title: String },
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

/// The real implementation, delegating to the provider chains in `crate::ai`.
pub struct RealAi;

impl Ai for RealAi {
    fn expand_prompts(&self, body: &str, title: &str) -> Result<(String, usize)> {
        expand_leo_prompts(body, title)
    }
    fn structure(&self, transcript: &str) -> Result<(String, String)> {
        crate::ai::structure_notes(transcript)
    }
    fn structure_append(&self, transcript: &str, existing: &str) -> Result<String> {
        crate::ai::structure_notes_append(transcript, existing)
    }
}

// ── Note reference resolution ───────────────────────────────────────────────

/// A note rendered just enough to disambiguate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteBrief {
    /// 1-based position in the caller's current numbering, when it has one.
    pub index: Option<usize>,
    pub id: String,
    pub title: String,
}

/// The result of resolving a user-typed note reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    One(String),
    /// A title substring matched more than one note.
    Many(Vec<NoteBrief>),
    None,
}

/// Resolve a reference the same way everywhere: list number, then ID prefix,
/// then unique title substring. Returns structured data rather than printing,
/// so both shells can render disambiguation their own way.
///
/// Always yields a note's full ID, never the prefix the user typed. Downstream
/// `Store` lookups accept prefixes, but an `Outcome` or `ConfirmedAction` may
/// outlive the store state it was built from, and a prefix that is unique today
/// can become ambiguous after the next `sync pull`.
pub fn resolve(input: &str, store: &Store, numbering: &[String]) -> Resolved {
    if let Ok(n) = input.parse::<usize>() {
        if n >= 1 && n <= numbering.len() {
            let id = &numbering[n - 1];
            if let Some(note) = store.find_note(id) {
                return Resolved::One(note.id.clone());
            }
        }
    }
    if let Some(note) = store.find_note(input) {
        return Resolved::One(note.id.clone());
    }
    let matches = store.find_by_title(input);
    match matches.len() {
        0 => Resolved::None,
        1 => Resolved::One(matches[0].id.clone()),
        _ => Resolved::Many(
            matches
                .iter()
                .map(|note| NoteBrief {
                    index: numbering.iter().position(|id| id == &note.id).map(|p| p + 1),
                    id: note.id.clone(),
                    title: note.title.clone(),
                })
                .collect(),
        ),
    }
}

/// Render a failed resolution as output lines.
fn unresolved(input: &str, resolved: Resolved) -> Outcome {
    match resolved {
        Resolved::Many(briefs) => {
            let mut lines = vec![Line::warn(format!("Multiple notes match \"{input}\":"))];
            for b in briefs {
                let idx = match b.index {
                    Some(i) => format!("{i:>3}"),
                    None => "   ".to_string(),
                };
                let short = &b.id[..std::cmp::min(8, b.id.len())];
                lines.push(Line::plain(format!("{idx} {short} {}", b.title)));
            }
            lines.push(Line::dim("Use a number or ID prefix to pick one."));
            Outcome::lines(lines)
        }
        _ => Outcome::line(Line::bad(format!("No note found: {input}"))),
    }
}

/// Resolve or return the rendered failure, so handlers stay one line each.
macro_rules! resolve_or_return {
    ($input:expr, $store:expr, $numbering:expr) => {
        match resolve($input, $store, $numbering) {
            Resolved::One(id) => id,
            other => return Ok(unresolved($input, other)),
        }
    };
}

// ── Parsing ─────────────────────────────────────────────────────────────────

/// All verbs and their aliases, in help order. The completion engine reads
/// this too, so a new verb becomes completable for free.
/// An alias earns its place by being something a user already types, not by
/// saving a keystroke. Three kinds survive:
///
/// * shell muscle memory — `ls`, `rm`, `mv`, `exit`, `q`;
/// * the same letter as the key that does it in the panes — `e`, `x`, `?`;
/// * nothing else.
///
/// Seventeen aliases became six. The rest were a second name to learn for no
/// gain, and some actively misled: `l` listed notes here while moving between
/// panes there, and `d` deleted a note here while dropping a provider from a
/// chain on the settings screen.
pub const VERBS: &[(&str, &[&str])] = &[
    ("new", &[]),
    ("list", &["ls"]),
    ("view", &[]),
    ("edit", &["e"]),
    ("delete", &["rm"]),
    ("rename", &[]),
    ("check", &["x"]),
    ("search", &[]),
    ("tags", &[]),
    ("undo", &["u"]),
    ("remind", &[]),
    ("listen", &[]),
    ("ask", &[]),
    ("export", &[]),
    ("mkdir", &[]),
    ("cd", &[]),
    ("mv", &[]),
    ("rmdir", &[]),
    ("sync", &[]),
    ("model", &[]),
    ("config", &[]),
    ("help", &["?"]),
    ("quit", &["exit", "q"]),
];

/// Words that used to work: what to use instead, and why it changed.
///
/// Removing a word someone has in their fingers is only kind if the removal
/// explains itself. "Unknown command: d" reads like a typo and sends the user
/// hunting; naming the replacement costs one line. A replacement starting with
/// `:` is a command; anything else is a key or a place on screen.
pub const RETIRED: &[(&str, &str, &str)] = &[
    ("l", ":list", ONE_NAME),
    ("d", ":delete", ONE_NAME),
    ("del", ":delete", ONE_NAME),
    ("n", ":new", ONE_NAME),
    ("v", ":view", ONE_NAME),
    ("rem", ":remind", ONE_NAME),
    ("rec", ":listen", ONE_NAME),
    ("exp", ":export", ONE_NAME),
    ("move", ":mv", ONE_NAME),
    ("h", ":help", ONE_NAME),
    ("find", ":search", ONE_NAME),
    ("expand", ":ask", ONE_NAME),
    ("uncheck", ":check", ONE_NAME),
    (
        "env",
        ":model login <provider>",
        "keys live in your OS keychain now, not a plaintext file",
    ),
    ("pwd", "the status bar", "it always shows where you are"),
    ("clear", "Esc", "it closes whatever output is pinned"),
];

const ONE_NAME: &str = "one name per command now, so there is less to learn";

/// Every word that can start a command, canonical names and aliases alike.
pub fn all_verb_words() -> Vec<&'static str> {
    let mut out = Vec::new();
    for (canon, aliases) in VERBS {
        out.push(*canon);
        out.extend_from_slice(aliases);
    }
    out
}

/// Split on whitespace, keeping quoted runs together.
pub fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for ch in input.chars() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '"' | '\'' => {
                    in_quotes = true;
                    quote_char = ch;
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                }
                _ => current.push(ch),
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Strip a natural-language `hey leo` / `leo` prefix, so "hey leo remind me to
/// call mom" works. Only strips when something follows.
pub fn strip_leo_prefix(tokens: &mut Vec<String>) {
    if tokens.len() >= 2
        && tokens[0].eq_ignore_ascii_case("hey")
        && tokens[1].eq_ignore_ascii_case("leo")
    {
        tokens.drain(0..2);
    } else if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("leo") {
        tokens.drain(0..1);
    }
}

/// Parse one command line into an [`Action`].
pub fn parse(line: &str) -> Parsed {
    let mut tokens = tokenize(line.trim());
    if tokens.is_empty() {
        return Parsed::Empty;
    }
    strip_leo_prefix(&mut tokens);
    if tokens.is_empty() {
        return Parsed::Empty;
    }

    let verb = tokens[0].to_lowercase();
    let args = &tokens[1..];
    let joined = || args.join(" ");
    let usage = |s: &str| Parsed::Usage(s.to_string());
    let act = |a: Action| Parsed::Action(a);

    match verb.as_str() {
        "new" => act(Action::New {
            title: if args.is_empty() { None } else { Some(joined()) },
        }),

        "list" | "ls" => {
            let mut tag = None;
            let mut limit = 20;
            for arg in args {
                if let Some(t) = arg.strip_prefix('#') {
                    tag = Some(t.to_string());
                } else if let Ok(n) = arg.parse::<usize>() {
                    limit = n;
                }
            }
            act(Action::List { tag, limit })
        }

        "view" => {
            if args.is_empty() {
                usage("view <note>")
            } else {
                act(Action::View { note: joined() })
            }
        }

        // A missing note means the selected one; see `fill_selected`.
        "edit" | "e" => act(Action::Edit { note: joined() }),
        "delete" | "rm" => act(Action::Delete { note: joined() }),

        // The checkbox number is the last token, so everything before it is the
        // note reference — a title with spaces still resolves.
        "check" | "x" => {
            if args.len() < 2 {
                return usage("check <note> <checkbox number>");
            }
            match args.last().unwrap().parse::<usize>() {
                Ok(index) if index >= 1 => act(Action::Check {
                    note: args[..args.len() - 1].join(" "),
                    index,
                }),
                _ => Parsed::Usage("Checkbox number must be a positive integer.".to_string()),
            }
        }

        "search" => {
            let full_text = args.first().map(|s| s == "-f").unwrap_or(false);
            let rest = if full_text { &args[1..] } else { args };
            let query = rest.join(" ");
            if query.is_empty() {
                usage("search [-f] <query>")
            } else {
                act(Action::Search { query, full_text })
            }
        }

        "remind" => {
            if args.is_empty() {
                return usage("remind <what to remember>");
            }
            // Normalize the natural phrasing "remind me to X".
            let text = joined();
            let text = text
                .strip_prefix("me to ")
                .or_else(|| text.strip_prefix("me "))
                .unwrap_or(&text)
                .trim()
                .to_string();
            if text.is_empty() {
                usage("remind <what to remember>")
            } else {
                act(Action::Remind { text })
            }
        }

        "listen" => {
            let screen = args.iter().any(|a| a == "--screen");
            let rest: Vec<String> =
                args.iter().filter(|a| a.as_str() != "--screen").cloned().collect();

            if rest.first().map(|s| s.eq_ignore_ascii_case("add")).unwrap_or(false) {
                return act(Action::Listen {
                    title: None,
                    append_to: Some(rest[1..].join(" ")),
                    screen,
                });
            }
            act(Action::Listen {
                title: if rest.is_empty() { None } else { Some(rest.join(" ")) },
                append_to: None,
                screen,
            })
        }

        // Format is the last token; the note reference is everything before it.
        "export" => {
            if args.len() < 2 {
                return usage("export <note> <format>   (txt, md, html, docx, pdf, rtf, odt)");
            }
            act(Action::Export {
                note: args[..args.len() - 1].join(" "),
                format: args.last().unwrap().to_lowercase(),
            })
        }

        "ask" => act(Action::Ask { note: joined() }),

        "tags" => act(Action::Tags),
        "undo" | "u" => act(Action::Undo),

        // The whole line is the new title; the note is always the selected one
        // (see `fill_selected`), which is what the `r` key pre-fills this for.
        "rename" => {
            if args.is_empty() {
                usage("rename <new title>")
            } else {
                act(Action::Rename { note: String::new(), title: joined() })
            }
        }

        "mkdir" => {
            let name = joined().trim().to_string();
            if name.is_empty() {
                usage("mkdir <name>")
            } else {
                act(Action::Mkdir { name })
            }
        }

        "cd" => act(Action::Cd { path: joined().trim().to_string() }),


        // With one argument, that is the directory and the note is the
        // selected one.
        "mv" => {
            if args.is_empty() {
                return usage("mv [note...] <directory>");
            }
            act(Action::Mv {
                notes: args[..args.len() - 1].to_vec(),
                dir: args.last().unwrap().trim_matches('/').to_string(),
            })
        }

        // `-r` mirrors the shell, where recursive removal is also opt-in.
        "rmdir" => {
            let recursive = args.iter().any(|a| a == "-r" || a == "--recursive");
            let name = args
                .iter()
                .filter(|a| a.as_str() != "-r" && a.as_str() != "--recursive")
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            if name.is_empty() {
                usage("rmdir [-r] <name>")
            } else {
                act(Action::Rmdir { name, recursive })
            }
        }

        "sync" => match args.first().map(|s| s.to_lowercase()).as_deref() {
            Some("init") => act(Action::Sync(SyncAction::Init)),
            Some("connect") => match args.get(1) {
                Some(url) => act(Action::Sync(SyncAction::Connect { url: url.clone() })),
                None => usage("sync connect <url>"),
            },
            Some("push") => act(Action::Sync(SyncAction::Push)),
            Some("pull") => act(Action::Sync(SyncAction::Pull)),
            Some("status") => act(Action::Sync(SyncAction::Status)),
            _ => usage("sync <init | connect <url> | push | pull | status>"),
        },

        "model" => match args.first().map(|s| s.to_lowercase()).as_deref() {
            Some("list") => act(Action::Model(ModelAction::List)),
            Some("test") => match args.get(1) {
                Some(name) => act(Action::Model(ModelAction::Test { name: name.clone() })),
                None => usage("model test <provider>"),
            },
            Some("login") => match args.get(1) {
                Some(name) => act(Action::Model(ModelAction::Login { name: name.clone() })),
                None => usage("model login <provider>"),
            },
            Some("logout") => match args.get(1) {
                Some(name) => act(Action::Model(ModelAction::Logout { name: name.clone() })),
                None => usage("model logout <provider>"),
            },
            _ => usage("model <list | test <provider> | login <provider> | logout <provider>>"),
        },

        "config" => match args.first().map(|s| s.to_lowercase()).as_deref() {
            Some("edit") | None => act(Action::Config(ConfigAction::Edit)),
            Some("path") => act(Action::Config(ConfigAction::Path)),
            _ => usage("config <edit | path>"),
        },

        "help" | "?" => act(Action::Help),
        "quit" | "exit" | "q" => act(Action::Quit),

        _ => match RETIRED.iter().find(|(alias, _, _)| *alias == verb.as_str()) {
            Some((alias, replacement, why)) => Parsed::Retired {
                verb: alias,
                replacement,
                why,
            },
            None => Parsed::Unknown(verb),
        },
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// Read-only context a handler needs from its shell.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    /// Directory the user is currently in; `""` is root.
    pub current_dir: &'a str,
    /// Note IDs behind the current 1-based numbering.
    pub numbering: &'a [String],
    /// The note the user is looking at, which is what a command means when it
    /// names no note. `None` on the CLI, which has no selection.
    pub selected: Option<&'a str>,
}

/// Apply an action. The only entry point a shell needs.
/// Put the selected note into an action that named none.
///
/// A command that leaves its note out means the one on screen, so nobody has to
/// read a number off the list to act on what they are already looking at. With
/// nothing selected — always the case on the CLI — it says so instead of
/// guessing.
pub fn fill_selected(action: Action, selected: Option<&str>) -> std::result::Result<Action, Line> {
    let omitted = match &action {
        Action::Edit { note }
        | Action::Delete { note }
        | Action::Ask { note }
        | Action::Rename { note, .. } => note.is_empty(),
        Action::Mv { notes, .. } => notes.is_empty(),
        Action::Listen { append_to: Some(note), .. } => note.is_empty(),
        _ => false,
    };
    if !omitted {
        return Ok(action);
    }
    let Some(id) = selected else {
        return Err(Line::bad(
            "No note selected. Select one, or name it: a number, a title, or an ID.",
        ));
    };
    let id = id.to_string();
    Ok(match action {
        Action::Edit { .. } => Action::Edit { note: id },
        Action::Delete { .. } => Action::Delete { note: id },
        Action::Ask { .. } => Action::Ask { note: id },
        Action::Rename { title, .. } => Action::Rename { note: id, title },
        Action::Mv { dir, .. } => Action::Mv { notes: vec![id], dir },
        Action::Listen { title, screen, .. } => Action::Listen { title, append_to: Some(id), screen },
        other => other,
    })
}

pub fn apply(
    action: Action,
    store: &mut Store,
    ctx: Ctx<'_>,
    ai: &dyn Ai,
) -> Result<Outcome> {
    let action = match fill_selected(action, ctx.selected) {
        Ok(action) => action,
        Err(line) => return Ok(Outcome::line(line)),
    };
    match action {
        Action::New { title } => Ok(new_note(title, ctx.current_dir)),
        Action::List { tag, limit } => Ok(list(store, tag.as_deref(), limit, ctx.current_dir)),
        Action::View { note } => Ok(view(store, &note, ctx.numbering)),
        Action::Edit { note } => Ok(edit(store, &note, ctx.numbering)),
        Action::Delete { note } => Ok(delete(store, &note, ctx.numbering)),
        Action::Check { note, index } => check(store, &note, index, ctx.numbering),
        Action::Search { query, full_text } => Ok(search(store, &query, full_text)),
        Action::Remind { text } => remind(store, &text),
        Action::Listen { title, append_to, screen } => {
            Ok(listen(store, title, append_to, screen, ctx.current_dir))
        }
        Action::Export { note, format } => export(store, &note, &format, ctx.numbering),
        Action::Ask { note } => ask(store, &note, ctx.numbering, ai),
        Action::Tags => Ok(tags(store)),
        Action::Undo => Ok(undo(store)),
        Action::Mkdir { name } => mkdir(store, &name, ctx.current_dir),
        Action::Cd { path } => Ok(cd(store, &path, ctx.current_dir)),
        Action::Mv { notes, dir } => mv(store, &notes, &dir, ctx.numbering),
        Action::Rename { note, title } => rename(store, &note, &title, ctx.numbering),
        Action::Rmdir { name, recursive } => rmdir(store, &name, recursive, ctx.current_dir),
        Action::Sync(a) => Ok(Outcome::effect(Effect::Sync(a))),
        Action::Model(a) => Ok(Outcome::effect(Effect::Model(a))),
        Action::Config(a) => Ok(Outcome::effect(Effect::Config(a))),
        Action::Help => Ok(Outcome::effect(Effect::ShowHelp)),
        Action::Quit => Ok(Outcome::effect(Effect::Quit)),
    }
}

/// `new` — ask the shell to open an editor on a frontmatter template.
fn new_note(title: Option<String>, dir: &str) -> Outcome {
    let title = title.unwrap_or_default();
    let path = std::env::temp_dir().join(format!("leo-new-{}.md", uuid::Uuid::new_v4()));
    Outcome::effect(Effect::Edit(EditRequest {
        seed: format!("---\ntitle: {title}\ntags: \n---\n"),
        path,
        target: EditTarget::NewNote { fallback_title: title, dir: dir.to_string() },
    }))
}

/// `list` — subdirectories first, then notes, and renumber.
fn list(store: &Store, tag: Option<&str>, limit: usize, dir: &str) -> Outcome {
    let subdirs = store.subdirs(dir);
    let notes = store.list_notes_in_dir(dir, tag, limit);

    if subdirs.is_empty() && notes.is_empty() {
        let line = if tag.is_some() {
            Line::dim("No notes with that tag.")
        } else {
            Line::dim("No notes yet. Type `new` to create one.")
        };
        return Outcome { selection: Some(Vec::new()), ..Outcome::line(line) };
    }

    let mut lines = vec![Line::blank()];
    for name in &subdirs {
        lines.push(Line::dir(format!("{name}/")));
    }
    if !subdirs.is_empty() && !notes.is_empty() {
        lines.push(Line::blank());
    }

    let mut selection = Vec::with_capacity(notes.len());
    for (i, note) in notes.iter().enumerate() {
        selection.push(note.id.clone());
        lines.push(Line::plain(format!("{:>3} {}", i + 1, note.format_summary())));
    }
    lines.push(Line::blank());

    Outcome { selection: Some(selection), ..Outcome::lines(lines) }
}

fn view(store: &Store, note: &str, numbering: &[String]) -> Outcome {
    let id = match resolve(note, store, numbering) {
        Resolved::One(id) => id,
        other => return unresolved(note, other),
    };
    Outcome::effect(Effect::ShowNote { id })
}

/// `edit` — hand the shell a temp file seeded with the note's current content.
fn edit(store: &Store, note: &str, numbering: &[String]) -> Outcome {
    let id = match resolve(note, store, numbering) {
        Resolved::One(id) => id,
        other => return unresolved(note, other),
    };
    let n = store.find_note(&id).expect("resolve returned a live id");
    let seed = format!(
        "---\ntitle: {}\ntags: {}\n---\n{}",
        n.title,
        n.tags.join(", "),
        n.body
    );
    let path = std::env::temp_dir().join(format!("leo-{}.md", &id[..std::cmp::min(8, id.len())]));
    Outcome::effect(Effect::Edit(EditRequest {
        path,
        seed,
        target: EditTarget::Existing {
            id: id.clone(),
            old_title: n.title.clone(),
            old_tags: n.tags.clone(),
            old_body: n.body.clone(),
        },
    }))
}

fn delete(store: &Store, note: &str, numbering: &[String]) -> Outcome {
    let id = match resolve(note, store, numbering) {
        Resolved::One(id) => id,
        other => return unresolved(note, other),
    };
    let title = store.find_note(&id).expect("resolve returned a live id").title.clone();
    Outcome::effect(Effect::Confirm {
        prompt: format!("Delete {title}?"),
        on_yes: ConfirmedAction::DeleteNote { id, title },
    })
}

fn check(store: &mut Store, note: &str, index: usize, numbering: &[String]) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    match store.toggle_checkbox(&id, index) {
        Some(state) => {
            store.save()?;
            Ok(Outcome { dirty: true, ..Outcome::line(Line::plain(state)) })
        }
        None => Ok(Outcome::line(Line::bad(format!(
            "No checkbox #{index} in that note."
        )))),
    }
}

fn search(store: &Store, query: &str, full_text: bool) -> Outcome {
    let results = store.search(query, full_text);
    if results.is_empty() {
        return Outcome {
            selection: Some(Vec::new()),
            ..Outcome::line(Line::dim(format!("No notes match '{query}'.")))
        };
    }

    let mut lines = vec![Line::blank()];
    let mut selection = Vec::with_capacity(results.len());
    for (i, note) in results.iter().enumerate() {
        selection.push(note.id.clone());
        let dir_info = if note.directory.is_empty() {
            String::new()
        } else {
            format!("  {}/", note.directory)
        };
        lines.push(Line::plain(format!(
            "{:>3} {}{}",
            i + 1,
            note.format_summary(),
            dir_info
        )));
    }
    lines.push(Line::blank());
    Outcome { selection: Some(selection), ..Outcome::lines(lines) }
}

fn remind(store: &mut Store, text: &str) -> Result<Outcome> {
    let item = format!("- [ ] {text}");
    // Reminders always live at the root, in one note tagged #reminder.
    if let Some(note) = store.find_by_tag_mut("reminder") {
        note.body.push('\n');
        note.body.push_str(&item);
        note.updated_at = chrono::Utc::now();
        store.save()?;
        Ok(Outcome { dirty: true, ..Outcome::line(Line::good(format!("Added {text}"))) })
    } else {
        store.create_note("Reminders", &item, vec!["reminder".to_string()], "")?;
        store.save()?;
        Ok(Outcome {
            dirty: true,
            ..Outcome::line(Line::good(format!("Created Reminders + {text}")))
        })
    }
}

/// `listen` — validate the append target before spending time recording.
fn listen(
    store: &Store,
    title: Option<String>,
    append_to: Option<String>,
    screen: bool,
    dir: &str,
) -> Outcome {
    if let Some(target) = &append_to {
        if store.find_by_index_or_prefix(target).is_none() {
            return Outcome::line(Line::bad(format!("No note found: {target}")));
        }
    }
    Outcome::effect(Effect::Listen(ListenRequest {
        screen,
        title,
        append_to,
        dir: dir.to_string(),
    }))
}

fn export(store: &Store, note: &str, format: &str, numbering: &[String]) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    let n = store.find_note(&id).expect("resolve returned a live id");
    // Desktop, then home, then the working directory. `export` takes no path
    // argument, so nothing here needs filesystem completion. Each candidate is
    // checked for existence rather than trusted: `dirs::desktop_dir()` reports
    // `$HOME/Desktop` on macOS whether or not that directory was ever created,
    // and writing into a missing directory fails with a bare "No such file or
    // directory" that names neither the note nor the path.
    let output_dir = [dirs::desktop_dir(), dirs::home_dir()]
        .into_iter()
        .flatten()
        .find(|d| d.is_dir())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let path = crate::export::export_note(n, format.trim_start_matches('.'), &output_dir)?;
    Ok(Outcome::line(Line::good(format!("Exported {}", path.display()))))
}

fn ask(store: &mut Store, note: &str, numbering: &[String], ai: &dyn Ai) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    let (title, body) = {
        let n = store.find_note(&id).expect("resolve returned a live id");
        (n.title.clone(), n.body.clone())
    };

    let count = body.lines().filter(|l| is_leo_prompt(l).is_some()).count();
    if count == 0 {
        return Ok(Outcome::line(Line::dim("No @leo prompts found in this note.")));
    }

    let (expanded, _) = ai.expand_prompts(&body, &title)?;

    let n = store.find_note_mut(&id).expect("resolve returned a live id");
    n.body = expanded;
    n.updated_at = chrono::Utc::now();
    let short = n.id[..std::cmp::min(8, n.id.len())].to_string();
    let title = n.title.clone();
    store.save()?;
    Ok(Outcome {
        dirty: true,
        ..Outcome::line(Line::good(format!("Updated \"{title}\" {short}")))
    })
}

/// `undo` — take back the last destructive change.
///
/// A handler rather than a TUI-only key, so the same step back works from the `:`
/// line and reuses the store's stack instead of a second one.
fn undo(store: &mut Store) -> Outcome {
    match store.undo() {
        Some(what) => Outcome {
            lines: vec![Line::good(what)],
            dirty: true,
            ..Outcome::default()
        },
        None => Outcome::line(Line::dim("Nothing to undo.")),
    }
}

fn tags(store: &Store) -> Outcome {
    let tags = store.tags();
    if tags.is_empty() {
        return Outcome::line(Line::dim("No tags yet."));
    }
    let mut lines = vec![Line::blank()];
    for (tag, count) in &tags {
        lines.push(Line::plain(format!("#{tag} ({count})")));
    }
    lines.push(Line::blank());
    Outcome::lines(lines)
}

/// Join a name onto the current directory, tolerating stray slashes.
fn under(current_dir: &str, name: &str) -> String {
    if current_dir.is_empty() {
        name.trim_matches('/').to_string()
    } else {
        format!("{}/{}", current_dir, name.trim_matches('/'))
    }
}

fn mkdir(store: &mut Store, name: &str, current_dir: &str) -> Result<Outcome> {
    let full = under(current_dir, name);
    if store.dir_exists(&full) {
        return Ok(Outcome::line(Line::dim(format!(
            "Directory already exists: {full}/"
        ))));
    }
    store.create_dir(&full);
    store.save()?;
    Ok(Outcome { dirty: true, ..Outcome::line(Line::good(format!("Created {full}/"))) })
}

/// `cd` — resolve `..`, `/`, `~`, and `../sibling` against the current
/// directory. Pure path arithmetic plus one existence check.
pub fn resolve_cd(path: &str, store: &Store, current_dir: &str) -> std::result::Result<String, String> {
    let target = path.trim();
    if target.is_empty() || target == "/" || target == "~" {
        return Ok(String::new());
    }

    let parent_of = |dir: &str| -> String {
        match dir.rfind('/') {
            Some(pos) => dir[..pos].to_string(),
            None => String::new(),
        }
    };

    if target == ".." {
        return Ok(parent_of(current_dir));
    }

    let mut base = current_dir.to_string();
    let mut remaining = target;
    while let Some(rest) = remaining.strip_prefix("../") {
        base = parent_of(&base);
        remaining = rest;
    }
    if remaining == ".." {
        base = parent_of(&base);
        remaining = "";
    }

    let full = if remaining.is_empty() {
        base
    } else if remaining.starts_with('/') || base.is_empty() {
        // An absolute path ignores the base; an empty base has nothing to join.
        remaining.trim_matches('/').to_string()
    } else {
        format!("{}/{}", base, remaining.trim_matches('/'))
    };

    if full.is_empty() || store.dir_exists(&full) {
        Ok(full)
    } else {
        Err(format!("No such directory: {full}/"))
    }
}

fn cd(store: &Store, path: &str, current_dir: &str) -> Outcome {
    match resolve_cd(path, store, current_dir) {
        Ok(dir) => Outcome { new_dir: Some(dir), dirty: true, ..Outcome::empty() },
        Err(msg) => Outcome::line(Line::bad(msg)),
    }
}

/// `rename` — change a note's title and nothing else.
fn rename(store: &mut Store, note: &str, title: &str, numbering: &[String]) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    let title = title.trim();
    let n = store.find_note_mut(&id).expect("resolve returned a live id");
    let old = std::mem::replace(&mut n.title, title.to_string());
    n.updated_at = chrono::Utc::now();
    store.save()?;
    Ok(Outcome {
        dirty: true,
        ..Outcome::line(Line::good(format!("Renamed \"{old}\" to \"{title}\"")))
    })
}

fn mv(store: &mut Store, notes: &[String], dir: &str, numbering: &[String]) -> Result<Outcome> {
    if !dir.is_empty() && !store.dir_exists(dir) {
        return Ok(Outcome::line(Line::bad(format!("No such directory: {dir}/"))));
    }

    let mut lines = Vec::new();
    let mut moved = 0;
    for arg in notes {
        let id = match resolve(arg, store, numbering) {
            Resolved::One(id) => id,
            other => {
                lines.extend(unresolved(arg, other).lines);
                continue;
            }
        };
        match store.move_note(&id, dir) {
            Some(title) => {
                let dest = if dir.is_empty() { "/" } else { dir };
                lines.push(Line::good(format!("Moved \"{title}\" to {dest}")));
                moved += 1;
            }
            None => lines.push(Line::bad(format!("Failed to move note: {arg}"))),
        }
    }

    if moved > 0 {
        store.save()?;
    }
    Ok(Outcome { dirty: moved > 0, ..Outcome::lines(lines) })
}

fn rmdir(store: &mut Store, name: &str, recursive: bool, current_dir: &str) -> Result<Outcome> {
    let full = under(current_dir, name);
    if !store.dir_exists(&full) {
        return Ok(Outcome::line(Line::bad(format!("No such directory: {full}/"))));
    }

    if recursive {
        let (notes, dirs) = store.dir_contents(&full);
        // An empty directory needs no warning, so delete it outright.
        if notes == 0 && dirs <= 1 {
            store.delete_dir_recursive(&full);
            store.save()?;
            return Ok(Outcome {
                dirty: true,
                ..Outcome::line(Line::dim(format!("Removed {full}/")))
            });
        }
        // Otherwise say exactly what will be lost before asking.
        let mut what = Vec::new();
        if notes > 0 {
            what.push(format!("{notes} note{}", plural(notes)));
        }
        if dirs > 1 {
            what.push(format!("{} subdirector{}", dirs - 1, if dirs - 1 == 1 { "y" } else { "ies" }));
        }
        return Ok(Outcome::effect(Effect::Confirm {
            prompt: format!("Delete {full}/ and its {}?", what.join(" and ")),
            on_yes: ConfirmedAction::DeleteDir { path: full },
        }));
    }

    if store.delete_dir(&full) {
        store.save()?;
        Ok(Outcome { dirty: true, ..Outcome::line(Line::dim(format!("Removed {full}/"))) })
    } else {
        // Name the way out rather than just refusing.
        Ok(Outcome::line(Line::bad(format!(
            "{full}/ is not empty — use `rmdir -r {name}` to delete it and its contents"
        ))))
    }
}

/// "s" unless there is exactly one.
fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

// ── Second-phase handlers ───────────────────────────────────────────────────
// These take the result of an Effect the shell performed and finish the work.
// Keeping them pure over `&mut Store` means the editor, microphone, and
// confirmation prompt are the only parts a test cannot exercise.

/// Finish an editor session started by [`Effect::Edit`].
pub fn apply_edit(
    store: &mut Store,
    target: &EditTarget,
    raw: &str,
    ai: &dyn Ai,
) -> Result<Outcome> {
    let (parsed_title, parsed_tags, body) = parse_frontmatter(raw);

    match target {
        EditTarget::NewNote { fallback_title, dir } => {
            if body.trim().is_empty() {
                return Ok(Outcome::line(Line::dim("Empty note, cancelled.")));
            }
            let title = if parsed_title.is_empty() {
                fallback_title.clone()
            } else {
                parsed_title
            };
            let note = store.create_note(title, body, parsed_tags, dir)?;
            let short = note.id[..std::cmp::min(8, note.id.len())].to_string();
            store.save()?;
            Ok(Outcome { dirty: true, ..Outcome::line(Line::good(format!("Created {short}"))) })
        }

        EditTarget::Existing { id, old_title, old_tags, old_body } => {
            let title = if parsed_title.is_empty() { old_title.clone() } else { parsed_title };
            let mut body = body;

            // Expand any @leo prompts the user added, in one pass, before saving.
            let count = body.lines().filter(|l| is_leo_prompt(l).is_some()).count();
            let mut lines = Vec::new();
            if count > 0 {
                match ai.expand_prompts(&body, &title) {
                    Ok((expanded, n)) => {
                        body = expanded;
                        lines.push(Line::dim(format!(
                            "Expanded {n} prompt{}",
                            if n == 1 { "" } else { "s" }
                        )));
                    }
                    // A failed expansion must not lose the user's edit.
                    Err(e) => lines.push(Line::warn(format!("Expansion failed: {e}"))),
                }
            }

            if title == *old_title && parsed_tags == *old_tags && body.trim() == old_body.trim() {
                lines.push(Line::dim("No changes."));
                return Ok(Outcome::lines(lines));
            }

            let note = store
                .find_note_mut(id)
                .ok_or_else(|| anyhow::anyhow!("note {id} disappeared while editing"))?;
            note.title = title.clone();
            note.tags = parsed_tags;
            note.body = body;
            note.updated_at = chrono::Utc::now();
            store.save()?;
            lines.push(Line::good(format!("Updated {title}")));
            Ok(Outcome { dirty: true, ..Outcome::lines(lines) })
        }
    }
}

/// Apply a confirmed destructive action.
pub fn apply_confirmed(store: &mut Store, action: &ConfirmedAction) -> Result<Outcome> {
    match action {
        ConfirmedAction::DeleteNote { id, .. } => {
            if store.delete_note(id) {
                store.save()?;
                Ok(Outcome { dirty: true, ..Outcome::line(Line::good("Deleted.")) })
            } else {
                Ok(Outcome::line(Line::bad("Nothing deleted.")))
            }
        }

        ConfirmedAction::DeleteDir { path } => {
            let (notes, dirs) = store.delete_dir_recursive(path);
            if notes == 0 && dirs == 0 {
                return Ok(Outcome::line(Line::bad("Nothing deleted.")));
            }
            store.save()?;
            let mut parts = vec![format!("Removed {path}/")];
            if notes > 0 {
                parts.push(format!("with {notes} note{}", plural(notes)));
            }
            Ok(Outcome {
                dirty: true,
                ..Outcome::line(Line::good(parts.join(" ")))
            })
        }
    }
}

/// Turn a finished recording's transcript into a saved note.
pub fn apply_transcript(
    store: &mut Store,
    req: &ListenRequest,
    transcript: &str,
    ai: &dyn Ai,
) -> Result<Outcome> {
    if transcript.trim().is_empty() {
        return Ok(Outcome::line(Line::dim("No speech detected.")));
    }

    if let Some(target) = &req.append_to {
        let existing = match store.find_by_index_or_prefix(target) {
            Some(n) => n.body.clone(),
            None => return Ok(Outcome::line(Line::bad(format!("No note found: {target}")))),
        };
        let addition = ai.structure_append(transcript, &existing)?;
        let note = store
            .find_by_index_or_prefix_mut(target)
            .expect("target existed a moment ago");
        note.body = format!("{}\n\n{}", note.body, addition);
        note.updated_at = chrono::Utc::now();
        let title = note.title.clone();
        let short = note.id[..std::cmp::min(8, note.id.len())].to_string();
        store.save()?;
        return Ok(Outcome {
            dirty: true,
            ..Outcome::line(Line::good(format!("Updated \"{title}\" {short}")))
        });
    }

    let (ai_title, body) = ai.structure(transcript)?;
    let title = req.title.clone().unwrap_or(ai_title);
    let note = store.create_note(&title, &body, vec!["listen".to_string()], &req.dir)?;
    let short = note.id[..std::cmp::min(8, note.id.len())].to_string();
    store.save()?;
    Ok(Outcome {
        dirty: true,
        ..Outcome::line(Line::good(format!("Created \"{title}\" {short}")))
    })
}

/// Recompute the note numbering after the store changed.
pub fn numbering_for(store: &Store, dir: &str) -> Vec<String> {
    store
        .list_notes_in_dir(dir, None, usize::MAX)
        .iter()
        .map(|n| n.id.clone())
        .collect()
}

/// The same numbering, narrowed to notes matching `query`.
///
/// Matches titles and tags rather than bodies: this runs on every keystroke, and
/// a filter that suddenly matches a note whose title looks unrelated is more
/// confusing than helpful. Full-text search is what `search -f` is for.
///
/// Case-insensitive, and a blank query matches everything, so an empty filter is
/// the same as no filter.
pub fn filtered_numbering(store: &Store, dir: &str, query: &str) -> Vec<String> {
    let needle = query.trim().to_lowercase();
    store
        .list_notes_in_dir(dir, None, usize::MAX)
        .iter()
        .filter(|n| {
            needle.is_empty()
                || n.title.to_lowercase().contains(&needle)
                || n.tags.iter().any(|t| t.to_lowercase().contains(&needle))
        })
        .map(|n| n.id.clone())
        .collect()
}

// ── Frontmatter and @leo prompts ────────────────────────────────────────────

/// Parse an editor buffer's `---` frontmatter block into (title, tags, body).
/// Malformed or absent frontmatter yields an empty title and tags with the
/// whole buffer as the body, so a user who deletes the header keeps their text.
pub fn parse_frontmatter(raw: &str) -> (String, Vec<String>, String) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), Vec::new(), raw.to_string());
    }

    let after_open = trimmed[3..].trim_start_matches('-');
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);

    let Some(close_pos) = after_open.find("\n---") else {
        return (String::new(), Vec::new(), raw.to_string());
    };

    let front = &after_open[..close_pos];
    let body_start = close_pos + 4; // past "\n---"
    let body = after_open[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&after_open[body_start..]);

    let mut title = String::new();
    let mut tags = Vec::new();
    for line in front.lines() {
        if let Some(val) = line.strip_prefix("title:") {
            title = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("tags:") {
            tags = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    (title, tags, body.to_string())
}

/// If `line` is `@leo <question>`, return the question.
pub fn is_leo_prompt(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.len() < 5 || !trimmed[..5].eq_ignore_ascii_case("@leo ") {
        return None;
    }
    let q = trimmed[5..].trim();
    if q.is_empty() {
        None
    } else {
        Some(q)
    }
}

/// Replace every `@leo` line with the model's answer, giving each one five
/// lines of surrounding context plus the whole note for background. A prompt
/// that fails to expand is left in place rather than dropped.
pub fn expand_leo_prompts(body: &str, title: &str) -> Result<(String, usize)> {
    let lines: Vec<&str> = body.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut count = 0;

    for (i, &line) in lines.iter().enumerate() {
        let Some(question) = is_leo_prompt(line) else {
            result.push(line.to_string());
            continue;
        };

        let before = lines[i.saturating_sub(5)..i].join("\n");
        let after_end = (i + 6).min(lines.len());
        let after = lines[(i + 1)..after_end].join("\n");
        let local_context = format!("{before}\n{after}");

        match crate::ai::expand_prompt(question, &local_context, title, body) {
            Ok(expansion) if !expansion.is_empty() => {
                result.push(expansion);
                count += 1;
            }
            _ => result.push(line.to_string()),
        }
    }

    Ok((result.join("\n"), count))
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn act(line: &str) -> Action {
        match parse(line) {
            Parsed::Action(a) => a,
            other => panic!("expected an action for {line:?}, got {other:?}"),
        }
    }

    fn usage(line: &str) -> String {
        match parse(line) {
            Parsed::Usage(u) => u,
            other => panic!("expected usage for {line:?}, got {other:?}"),
        }
    }

    #[test]
    fn blank_input_is_empty() {
        assert_eq!(parse(""), Parsed::Empty);
        assert_eq!(parse("   "), Parsed::Empty);
        // A bare prefix with nothing after it is not a command either.
        assert_eq!(parse("hey leo"), Parsed::Empty);
    }

    #[test]
    fn unknown_verb_is_reported_with_the_verb() {
        assert_eq!(parse("frobnicate 3"), Parsed::Unknown("frobnicate".to_string()));
    }

    /// Every live alias must parse exactly like the verb it abbreviates.
    #[test]
    fn every_alias_maps_to_the_same_action_as_its_canonical_verb() {
        let pairs = [
            ("ls", "list"),
            ("e 1", "edit 1"),
            ("rm 1", "delete 1"),
            ("x 1 2", "check 1 2"),
            ("?", "help"),
            ("exit", "quit"),
            ("q", "quit"),
        ];
        for (alias, canonical) in pairs {
            assert_eq!(
                parse(alias),
                parse(canonical),
                "alias {alias:?} should parse like {canonical:?}"
            );
        }
    }

    /// A retired alias must name its replacement. Removing a word someone has in
    /// their fingers is only kind if the removal explains itself; "unknown
    /// command: d" reads like a typo.
    #[test]
    fn every_retired_alias_names_a_real_replacement() {
        for (alias, instead, why) in RETIRED {
            match parse(alias) {
                Parsed::Retired { verb, replacement, why: said } => {
                    assert_eq!(verb, *alias);
                    assert_eq!(replacement, *instead);
                    assert_eq!(said, *why);
                    assert!(!why.is_empty(), "{alias} retires without a reason");
                    // A `:` replacement has to be something that actually parses.
                    if let Some(command) = instead.strip_prefix(':') {
                        assert!(
                            !matches!(parse(command), Parsed::Unknown(_) | Parsed::Retired { .. }),
                            "{alias} points at {instead}, which is not a verb"
                        );
                    }
                }
                other => panic!("{alias} should be retired, got {other:?}"),
            }
        }
    }

    /// A retired alias must not also be live, or the table contradicts itself.
    #[test]
    fn no_retired_alias_is_still_in_the_verb_table() {
        for (alias, _, _) in RETIRED {
            assert!(
                !all_verb_words().contains(alias),
                "{alias} is both retired and live"
            );
        }
    }

    /// Verbs left over from the line-oriented shell, where the screen scrolled
    /// and nothing showed the directory. The panes do both now.
    #[test]
    fn pwd_and_clear_point_at_what_replaced_them() {
        match parse("pwd") {
            Parsed::Retired { replacement, .. } => {
                assert!(replacement.contains("status"), "{replacement}")
            }
            other => panic!("expected Retired, got {other:?}"),
        }
        match parse("clear") {
            Parsed::Retired { replacement, .. } => assert_eq!(replacement, "Esc"),
            other => panic!("expected Retired, got {other:?}"),
        }
    }

    /// The point of the prune: one name per command, give or take the few that
    /// come from the shell or mirror a key.
    #[test]
    fn the_vocabulary_stays_small() {
        let aliases: usize = VERBS.iter().map(|(_, a)| a.len()).sum();
        assert!(aliases <= 8, "aliases crept back up to {aliases}");
    }

    #[test]
    fn verbs_are_case_insensitive() {
        assert_eq!(act("LIST"), act("list"));
        assert_eq!(act("View 1"), act("view 1"));
    }

    #[test]
    fn hey_leo_prefix_is_stripped() {
        assert_eq!(
            act("hey leo remind me to call mom"),
            Action::Remind { text: "call mom".to_string() }
        );
        assert_eq!(act("leo list"), Action::List { tag: None, limit: 20 });
        // "leo" alone as the whole line is not a command.
        assert_eq!(parse("leo"), Parsed::Unknown("leo".to_string()));
    }

    #[test]
    fn list_parses_tag_and_limit_in_any_order() {
        assert_eq!(act("list"), Action::List { tag: None, limit: 20 });
        assert_eq!(
            act("list #rust"),
            Action::List { tag: Some("rust".to_string()), limit: 20 }
        );
        assert_eq!(act("list 5"), Action::List { tag: None, limit: 5 });
        assert_eq!(
            act("list 5 #rust"),
            Action::List { tag: Some("rust".to_string()), limit: 5 }
        );
    }

    #[test]
    fn multi_word_note_references_are_joined() {
        assert_eq!(
            act("view Rust ownership notes"),
            Action::View { note: "Rust ownership notes".to_string() }
        );
    }

    #[test]
    fn quoted_arguments_stay_together() {
        assert_eq!(
            act("new \"My Note\""),
            Action::New { title: Some("My Note".to_string()) }
        );
    }

    /// `check` takes the checkbox number as the LAST token, so a multi-word
    /// title in front of it must still resolve.
    #[test]
    fn check_takes_its_number_from_the_end() {
        assert_eq!(
            act("check Rust ownership 3"),
            Action::Check { note: "Rust ownership".to_string(), index: 3 }
        );
    }

    #[test]
    fn check_rejects_a_non_numeric_or_zero_index() {
        assert!(usage("check 1 abc").contains("positive integer"));
        assert!(usage("check 1 0").contains("positive integer"));
        assert!(usage("check 1").contains("check <note>"));
    }

    /// `export` takes the format as the LAST token, same shape as `check`.
    #[test]
    fn export_takes_its_format_from_the_end() {
        assert_eq!(
            act("export Rust ownership md"),
            Action::Export { note: "Rust ownership".to_string(), format: "md".to_string() }
        );
        // Format is lowercased so `MD` works.
        assert_eq!(
            act("export 1 MD"),
            Action::Export { note: "1".to_string(), format: "md".to_string() }
        );
    }

    /// `mv` takes the directory last and any number of notes before it.
    #[test]
    fn mv_takes_the_directory_from_the_end() {
        assert_eq!(
            act("mv 1 2 3 cs130"),
            Action::Mv {
                notes: vec!["1".to_string(), "2".to_string(), "3".to_string()],
                dir: "cs130".to_string(),
            }
        );
        // A trailing slash on the destination is tolerated, and `/` means root.
        assert_eq!(
            act("mv 1 /"),
            Action::Mv { notes: vec!["1".to_string()], dir: String::new() }
        );
    }

    #[test]
    fn search_recognizes_the_full_text_flag() {
        assert_eq!(
            act("search rust"),
            Action::Search { query: "rust".to_string(), full_text: false }
        );
        assert_eq!(
            act("search -f rust"),
            Action::Search { query: "rust".to_string(), full_text: true }
        );
        assert!(usage("search").contains("search"));
        assert!(usage("search -f").contains("search"));
    }

    #[test]
    fn remind_strips_the_natural_phrasing() {
        assert_eq!(
            act("remind me to buy groceries"),
            Action::Remind { text: "buy groceries".to_string() }
        );
        assert_eq!(
            act("remind me buy groceries"),
            Action::Remind { text: "buy groceries".to_string() }
        );
        assert_eq!(
            act("remind buy groceries"),
            Action::Remind { text: "buy groceries".to_string() }
        );
    }

    #[test]
    fn listen_parses_screen_flag_title_and_append_target() {
        assert_eq!(
            act("listen"),
            Action::Listen { title: None, append_to: None, screen: false }
        );
        assert_eq!(
            act("listen CS 101 Lecture"),
            Action::Listen {
                title: Some("CS 101 Lecture".to_string()),
                append_to: None,
                screen: false,
            }
        );
        assert_eq!(
            act("listen add 1"),
            Action::Listen { title: None, append_to: Some("1".to_string()), screen: false }
        );
        // --screen is positional-agnostic and never lands in the title.
        assert_eq!(
            act("listen --screen Lecture 3"),
            Action::Listen {
                title: Some("Lecture 3".to_string()),
                append_to: None,
                screen: true,
            }
        );
        assert_eq!(
            act("listen Lecture 3 --screen"),
            Action::Listen {
                title: Some("Lecture 3".to_string()),
                append_to: None,
                screen: true,
            }
        );
        // No note after `add` means the selected one; see `fill_selected`.
        assert_eq!(act("listen add"), Action::Listen { title: None, append_to: Some(String::new()), screen: false });
    }

    #[test]
    fn cd_accepts_no_argument_as_root() {
        assert_eq!(act("cd"), Action::Cd { path: String::new() });
        assert_eq!(act("cd .."), Action::Cd { path: "..".to_string() });
        assert_eq!(act("cd cs130"), Action::Cd { path: "cs130".to_string() });
    }

    #[test]
    fn sync_subcommands_parse() {
        assert_eq!(act("sync init"), Action::Sync(SyncAction::Init));
        assert_eq!(act("sync push"), Action::Sync(SyncAction::Push));
        assert_eq!(act("sync pull"), Action::Sync(SyncAction::Pull));
        assert_eq!(act("sync status"), Action::Sync(SyncAction::Status));
        assert_eq!(
            act("sync connect https://example.com/n.git"),
            Action::Sync(SyncAction::Connect { url: "https://example.com/n.git".to_string() })
        );
        assert!(usage("sync connect").contains("connect"));
        assert!(usage("sync").contains("init"));
        assert!(usage("sync bogus").contains("init"));
    }

    #[test]
    fn model_subcommands_parse() {
        assert_eq!(act("model list"), Action::Model(ModelAction::List));
        assert_eq!(
            act("model login openrouter"),
            Action::Model(ModelAction::Login { name: "openrouter".to_string() })
        );
        assert_eq!(
            act("model test groq"),
            Action::Model(ModelAction::Test { name: "groq".to_string() })
        );
        assert_eq!(
            act("model logout hf"),
            Action::Model(ModelAction::Logout { name: "hf".to_string() })
        );
        assert!(usage("model login").contains("login"));
        assert!(usage("model").contains("list"));
    }

    #[test]
    fn config_defaults_to_edit() {
        assert_eq!(act("config"), Action::Config(ConfigAction::Edit));
        assert_eq!(act("config path"), Action::Config(ConfigAction::Path));
        assert!(usage("config bogus").contains("edit"));
    }

    #[test]
    fn rmdir_takes_an_opt_in_recursive_flag() {
        assert_eq!(
            act("rmdir cs130"),
            Action::Rmdir { name: "cs130".to_string(), recursive: false }
        );
        for line in ["rmdir -r cs130", "rmdir cs130 -r", "rmdir --recursive cs130"] {
            assert_eq!(
                act(line),
                Action::Rmdir { name: "cs130".to_string(), recursive: true },
                "for {line:?}"
            );
        }
        // The flag alone is not a directory name.
        assert!(matches!(parse("rmdir -r"), Parsed::Usage(_)));
    }

    #[test]
    fn usage_is_returned_for_verbs_missing_a_required_argument() {
        for line in ["view", "mkdir", "rmdir", "export 1", "mv"] {
            assert!(
                matches!(parse(line), Parsed::Usage(_)),
                "{line:?} should report usage"
            );
        }
    }

    #[test]
    fn every_verb_and_alias_in_the_table_parses_to_something_known() {
        for word in all_verb_words() {
            // Bare verbs may legitimately want arguments; what must never
            // happen is a verb in the table being reported as unknown.
            assert!(
                !matches!(parse(word), Parsed::Unknown(_)),
                "{word:?} is in VERBS but parse() calls it unknown"
            );
        }
    }

    #[test]
    fn tokenize_keeps_quoted_runs_and_drops_empty_gaps() {
        assert_eq!(tokenize("a  b\tc"), vec!["a", "b", "c"]);
        assert_eq!(tokenize("new \"two words\""), vec!["new", "two words"]);
        assert_eq!(tokenize("new 'single quoted'"), vec!["new", "single quoted"]);
        assert!(tokenize("   ").is_empty());
    }
}

#[cfg(test)]
mod handler_tests {
    use super::*;

    /// An `Ai` double: records what it was asked and returns canned answers, so
    /// handler tests never touch the network.
    struct FakeAi {
        expand_to: Option<String>,
        structured: (String, String),
        appended: String,
        fail: bool,
    }

    impl Default for FakeAi {
        fn default() -> Self {
            FakeAi {
                expand_to: None,
                structured: ("AI Title".to_string(), "- ai body".to_string()),
                appended: "- appended".to_string(),
                fail: false,
            }
        }
    }

    impl Ai for FakeAi {
        fn expand_prompts(&self, body: &str, _title: &str) -> Result<(String, usize)> {
            if self.fail {
                anyhow::bail!("no provider available");
            }
            match &self.expand_to {
                Some(text) => Ok((text.clone(), 1)),
                None => Ok((body.to_string(), 0)),
            }
        }
        fn structure(&self, _transcript: &str) -> Result<(String, String)> {
            if self.fail {
                anyhow::bail!("no provider available");
            }
            Ok(self.structured.clone())
        }
        fn structure_append(&self, _transcript: &str, _existing: &str) -> Result<String> {
            if self.fail {
                anyhow::bail!("no provider available");
            }
            Ok(self.appended.clone())
        }
    }

    /// A store on a temp directory, the same pattern `store.rs` tests use.
    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (store, dir)
    }

    fn ctx<'a>(dir: &'a str, numbering: &'a [String]) -> Ctx<'a> {
        Ctx { current_dir: dir, numbering, selected: None }
    }

    fn seed(store: &mut Store, title: &str, body: &str, dir: &str) -> String {
        let id = store.create_note(title, body, vec![], dir).unwrap().id.clone();
        store.save().unwrap();
        id
    }

    fn ctx_selected<'a>(numbering: &'a [String], selected: &'a str) -> Ctx<'a> {
        Ctx { current_dir: "", numbering, selected: Some(selected) }
    }

    // ── the selected note ───────────────────────────────────────────────────

    /// Leaving the note out means "the one I'm looking at", so the user never
    /// has to read a number off the screen to act on what is already selected.
    #[test]
    fn verbs_that_take_one_note_parse_without_it() {
        let parsed = |line: &str| match parse(line) {
            Parsed::Action(a) => a,
            other => panic!("{line:?} did not parse: {other:?}"),
        };
        assert_eq!(parsed("edit"), Action::Edit { note: String::new() });
        assert_eq!(parsed("delete"), Action::Delete { note: String::new() });
        assert_eq!(parsed("ask"), Action::Ask { note: String::new() });
        assert_eq!(
            parsed("mv cs130"),
            Action::Mv { notes: vec![], dir: "cs130".to_string() }
        );
        assert_eq!(
            parsed("listen add"),
            Action::Listen { title: None, append_to: Some(String::new()), screen: false }
        );
    }

    #[test]
    fn an_omitted_note_means_the_selected_one() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Other", "", "");
        let id = seed(&mut store, "Graphs", "", "");
        let numbering = numbering_for(&store, "");
        let out = apply(
            Action::Delete { note: String::new() },
            &mut store,
            ctx_selected(&numbering, &id),
            &FakeAi::default(),
        )
        .unwrap();
        match out.effect {
            Effect::Confirm { on_yes: ConfirmedAction::DeleteNote { id: target, .. }, .. } => {
                assert_eq!(target, id)
            }
            other => panic!("expected a delete confirmation, got {other:?}"),
        }
    }

    #[test]
    fn mv_without_notes_moves_the_selected_one() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let id = seed(&mut store, "Graphs", "", "");
        let numbering = numbering_for(&store, "");
        apply(
            Action::Mv { notes: vec![], dir: "cs130".to_string() },
            &mut store,
            ctx_selected(&numbering, &id),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(store.find_note(&id).unwrap().directory, "cs130");
    }

    #[test]
    fn rename_takes_the_new_title_and_means_the_selected_note() {
        let parsed = match parse("rename Graph traversals") {
            Parsed::Action(a) => a,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            parsed,
            Action::Rename { note: String::new(), title: "Graph traversals".to_string() }
        );
        assert!(matches!(parse("rename"), Parsed::Usage(_)));
    }

    #[test]
    fn renaming_changes_only_the_title() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Graphs", "BFS and DFS", "cs130");
        let numbering = numbering_for(&store, "cs130");
        let out = apply(
            Action::Rename { note: String::new(), title: "Graph traversals".to_string() },
            &mut store,
            ctx_selected(&numbering, &id),
            &FakeAi::default(),
        )
        .unwrap();
        let note = store.find_note(&id).unwrap();
        assert_eq!(note.title, "Graph traversals");
        assert_eq!(note.body, "BFS and DFS");
        assert_eq!(note.directory, "cs130");
        assert!(out.dirty);
        // And it survives a reload, so the file on disk was rewritten too.
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().title, "Graph traversals");
    }

    /// The CLI has no selection, so an omitted note has to say so rather than
    /// guess.
    #[test]
    fn an_omitted_note_with_nothing_selected_says_so() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Graphs", "", "");
        let out = apply(
            Action::Edit { note: String::new() },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::None);
        assert!(out.text().contains("No note selected"), "{}", out.text());
    }

    // ── resolution ──────────────────────────────────────────────────────────

    #[test]
    fn resolves_by_list_number_id_prefix_and_unique_title() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Rust ownership", "body", "");
        let numbering = vec![id.clone()];

        assert_eq!(resolve("1", &store, &numbering), Resolved::One(id.clone()));
        assert_eq!(resolve(&id[..8], &store, &numbering), Resolved::One(id.clone()));
        assert_eq!(resolve("ownership", &store, &numbering), Resolved::One(id));
    }

    #[test]
    fn an_out_of_range_number_does_not_resolve() {
        let (mut store, _d) = temp_store();
        // A fixed id, so the assertions below cannot accidentally pass or fail
        // on a random UUID that happens to start with the digit under test.
        let id = "aaaaaaaa-0000-0000-0000-000000000000".to_string();
        store.notes.push(crate::notes::Note {
            id: id.clone(),
            title: "One".to_string(),
            body: "body".to_string(),
            tags: vec![],
            directory: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        let numbering = vec![id];

        // Past the end of the numbering, and 0 which is never a valid index.
        assert_eq!(resolve("7", &store, &numbering), Resolved::None);
        assert_eq!(resolve("0", &store, &numbering), Resolved::None);
    }

    #[test]
    fn an_ambiguous_title_reports_every_candidate_with_its_number() {
        let (mut store, _d) = temp_store();
        let a = seed(&mut store, "Lecture 1 graphs", "b", "");
        let b = seed(&mut store, "Lecture 2 graphs", "b", "");
        let numbering = vec![a, b];

        let Resolved::Many(briefs) = resolve("graphs", &store, &numbering) else {
            panic!("expected an ambiguous match");
        };
        assert_eq!(briefs.len(), 2);
        assert!(briefs.iter().all(|b| b.index.is_some()));

        // And the rendered form names them without claiming a failure.
        let out = unresolved("graphs", resolve("graphs", &store, &numbering));
        assert!(out.text().contains("Multiple notes match"));
        assert!(out.text().contains("Lecture 1 graphs"));
        assert!(out.text().contains("Lecture 2 graphs"));
    }

    // ── read-only handlers ──────────────────────────────────────────────────

    #[test]
    fn list_numbers_notes_and_shows_subdirectories() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let id = seed(&mut store, "Root note", "b", "");
        seed(&mut store, "Nested", "b", "cs130");

        let out = apply(
            Action::List { tag: None, limit: 20 },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(out.selection, Some(vec![id]));
        assert!(out.text().contains("cs130/"), "got: {}", out.text());
        assert!(out.text().contains("Root note"));
        // A note in a subdirectory is not listed at the root.
        assert!(!out.text().contains("Nested"));
    }

    #[test]
    fn list_on_an_empty_store_clears_the_numbering() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::List { tag: None, limit: 20 },
            &mut store,
            ctx("", &["stale".to_string()]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.selection, Some(Vec::new()));
        assert!(out.text().contains("No notes yet"));
    }

    #[test]
    fn search_renumbers_across_directories() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Root graphs", "b", "");
        seed(&mut store, "Nested graphs", "b", "cs130");

        let out = apply(
            Action::Search { query: "graphs".to_string(), full_text: false },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(out.selection.as_ref().map(|s| s.len()), Some(2));
        assert!(out.text().contains("cs130/"), "search shows the directory: {}", out.text());
    }

    #[test]
    fn view_asks_the_shell_to_show_the_resolved_note() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Note", "b", "");
        let out = apply(
            Action::View { note: "1".to_string() },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::ShowNote { id });
    }

    #[test]
    fn a_handler_given_an_unresolvable_note_reports_it_and_does_nothing() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::View { note: "nope".to_string() },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::None);
        assert!(out.text().contains("No note found: nope"));
    }

    #[test]
    fn tags_counts_each_tag() {
        let (mut store, _d) = temp_store();
        store.create_note("A", "b", vec!["rust".to_string()], "").unwrap();
        store.create_note("B", "b", vec!["rust".to_string()], "").unwrap();
        store.save().unwrap();

        let out = apply(Action::Tags, &mut store, ctx("", &[]), &FakeAi::default()).unwrap();
        assert!(out.text().contains("#rust (2)"), "got: {}", out.text());
    }

    // ── mutating handlers ───────────────────────────────────────────────────

    #[test]
    fn check_toggles_a_checkbox_and_persists() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Tasks", "- [ ] first\n- [ ] second", "");

        let out = apply(
            Action::Check { note: "1".to_string(), index: 1 },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.dirty);
        assert!(store.find_note(&id).unwrap().body.contains("- [x] first"));

        // Reloading from disk proves it was saved, not just mutated in memory.
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(reloaded.find_note(&id).unwrap().body.contains("- [x] first"));
    }

    #[test]
    fn check_reports_an_out_of_range_checkbox_without_saving() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Tasks", "- [ ] only one", "");
        let out = apply(
            Action::Check { note: "1".to_string(), index: 9 },
            &mut store,
            ctx("", &[id]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(!out.dirty);
        assert!(out.text().contains("No checkbox #9"));
    }

    #[test]
    fn remind_creates_the_note_then_appends_to_it() {
        let (mut store, _d) = temp_store();

        let first = apply(
            Action::Remind { text: "buy milk".to_string() },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(first.text().contains("Created Reminders"));
        assert_eq!(store.notes.len(), 1);

        let second = apply(
            Action::Remind { text: "call mom".to_string() },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(second.text().contains("Added call mom"));
        // Still one note, now with both items.
        assert_eq!(store.notes.len(), 1);
        let body = &store.notes[0].body;
        assert!(body.contains("- [ ] buy milk"));
        assert!(body.contains("- [ ] call mom"));
    }

    #[test]
    fn mkdir_creates_relative_to_the_current_directory() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");

        apply(
            Action::Mkdir { name: "lec".to_string() },
            &mut store,
            ctx("cs130", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(store.dir_exists("cs130/lec"));
    }

    #[test]
    fn mkdir_on_an_existing_directory_is_not_an_error() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let out = apply(
            Action::Mkdir { name: "cs130".to_string() },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("already exists"));
        assert!(!out.dirty);
    }

    #[test]
    fn rmdir_refuses_a_non_empty_directory() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        seed(&mut store, "Nested", "b", "cs130");

        let out = apply(
            Action::Rmdir { name: "cs130".to_string(), recursive: false },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("not empty"), "got: {}", out.text());
        assert!(store.dir_exists("cs130"));
    }

    /// Deleting a directory with notes in it must ask first, and the prompt has
    /// to say how much is at stake — this is the only action in leo that can
    /// destroy more than one note.
    #[test]
    fn a_recursive_rmdir_asks_before_deleting_and_names_the_damage() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        seed(&mut store, "One", "b", "cs130");
        seed(&mut store, "Two", "b", "cs130/lec");

        let out = apply(
            Action::Rmdir { name: "cs130".to_string(), recursive: true },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        match out.effect {
            Effect::Confirm { prompt, on_yes } => {
                assert!(prompt.contains("cs130/"), "prompt: {prompt}");
                assert!(prompt.contains("2 notes"), "prompt: {prompt}");
                assert!(prompt.contains("1 subdirectory"), "prompt: {prompt}");
                assert_eq!(on_yes, ConfirmedAction::DeleteDir { path: "cs130".to_string() });
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
        // Nothing is gone yet.
        assert!(store.dir_exists("cs130"));
        assert_eq!(store.notes.len(), 2);
    }

    #[test]
    fn confirming_removes_the_directory_and_its_contents() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        seed(&mut store, "One", "b", "cs130");
        seed(&mut store, "Two", "b", "cs130/lec");
        seed(&mut store, "Elsewhere", "b", "");

        let out = apply_confirmed(
            &mut store,
            &ConfirmedAction::DeleteDir { path: "cs130".to_string() },
        )
        .unwrap();

        assert!(out.dirty);
        assert!(out.text().contains("2 notes"), "got: {}", out.text());
        assert!(!store.dir_exists("cs130"));
        assert!(!store.dir_exists("cs130/lec"));
        assert_eq!(store.notes.len(), 1, "the unrelated note survives");

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 1);
    }

    /// An empty directory is not worth a prompt.
    #[test]
    fn a_recursive_rmdir_on_an_empty_directory_just_does_it() {
        let (mut store, _d) = temp_store();
        store.create_dir("empty");

        let out = apply(
            Action::Rmdir { name: "empty".to_string(), recursive: true },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(out.effect, Effect::None);
        assert!(out.dirty);
        assert!(!store.dir_exists("empty"));
    }

    /// The plain form still refuses, but now says how to proceed.
    #[test]
    fn a_plain_rmdir_on_a_full_directory_points_at_the_recursive_form() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        seed(&mut store, "One", "b", "cs130");

        let out = apply(
            Action::Rmdir { name: "cs130".to_string(), recursive: false },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert!(out.text().contains("rmdir -r cs130"), "got: {}", out.text());
        assert!(store.dir_exists("cs130"));
        assert_eq!(store.notes.len(), 1);
    }

    #[test]
    fn mv_moves_several_notes_and_reports_each() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let a = seed(&mut store, "One", "b", "");
        let b = seed(&mut store, "Two", "b", "");

        let out = apply(
            Action::Mv {
                notes: vec!["1".to_string(), "2".to_string()],
                dir: "cs130".to_string(),
            },
            &mut store,
            ctx("", &[a.clone(), b.clone()]),
            &FakeAi::default(),
        )
        .unwrap();

        assert!(out.dirty);
        assert_eq!(store.find_note(&a).unwrap().directory, "cs130");
        assert_eq!(store.find_note(&b).unwrap().directory, "cs130");
        assert_eq!(out.lines.iter().filter(|l| l.kind == Kind::Good).count(), 2);
    }

    #[test]
    fn mv_to_a_missing_directory_moves_nothing() {
        let (mut store, _d) = temp_store();
        let a = seed(&mut store, "One", "b", "");
        let out = apply(
            Action::Mv { notes: vec!["1".to_string()], dir: "ghost".to_string() },
            &mut store,
            ctx("", std::slice::from_ref(&a)),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("No such directory"));
        assert_eq!(store.find_note(&a).unwrap().directory, "");
    }

    #[test]
    fn mv_keeps_going_when_one_reference_is_bad() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let a = seed(&mut store, "One", "b", "");

        let out = apply(
            Action::Mv {
                notes: vec!["1".to_string(), "ghost-note".to_string()],
                dir: "cs130".to_string(),
            },
            &mut store,
            ctx("", std::slice::from_ref(&a)),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(store.find_note(&a).unwrap().directory, "cs130");
        assert!(out.text().contains("No note found: ghost-note"));
        assert!(out.dirty, "the one note that did move must still be saved");
    }

    // ── cd ──────────────────────────────────────────────────────────────────

    #[test]
    fn cd_navigates_up_down_and_to_root() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        store.create_dir("cs162");

        let cases: &[(&str, &str, &str)] = &[
            // (from, argument, expected)
            ("", "cs130", "cs130"),
            ("cs130", "lec", "cs130/lec"),
            ("cs130/lec", "..", "cs130"),
            ("cs130", "..", ""),
            ("", "..", ""),
            ("cs130/lec", "/", ""),
            ("cs130/lec", "", ""),
            ("cs130/lec", "~", ""),
            ("cs130", "/cs162", "cs162"),
            ("cs130/lec", "../../cs162", "cs162"),
        ];
        for (from, arg, expected) in cases {
            assert_eq!(
                resolve_cd(arg, &store, from).as_deref(),
                Ok(*expected),
                "cd {arg:?} from {from:?}"
            );
        }
    }

    #[test]
    fn cd_into_a_missing_directory_is_an_error_and_does_not_move() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::Cd { path: "ghost".to_string() },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.new_dir, None);
        assert!(out.text().contains("No such directory"));
    }

    // ── editor round trip ───────────────────────────────────────────────────

    #[test]
    fn new_requests_an_editor_seeded_with_the_title() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::New { title: Some("My Note".to_string()) },
            &mut store,
            ctx("cs130", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        let Effect::Edit(req) = out.effect else {
            panic!("expected an edit request, got {:?}", out.effect);
        };
        assert!(req.seed.contains("title: My Note"));
        assert_eq!(
            req.target,
            EditTarget::NewNote {
                fallback_title: "My Note".to_string(),
                dir: "cs130".to_string()
            }
        );
    }

    #[test]
    fn applying_an_edited_new_note_creates_it_in_the_right_directory() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "Fallback".to_string(),
            dir: "cs130".to_string(),
        };
        let raw = "---\ntitle: Real Title\ntags: rust, cli\n---\nThe body.";

        let out = apply_edit(&mut store, &target, raw, &FakeAi::default()).unwrap();
        assert!(out.dirty);
        assert_eq!(store.notes.len(), 1);
        let note = &store.notes[0];
        assert_eq!(note.title, "Real Title");
        assert_eq!(note.tags, vec!["rust", "cli"]);
        assert_eq!(note.body.trim(), "The body.");
        assert_eq!(note.directory, "cs130");
    }

    #[test]
    fn an_empty_body_cancels_note_creation() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "T".to_string(),
            dir: String::new(),
        };
        let out = apply_edit(&mut store, &target, "---\ntitle: T\ntags: \n---\n   \n", &FakeAi::default())
            .unwrap();
        assert!(out.text().contains("cancelled"));
        assert!(store.notes.is_empty());
    }

    #[test]
    fn a_deleted_title_line_falls_back_to_the_typed_title() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "Typed Title".to_string(),
            dir: String::new(),
        };
        // No frontmatter at all: the whole buffer is the body.
        let out = apply_edit(&mut store, &target, "just a body", &FakeAi::default()).unwrap();
        assert!(out.dirty);
        assert_eq!(store.notes[0].title, "Typed Title");
        assert_eq!(store.notes[0].body, "just a body");
    }

    #[test]
    fn an_unchanged_edit_reports_no_changes_and_does_not_touch_the_note() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Title", "body", "");
        let before = store.find_note(&id).unwrap().updated_at;

        let target = EditTarget::Existing {
            id: id.clone(),
            old_title: "Title".to_string(),
            old_tags: vec![],
            old_body: "body".to_string(),
        };
        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: Title\ntags: \n---\nbody",
            &FakeAi::default(),
        )
        .unwrap();

        assert!(!out.dirty);
        assert!(out.text().contains("No changes"));
        assert_eq!(store.find_note(&id).unwrap().updated_at, before);
    }

    #[test]
    fn editing_expands_leo_prompts_before_saving() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Title", "old body", "");
        let target = EditTarget::Existing {
            id: id.clone(),
            old_title: "Title".to_string(),
            old_tags: vec![],
            old_body: "old body".to_string(),
        };
        let ai = FakeAi {
            expand_to: Some("- the expanded answer".to_string()),
            ..FakeAi::default()
        };

        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: Title\ntags: \n---\n@leo what is BFS?",
            &ai,
        )
        .unwrap();

        assert!(out.dirty);
        assert_eq!(store.find_note(&id).unwrap().body, "- the expanded answer");
        assert!(out.text().contains("Expanded 1 prompt"));
    }

    /// A failed expansion must never cost the user their edit.
    #[test]
    fn a_failed_expansion_still_saves_the_edit_and_warns() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Title", "old body", "");
        let target = EditTarget::Existing {
            id: id.clone(),
            old_title: "Title".to_string(),
            old_tags: vec![],
            old_body: "old body".to_string(),
        };
        let ai = FakeAi { fail: true, ..FakeAi::default() };

        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: Title\ntags: \n---\nnew text\n@leo what is BFS?",
            &ai,
        )
        .unwrap();

        assert!(out.dirty, "the edit must be saved even though expansion failed");
        assert!(out.text().contains("Expansion failed"));
        let body = &store.find_note(&id).unwrap().body;
        assert!(body.contains("new text"));
        assert!(body.contains("@leo what is BFS?"), "the prompt line is kept");
    }

    // ── delete confirmation ─────────────────────────────────────────────────

    #[test]
    fn delete_asks_for_confirmation_before_removing_anything() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Doomed", "b", "");

        let out = apply(
            Action::Delete { note: "1".to_string() },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(
            out.effect,
            Effect::Confirm {
                prompt: "Delete Doomed?".to_string(),
                on_yes: ConfirmedAction::DeleteNote { id: id.clone(), title: "Doomed".to_string() },
            }
        );
        // Nothing is gone yet.
        assert!(store.find_note(&id).is_some());
    }

    #[test]
    fn a_confirmed_delete_removes_the_note_from_disk() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Doomed", "b", "");

        let out = apply_confirmed(
            &mut store,
            &ConfirmedAction::DeleteNote { id: id.clone(), title: "Doomed".to_string() },
        )
        .unwrap();

        assert!(out.dirty);
        assert!(store.find_note(&id).is_none());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(reloaded.find_note(&id).is_none());
    }

    // ── listen round trip ───────────────────────────────────────────────────

    #[test]
    fn listen_validates_the_append_target_before_recording() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::Listen {
                title: None,
                append_to: Some("ghost".to_string()),
                screen: false,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::None, "must not start recording");
        assert!(out.text().contains("No note found: ghost"));
    }

    #[test]
    fn listen_requests_a_recording_carrying_the_current_directory() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::Listen {
                title: Some("Lecture 3".to_string()),
                append_to: None,
                screen: true,
            },
            &mut store,
            ctx("cs130", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(
            out.effect,
            Effect::Listen(ListenRequest {
                screen: true,
                title: Some("Lecture 3".to_string()),
                append_to: None,
                dir: "cs130".to_string(),
            })
        );
    }

    #[test]
    fn a_transcript_becomes_a_new_note_titled_by_the_model() {
        let (mut store, _d) = temp_store();
        let req = ListenRequest {
            screen: false,
            title: None,
            append_to: None,
            dir: "cs130".to_string(),
        };
        let out = apply_transcript(&mut store, &req, "some speech", &FakeAi::default()).unwrap();

        assert!(out.dirty);
        assert_eq!(store.notes.len(), 1);
        assert_eq!(store.notes[0].title, "AI Title");
        assert_eq!(store.notes[0].directory, "cs130");
        assert_eq!(store.notes[0].tags, vec!["listen"]);
    }

    #[test]
    fn a_user_supplied_title_wins_over_the_models() {
        let (mut store, _d) = temp_store();
        let req = ListenRequest {
            screen: false,
            title: Some("My Title".to_string()),
            append_to: None,
            dir: String::new(),
        };
        apply_transcript(&mut store, &req, "some speech", &FakeAi::default()).unwrap();
        assert_eq!(store.notes[0].title, "My Title");
    }

    #[test]
    fn an_empty_transcript_creates_nothing() {
        let (mut store, _d) = temp_store();
        let req = ListenRequest {
            screen: false,
            title: None,
            append_to: None,
            dir: String::new(),
        };
        let out = apply_transcript(&mut store, &req, "   ", &FakeAi::default()).unwrap();
        assert!(out.text().contains("No speech detected"));
        assert!(store.notes.is_empty());
        assert!(!out.dirty);
    }

    #[test]
    fn appending_a_transcript_keeps_the_existing_body() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Lecture", "## Existing\n- old point", "");
        let req = ListenRequest {
            screen: false,
            title: None,
            append_to: Some(id.clone()),
            dir: String::new(),
        };

        apply_transcript(&mut store, &req, "more speech", &FakeAi::default()).unwrap();
        let body = &store.find_note(&id).unwrap().body;
        assert!(body.contains("- old point"), "existing content survives");
        assert!(body.contains("- appended"));
        assert_eq!(store.notes.len(), 1, "append must not create a second note");
    }

    // ── ask ─────────────────────────────────────────────────────────────────

    #[test]
    fn ask_on_a_note_without_prompts_does_nothing() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Plain", "no prompts here", "");
        let out = apply(
            Action::Ask { note: "1".to_string() },
            &mut store,
            ctx("", &[id]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("No @leo prompts"));
        assert!(!out.dirty);
    }

    #[test]
    fn ask_replaces_the_prompt_line_with_the_expansion() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Graphs", "@leo what is BFS?", "");
        let ai = FakeAi {
            expand_to: Some("- BFS explores level by level".to_string()),
            ..FakeAi::default()
        };

        let out = apply(
            Action::Ask { note: "1".to_string() },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &ai,
        )
        .unwrap();

        assert!(out.dirty);
        assert_eq!(
            store.find_note(&id).unwrap().body,
            "- BFS explores level by level"
        );
    }

    /// A retired verb must point at its replacement. Answering "unknown
    /// command" would read like a typo and send the user hunting.
    #[test]
    fn the_retired_env_verb_names_its_replacement() {
        match parse("env") {
            Parsed::Retired {
                verb,
                replacement,
                why,
            } => {
                assert_eq!(verb, "env");
                assert!(replacement.contains("model login"), "{replacement}");
                assert!(why.contains("keychain"), "{why}");
            }
            other => panic!("expected Retired, got {other:?}"),
        }
    }

    /// It must also be gone from the vocabulary, so it stops appearing in
    /// completion and help.
    #[test]
    fn env_is_no_longer_a_verb() {
        assert!(
            !VERBS.iter().any(|(v, aliases)| *v == "env"
                || aliases.contains(&"env")),
            "env is still in the verb table"
        );
    }

    // ── shell-delegated actions ─────────────────────────────────────────────

    #[test]
    fn sync_model_and_config_are_delegated_to_the_shell_untouched() {
        let (mut store, _d) = temp_store();
        let ai = FakeAi::default();
        let cases = [
            (Action::Sync(SyncAction::Push), Effect::Sync(SyncAction::Push)),
            (Action::Model(ModelAction::List), Effect::Model(ModelAction::List)),
            (Action::Config(ConfigAction::Path), Effect::Config(ConfigAction::Path)),
            (Action::Help, Effect::ShowHelp),
            (Action::Quit, Effect::Quit),
        ];
        for (action, expected) in cases {
            let out = apply(action.clone(), &mut store, ctx("", &[]), &ai).unwrap();
            assert_eq!(out.effect, expected, "for {action:?}");
        }
    }

    // ── frontmatter and prompts ─────────────────────────────────────────────

    #[test]
    fn frontmatter_parses_title_tags_and_body() {
        let (title, tags, body) =
            parse_frontmatter("---\ntitle: T\ntags: a, b\n---\nbody line\nsecond");
        assert_eq!(title, "T");
        assert_eq!(tags, vec!["a", "b"]);
        assert_eq!(body, "body line\nsecond");
    }

    #[test]
    fn absent_or_malformed_frontmatter_keeps_the_whole_buffer_as_body() {
        let raw = "no frontmatter here";
        assert_eq!(parse_frontmatter(raw), (String::new(), vec![], raw.to_string()));

        let unterminated = "---\ntitle: T\nbody with no closing marker";
        assert_eq!(
            parse_frontmatter(unterminated),
            (String::new(), vec![], unterminated.to_string())
        );
    }

    #[test]
    fn empty_tags_do_not_become_an_empty_tag() {
        let (_, tags, _) = parse_frontmatter("---\ntitle: T\ntags: \n---\nbody");
        assert!(tags.is_empty());
        let (_, tags, _) = parse_frontmatter("---\ntitle: T\ntags: a, , b\n---\nbody");
        assert_eq!(tags, vec!["a", "b"]);
    }

    #[test]
    fn leo_prompt_detection_is_case_insensitive_and_needs_a_question() {
        assert_eq!(is_leo_prompt("@leo what is BFS?"), Some("what is BFS?"));
        assert_eq!(is_leo_prompt("  @LEO what is BFS?  "), Some("what is BFS?"));
        assert_eq!(is_leo_prompt("@leo"), None);
        assert_eq!(is_leo_prompt("@leo   "), None);
        assert_eq!(is_leo_prompt("email me @leo later"), None);
        assert_eq!(is_leo_prompt(""), None);
        // Must not panic on a short or multi-byte line.
        assert_eq!(is_leo_prompt("@le"), None);
        assert_eq!(is_leo_prompt("héllo"), None);
    }

    #[test]
    fn numbering_reflects_the_current_directory() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let root = seed(&mut store, "Root", "b", "");
        let nested = seed(&mut store, "Nested", "b", "cs130");

        assert_eq!(numbering_for(&store, ""), vec![root]);
        assert_eq!(numbering_for(&store, "cs130"), vec![nested]);
    }

    /// Every note gets a number, not only the twenty newest: the panes list the
    /// whole directory, and `leo view 30` must work after `leo list --limit 50`.
    #[test]
    fn numbering_covers_every_note_in_the_directory() {
        let (mut store, _d) = temp_store();
        for i in 0..25 {
            store.create_note(format!("Note {i}"), "", vec![], "").unwrap();
        }
        store.save().unwrap();
        assert_eq!(numbering_for(&store, "").len(), 25);
    }
}
