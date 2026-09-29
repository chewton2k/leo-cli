use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

pub const NOT_INSTALLED: &str =
    "Obsidian is not installed. Get it free at https://obsidian.md, then run this again.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub path: PathBuf,
    pub copied: bool,
}

impl Opened {
    pub fn describe(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Opening {} in Obsidian.", self.path.display()),
            "First time? If Obsidian does not show your notes, choose \"Open folder as vault\" and pick that folder.".to_string(),
        ];
        if self.copied {
            lines.push("The folder's path is on your clipboard.".to_string());
        }
        lines
    }
}

pub fn installed() -> bool {
    if let Some(marker) = std::env::var_os("LEO_OBSIDIAN_APP").filter(|v| !v.is_empty()) {
        return Path::new(&marker).exists();
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        return Path::new("/Applications/Obsidian.app").exists()
            || home.join("Applications/Obsidian.app").exists();
    }
    crate::paths::on_path("obsidian") || has_link_handler()
}

fn has_link_handler() -> bool {
    Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/obsidian"])
        .output()
        .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn uri(path: &Path) -> String {
    format!(
        "obsidian://open?path={}",
        percent_encode(&path.to_string_lossy())
    )
}

fn copy_to_clipboard(text: &str) -> bool {
    let tools: [(&str, &[&str]); 4] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (tool, args) in tools {
        if !crate::paths::on_path(tool) {
            continue;
        }
        let Ok(mut child) = Command::new(tool)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let wrote = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
        let finished = child.wait().is_ok_and(|status| status.success());
        if wrote && finished {
            return true;
        }
    }
    false
}

pub fn open(notes_dir: &Path) -> Result<Opened> {
    if !installed() {
        anyhow::bail!(NOT_INSTALLED);
    }
    let path = notes_dir
        .canonicalize()
        .unwrap_or_else(|_| notes_dir.to_path_buf());
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let status = Command::new(opener)
        .arg(uri(&path))
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("could not run {opener}"))?;
    if !status.success() {
        anyhow::bail!("{opener} could not open Obsidian");
    }
    let copied = copy_to_clipboard(&path.to_string_lossy());
    Ok(Opened { path, copied })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn the_link_encodes_everything_but_plain_characters() {
        assert_eq!(
            uri(Path::new("/Users/me/My Notes")),
            "obsidian://open?path=%2FUsers%2Fme%2FMy%20Notes"
        );
        assert_eq!(
            uri(Path::new("/a-b_c.d~e")),
            "obsidian://open?path=%2Fa-b_c.d~e"
        );
        assert_eq!(uri(Path::new("/笔")), "obsidian://open?path=%2F%E7%AC%94");
    }

    #[test]
    fn the_override_says_whether_obsidian_is_installed() {
        let _guard = ENV.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("LEO_OBSIDIAN_APP", tmp.path());
        assert!(installed());
        std::env::set_var("LEO_OBSIDIAN_APP", tmp.path().join("missing"));
        assert!(!installed());
        std::env::remove_var("LEO_OBSIDIAN_APP");
    }

    #[test]
    fn the_report_says_how_to_add_the_folder_as_a_vault() {
        let opened = Opened {
            path: PathBuf::from("/notes"),
            copied: true,
        };
        let text = opened.describe().join("\n");
        assert!(text.contains("/notes"), "{text}");
        assert!(text.contains("Open folder as vault"), "{text}");
        assert!(text.contains("clipboard"), "{text}");
        let plain = Opened {
            path: PathBuf::from("/notes"),
            copied: false,
        };
        assert!(!plain.describe().join("\n").contains("clipboard"));
    }
}
