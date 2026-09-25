//! `leo setup`: what works here, and fixing what does not.

use std::io::IsTerminal;

use anyhow::Result;

use leo_core::{store, sync};
use leo_services::config::{self, Config};
use leo_services::{health, providers};

/// `leo setup`: what works, where things live, and a key stored on the spot
/// for anything the AI chains are missing.
pub fn run() -> Result<()> {
    report()?;
    providers::model(providers::ModelAction::List)?;

    let store = store::Store::load()?;
    println!("  notes   {}", store.notes_dir.display());
    println!("  config  {}", Config::config_path()?.display());
    println!();

    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    let cfg = Config::load();
    let missing = providers::providers_missing_keys(&cfg, config::secret::default_store().as_ref());
    if !missing.is_empty() {
        let name = super::prompt::ask(&format!(
            "  Store an API key now? Which provider ({}), or Enter to skip: ",
            missing.join(", ")
        ))?;
        if !name.is_empty() {
            providers::model(providers::ModelAction::Login { name })?;
        }
    }
    if !sync::is_initialized(&store.notes_dir) {
        println!("  Backup is off. `leo sync` sets it up.");
    }
    Ok(())
}

/// The quick report `setup` opens with: what is installed and what is not.
/// `leo doctor` is the thorough version.
fn report() -> Result<()> {
    let config = Config::load();
    let checks = health::report(&config, config::secret::default_store().as_ref());

    println!();
    let missing = checks.iter().filter(|c| print_check(c)).count();
    println!();
    if missing == 0 {
        println!("  Everything leo can use is available.");
    } else {
        println!(
            "  {missing} thing{} missing. Notes, search and backup work regardless.",
            if missing == 1 { " is" } else { "s are" }
        );
    }
    println!();
    Ok(())
}

/// Print one check: ok, note, or no with what it is for and how to fix it.
/// Returns whether it failed.
pub(super) fn print_check(check: &health::Check) -> bool {
    use health::State;
    let mark = match &check.state {
        State::Ready => "ok  ",
        State::Warn { .. } => "note",
        State::Missing { .. } => "no  ",
    };
    let detail = check
        .detail
        .as_deref()
        .map(|d| format!(" — {d}"))
        .unwrap_or_default();
    println!("  {mark} {}{detail}", check.what);
    match &check.state {
        State::Ready => false,
        State::Warn { note } => {
            println!("       {note}");
            false
        }
        State::Missing { fix } => {
            println!("       needed for {}", check.needed_for);
            for line in fix.lines() {
                println!("       {}", line.trim());
            }
            true
        }
    }
}
