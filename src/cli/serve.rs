use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use leo_core::paths::on_path;

const SHUTDOWN_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

pub fn run(future: impl std::future::Future<Output = Result<()>>) -> Result<()> {
    run_with(future, leo_services::ai::stop_writing, SHUTDOWN_LIMIT)
}

fn run_with(
    future: impl std::future::Future<Output = Result<()>>,
    stop: fn(),
    limit: std::time::Duration,
) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(async {
        let signal = tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                stop();
            }
        });
        let result = future.await;
        stop();
        signal.abort();
        result
    });
    runtime.shutdown_timeout(limit);
    result
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    static STOPPED: AtomicUsize = AtomicUsize::new(0);

    fn stopped() {
        STOPPED.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn server_exit_stops_writers_without_waiting_for_a_blocked_job() {
        let before = STOPPED.load(Ordering::Relaxed);
        let started = Instant::now();
        let result = run_with(
            async {
                let (tx, rx) = tokio::sync::oneshot::channel();
                tokio::task::spawn_blocking(move || {
                    let _ = tx.send(());
                    std::thread::sleep(Duration::from_secs(2));
                });
                let _ = rx.await;
                anyhow::bail!("sample server failure")
            },
            stopped,
            Duration::from_millis(20),
        );
        assert_eq!(result.unwrap_err().to_string(), "sample server failure");
        assert_eq!(STOPPED.load(Ordering::Relaxed), before + 1);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
