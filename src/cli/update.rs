use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use colored::Colorize;

fn ensure_speech_model() {
    if std::env::var_os("LEO_INSTALL_NO_MODEL").is_some() {
        return;
    }
    use leo_services::providers::ModelState;
    leo_services::providers::remove_old_models();
    match leo_services::providers::speech_model_state() {
        ModelState::Ready => return,
        ModelState::Missing => {
            println!();
            println!("  The speech model is missing. Downloading Parakeet (670 MB, once)…");
        }
        ModelState::Damaged => {
            println!();
            println!("  The speech model is damaged. Downloading the damaged parts again…");
        }
    }
    match leo_services::providers::download_speech_model() {
        Ok(path) => println!("  Saved {}", path.display()),
        Err(e) => println!(
            "  {}",
            format!("Could not download it ({e}). leo will try again when it starts.").dimmed()
        ),
    }
}

fn ensure_meaning_model() {
    if !leo_services::meaning::wanted() {
        return;
    }
    println!(
        "  Downloading the model that finds notes by meaning ({} MB, once)…",
        leo_services::meaning::MODEL_MB
    );
    match leo_services::meaning::fetch(false) {
        Ok(path) => println!("  Saved {}", path.display()),
        Err(e) => println!(
            "  {}",
            format!("Could not download it ({e}). leo serve will try again.").dimmed()
        ),
    }
}

pub fn run(force: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    if !force {
        match leo_services::update::latest_release() {
            Ok(latest) if !leo_services::update::is_newer(&latest, current) => {
                println!();
                println!("  leo {current} is the latest version.");
                ensure_speech_model();
                ensure_meaning_model();
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

    let mut cmd = match (std::env::var_os("LEO_UPDATE_SCRIPT"), cfg!(windows)) {
        (Some(script), false) => {
            let mut cmd = Command::new("sh");
            cmd.arg(script);
            cmd
        }
        (None, false) => {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(leo_services::update::install_command());
            cmd
        }
        (Some(script), true) => {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script);
            cmd
        }
        (None, true) => {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
                .arg(leo_services::update::install_command());
            cmd
        }
    };
    let status = cmd
        .env("LEO_INSTALL_DIR", dir)
        .env("LEO_INSTALL_SKIP_PATH", "1")
        .status()
        .context(if cfg!(windows) {
            "could not run the installer (is PowerShell available?)"
        } else {
            "could not run the installer (is curl installed?)"
        })?;
    if !status.success() {
        anyhow::bail!("the update did not finish; leo was left as it was");
    }
    Ok(())
}
