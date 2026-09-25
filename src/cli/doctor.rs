//! `leo doctor`: the full health scan, printed by section.

use anyhow::Result;

use leo_core::store::Store;
use leo_services::config::{self, Config};
use leo_services::doctor::{self, Probe};

pub fn run() -> Result<()> {
    println!();
    println!("  Checking leo, your notes, the AI, recording and backup.");
    println!("  This sends one small request to each AI in use and listens to the");
    println!("  microphone for half a second.");

    let config = Config::load();
    let notes_dir = Store::notes_dir()?;
    let config_path = Config::config_path()?;
    let sections = doctor::scan(
        &config,
        config::secret::default_store().as_ref(),
        &notes_dir,
        &config_path,
        Probe::all(),
    );

    let mut failed = 0;
    for section in &sections {
        println!();
        println!("{}", section.title);
        for check in &section.checks {
            if super::setup::print_check(check) {
                failed += 1;
            }
        }
    }

    println!();
    if failed == 0 {
        println!("  Everything checked out.");
        println!();
        Ok(())
    } else {
        println!(
            "  {failed} problem{} found; each says how to fix it above.",
            if failed == 1 { "" } else { "s" }
        );
        println!();
        std::process::exit(1);
    }
}
