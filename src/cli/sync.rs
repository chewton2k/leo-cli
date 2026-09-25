//! `leo sync`: back up, or set backup up the first time.

use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use super::SyncCommands;
use leo_core::{store, sync};

pub fn run(command: Option<SyncCommands>) -> Result<()> {
    let store = store::Store::load()?;
    match command {
        None => sync_or_set_up(&store.notes_dir),
        Some(command) => run_sync_command(command, &store.notes_dir),
    }
}

fn run_sync_command(command: SyncCommands, notes_dir: &std::path::Path) -> Result<()> {
    match command {
        SyncCommands::Init => sync::init(notes_dir),
        SyncCommands::Connect { url } => sync::connect(notes_dir, &url),
        SyncCommands::Push => sync::push(notes_dir),
        SyncCommands::Pull => sync::pull(notes_dir),
        SyncCommands::Status => sync::status(notes_dir),
    }
}

/// `leo sync` on its own: back up, or — the first time — ask for the remote,
/// set the repository up, and push.
fn sync_or_set_up(notes_dir: &std::path::Path) -> Result<()> {
    if sync::is_initialized(notes_dir) && sync::remote_url(notes_dir).is_some() {
        return sync::now(notes_dir);
    }
    if !std::io::stdin().is_terminal() {
        return sync::now(notes_dir);
    }
    println!("  Backup is not set up yet. Make an empty repository on GitHub — or, on a");
    println!("  second computer, use the one your notes are already backed up to — then");
    let url = super::prompt::ask(
        "  paste its URL (e.g. git@github.com:you/notes.git), or Enter to skip: ",
    )?;
    if url.is_empty() {
        return Ok(());
    }
    if !sync::is_initialized(notes_dir) {
        sync::init(notes_dir)?;
    }
    sync::connect(notes_dir, &url)?;
    // Pulls first, so notes already in the repository come down too.
    sync::now(notes_dir)?;
    println!(
        "  {} Backed up. From now on, `leo sync` does it again.",
        "ok".green()
    );
    Ok(())
}
