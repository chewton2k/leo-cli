//! `leo backup`: back up, or set backup up the first time.

use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use super::BackupCommands;
use leo_core::{store, sync};

pub fn run(command: Option<BackupCommands>) -> Result<()> {
    let store = store::Store::load()?;
    match command {
        None => sync_or_set_up(&store.notes_dir),
        Some(command) => run_sync_command(command, &store.notes_dir),
    }
}

fn run_sync_command(command: BackupCommands, notes_dir: &std::path::Path) -> Result<()> {
    match command {
        BackupCommands::Init => {
            sync::init(notes_dir)?;
            println!(
                "  {} Backup repository ready in {}",
                "ok".green(),
                notes_dir.display()
            );
            Ok(())
        }
        BackupCommands::Connect { url } => sync::connect(notes_dir, &url),
        BackupCommands::Push => sync::push(notes_dir),
        BackupCommands::Pull => sync::pull(notes_dir),
        BackupCommands::Status => sync::status(notes_dir),
        BackupCommands::Github { name } => github(notes_dir, &name),
    }
}

fn github(notes_dir: &std::path::Path, name: &str) -> Result<()> {
    let backup = sync::github(notes_dir, name)?;
    println!("  {} {}", "ok".green(), backup.describe());
    println!("  From now on, `leo backup` backs up again.");
    Ok(())
}

/// `leo backup` on its own: back up, or — the first time — ask for the remote,
/// set the repository up, and push.
fn sync_or_set_up(notes_dir: &std::path::Path) -> Result<()> {
    if sync::is_initialized(notes_dir) && sync::remote_url(notes_dir).is_some() {
        return sync::now(notes_dir);
    }
    if !std::io::stdin().is_terminal() {
        return sync::now(notes_dir);
    }
    if sync::gh_ready() {
        let answer = super::prompt::ask(&format!(
            "  Back up to a private GitHub repository, {}? It is made for you, or joined\n  if you already have one. [Y/n] ",
            sync::GITHUB_REPO
        ))?;
        if answer.is_empty()
            || answer.eq_ignore_ascii_case("y")
            || answer.eq_ignore_ascii_case("yes")
        {
            return github(notes_dir, sync::GITHUB_REPO);
        }
    } else {
        println!("  Tip: with GitHub's gh tool (brew install gh, then gh auth login),");
        println!("  `leo backup github` sets this up in one step.");
        println!();
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
        "  {} Backed up. From now on, `leo backup` does it again.",
        "ok".green()
    );
    Ok(())
}
