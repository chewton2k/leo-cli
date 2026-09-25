mod cli;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    // Load .env from the leo data directory so the installed binary finds it
    // regardless of working directory. Also try current directory for development.
    if let Ok(data_dir) = leo_core::paths::data_dir() {
        dotenvy::from_path(data_dir.join(".env")).ok();
    }
    dotenvy::dotenv().ok();
    cli::run(cli::Cli::parse())
}
