use std::process::{Command, Stdio};

use anyhow::{Context, Result};

pub fn link(uri: &str) -> Result<()> {
    let (opener, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![uri])
    } else if cfg!(windows) {
        ("cmd", vec!["/C", "start", "", uri])
    } else {
        ("xdg-open", vec![uri])
    };
    let status = Command::new(opener)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("could not run {opener}"))?;
    if !status.success() {
        anyhow::bail!("{opener} could not open {uri}");
    }
    Ok(())
}
