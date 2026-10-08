//! The command-line surface: clap definitions and one module per group of
//! subcommands. Each converts its arguments into the same `Action`s and
//! provider calls the TUI uses, so the two cannot drift apart.

mod backup;
mod doctor;
mod notes;
mod prompt;
mod serve;
mod uninstall;
mod update;
mod web_settings;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum RecordWhat {
    Screen,
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
    #[command(hide = true)]
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
    #[command(hide = true)]
    View {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Edit an existing note in $EDITOR
    #[command(hide = true)]
    Edit {
        /// Note ID (or unique prefix)
        id: String,
    },

    /// Move a note to the trash
    #[command(hide = true)]
    Delete {
        /// Note ID (or unique prefix)
        id: String,

        #[arg(short, long, hide = true)]
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

    /// Record, and turn what was said into a note
    Record {
        /// Optional title (AI generates one if omitted)
        #[arg(short, long)]
        title: Option<String>,

        /// Append to an existing note instead of creating a new one
        #[arg(short, long)]
        add: Option<String>,

        #[arg(
            value_enum,
            help = "`screen` records what the computer plays instead of the microphone"
        )]
        what: Option<RecordWhat>,

        #[arg(long, hide = true)]
        screen: bool,
    },

    #[command(about = "Ask a question, answered from your notes")]
    Ask {
        #[arg(required = true, num_args = 1.., help = "The question, e.g. what is due on Friday?")]
        question: Vec<String>,
    },

    #[command(about = "Pin a note to the top of its list, or unpin it", hide = true)]
    Pin {
        #[arg(help = "Note number, ID prefix or title")]
        id: String,
    },

    #[command(
        about = "Deleted notes, kept 30 days: list them, restore one, or empty the trash",
        hide = true
    )]
    Trash {
        #[command(subcommand)]
        command: Option<TrashCommands>,
    },

    /// Your notes on your phone, from any network
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3131)]
        port: u16,

        #[arg(
            long,
            help = "Only on this Wi-Fi, without the link that works from anywhere (needs no cloudflared)"
        )]
        local: bool,

        #[arg(long, hide = true)]
        anywhere: bool,

        #[arg(long, help = "Make a new link, so every old one stops working")]
        new_token: bool,
    },

    #[command(
        about = "Open your notes in Obsidian (they are already Markdown files it can read)",
        hide = true
    )]
    Obsidian,

    #[command(
        about = "Check that everything works (leo, your notes, the AI, recording, backup) and say how to fix what does not"
    )]
    Doctor,

    #[command(
        about = "Update leo to the latest release, if there is a newer one, and make sure the speech model is there and intact",
        hide = true
    )]
    Update {
        #[arg(long, help = "Reinstall even when already on the latest version")]
        force: bool,
    },

    #[command(
        about = "Remove leo and everything it made from this computer. Your notes stay",
        hide = true
    )]
    Uninstall {
        #[arg(short, long, help = "Do not ask first")]
        yes: bool,
    },

    #[command(about = "Back up your notes now, or set backup up the first time")]
    Backup {
        #[command(subcommand)]
        command: Option<BackupCommands>,
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
enum BackupCommands {
    #[command(
        about = "Back up to a private GitHub repository, made for you with GitHub's gh tool, or joined if you already have one"
    )]
    Github {
        #[arg(default_value = leo_core::sync::GITHUB_REPO, help = "The repository's name")]
        name: String,
    },
    #[command(hide = true)]
    Init,
    /// Back up to a repository you made, by its URL
    Connect {
        /// Remote URL, e.g. https://github.com/user/leo-notes.git
        /// or git@github.com:user/leo-notes.git (SSH)
        url: String,
    },
    #[command(hide = true)]
    Push,
    #[command(hide = true)]
    Pull,
    #[command(hide = true)]
    Status,
}

const EXAMPLES: &[(&str, &str)] = &[
    ("leo", "open the app"),
    (
        "leo new \"Lecture 4 #exam\"",
        "a note tagged exam; \"cs130/ Lecture 4\" puts it in cs130",
    ),
    ("leo search graphs", "every note that mentions it"),
    ("leo record", "record, then turn what was said into a note"),
    (
        "leo ask \"what is due Friday?\"",
        "an answer from your notes",
    ),
    ("leo serve", "your notes on your phone, from any network"),
    ("leo backup", "back up now, or set backup up the first time"),
    (
        "leo doctor",
        "check that everything works, and fix what does not",
    ),
];

const MORE_EXAMPLES: &[(&str, &str)] = &[
    (
        "leo list",
        "notes at the top level; add a folder: leo list cs130",
    ),
    ("leo list --tag exam", "only notes tagged exam"),
    (
        "leo view 2",
        "a note, by its number in leo list, its title or ID",
    ),
    ("leo edit 2", "open it in your editor"),
    ("leo delete 2", "move it to the trash"),
    ("leo pin 2", "keep it at the top of its list"),
    ("leo trash", "what was deleted"),
    ("leo trash restore 1", "bring one back"),
    ("leo trash empty", "delete the trash for good"),
    ("leo record --add 2", "add a recording to a note"),
    (
        "leo serve --new-token",
        "a new link; old links stop working",
    ),
    (
        "leo serve --local",
        "only on this Wi-Fi, without cloudflared",
    ),
    ("leo backup github", "set up backup with GitHub's gh tool"),
    (
        "leo backup connect <url>",
        "back up to a repository you made",
    ),
    ("leo obsidian", "open your notes in Obsidian"),
    ("leo update", "install a newer version, if there is one"),
    ("leo uninstall", "remove leo; your notes stay"),
];

fn examples(all: bool) -> String {
    let shown: Vec<&(&str, &str)> = if all {
        EXAMPLES.iter().chain(MORE_EXAMPLES).collect()
    } else {
        EXAMPLES.iter().collect()
    };
    let width = shown.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
    let mut out = String::from("How to use it:\n");
    for (command, what) in shown {
        out.push_str(&format!("  {command:<width$}   {what}\n"));
    }
    if all {
        out.push_str("\nEvery command's options: leo <command> --help");
    } else {
        out.push_str("\nEvery command: leo help --all");
    }
    out
}

fn wants_everything(args: &[String]) -> bool {
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    matches!(
        rest.as_slice(),
        ["help", "--all"] | ["--help", "--all"] | ["-h", "--all"] | ["--all", "--help"]
    )
}

fn command(all: bool) -> clap::Command {
    use clap::CommandFactory;
    let command = Cli::command().after_help(examples(all));
    if all {
        command.mut_subcommands(|sub| sub.hide(false))
    } else {
        command
    }
}

pub fn parse() -> Cli {
    use clap::FromArgMatches;
    let args: Vec<String> = std::env::args().collect();
    if wants_everything(&args) {
        let _ = command(true).print_help();
        std::process::exit(0);
    }
    let matches = command(false).get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// Run whatever the command line asked for.
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Commands::Serve {
            port,
            local,
            anywhere: _,
            new_token,
        }) => {
            if !local {
                serve::ensure_tunnel_tool()?;
            }
            let writer: leo_web::Writer =
                std::sync::Arc::new(|system: &str, user: &str, most: u32| {
                    leo_services::ai::chat_outcome(
                        leo_services::ai::chat::Prompt {
                            system: system.to_string(),
                            user: user.to_string(),
                        },
                        most,
                    )
                    .map(|outcome| outcome.value)
                });
            let streamer: leo_web::Streamer = std::sync::Arc::new(
                |system: &str,
                 user: &str,
                 most: u32,
                 piece: &mut dyn FnMut(&str),
                 restart: &mut dyn FnMut()| {
                    leo_services::ai::chat_streaming(
                        leo_services::ai::chat::Prompt {
                            system: system.to_string(),
                            user: user.to_string(),
                        },
                        most,
                        piece,
                        restart,
                    )
                },
            );
            tokio::runtime::Runtime::new()?.block_on(leo_web::serve(
                leo_web::ServeOptions {
                    port,
                    local,
                    new_token,
                },
                leo_web::Powers {
                    writer: Some(writer),
                    chat: Some(streamer),
                    settings: Some(std::sync::Arc::new(web_settings::WebSettings)),
                    importer: Some(std::sync::Arc::new(
                        |files: Vec<leo_web::UploadFile>,
                         progress: &mut dyn FnMut(&str, usize, usize)| {
                            let uploads: Vec<leo_services::import::Upload> = files
                                .into_iter()
                                .map(|f| leo_services::import::Upload {
                                    name: f.name,
                                    mime: f.mime,
                                    bytes: f.bytes,
                                })
                                .collect();
                            leo_services::import::import(&uploads, progress)
                        },
                    )),
                },
            ))
        }
        Some(Commands::Doctor) => doctor::run(),
        Some(Commands::Uninstall { yes }) => uninstall::run(yes),
        Some(Commands::Update { force }) => update::run(force),
        Some(Commands::Backup { command }) => backup::run(command),
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
    fn doctor_and_bare_backup_parse() {
        assert!(matches!(
            Cli::try_parse_from(["leo", "doctor"]).unwrap().command,
            Some(Commands::Doctor)
        ));
        assert!(matches!(
            Cli::try_parse_from(["leo", "backup"]).unwrap().command,
            Some(Commands::Backup { command: None })
        ));
    }

    #[test]
    fn old_names_are_gone() {
        for old in [
            &["leo", "setup"][..],
            &["leo", "model", "list"],
            &["leo", "config", "path"],
            &["leo", "env"],
            &["leo", "sync"],
            &["leo", "sync", "github"],
            &["leo", "listen"],
        ] {
            assert!(Cli::try_parse_from(old).is_err(), "{old:?} still parses");
        }
    }

    #[test]
    fn ask_takes_a_question_of_several_words() {
        match Cli::try_parse_from(["leo", "ask", "what", "is", "due?"])
            .unwrap()
            .command
        {
            Some(Commands::Ask { question }) => assert_eq!(question.join(" "), "what is due?"),
            _ => panic!("ask did not parse"),
        }
    }

    #[test]
    fn every_example_in_the_help_is_a_real_command() {
        for (example, _) in EXAMPLES.iter().chain(MORE_EXAMPLES) {
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
    fn the_short_help_points_at_the_full_one() {
        let text = examples(false);
        assert!(text.contains("leo serve "), "{text}");
        assert!(!text.contains("--anywhere"), "{text}");
        assert!(text.contains("leo help --all"), "{text}");
        let all = examples(true);
        assert!(all.contains("leo trash restore 1"), "{all}");
        assert!(all.contains("leo <command> --help"), "{all}");
    }

    fn listed(help: &str) -> Vec<String> {
        help.lines()
            .skip_while(|l| !l.starts_with("Commands:"))
            .skip(1)
            .take_while(|l| l.starts_with("  "))
            .filter_map(|l| l.split_whitespace().next().map(str::to_string))
            .collect()
    }

    #[test]
    fn the_top_level_help_lists_only_the_everyday_commands() {
        let help = command(false).render_help().to_string();
        let shown = listed(&help);
        for everyday in [
            "new", "search", "record", "ask", "serve", "backup", "doctor",
        ] {
            assert!(
                shown.iter().any(|c| c == everyday),
                "{everyday} missing:\n{help}"
            );
        }
        for hidden in [
            "list",
            "view",
            "edit",
            "delete",
            "pin",
            "trash",
            "obsidian",
            "update",
            "uninstall",
        ] {
            assert!(
                !shown.iter().any(|c| c == hidden),
                "{hidden} is listed:\n{help}"
            );
        }
        assert!(shown.len() <= 8, "{shown:?}");
    }

    #[test]
    fn help_all_lists_every_command() {
        let help = command(true).render_help().to_string();
        let shown = listed(&help);
        for every in [
            "list",
            "trash",
            "obsidian",
            "update",
            "uninstall",
            "record",
            "backup",
        ] {
            assert!(shown.iter().any(|c| c == every), "{every} missing:\n{help}");
        }
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(wants_everything(&args(&["leo", "help", "--all"])));
        assert!(wants_everything(&args(&["leo", "--help", "--all"])));
        assert!(!wants_everything(&args(&["leo", "help"])));
    }
}
