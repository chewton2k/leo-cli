use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MARK: &str = "__LEO_PATH__";

pub fn login_path(shell: &Path, timeout: Duration) -> Option<String> {
    let mut command = std::process::Command::new(shell);
    command
        .args([
            "-l",
            "-i",
            "-c",
            &format!("printf '{MARK}%s{MARK}' \"$PATH\""),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = sender.send(text);
    });
    let text = match received.recv_timeout(timeout) {
        Ok(text) => text,
        Err(_) => {
            leo_core::children::stop_tree(&mut child);
            let _ = child.wait();
            return None;
        }
    };
    let _ = child.wait();
    let (_, rest) = text.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    Some(path.to_string()).filter(|p| !p.is_empty())
}

pub fn merged(current: &str, login: Option<&str>, extra: &[PathBuf]) -> String {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut seen = Vec::<String>::new();
    let extras = extra.iter().map(|p| p.to_string_lossy().into_owned());
    let entries = login
        .unwrap_or("")
        .split(separator)
        .chain(current.split(separator))
        .map(str::to_string)
        .chain(extras);
    for entry in entries {
        if !entry.is_empty() && !seen.contains(&entry) {
            seen.push(entry);
        }
    }
    seen.join(&separator.to_string())
}

pub fn usual_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home {
        folders.push(home.join(".local/bin"));
        folders.push(home.join(".cargo/bin"));
    }
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[cfg(unix)]
    fn fake_shell(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_noisy_login_shell_still_gives_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "echo 'Welcome to oh-my-zsh'; printf '__LEO_PATH__/opt/homebrew/bin:/usr/bin__LEO_PATH__'; echo bye",
        );
        assert_eq!(
            login_path(&shell, Duration::from_secs(3)).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_login_shell_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "sleep 30");
        let began = Instant::now();
        assert_eq!(login_path(&shell, Duration::from_millis(300)), None);
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn merged_keeps_login_first_without_duplicates() {
        let got = merged(
            "/usr/bin:/bin",
            Some("/opt/homebrew/bin:/usr/bin"),
            &[
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ],
        );
        assert_eq!(got, "/opt/homebrew/bin:/usr/bin:/bin:/usr/local/bin");
    }
}
