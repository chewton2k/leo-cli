use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{start_in, Places, Powers, Running, StartOptions};

async fn running() -> (Running, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let places = Places {
        store: leo_core::store::Store::load_from(&dir.path().join("notes")).unwrap(),
        sessions: dir.path().join("sessions.json"),
    };
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "0123456789abcdef0123456789abcdef".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    (running, dir)
}

async fn ask(port: u16, path: &str, host: &str) -> String {
    ask_with(port, path, host, "").await
}

async fn ask_with(port: u16, path: &str, host: &str, cookie: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    reply
}

#[tokio::test]
async fn a_foreign_host_is_refused() {
    let (running, _dir) = running().await;
    let port = running.port();
    let reply = ask(
        port,
        "/?token=0123456789abcdef0123456789abcdef",
        &format!("evil.example:{port}"),
    )
    .await;
    assert!(reply.starts_with("HTTP/1.1 421"), "{reply}");
    let other_port = ask(port, "/", &format!("127.0.0.1:{}", port.wrapping_add(1))).await;
    assert!(other_port.starts_with("HTTP/1.1 421"), "{other_port}");
    running.stop().await;
}

#[tokio::test]
async fn the_window_signs_in_with_a_strict_cookie() {
    let (running, _dir) = running().await;
    let port = running.port();
    let reply = ask(
        port,
        "/?token=0123456789abcdef0123456789abcdef",
        &format!("127.0.0.1:{port}"),
    )
    .await;
    assert!(reply.starts_with("HTTP/1.1 303"), "{reply}");
    let cookie = reply
        .lines()
        .find(|line| {
            line.to_ascii_lowercase()
                .starts_with("set-cookie: leo_session=")
        })
        .unwrap_or_default();
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(!cookie.contains("Secure"), "{cookie}");
    let localhost = ask(port, "/", &format!("localhost:{port}")).await;
    assert!(localhost.starts_with("HTTP/1.1 401"), "{localhost}");
    running.stop().await;
}

#[tokio::test]
async fn the_signed_in_browsers_list_is_gone() {
    let (running, _dir) = running().await;
    let port = running.port();
    let here = format!("127.0.0.1:{port}");
    let signed_in = ask(port, "/?token=0123456789abcdef0123456789abcdef", &here).await;
    let cookie = signed_in
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
        })
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_string();
    let notes = ask_with(port, "/api/notes", &here, &cookie).await;
    assert!(notes.starts_with("HTTP/1.1 200"), "{notes}");
    let reply = ask_with(port, "/api/sessions", &here, &cookie).await;
    assert!(
        reply.starts_with("HTTP/1.1 404") || reply.starts_with("HTTP/1.1 405"),
        "{reply}"
    );
    running.stop().await;
}
