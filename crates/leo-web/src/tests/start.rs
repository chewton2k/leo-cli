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

struct Backing {
    calls: std::sync::atomic::AtomicUsize,
    marker: std::path::PathBuf,
}

impl crate::SettingsApi for Backing {
    fn describe(&self, _: &std::path::Path) -> serde_json::Value {
        serde_json::json!({})
    }

    fn apply(&self, _: &serde_json::Value) -> anyhow::Result<String> {
        anyhow::bail!("Not used")
    }

    fn test(&self, _: &str) -> anyhow::Result<String> {
        anyhow::bail!("Not used")
    }

    fn backup(&self, _: &std::path::Path) -> anyhow::Result<Option<std::process::Command>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg(format!("touch '{}'", self.marker.display()));
        Ok(Some(command))
    }
}

async fn backed_up() -> (crate::Running, std::sync::Arc<Backing>, tempfile::TempDir) {
    let (places, dir) = places();
    let backing = std::sync::Arc::new(Backing {
        calls: Default::default(),
        marker: dir.path().join("pushed"),
    });
    let powers = Powers {
        settings: Some(backing.clone()),
        ..Powers::default()
    };
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        powers,
    )
    .await
    .unwrap();
    (running, backing, dir)
}

async fn appears(path: &std::path::Path, within: Duration) -> bool {
    let began = Instant::now();
    while began.elapsed() < within {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    path.exists()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_can_be_asked_for_from_a_thread_outside_the_engine() {
    let (running, backing, dir) = backed_up().await;
    std::thread::scope(|scope| {
        scope.spawn(|| running.back_up()).join().unwrap();
    });
    assert!(appears(&dir.path().join("pushed"), Duration::from_secs(6)).await);
    assert_eq!(backing.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    running.stop().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn quitting_runs_the_last_backup_instead_of_dropping_it() {
    let (running, _backing, dir) = backed_up().await;
    running.stop().await;
    assert!(appears(&dir.path().join("pushed"), Duration::from_secs(3)).await);
}
