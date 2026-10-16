use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

const WAIT: Duration = Duration::from_secs(90);
const MOST_LINE: usize = 16 * 1024;
const MOST_STARTUP_LINES: usize = 4096;
const CONNECTION: &str = "Registered tunnel connection";
const RETRY: &str = "Check the internet connection or VPN/firewall, then try again. `leo serve --local` works on this Wi-Fi.";

pub const MISSING: &str =
    "leo serve opens a link that works from anywhere through Cloudflare's free tunnel tool, \
cloudflared. Install it (brew install cloudflared; on Windows, winget install Cloudflare.cloudflared; other systems: \
https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) \
and run this again. No Cloudflare account is needed. `leo serve --local` works without it, \
on this Wi-Fi only.";

struct TunnelChild {
    child: Child,
    #[cfg(unix)]
    group: Option<u32>,
}

impl TunnelChild {
    fn stop(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.group.take().and_then(|pid| i32::try_from(pid).ok()) {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
        let _ = self.child.start_kill();
    }

    async fn finish(&mut self) {
        self.stop();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

impl Drop for TunnelChild {
    fn drop(&mut self) {
        self.stop();
        let _ = self.child.try_wait();
    }
}

pub struct Tunnel {
    pub url: String,
    child: TunnelChild,
    drain: tokio::task::JoinHandle<()>,
}

impl Tunnel {
    pub async fn stop(mut self) {
        self.drain.abort();
        self.child.finish().await;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

pub async fn start(port: u16) -> Result<Tunnel> {
    if !leo_core::paths::on_path("cloudflared") {
        anyhow::bail!(MISSING);
    }
    let mut cmd = Command::new("cloudflared");
    cmd.args([
        "tunnel",
        "--no-autoupdate",
        "--protocol",
        "auto",
        "--loglevel",
        "info",
        "--url",
    ])
    .arg(format!("http://127.0.0.1:{port}"))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let child = cmd.spawn().context("could not start cloudflared")?;
    let mut child = TunnelChild {
        #[cfg(unix)]
        group: child.id(),
        child,
    };

    let Some(stderr) = child.child.stderr.take() else {
        child.finish().await;
        anyhow::bail!("no output from cloudflared");
    };
    let mut reader = BufReader::new(stderr);
    eprintln!(
        "  Connecting to Cloudflare (up to {} seconds)…",
        WAIT.as_secs()
    );
    let found = match wait_for(&mut reader, WAIT).await {
        Ok(found) => found,
        Err(e) => {
            child.finish().await;
            return Err(e);
        }
    };
    match child.child.try_wait() {
        Ok(None) => {}
        Ok(Some(_)) => {
            child.finish().await;
            anyhow::bail!("Cloudflared stopped before the link was ready. {RETRY}");
        }
        Err(e) => {
            child.finish().await;
            return Err(e).context("could not check cloudflared");
        }
    }

    let drain = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
        eprintln!("  The Cloudflare link stopped. Restart `leo serve` to make a new phone link.");
    });
    Ok(Tunnel {
        url: found,
        child,
        drain,
    })
}

async fn wait_for<R: AsyncBufRead + Unpin>(reader: &mut R, wait: Duration) -> Result<String> {
    let mut address = None;
    let mut connected = false;
    let result = tokio::time::timeout(wait, async {
        for _ in 0..MOST_STARTUP_LINES {
            let Some(line) = log_line(reader)
                .await
                .context("Could not read Cloudflared's connection status. Update cloudflared and try again")?
            else {
                anyhow::bail!("Cloudflared stopped before the link connected. {RETRY}");
            };
            if address.is_none() {
                address = address_in(&line);
            }
            connected |= connection_in(&line);
            if connected {
                if let Some(url) = address.take() {
                    return Ok(url);
                }
            }
        }
        anyhow::bail!("Cloudflared produced too much output before connecting. Update cloudflared and try again. {RETRY}")
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) if address.is_some() => {
            anyhow::bail!(
                "Cloudflare created an address but did not connect within {} seconds. {RETRY}",
                wait.as_secs()
            )
        }
        Err(_) => anyhow::bail!(
            "Cloudflare did not create a connected link within {} seconds. {RETRY}",
            wait.as_secs()
        ),
    }
}

async fn log_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return Ok((!line.is_empty()).then(|| String::from_utf8_lossy(&line).into_owned()));
        }
        let end = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buffer.len(), |i| i + 1);
        if line.len() + end > MOST_LINE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Cloudflared's status line is too long",
            ));
        }
        line.extend_from_slice(&buffer[..end]);
        reader.consume(end);
        if line.last() == Some(&b'\n') {
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
    }
}

fn connection_in(line: &str) -> bool {
    if line.trim_start().starts_with('{') {
        return serde_json::from_str::<serde_json::Value>(line)
            .is_ok_and(|entry| entry["level"] == "info" && entry["message"] == CONNECTION);
    }
    let message = line.strip_prefix("INF ").or_else(|| {
        let (_, rest) = line.split_once(' ')?;
        rest.strip_prefix("INF ")
    });
    message.is_some_and(|message| {
        message
            .trim_end()
            .strip_prefix(CONNECTION)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
}

fn address_in(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || matches!(c, '|' | '"' | '\'' | '(' | ')' | '\\'))
        .find_map(|word| {
            let url = word.trim_end_matches('/');
            let label = url
                .strip_prefix("https://")?
                .strip_suffix(".trycloudflare.com")?;
            (!label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'))
            .then(|| url.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn owned_tunnel_children_are_reaped_on_shutdown_and_cancellation() {
        for cancel in [false, true] {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", "sleep 30"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .kill_on_drop(true);
            let child = command.spawn().unwrap();
            let pid = child.id().unwrap();
            let mut child = TunnelChild {
                child,
                group: Some(pid),
            };
            if cancel {
                drop(child);
            } else {
                child.finish().await;
                assert!(child.child.try_wait().unwrap().is_some());
            }
            let started = std::time::Instant::now();
            while unsafe { libc::kill(i32::try_from(pid).unwrap(), 0) } == 0 {
                assert!(
                    started.elapsed() < Duration::from_secs(2),
                    "the tunnel child was not reaped"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

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

    const ADDRESS: &str = "INF | https://quiet-fox-123.trycloudflare.com |\n";
    const REGISTERED: &str =
        "2026-10-12T10:00:00Z INF Registered tunnel connection connIndex=0 protocol=http2\n";

    #[tokio::test]
    async fn an_address_is_not_a_connection() {
        for output in [
            ADDRESS.to_string(),
            REGISTERED.to_string(),
            format!("{ADDRESS}ERR dial timeout\n"),
        ] {
            let error = wait_for(&mut output.as_bytes(), Duration::from_secs(1))
                .await
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("stopped before the link connected"));
            assert!(error.to_string().contains("--local"));
        }
    }

    #[tokio::test]
    async fn both_orders_and_both_log_formats_connect() {
        let json = "{\"level\":\"info\",\"message\":\"Registered tunnel connection\",\"protocol\":\"http2\"}\n";
        let json_address =
            "{\"level\":\"info\",\"message\":\"https://quiet-fox-123.trycloudflare.com\"}\n";
        for output in [
            format!("{ADDRESS}{REGISTERED}"),
            format!("{REGISTERED}{ADDRESS}"),
            format!("{json_address}{json}"),
            format!("{json}{json_address}"),
        ] {
            assert_eq!(
                wait_for(&mut output.as_bytes(), Duration::from_secs(1))
                    .await
                    .unwrap(),
                "https://quiet-fox-123.trycloudflare.com"
            );
        }
    }

    #[tokio::test]
    async fn a_delayed_connection_does_not_publish_an_address_early() {
        use tokio::io::AsyncWriteExt;
        let (mut write, read) = tokio::io::duplex(MOST_LINE);
        let mut reader = BufReader::new(read);
        write.write_all(ADDRESS.as_bytes()).await.unwrap();
        let opening = wait_for(&mut reader, Duration::from_secs(1));
        tokio::pin!(opening);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut opening)
                .await
                .is_err()
        );
        write.write_all(REGISTERED.as_bytes()).await.unwrap();
        assert_eq!(
            opening.await.unwrap(),
            "https://quiet-fox-123.trycloudflare.com"
        );
    }

    #[tokio::test]
    async fn a_stalled_startup_times_out_before_and_after_allocation() {
        use tokio::io::AsyncWriteExt;
        for allocated in [false, true] {
            let (mut write, read) = tokio::io::duplex(MOST_LINE);
            if allocated {
                write.write_all(ADDRESS.as_bytes()).await.unwrap();
            }
            let error = wait_for(&mut BufReader::new(read), Duration::from_millis(20))
                .await
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(if allocated {
                    "created an address but did not connect"
                } else {
                    "did not create a connected link"
                }),
                "{error}"
            );
            assert!(error.contains("--local"), "{error}");
        }
    }

    #[test]
    fn failures_and_quoted_registration_messages_are_not_connections() {
        for line in [
            "ERR Registered tunnel connection failed",
            "INF error: Registered tunnel connection",
            "ERR quoting INF Registered tunnel connection",
            "INF Registered tunnel connections missing",
            "{\"level\":\"error\",\"message\":\"Registered tunnel connection\"}",
            "{bad json}",
        ] {
            assert!(!connection_in(line), "{line}");
        }
    }

    #[test]
    fn only_a_plain_quick_tunnel_host_is_accepted() {
        for url in [
            "https://user@quiet-fox.trycloudflare.com",
            "https://evil.test/path.trycloudflare.com",
            "https://quiet-fox.trycloudflare.com:443",
            "https://quiet-fox.trycloudflare.com/?secret=value",
            "https://quiet-fox.trycloudflare.com.evil.test",
            "https://.trycloudflare.com",
            "https://-bad.trycloudflare.com",
            "https://bad-.trycloudflare.com",
            "https://a.b.trycloudflare.com",
            "http://quiet-fox.trycloudflare.com",
            "https://文字.trycloudflare.com",
        ] {
            assert_eq!(address_in(url), None, "{url}");
        }
        assert_eq!(
            address_in(&format!("https://{}.trycloudflare.com", "a".repeat(64))),
            None
        );
        assert_eq!(address_in("INF https://www.cloudflare.com/website-terms/ | https://quiet-fox.trycloudflare.com/ |"), Some("https://quiet-fox.trycloudflare.com".into()));
    }

    #[tokio::test]
    async fn output_limits_apply_before_registration() {
        let oversized = format!("{}\n{ADDRESS}{REGISTERED}", "a".repeat(MOST_LINE + 1));
        let error = wait_for(&mut oversized.as_bytes(), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("status line is too long"));
        let noisy = format!(
            "{}{ADDRESS}{REGISTERED}",
            "INF retrying\n".repeat(MOST_STARTUP_LINES)
        );
        let error = wait_for(&mut noisy.as_bytes(), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("too much output"));
    }

    #[tokio::test]
    async fn partial_unicode_and_invalid_output_are_read_without_panicking() {
        let mut bytes = vec![0xff, b'\n'];
        bytes.extend_from_slice("INF 文字\n".as_bytes());
        bytes.extend_from_slice(ADDRESS.as_bytes());
        bytes.extend_from_slice(REGISTERED.trim_end().as_bytes());
        let mut reader = BufReader::with_capacity(1, bytes.as_slice());
        assert_eq!(
            wait_for(&mut reader, Duration::from_secs(1)).await.unwrap(),
            "https://quiet-fox-123.trycloudflare.com"
        );
    }
}
