use std::io::IsTerminal;

use anyhow::Result;

use leo_core::store::Store;
use leo_services::config::{self, Config};
use leo_services::doctor::{self, Probe};
use leo_services::providers;

pub fn run() -> Result<()> {
    println!();
    println!("  Checking leo, your notes, the AI, recording and backup.");
    println!("  This sends one small request to each AI in use and listens to the");
    println!("  microphone for half a second.");
    println!();

    let config = Config::load();
    let secrets = config::secret::default_store();
    let sections = doctor::scan(
        &config,
        secrets.as_ref(),
        &Store::notes_dir()?,
        &Config::config_path()?,
        Probe::all(),
    );
    let (lines, failed) = doctor::report(&sections);
    leo_tui::shell::render(&lines);

    println!();
    if failed == 0 {
        println!("  Everything checked out.");
    } else {
        println!(
            "  {failed} problem{} found; each says how to fix it above.",
            if failed == 1 { "" } else { "s" }
        );
    }
    println!();

    if std::io::stdin().is_terminal() {
        let missing = providers::providers_missing_keys(&config, secrets.as_ref());
        if !missing.is_empty() {
            let name = super::prompt::ask(&format!(
                "  Store an API key now? Which provider ({}), or Enter to skip: ",
                missing.join(", ")
            ))?;
            if !name.is_empty() {
                providers::model(providers::ModelAction::Login { name })?;
            }
        }
    }

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
