use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use colored::Colorize;

fn ensure_speech_model() {
    if std::env::var_os("LEO_INSTALL_SKIP_MODEL").is_some() {
        return;
    }
    use leo_services::providers::ModelState;
    match leo_services::providers::speech_model_state() {
        ModelState::Ready => return,
        ModelState::Missing => {
            println!();
            println!("  The speech model is missing. Downloading base.en (142 MB, once)…");
        }
        ModelState::Damaged => {
            println!();
            println!("  The speech model is damaged. Downloading base.en again (142 MB)…");
        }
    }
    match leo_services::providers::download_speech_model() {
        Ok(path) => println!("  Saved {}", path.display()),
        Err(e) => println!(
            "  {}",
            format!("Could not download it ({e}). /settings in leo can try again.").dimmed()
        ),
    }
}

pub fn run(force: bool) -> Result<()> {
    ensure_speech_model();
    let current = env!("CARGO_PKG_VERSION");
    if !force {
        match leo_services::update::latest_release() {
            Ok(latest) if !leo_services::update::is_newer(&latest, current) => {
                println!();
                println!("  leo {current} is the latest version. Nothing to download.");
                println!(
                    "  {}",
                    "`leo update --force` reinstalls it anyway.".dimmed()
                );
                println!();
                return Ok(());
            }
            Ok(latest) => {
                println!();
                println!("  leo {latest} is out (you have {current}). Updating…");
            }
            Err(_) => {
                println!();
                println!(
                    "  {}",
                    "Could not ask GitHub for the latest version; trying the installer.".dimmed()
                );
            }
        }
    }
    let exe = std::env::current_exe().context("could not tell where leo is installed")?;
    let dir = exe.parent().unwrap_or(Path::new("."));

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
        .env("LEO_INSTALL_SKIP_MODEL", "1")
        .status()
        .context("could not run the installer (is curl installed?)")?;
    if !status.success() {
        anyhow::bail!("the update did not finish; leo was left as it was");
    }
    Ok(())
}
