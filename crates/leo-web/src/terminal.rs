use anyhow::Result;
use colored::Colorize;

pub(crate) fn should_open(typing: bool, showing: bool, refused: bool) -> bool {
    typing && showing && !refused
}

pub fn hyperlink(url: &str, shown: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{shown}\x1b]8;;\x1b\\")
}

pub fn styled_link(url: &str, terminal: bool, program: Option<&str>) -> String {
    let shown = url.cyan().underline().to_string();
    if terminal && program != Some("Apple_Terminal") {
        hyperlink(url, &shown)
    } else {
        shown
    }
}

pub(crate) fn clickable(url: &str) -> String {
    use std::io::IsTerminal;
    let program = std::env::var("TERM_PROGRAM").ok();
    styled_link(url, std::io::stdout().is_terminal(), program.as_deref())
}

pub(crate) fn open_on_enter(url: String) {
    use std::io::{BufRead, IsTerminal};
    if !std::io::stdin().is_terminal() {
        return;
    }
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if line.is_err() {
                break;
            }
            match leo_core::open::link(&url) {
                Ok(()) => println!("  {}", "Opened in your browser.".dimmed()),
                Err(e) => println!(
                    "  Could not open a browser ({e}). The link above works in any browser."
                ),
            }
        }
    });
}

pub(crate) async fn bind(wanted: u16) -> Result<(tokio::net::TcpListener, u16)> {
    let mut last = None;
    for port in wanted..wanted.saturating_add(20) {
        match tokio::net::TcpListener::bind(std::net::SocketAddr::from(([0, 0, 0, 0], port))).await
        {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "ports {wanted} to {} are all in use ({}); try `leo serve --port 4000`",
        wanted.saturating_add(19),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

pub(crate) fn print_qr(link: &str) {
    if let Ok(code) = qrcode::QrCode::new(link) {
        use qrcode::render::unicode::Dense1x2;
        let qr = code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build();
        println!();
        for line in qr.lines() {
            println!("  {line}");
        }
        println!();
    }
}

pub(crate) fn keep_awake() -> Option<std::process::Child> {
    if !cfg!(target_os = "macos") || !leo_core::paths::on_path("caffeinate") {
        return None;
    }
    std::process::Command::new("caffeinate")
        .args(["-i", "-w", &std::process::id().to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_is_clickable_and_still_reads_as_itself() {
        let url = "https://example.trycloudflare.com/?token=abc";
        let link = hyperlink(url, url);
        assert_eq!(
            link,
            "\x1b]8;;https://example.trycloudflare.com/?token=abc\x1b\\https://example.trycloudflare.com/?token=abc\x1b]8;;\x1b\\"
        );
        assert!(!clickable(url).contains("\x1b]8"), "a pipe gets plain text");
        assert!(should_open(true, true, false));
        assert!(!should_open(false, true, false));
        assert!(!should_open(true, false, false));
        assert!(!should_open(true, true, true));
        assert!(styled_link(url, true, Some("iTerm.app")).contains("\x1b]8;;"));
        assert!(styled_link(url, true, None).contains("\x1b]8;;"));
        let apple = styled_link(url, true, Some("Apple_Terminal"));
        assert!(
            !apple.contains("\x1b]8"),
            "Terminal.app finds plain links itself"
        );
        assert!(apple.contains(url));
    }
}
