use anyhow::{Context, Result};
use colored::Colorize;

fn ensure_speech_model() {
    if std::env::var_os("LEO_INSTALL_NO_MODEL").is_some() {
        return;
    }
    use leo_services::providers::ModelState;
    let state = leo_services::providers::speech_model_state();
    match state {
        ModelState::Ready => return,
        ModelState::Missing => {
            println!();
            println!("  The selected local speech model is missing. Downloading it from the model library…");
        }
        ModelState::Damaged => {
            println!();
            println!("  The selected local speech model is damaged. Downloading it again…");
        }
    }
    if let Some(message) = model_check(state, leo_services::providers::download_speech_model) {
        println!("  {message}");
    }
}

fn model_check(
    state: leo_services::providers::ModelState,
    download: impl FnOnce() -> Result<std::path::PathBuf>,
) -> Option<String> {
    if state == leo_services::providers::ModelState::Ready {
        return None;
    }
    Some(match download() {
        Ok(path) => format!("Saved {}", path.display()),
        Err(error) => {
            format!("Could not download it ({error}). Try again in Settings → Speech models.")
        }
    })
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
            format!("Could not download it ({e}). leo will try again when it opens.").dimmed()
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
    let fallback = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join("Leo/bin")
    } else {
        dirs::home_dir().unwrap_or_default().join(".local/bin")
    };
    let dir = super::installation::cli_dir(
        &exe,
        &leo_core::paths::config_dir()?.join("app-cli-dir"),
        &fallback,
    )?;

    let mut cmd = match (std::env::var_os("LEO_UPDATE_SCRIPT"), cfg!(windows)) {
        (Some(script), false) => {
            let mut cmd = leo_core::children::quiet_command("sh");
            cmd.arg(script);
            cmd
        }
        (None, false) => {
            let mut cmd = leo_core::children::quiet_command("sh");
            cmd.arg("-c").arg(leo_services::update::install_command());
            cmd
        }
        (Some(script), true) => {
            let mut cmd = leo_core::children::quiet_command("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script);
            cmd
        }
        (None, true) => {
            let mut cmd = leo_core::children::quiet_command("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
                .arg(leo_services::update::install_command());
            cmd
        }
    };
    cmd.env("LEO_INSTALL_DIR", &dir)
        .env("LEO_INSTALL_SKIP_PATH", "1");
    #[cfg(windows)]
    if std::env::var_os("LEO_UPDATE_SCRIPT").is_none() {
        cmd.env("LEO_UPDATE_PARENT_PID", std::process::id().to_string())
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()
            .context("The updater could not start. Download the Windows installer from Leo’s releases page.")?;
        println!("  The Leo installer will run after this command exits. Quit the Leo app first if it is open.");
        return Ok(());
    }
    let status = cmd.status().context(if cfg!(windows) {
        "could not run the installer (is PowerShell available?)"
    } else {
        "could not run the installer (is curl installed?)"
    })?;
    if !status.success() {
        anyhow::bail!("the update did not finish; leo was left as it was");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use leo_services::providers::ModelState;
    #[test]
    fn catalog_model_check_skips_ready_and_reports_missing_damaged_and_failed_downloads() {
        assert!(
            super::model_check(ModelState::Ready, || panic!("must not download twice")).is_none()
        );
        for state in [ModelState::Missing, ModelState::Damaged] {
            let output =
                super::model_check(state, || Ok(std::path::PathBuf::from("local/model.gguf")))
                    .unwrap();
            assert!(output.contains("model.gguf"));
            let error = super::model_check(state, || anyhow::bail!("checksum mismatch")).unwrap();
            assert!(error.contains("Speech models") && error.contains("checksum mismatch"));
        }
    }
}
