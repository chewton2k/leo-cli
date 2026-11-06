use super::*;

#[cfg(unix)]
#[tokio::test]
async fn retired_backup_endpoints_and_lifecycle_leave_existing_repositories_untouched() {
    use std::collections::BTreeMap;
    use std::path::Path;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
        fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().into(),
                        std::fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        collect(root, root, &mut files);
        files
    }

    async fn ask(port: u16, method: &str, path: &str, cookie: &str, body: &str) -> Vec<u8> {
        let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            socket.read_to_end(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        response
    }

    if std::env::var_os("LEO_RETIRED_BACKUP_CHILD").is_none() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let notes = home.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&notes, &["init", "-q", "-b", "main"]);
        git(
            &notes,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--allow-empty",
                "-qm",
                "Existing notes history",
            ],
        );
        let remote = home.path().join("remote.git");
        git(
            home.path(),
            &["init", "--bare", "-q", remote.to_str().unwrap()],
        );
        git(
            &notes,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&notes, &["push", "-u", "origin", "main"]);
        std::fs::write(
            home.path().join("config.toml"),
            "[sync]\nauto_push = \"when_idle\"\nidle_secs = 0\n[chat]\nchain = [\"ollama\"]\n",
        )
        .unwrap();
        let history = snapshot(&notes.join(".git"));
        let remote_before = snapshot(&remote);
        let config = std::fs::read(home.path().join("config.toml")).unwrap();
        let bin = home.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        for program in ["git", "gh"] {
            let script = bin.join(program);
            std::fs::write(
                &script,
                "#!/bin/sh\nprintf '%s\\n' called >> \"$LEO_HOME/process-called\"\nexit 1\n",
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::backup::retired_backup_endpoints_and_lifecycle_leave_existing_repositories_untouched", "--nocapture"])
            .env("LEO_RETIRED_BACKUP_CHILD", "1").env("LEO_HOME", home.path())
            .env("PATH", bin).env("LEO_NO_MICROPHONE", "1").env("LEO_NO_UPDATE_CHECK", "1")
            .env_remove("LEO_USE_KEYCHAIN").output().unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            !home.path().join("process-called").exists(),
            "git or gh was invoked"
        );
        assert_eq!(snapshot(&notes.join(".git")), history);
        assert_eq!(snapshot(&remote), remote_before);
        assert_eq!(
            std::fs::read(home.path().join("config.toml")).unwrap(),
            config
        );
        assert!(!notes.join(".gitignore").exists());
        assert!(!notes.join(".gitattributes").exists());
        let saved = Store::load_from(&notes).unwrap();
        assert_eq!(saved.notes.len(), 1);
        assert_eq!(saved.notes[0].body, "Durable note edit");
        return;
    }

    let home = std::path::PathBuf::from(std::env::var_os("LEO_HOME").unwrap());
    let notes = home.join("notes");
    let running = start_in(
        Places {
            store: Store::load_from(&notes).unwrap(),
            sessions: home.join("sessions.json"),
        },
        StartOptions {
            port: 0,
            token: "test-token".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    let port = running.port();
    let signin = ask(port, "GET", "/?token=test-token", "", "").await;
    let signin = String::from_utf8(signin).unwrap();
    let cookie = signin
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let unauthorized = ask(port, "POST", "/api/notes", "", "{}").await;
    assert!(unauthorized.starts_with(b"HTTP/1.1 401"));
    let created = ask(
        port,
        "POST",
        "/api/notes",
        cookie,
        r#"{"title":"Kept","body":"Durable note edit"}"#,
    )
    .await;
    assert!(
        created.starts_with(b"HTTP/1.1 200") || created.starts_with(b"HTTP/1.1 201"),
        "{}",
        String::from_utf8_lossy(&created)
    );
    let created = String::from_utf8(created).unwrap();
    let note: serde_json::Value =
        serde_json::from_str(created.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let id = note["id"].as_str().unwrap().to_string();
    for (method, path, body) in [
        ("GET", "/api/backup", ""),
        ("POST", "/api/backup", "{}"),
        ("POST", "/api/backup/setup", r#"{"name":"owner/notes"}"#),
    ] {
        let response = ask(port, method, path, cookie, body).await;
        assert!(
            response.starts_with(b"HTTP/1.1 404"),
            "{method} {path}: {}",
            String::from_utf8_lossy(&response)
        );
    }
    let inventory = running
        .state
        .with_store(|store| {
            let graphs = crate::graph::Graphs::for_notes(&store.notes_dir, None);
            Ok(crate::storage::areas(
                store,
                &graphs,
                &store.notes_dir.join("chats"),
                chrono::Utc::now(),
            ))
        })
        .await
        .unwrap();
    assert!(!inventory.iter().any(|area| area.id == "backup"));
    let export = ask(port, "GET", "/api/export", cookie, "").await;
    assert!(
        export.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&export)
    );
    assert!(export.windows(4).any(|part| part == b"PK\x03\x04"));
    running
        .state
        .with_store(move |store| {
            store.delete_note(&id);
            store
                .save()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            assert_eq!(store.trashed().len(), 1);
            store.undo().unwrap();
            store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        })
        .await
        .unwrap();
    running.set_visible(false);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    running.set_visible(true);
    running.stop().await;
}
