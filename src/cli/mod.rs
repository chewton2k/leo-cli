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

    #[command(about = "Pin a note to the top of its list, or unpin it")]
    Pin {
        #[arg(help = "Note number, ID prefix or title")]
        id: String,
    },

    #[command(about = "Deleted notes, kept 30 days: list them, restore one, or empty the trash")]
    Trash {
        #[command(subcommand)]
        command: Option<TrashCommands>,
    },

    /// Start a web server to view/edit notes from your phone
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3131)]
        port: u16,

        #[arg(
            long,
            help = "Also open a public link that works from any network, through a Cloudflare tunnel (needs cloudflared)"
        )]
        anywhere: bool,

        #[arg(long, help = "Make a new link, so every old one stops working")]
        new_token: bool,
    },

    #[command(about = "Open your notes in Obsidian (they are already Markdown files it can read)")]
    Obsidian,

    #[command(
        about = "Check that everything works (leo, your notes, the AI, recording, backup) and say how to fix what does not"
    )]
    Doctor,

    #[command(about = "Update leo to the latest release, if there is a newer one")]
    Update {
        #[arg(long, help = "Reinstall even when already on the latest version")]
        force: bool,
    },

    #[command(about = "Remove leo from this computer. Your notes, settings and keys stay")]
    Uninstall {
        #[arg(short, long, help = "Do not ask first")]
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
    #[command(about = "Bring a note back to where it was")]
    Restore {
        #[arg(
            required = true,
            num_args = 1..,
            help = "Its number in `leo trash`, or part of its title"
        )]
        which: Vec<String>,
    },
    #[command(about = "Delete everything in the trash for good")]
    Empty {
        #[arg(short, long, help = "Skip the confirmation")]
        force: bool,
    },
}

#[derive(Subcommand)]
enum SyncCommands {
    #[command(
        about = "Back up to a private GitHub repository, made for you with GitHub's gh tool, or joined if you already have one"
    )]
    Github {
        #[arg(default_value = leo_core::sync::GITHUB_REPO, help = "The repository's name")]
        name: String,
    },
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

const EXAMPLES: &[(&str, &str)] = &[
    ("leo", "open the app"),
    (
        "leo new \"Lecture 4 #exam\"",
        "a note tagged exam; \"cs130/ Lecture 4\" puts it in cs130",
    ),
    (
        "leo list",
        "notes at the top level; add a folder: leo list cs130",
    ),
    ("leo list --tag exam", "only notes tagged exam"),
    ("leo search graphs", "every note that mentions it"),
    (
        "leo view 2",
        "a note, by its number in leo list, its title or ID",
    ),
    ("leo edit 2", "open it in your editor"),
    (
        "leo delete 2",
        "move it to the trash (--force skips the question)",
    ),
    ("leo pin 2", "keep it at the top of its list"),
    ("leo trash", "what was deleted"),
    ("leo trash restore 1", "bring one back"),
    ("leo trash empty", "delete the trash for good"),
    ("leo listen", "record, then turn it into notes"),
    ("leo listen --add 2", "add a recording to a note"),
    ("leo ask 2", "answer the @leo lines in a note"),
    ("leo serve", "your notes on your phone, on the same Wi-Fi"),
    (
        "leo serve --anywhere",
        "the same from any network (needs cloudflared)",
    ),
    (
        "leo serve --new-token",
        "a new link; old links stop working",
    ),
    ("leo sync", "back up to GitHub now"),
    ("leo sync github", "set up backup with GitHub's gh tool"),
    ("leo sync connect <url>", "back up to a repository you made"),
    ("leo obsidian", "open your notes in Obsidian"),
    (
        "leo doctor",
        "check that everything works, and store an API key",
    ),
    ("leo update", "install a newer version, if there is one"),
    ("leo uninstall", "remove leo; your notes stay"),
];

fn examples() -> String {
    let width = EXAMPLES.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
    let mut out = String::from("How to use it:\n");
    for (command, what) in EXAMPLES {
        out.push_str(&format!("  {command:<width$}   {what}\n"));
    }
    out.push_str("\nEvery command's options: leo <command> --help");
    out
}

pub fn parse() -> Cli {
    use clap::{CommandFactory, FromArgMatches};
    let matches = Cli::command().after_help(examples()).get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// Run whatever the command line asked for.
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Commands::Serve {
            port,
            anywhere,
            new_token,
        }) => tokio::runtime::Runtime::new()?.block_on(leo_web::serve(leo_web::ServeOptions {
            port,
            anywhere,
            new_token,
        })),
        Some(Commands::Doctor) => doctor::run(),
        Some(Commands::Uninstall { yes }) => uninstall::run(yes),
        Some(Commands::Update { force }) => update::run(force),
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
        for old in [
            &["leo", "setup"][..],
            &["leo", "model", "list"],
            &["leo", "config", "path"],
            &["leo", "env"],
        ] {
            assert!(Cli::try_parse_from(old).is_err(), "{old:?} still parses");
        }
    }

    #[test]
    fn every_example_in_the_help_is_a_real_command() {
        for (example, _) in EXAMPLES {
            let args: Vec<String> = example
                .replace("<url>", "https://github.com/me/notes.git")
                .split('"')
                .enumerate()
                .flat_map(|(i, part)| {
                    if i % 2 == 1 {
                        vec![part.to_string()]
                    } else {
                        part.split_whitespace().map(str::to_string).collect()
                    }
                })
                .collect();
            assert!(
                Cli::try_parse_from(&args).is_ok(),
                "the help shows `{example}`, which does not parse"
            );
        }
    }

    #[test]
    fn the_help_shows_how_to_use_serve() {
        let text = examples();
        assert!(text.contains("leo serve --anywhere"), "{text}");
        assert!(text.contains("leo <command> --help"), "{text}");
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
