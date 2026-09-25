//! The command-line surface: clap definitions and one module per group of
//! subcommands. Each converts its arguments into the same `Action`s and
//! provider calls the TUI uses, so the two cannot drift apart.

mod notes;
mod prompt;
mod setup;
mod sync;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{Parser, Subcommand};

use leo_services::providers;

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

    /// List all notes (newest first)
    List {
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

    /// Start a web server to view/edit notes from your phone
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3131)]
        port: u16,
    },

    /// See what works here, and fix what does not: AI keys, recording, backup
    Setup,

    /// The report half of `setup`, without the questions.
    #[command(hide = true)]
    Doctor,

    /// Retired. Use `leo model login`, which stores keys for you.
    #[command(hide = true)]
    Env,

    /// Back up your notes to git: pull, then push. Sets backup up the first time.
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommands>,
    },

    /// Inspect, test, and authenticate AI model providers
    #[command(hide = true)]
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },

    /// Open or show the leo model config file
    #[command(hide = true)]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ModelCommands {
    /// Show configured chains, reachability, and credential status
    List,
    /// Send one minimal request to a provider to check it works
    Test {
        /// Provider name from your config
        name: String,
    },
    /// Store a provider's API key (kept in a file only you can read)
    Login {
        /// Provider name from your config
        name: String,
    },
    /// Remove a provider's stored API key
    Logout {
        /// Provider name from your config
        name: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Open config.toml in $EDITOR, creating it if absent
    Edit,
    /// Print the path to config.toml
    Path,
}

impl From<ModelCommands> for providers::ModelAction {
    fn from(c: ModelCommands) -> Self {
        match c {
            ModelCommands::List => providers::ModelAction::List,
            ModelCommands::Test { name } => providers::ModelAction::Test { name },
            ModelCommands::Login { name } => providers::ModelAction::Login { name },
            ModelCommands::Logout { name } => providers::ModelAction::Logout { name },
        }
    }
}

impl From<ConfigCommands> for providers::ConfigAction {
    fn from(c: ConfigCommands) -> Self {
        match c {
            ConfigCommands::Edit => providers::ConfigAction::Edit,
            ConfigCommands::Path => providers::ConfigAction::Path,
        }
    }
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
        // Kept only so the name explains itself instead of erroring. It used to
        // write a plaintext `.env`, whose vars take precedence over the
        // keychain — so a file made months ago could silently shadow a key
        // stored the recommended way.
        Some(Commands::Env) => {
            println!("  `leo env` is gone: `leo setup` stores keys for you.");
            println!("  Env vars still work and still take precedence, for CI.");
            Ok(())
        }
        Some(Commands::Setup) => setup::run(),
        Some(Commands::Doctor) => setup::doctor(),
        Some(Commands::Sync { command }) => sync::run(command),
        Some(Commands::Model { command }) => providers::model(command.into()),
        Some(Commands::Config { command }) => providers::config_file(command.into()),
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

    /// Setup is the one command for "what works, and fix what does not", and
    /// `leo sync` alone backs up; the old names keep working for scripts.
    #[test]
    fn setup_and_bare_sync_parse() {
        assert!(matches!(
            Cli::try_parse_from(["leo", "setup"]).unwrap().command,
            Some(Commands::Setup)
        ));
        assert!(matches!(
            Cli::try_parse_from(["leo", "sync"]).unwrap().command,
            Some(Commands::Sync { command: None })
        ));
        for old in [
            &["leo", "doctor"][..],
            &["leo", "model", "list"],
            &["leo", "config", "path"],
        ] {
            assert!(Cli::try_parse_from(old).is_ok(), "{old:?} stopped parsing");
        }
    }

    /// The top-level help lists the commands someone needs, not every alias.
    #[test]
    fn the_top_level_help_is_short() {
        use clap::CommandFactory;
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("setup"), "{help}");
        for hidden in ["doctor", "model", "config"] {
            assert!(
                !help.contains(&format!("  {hidden} ")),
                "{hidden} is still listed:\n{help}"
            );
        }
    }
}
