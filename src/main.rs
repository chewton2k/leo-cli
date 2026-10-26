mod cli;

use anyhow::Result;

fn main() -> Result<()> {
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);
    // Load .env from the leo data directory only, never the current one: a
    // project's .env must not change where leo keeps notes or how it behaves.
    if let Ok(data_dir) = leo_core::paths::data_dir() {
        dotenvy::from_path(data_dir.join(".env")).ok();
    }
    cli::run(cli::parse())
}
