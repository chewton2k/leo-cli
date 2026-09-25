mod action;
mod ai;
mod cli;
mod config;
mod diag;
mod health;
mod listen;
mod manual;
mod notes;
mod providers;
mod shell;
mod store;
mod sync;
mod tui;
mod web;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    // Load .env from the leo data directory so the installed binary finds it
    // regardless of working directory. Also try current directory for development.
    if let Some(data_dir) = dirs::data_dir() {
        dotenvy::from_path(data_dir.join("leo").join(".env")).ok();
    }
    dotenvy::dotenv().ok();
    cli::run(cli::Cli::parse())
}
