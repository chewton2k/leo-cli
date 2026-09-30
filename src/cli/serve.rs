use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use leo_core::paths::on_path;

pub fn ensure_tunnel_tool() -> Result<()> {
    if on_path("cloudflared") {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() || !on_path("brew") {
        anyhow::bail!(leo_web::tunnel::MISSING);
    }
    println!();
    println!("  leo serve opens a link that works from anywhere, using Cloudflare's free");
    println!("  tunnel tool, cloudflared. No Cloudflare account is needed.");
    let answer = super::prompt::ask("  Install it now with Homebrew? [Y/n] ")?;
    if !(answer.is_empty()
        || answer.eq_ignore_ascii_case("y")
        || answer.eq_ignore_ascii_case("yes"))
    {
        anyhow::bail!(leo_web::tunnel::MISSING);
    }
    let installed = std::process::Command::new("brew")
        .args(["install", "cloudflared"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if installed && on_path("cloudflared") {
        println!("  {} cloudflared is installed.", "ok".green());
        Ok(())
    } else {
        anyhow::bail!(leo_web::tunnel::MISSING)
    }
}
