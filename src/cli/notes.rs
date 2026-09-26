//! Note subcommands: `new`, `list`, `edit` and the rest, run through the same
//! handlers as the `:` line.

use anyhow::Result;

use super::Commands;
use leo_core::{action, manual, store};
use leo_services::ai;
use leo_tui::shell;

/// Turn a CLI subcommand into an [`action::Action`], so scripting and the
/// interactive shell share one implementation. The few CLI-only affordances —
/// `new --body` skipping the editor and `delete --force` skipping the
/// confirmation — are handled here rather than in the vocabulary.
pub fn run(cmd: Commands) -> Result<()> {
    let mut store = store::Store::load()?;
    // Also on the CLI path, so `leo list` right after installing shows the
    // manual rather than an empty store.
    let _ = manual::install_if_absent(&mut store);
    let ai = ai::RealAi;
    // Scripting has no "last list", so references resolve against the default
    // sorted list — the same numbering `leo list` prints.
    let numbering = action::numbering_for(&store, "");

    // `new --body` is a non-interactive create, with no editor round trip.
    if let Commands::New {
        title,
        body: Some(body),
        tags,
    } = cmd
    {
        let (dir, title, mut named) = action::split_new(&store, &title, "");
        named.extend(tags);
        if !store.dir_exists(&dir) {
            store.create_dir(&dir);
        }
        let note = store.create_note(title, body, named, &dir)?;
        let short = note.id[..8].to_string();
        store.save()?;
        println!("Created note {short}");
        return Ok(());
    }

    let force_delete = matches!(
        cmd,
        Commands::Delete { force: true, .. }
            | Commands::Trash {
                command: Some(super::TrashCommands::Empty { force: true })
            }
    );

    // `leo list cs130` lists inside that directory; everything else works from
    // the top level.
    let current_dir = match &cmd {
        Commands::List { dir: Some(dir), .. } => {
            let dir = dir.trim_matches('/').to_string();
            if !store.dir_exists(&dir) {
                anyhow::bail!("No such directory: {dir}/");
            }
            dir
        }
        _ => String::new(),
    };

    let action = match cmd {
        Commands::New { title, .. } => action::Action::New { title: Some(title) },
        Commands::List { tag, limit, .. } => action::Action::List { tag, limit },
        Commands::View { id } => action::Action::View { note: id },
        Commands::Edit { id } => action::Action::Edit { note: id },
        Commands::Delete { id, .. } => action::Action::Delete { note: id },
        Commands::Search { query, .. } => action::Action::Search { query },
        Commands::Listen { title, add, screen } => action::Action::Listen {
            title,
            append_to: add,
            screen,
        },
        Commands::Ask { id } => action::Action::Ask { note: id },
        Commands::Pin { id } => action::Action::Pin { note: id },
        Commands::Trash { command } => action::Action::Trash(match command {
            None => action::TrashAction::List,
            Some(super::TrashCommands::Restore { which }) => action::TrashAction::Restore {
                which: which.join(" "),
            },
            Some(super::TrashCommands::Empty { .. }) => action::TrashAction::Empty,
        }),

        Commands::Serve { .. }
        | Commands::Doctor
        | Commands::Uninstall { .. }
        | Commands::Sync { .. } => {
            unreachable!("handled in main()")
        }
    };

    let outcome = action::apply(
        action,
        &mut store,
        action::Ctx {
            current_dir: &current_dir,
            numbering: &numbering,
            selected: None,
            marked: &[],
        },
        &ai,
    )?;
    absorb_cli(outcome, &mut store, &ai, force_delete)
}

/// Perform a CLI outcome's effect. Unlike the interactive shell there is no
/// screen to clear, no help pane, and nothing to quit.
fn absorb_cli(
    outcome: action::Outcome,
    store: &mut store::Store,
    ai: &ai::RealAi,
    force_delete: bool,
) -> Result<()> {
    shell::render(&outcome.lines);

    let next = match outcome.effect {
        action::Effect::None => return Ok(()),

        action::Effect::ShowNote { id } => {
            if let Some(note) = store.find_note(&id) {
                note.print_full();
            }
            return Ok(());
        }

        action::Effect::AskNotes { question } => {
            let notes: Vec<(String, String, String)> = store
                .relevant(&question, 6)
                .into_iter()
                .map(|n| (n.title.clone(), n.directory.clone(), n.body.clone()))
                .collect();
            if notes.is_empty() {
                println!("None of your notes mention that.");
                return Ok(());
            }
            // Print as it arrives; a fallback to another provider starts again.
            use std::io::Write;
            let answer = ai::answer_from_notes(
                &question,
                &notes,
                &mut |fragment| {
                    print!("{fragment}");
                    let _ = std::io::stdout().flush();
                },
                &mut || println!("\n(trying another provider)\n"),
            )?;
            if !answer.ends_with('\n') {
                println!();
            }
            return Ok(());
        }

        action::Effect::Edit(req) => shell::run_editor(store, req, ai)?,
        action::Effect::Confirm { prompt, on_yes } => {
            shell::confirm(store, &prompt, on_yes, force_delete)?
        }
        action::Effect::Listen(req) => shell::record_and_apply(store, req, ai)?,

        // Reachable only through the interactive shell.
        action::Effect::ShowHelp
        | action::Effect::Doctor
        | action::Effect::Quit
        | action::Effect::Sync(_) => return Ok(()),
    };

    shell::render(&next.lines);
    Ok(())
}
