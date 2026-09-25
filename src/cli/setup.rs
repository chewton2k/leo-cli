//! `leo setup`: what works here, and fixing what does not.

use std::io::IsTerminal;

use anyhow::Result;

use crate::config::{self, Config};
use crate::{health, providers, store, sync};

/// `leo setup`: what works, where things live, and a key stored on the spot
/// for anything the AI chains are missing.
pub fn run() -> Result<()> {
    doctor()?;
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

pub fn doctor() -> Result<()> {
    use health::State;

    let config = Config::load();
    let checks = health::report(&config, config::secret::default_store().as_ref());

    println!();
    let mut missing = 0;
    for check in &checks {
        let (mark, label) = match &check.state {
            State::Ready => ("ok  ", "".to_string()),
            State::Warn { note } => ("note", note.clone()),
            State::Missing { .. } => {
                missing += 1;
                ("no  ", String::new())
            }
        };
        let detail = check
            .detail
            .as_deref()
            .map(|d| format!(" — {d}"))
            .unwrap_or_default();
        println!("  {mark} {}{detail}", check.what);
        if !label.is_empty() {
            println!("       {label}");
        }
        if let State::Missing { fix } = &check.state {
            println!("       needed for {}", check.needed_for);
            for line in fix.lines() {
                println!("       {}", line.trim());
            }
        }
    }

    println!();
    if missing == 0 {
        println!("  Everything leo can use is available.");
    } else {
        println!(
            "  {missing} thing{} missing. Notes, search, export to text and sync work regardless.",
            if missing == 1 { " is" } else { "s are" }
        );
    }
    println!();
    Ok(())
}
