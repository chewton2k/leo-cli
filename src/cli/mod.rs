//! The command-line surface: clap definitions and one module per group of
//! subcommands. Each converts its arguments into the same `Action`s and
//! provider calls the TUI uses, so the two cannot drift apart.

mod doctor;
mod notes;
mod prompt;
mod sync;
mod uninstall;
mod update;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// leo — notes for programmers.
/// Run with no arguments to enter the interactive terminal.
#[derive(Parser)]
#[command(name = "leo", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new note
    New {
        /// Title, optionally led by an existing `dir/` and followed by #tags,
        /// e.g. "cs130/Lecture 4 #exam"
        title: String,

        /// Body text
        #[arg(short, long, allow_hyphen_values = true)]
        body: Option<String>,

        /// Tags, comma-separated (e.g. rust,cli)
        #[arg(short, long, value_delimiter = ',')]
        tags: Vec<String>,
    },

    /// List notes and directories, newest first: the top level, or DIR
    List {
        /// A directory to list instead of the top level, e.g. cs130
        dir: Option<String>,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Maximum number of notes to show
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },

    /// View the full content of a note
    View {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Edit an existing note in $EDITOR
    Edit {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Delete a note
    Delete {
        /// Note ID (or unique prefix)
        id: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Search every note: titles, bodies, and #tags
    Search {
        /// Search query; every word must match, and #word means a tag
        query: String,

        /// Accepted for old scripts. Bodies are always searched now.
        #[arg(short, long, hide = true)]
        full_text: bool,
    },

    /// Record audio and create structured notes from speech
    Listen {
        /// Optional title (AI generates one if omitted)
        #[arg(short, long)]
        title: Option<String>,

        /// Append to an existing note instead of creating a new one
        #[arg(short, long)]
        add: Option<String>,

        /// Capture system audio instead of microphone (requires BlackHole: brew install blackhole-2ch)
        #[arg(long)]
        screen: bool,
    },

    /// Expand all @leo prompts in a note using AI
    Ask {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Pin a note to the top of its list, or unpin it
    Pin {
        /// Note number, ID prefix or title
        id: String,
    },

    /// Deleted notes, kept 30 days: list them, restore one, or empty the trash
    Trash {
        #[command(subcommand)]
        command: Option<TrashCommands>,
    },

    /// Start a web server to view/edit notes from your phone
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3131)]
        port: u16,
    },

    /// Check that everything works — leo, your notes, the AI, recording,
    /// backup — and say how to fix what does not
    Doctor,

    /// Update leo to the latest release
    Update,

    /// Remove leo from this computer. Your notes, settings and keys stay
    Uninstall {
        /// Do not ask first
        #[arg(short, long)]
        yes: bool,
    },

    /// Back up your notes to git: pull, then push. Sets backup up the first time.
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommands>,
    },
}

#[derive(Subcommand)]
enum TrashCommands {
    /// Bring a note back to where it was
    Restore {
        /// Its number in `leo trash`, or part of its title
        #[arg(required = true, num_args = 1..)]
        which: Vec<String>,
    },
    /// Delete everything in the trash for good
    Empty {
        /// Skip the confirmation
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum SyncCommands {
    /// Initialize a git repo for your notes (run this first)
    Init,
    /// Connect the notes repo to a GitHub remote
    Connect {
        /// Remote URL, e.g. https://github.com/user/leo-notes.git
        /// or git@github.com:user/leo-notes.git (SSH)
        url: String,
    },
    /// Push notes to the remote
    Push,
    /// Pull notes from the remote
    Pull,
    /// Show git status of the notes repo
    Status,
}

/// Run whatever the command line asked for.
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Commands::Serve { port }) => {
            tokio::runtime::Runtime::new()?.block_on(leo_web::serve(port))
        }
        Some(Commands::Doctor) => doctor::run(),
        Some(Commands::Uninstall { yes }) => uninstall::run(yes),
        Some(Commands::Update) => update::run(),
        Some(Commands::Sync { command }) => sync::run(command),
        None => {
            if std::io::stdin().is_terminal() {
                leo_tui::run()
            } else {
                eprintln!(
                    "leo: interactive mode requires a terminal. Use subcommands for scripting."
                );
                std::process::exit(1);
            }
        }
        Some(cmd) => notes::run(cmd),
    }
}
#[cfg(test)]
mod cli_tests {
    use super::*;

    /// Doctor is the one command for "what works, and fix what does not", and
    /// `leo sync` alone backs up.
    #[test]
    fn doctor_and_bare_sync_parse() {
        assert!(matches!(
            Cli::try_parse_from(["leo", "doctor"]).unwrap().command,
            Some(Commands::Doctor)
        ));
        assert!(matches!(
            Cli::try_parse_from(["leo", "sync"]).unwrap().command,
            Some(Commands::Sync { command: None })
        ));
        // The old names are gone: doctor and Ctrl-S cover them.
        for old in [
            &["leo", "setup"][..],
            &["leo", "model", "list"],
            &["leo", "config", "path"],
            &["leo", "env"],
        ] {
            assert!(Cli::try_parse_from(old).is_err(), "{old:?} still parses");
        }
    }

    /// The top-level help lists the commands someone needs, not every alias.
    #[test]
    fn the_top_level_help_is_short() {
        use clap::CommandFactory;
        let help = Cli::command().render_help().to_string();
        assert!(
            help.contains("doctor"),
            "the health scan is not listed:\n{help}"
        );
        for hidden in ["setup", "model", "config"] {
            assert!(
                !help.contains(&format!("  {hidden} ")),
                "{hidden} is still listed:\n{help}"
            );
        }
    }
}
