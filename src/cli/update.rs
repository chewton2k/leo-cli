//! `leo update`: install the latest release over this copy of leo.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

/// Runs the same installer as the one-line install, pointed at the directory
/// this leo is in, so the update replaces it rather than landing somewhere
/// else on the PATH. The installer does the downloading, checking and the
/// "updated from … to …" message; it is told to leave shell files alone, since
/// leo is already reachable.
pub fn run() -> Result<()> {
    let exe = std::env::current_exe().context("could not tell where leo is installed")?;
    let dir = exe.parent().unwrap_or(Path::new("."));

    // Tests hand in the local installer instead of downloading one.
    let mut cmd = match std::env::var_os("LEO_UPDATE_SCRIPT") {
        Some(script) => {
            let mut cmd = Command::new("sh");
            cmd.arg(script);
            cmd
        }
        None => {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(leo_services::update::install_command());
            cmd
        }
    };
    let status = cmd
        .env("LEO_INSTALL_DIR", dir)
        .env("LEO_INSTALL_SKIP_PATH", "1")
        .status()
        .context("could not run the installer (is curl installed?)")?;
    if !status.success() {
        anyhow::bail!("the update did not finish; leo was left as it was");
    }
    Ok(())
}
