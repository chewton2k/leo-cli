use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

const WAIT: Duration = Duration::from_secs(45);

pub const MISSING: &str =
    "serving from anywhere needs Cloudflare's free tunnel tool, cloudflared. \
Install it (brew install cloudflared; other systems: \
https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) \
and run this again. No Cloudflare account is needed.";

pub struct Tunnel {
    pub url: String,
    _child: Child,
}

pub async fn start(port: u16) -> Result<Tunnel> {
    if !leo_core::paths::on_path("cloudflared") {
        anyhow::bail!(MISSING);
    }
    let mut child = Command::new("cloudflared")
        .args(["tunnel", "--no-autoupdate", "--url"])
        .arg(format!("http://127.0.0.1:{port}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("could not start cloudflared")?;

    let stderr = child.stderr.take().context("no output from cloudflared")?;
    let mut lines = BufReader::new(stderr).lines();
    let found = tokio::time::timeout(WAIT, async {
        let mut last = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = address_in(&line) {
                return Ok(url);
            }
            if !line.trim().is_empty() {
                last = line;
            }
        }
        anyhow::bail!("cloudflared stopped before it had an address: {last}")
    })
    .await
    .context(
        "Cloudflare did not give an address in time; check the internet connection and try again",
    )??;

    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
    Ok(Tunnel {
        url: found,
        _child: child,
    })
}

fn address_in(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '|')
        .collect();
    url.trim_end_matches('/')
        .ends_with(".trycloudflare.com")
        .then(|| url.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_found_in_cloudflareds_box() {
        let line = "2026-09-26T10:00:00Z INF |  https://quiet-fox-123.trycloudflare.com                                   |";
        assert_eq!(
            address_in(line).as_deref(),
            Some("https://quiet-fox-123.trycloudflare.com")
        );
    }

    #[test]
    fn other_links_are_not_the_address() {
        assert_eq!(
            address_in("INF see https://www.cloudflare.com/website-terms/"),
            None
        );
        assert_eq!(
            address_in("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
    }
}
