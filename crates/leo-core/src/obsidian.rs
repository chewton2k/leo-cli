use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

pub const NOT_INSTALLED: &str =
    "Obsidian is not installed. Get it free at https://obsidian.md, then run this again.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Known,
    Added,
    ByHand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub path: PathBuf,
    pub how: How,
    pub copied: bool,
}

impl Opened {
    pub fn describe(&self) -> Vec<String> {
        let folder = self.path.display();
        let mut lines = match self.how {
            How::Known => vec![format!("Opening your notes in Obsidian ({folder}).")],
            How::Added => vec![
                format!("Added {folder} to Obsidian as a vault, and opened it."),
                "Next time, leo obsidian opens it straight away.".to_string(),
            ],
            How::ByHand => vec![
                "Obsidian is already open, so leo cannot add your notes to its vault list.".to_string(),
                "In Obsidian, open the vault switcher (bottom left), choose \"Open folder as vault\", and pick:".to_string(),
                format!("  {folder}"),
                "Or quit Obsidian and run this again, and leo adds it for you.".to_string(),
            ],
        };
        if self.copied && self.how == How::ByHand {
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
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn running() -> bool {
    let name = if cfg!(target_os = "macos") {
        "Obsidian"
    } else {
        "obsidian"
    };
    Command::new("pgrep")
        .args(["-x", name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn config_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("obsidian").join("obsidian.json"))
}

pub fn vault_uri(id: &str) -> String {
    format!("obsidian://open?vault={id}")
}

fn vault_id_for(config: &str, candidates: &[PathBuf]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(config).ok()?;
    value["vaults"].as_object()?.iter().find_map(|(id, vault)| {
        let path = PathBuf::from(vault["path"].as_str()?);
        let known = path.canonicalize().unwrap_or_else(|_| path.clone());
        candidates
            .iter()
            .any(|c| *c == path || *c == known)
            .then(|| id.clone())
    })
}

fn with_vault(config: Option<&str>, folder: &Path, id: &str, ts: u64) -> Option<String> {
    let mut value: serde_json::Value = match config {
        Some(text) => serde_json::from_str(text).ok()?,
        None => serde_json::json!({}),
    };
    let root = value.as_object_mut()?;
    let vaults = root
        .entry("vaults")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()?;
    vaults.insert(
        id.to_string(),
        serde_json::json!({ "path": folder.to_string_lossy(), "ts": ts }),
    );
    serde_json::to_string(&value).ok()
}

fn new_vault_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

fn add_vault(config_path: &Path, folder: &Path) -> Result<String> {
    let existing = std::fs::read_to_string(config_path).ok();
    let id = new_vault_id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let text = with_vault(existing.as_deref(), folder, &id, ts)
        .context("Obsidian's vault list could not be read, so leo left it alone")?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(old) = &existing {
        std::fs::write(config_path.with_extension("json.leo-backup"), old)?;
    }
    let temp = config_path.with_extension("json.leo-tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, config_path)?;
    Ok(id)
}

fn launch(uri: &str) -> Result<()> {
    let (opener, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![uri])
    } else if cfg!(windows) {
        ("cmd", vec!["/C", "start", "", uri])
    } else {
        ("xdg-open", vec![uri])
    };
    let status = Command::new(opener)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("could not run {opener}"))?;
    if !status.success() {
        anyhow::bail!("{opener} could not open Obsidian");
    }
    Ok(())
}

fn copy_to_clipboard(text: &str) -> bool {
    let tools: [(&str, &[&str]); 5] = [
        ("pbcopy", &[]),
        ("clip", &[]),
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
    let candidates = [notes_dir.to_path_buf(), path.clone()];
    let config_path = config_file();
    let config = config_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    if let Some(id) = config.as_deref().and_then(|c| vault_id_for(c, &candidates)) {
        launch(&vault_uri(&id))?;
        return Ok(Opened {
            path,
            how: How::Known,
            copied: false,
        });
    }
    if let (Some(config_path), false) = (&config_path, running()) {
        if let Ok(id) = add_vault(config_path, &path) {
            launch(&vault_uri(&id))?;
            return Ok(Opened {
                path,
                how: How::Added,
                copied: false,
            });
        }
    }
    let copied = copy_to_clipboard(&path.to_string_lossy());
    launch("obsidian://open")?;
    Ok(Opened {
        path,
        how: How::ByHand,
        copied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn a_link_opens_a_known_vault_by_its_id() {
        assert_eq!(
            vault_uri("440fa080034404b3"),
            "obsidian://open?vault=440fa080034404b3"
        );
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
    fn a_folder_already_in_obsidians_list_is_found() {
        let config = r#"{"vaults":{"aaaa000000000001":{"path":"/Users/me/Vault","ts":1,"open":true},"bbbb000000000002":{"path":"/Users/me/notes","ts":2}},"frame":"hidden"}"#;
        assert_eq!(
            vault_id_for(config, &[PathBuf::from("/Users/me/notes")]).as_deref(),
            Some("bbbb000000000002")
        );
        assert_eq!(vault_id_for(config, &[PathBuf::from("/elsewhere")]), None);
        assert_eq!(vault_id_for("not json", &[PathBuf::from("/x")]), None);
    }

    #[test]
    fn adding_a_vault_keeps_every_other_setting() {
        let config = r#"{"vaults":{"aaaa000000000001":{"path":"/Users/me/Vault","ts":1,"open":true}},"frame":"hidden","updateDisabled":true}"#;
        let written = with_vault(
            Some(config),
            Path::new("/Users/me/notes"),
            "cccc000000000003",
            42,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(value["frame"], "hidden");
        assert_eq!(value["updateDisabled"], true);
        assert_eq!(
            value["vaults"]["aaaa000000000001"]["path"],
            "/Users/me/Vault"
        );
        assert_eq!(
            value["vaults"]["cccc000000000003"]["path"],
            "/Users/me/notes"
        );
        assert_eq!(value["vaults"]["cccc000000000003"]["ts"], 42);
    }

    #[test]
    fn a_missing_list_is_created_and_a_broken_one_is_left_alone() {
        let fresh = with_vault(None, Path::new("/n"), "dddd000000000004", 1).unwrap();
        assert!(fresh.contains("dddd000000000004"), "{fresh}");
        assert!(with_vault(Some("{broken"), Path::new("/n"), "dddd000000000004", 1).is_none());
        assert!(with_vault(Some("[1,2]"), Path::new("/n"), "dddd000000000004", 1).is_none());
    }

    #[test]
    fn new_vault_ids_look_like_obsidians() {
        let id = new_vault_id();
        assert_eq!(id.len(), 16);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(new_vault_id(), id);
    }

    #[test]
    fn each_way_of_opening_says_what_happened() {
        let path = PathBuf::from("/notes");
        let direct = Opened {
            path: path.clone(),
            how: How::Known,
            copied: false,
        }
        .describe()
        .join("\n");
        assert!(direct.contains("/notes"), "{direct}");
        assert!(!direct.contains("Open folder as vault"), "{direct}");
        let added = Opened {
            path: path.clone(),
            how: How::Added,
            copied: false,
        }
        .describe()
        .join("\n");
        assert!(added.contains("vault"), "{added}");
        let by_hand = Opened {
            path,
            how: How::ByHand,
            copied: true,
        }
        .describe()
        .join("\n");
        assert!(by_hand.contains("Open folder as vault"), "{by_hand}");
        assert!(by_hand.contains("clipboard"), "{by_hand}");
        assert!(by_hand.contains("quit Obsidian"), "{by_hand}");
    }
}
