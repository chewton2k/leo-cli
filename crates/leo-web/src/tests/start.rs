use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;

use crate::{start_in, Places, Powers, StartOptions};

fn places() -> (Places, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let places = Places {
        store: leo_core::store::Store::load_from(&dir.path().join("notes")).unwrap(),
        sessions: dir.path().join("sessions.json"),
    };
    (places, dir)
}

#[tokio::test]
async fn start_binds_loopback_only() {
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", running.port()))
            .await
            .is_ok()
    );
    assert!(running.sign_in_url().starts_with("http://127.0.0.1:"));
    assert!(running.sign_in_url().ends_with("/?token=t"));
    running.stop().await;
}

#[tokio::test]
async fn start_takes_the_next_port_when_one_is_busy() {
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = held.local_addr().unwrap().port();
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: busy,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    assert_ne!(running.port(), busy);
    running.stop().await;
}

#[tokio::test]
async fn stop_returns_within_the_drain_deadline() {
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    let mut open = tokio::net::TcpStream::connect(("127.0.0.1", running.port()))
        .await
        .unwrap();
    open.write_all(b"GET /api/notes HTTP/1.1\r\nHost: 127.0.0.1\r\n")
        .await
        .unwrap();
    let began = Instant::now();
    running.stop().await;
    assert!(began.elapsed() < Duration::from_secs(7));
}

#[tokio::test]
async fn stopping_frees_the_port() {
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    let port = running.port();
    running.stop().await;
    assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
}

#[test]
fn fresh_tokens_differ_and_look_like_saved_ones() {
    let a = crate::fresh_token();
    let b = crate::fresh_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}
