use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Settings(AtomicUsize);

impl SettingsApi for Settings {
    fn describe(&self, _: &std::path::Path) -> serde_json::Value {
        serde_json::json!({"refresh_count": self.0.load(Ordering::SeqCst)})
    }
    fn refresh_usage(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn apply(&self, _: &serde_json::Value) -> anyhow::Result<String> {
        anyhow::bail!("Not used")
    }
    fn test(&self, _: &str) -> anyhow::Result<String> {
        anyhow::bail!("Not used")
    }
}

async fn request(port: u16, method: &str, path: &str, cookie: &str) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }).await.unwrap()
}

#[tokio::test]
async fn only_explicit_authenticated_usage_refresh_polls_and_concurrent_refresh_is_bounded() {
    for available in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Settings::default());
        let places = crate::Places {
            store: Store::load_from(&dir.path().join("notes")).unwrap(),
            sessions: dir.path().join("sessions.json"),
        };
        let powers = crate::Powers {
            settings: available.then(|| fake.clone() as Arc<dyn SettingsApi>),
            ..Default::default()
        };
        let running = crate::start_in(
            places,
            crate::StartOptions {
                port: 0,
                token: "usage-test-token".into(),
            },
            powers,
        )
        .await
        .unwrap();
        let port = running.port();
        let signed = request(port, "GET", "/?token=usage-test-token", "").await;
        let cookie = signed
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("set-cookie: "))
            .unwrap()
            .split_once(": ")
            .unwrap()
            .1
            .split(';')
            .next()
            .unwrap();
        for cookie in ["", "leo_session=wrong"] {
            assert!(request(port, "POST", "/api/settings/usage", cookie)
                .await
                .starts_with("HTTP/1.1 401"));
        }
        assert_eq!(fake.0.load(Ordering::SeqCst), 0);
        if available {
            assert!(request(port, "GET", "/api/settings", cookie)
                .await
                .starts_with("HTTP/1.1 200"));
            let permit = running
                .state
                .usage_refreshes
                .clone()
                .acquire_owned()
                .await
                .unwrap();
            let busy = request(port, "POST", "/api/settings/usage", cookie).await;
            assert!(busy.starts_with("HTTP/1.1 409"));
            assert!(busy.contains("already refreshing"));
            assert!(request(port, "GET", "/api/settings", cookie)
                .await
                .starts_with("HTTP/1.1 200"));
            assert_eq!(fake.0.load(Ordering::SeqCst), 0);
            drop(permit);
            let fresh = request(port, "POST", "/api/settings/usage", cookie).await;
            assert!(fresh.starts_with("HTTP/1.1 200"));
            let body: serde_json::Value =
                serde_json::from_str(fresh.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["refresh_count"], 1);
            assert!(request(port, "GET", "/api/settings", cookie)
                .await
                .contains("\"refresh_count\":1"));
            assert_eq!(fake.0.load(Ordering::SeqCst), 1);
        } else {
            let missing = request(port, "POST", "/api/settings/usage", cookie).await;
            assert!(missing.starts_with("HTTP/1.1 503"));
            assert!(missing.contains("Settings are not available"));
        }
        running.stop().await;
    }
}
