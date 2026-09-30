use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use leo_core::store::Store;
use leo_services::config::Config;

const MARKER: &str = "# Added by the leo installer";

const KNOWN: &[&str] = &[
    "config.toml",
    "config.toml.new",
    "credentials.json",
    "serve-token",
    "update-check.json",
    "recent.json",
    ".env",
    ".manual-installed",
    "notes.json",
    "notes.json.bak",
    "recordings",
];

const TEMP_PREFIXES: &[&str] = &["leo-recording", "leo-live-", "leo-mic-probe-"];

fn leftovers(dir: &Path, notes: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let owned = dir.file_name().is_some_and(|n| n == "leo");
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path == notes {
            continue;
        }
        if notes.starts_with(&path) {
            if owned {
                out.extend(leftovers(&path, notes));
            }
            continue;
        }
        let name = entry.file_name();
        if owned || KNOWN.iter().any(|k| name == *k) {
            out.push(path);
        }
    }
    out
}

fn plan(notes: &Path, home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [leo_core::paths::config_dir(), leo_core::paths::data_dir()]
        .into_iter()
        .flatten()
        .collect();
    dirs.dedup();
    let mut out = Vec::new();
    for dir in dirs {
        let owned = dir.file_name().is_some_and(|n| n == "leo");
        if owned && !notes.starts_with(&dir) && dir.exists() {
            out.push(dir);
        } else {
            out.extend(leftovers(&dir, notes));
        }
    }
    let models = home.join(".leo");
    if !home.as_os_str().is_empty() && models.is_dir() && !notes.starts_with(&models) {
        out.push(models);
    }
    let temp = std::env::var_os("LEO_HOME")
        .is_none()
        .then(std::env::temp_dir)
        .and_then(|t| std::fs::read_dir(t).ok());
    if let Some(entries) = temp {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if TEMP_PREFIXES.iter().any(|p| name.starts_with(p)) && name.ends_with(".wav") {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() && !path.is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

pub fn run(yes: bool) -> Result<()> {
    let exe = std::env::current_exe().context("could not tell where leo is installed")?;
    let dirs: Vec<PathBuf> = [
        exe.parent(),
        exe.canonicalize().ok().as_deref().and_then(Path::parent),
    ]
    .into_iter()
    .flatten()
    .map(Path::to_path_buf)
    .collect();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let notes = Store::notes_dir()?;
    let doomed = plan(&notes, &home);

    if !yes {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("nothing removed; run `leo uninstall --yes` to confirm");
        }
        println!();
        println!("  This removes leo and everything it made, except your notes:");
        println!("    {}", pretty(&exe, &home));
        for path in &doomed {
            println!("    {}", pretty(path, &home));
        }
        println!("    the PATH line the installer added, and any API keys leo stored");
        println!();
        println!("  Your notes stay in {}.", pretty(&notes, &home));
        let answer = super::prompt::ask("  Remove leo? [y/N] ")?;
        if !answer.eq_ignore_ascii_case("y") && !answer.eq_ignore_ascii_case("yes") {
            println!("  Nothing removed.");
            return Ok(());
        }
    }

    let providers: Vec<String> = Config::load().providers.keys().cloned().collect();

    std::fs::remove_file(&exe).with_context(|| format!("could not remove {}", exe.display()))?;
    println!();
    println!("  Removed {}", pretty(&exe, &home));

    for path in &doomed {
        match remove(path) {
            Ok(()) => println!("  Removed {}", pretty(path, &home)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => println!("  Could not remove {}: {e}", pretty(path, &home)),
        }
    }

    let keychain_is_leos = std::env::var_os("LEO_HOME").is_none()
        || std::env::var("LEO_USE_KEYCHAIN").is_ok_and(|v| v != "0" && !v.is_empty());
    if keychain_is_leos {
        let gone = leo_services::config::secret::forget_keychain(&providers);
        if gone > 0 {
            println!(
                "  Removed {gone} key{} from the system keychain",
                if gone == 1 { "" } else { "s" }
            );
        }
    }

    for rc in [
        ".zshrc",
        ".bash_profile",
        ".bashrc",
        ".profile",
        ".config/fish/config.fish",
    ] {
        let path = home.join(rc);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(kept) = dirs
            .iter()
            .find_map(|dir| without_installer_lines(&text, dir))
        {
            std::fs::write(&path, kept)?;
            println!("  Removed the PATH line from {}", pretty(&path, &home));
        }
    }

    println!();
    println!("  Your notes are still in {}.", pretty(&notes, &home));
    println!("  Tools installed separately (SoX, cloudflared, Ollama, whisper.cpp) stay;");
    println!("  remove them with your package manager if you no longer want them.");
    println!("  Thank you for using leo!");
    println!();
    Ok(())
}

fn without_installer_lines(text: &str, dir: &Path) -> Option<String> {
    let dir = dir.to_string_lossy();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        let is_block = lines[i].trim_end() == MARKER
            && lines
                .get(i + 1)
                .is_some_and(|next| next.contains(dir.as_ref()));
        if is_block {
            if kept.last().is_some_and(|l| l.trim().is_empty()) {
                kept.pop();
            }
            changed = true;
            i += 2;
        } else {
            kept.push(lines[i]);
            i += 1;
        }
    }
    changed.then(|| kept.concat())
}

fn pretty(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }

    #[test]
    fn in_a_leo_folder_everything_but_the_notes_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let leo = tmp.path().join("leo");
        let notes = leo.join("notes");
        touch(&notes.join("a.md"));
        touch(&leo.join("config.toml"));
        touch(&leo.join("something-new.json"));
        touch(&leo.join("cache/x"));
        let mut found = leftovers(&leo, &notes);
        found.sort();
        assert_eq!(
            found,
            vec![
                leo.join("cache"),
                leo.join("config.toml"),
                leo.join("something-new.json")
            ]
        );
    }

    #[test]
    fn in_any_other_folder_only_leos_own_files_go() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("me");
        let notes = home.join("notes");
        touch(&notes.join("a.md"));
        touch(&home.join("config.toml"));
        touch(&home.join("serve-token"));
        touch(&home.join("taxes.pdf"));
        touch(&home.join("Documents/x"));
        let mut found = leftovers(&home, &notes);
        found.sort();
        assert_eq!(
            found,
            vec![home.join("config.toml"), home.join("serve-token")]
        );
    }

    #[test]
    fn notes_nested_deeper_keep_their_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let leo = tmp.path().join("leo");
        let notes = leo.join("data/notes");
        touch(&notes.join("a.md"));
        touch(&leo.join("data/recent.json"));
        let found = leftovers(&leo, &notes);
        assert_eq!(found, vec![leo.join("data/recent.json")]);
    }

    #[test]
    fn only_the_installer_block_for_this_directory_is_removed() {
        let dir = Path::new("/home/me/.local/bin");
        let text = "alias ll='ls -l'\n\n# Added by the leo installer\nexport PATH=\"/home/me/.local/bin:$PATH\"\nexport EDITOR=nano\n";
        assert_eq!(
            without_installer_lines(text, dir).as_deref(),
            Some("alias ll='ls -l'\nexport EDITOR=nano\n")
        );
    }

    #[test]
    fn a_file_without_the_block_is_left_alone() {
        let dir = Path::new("/home/me/.local/bin");
        assert_eq!(
            without_installer_lines("export PATH=\"$HOME/bin:$PATH\"\n", dir),
            None
        );
        let other = "# Added by the leo installer\nexport PATH=\"/opt/leo:$PATH\"\n";
        assert_eq!(without_installer_lines(other, dir), None);
    }

    #[test]
    fn fish_blocks_are_removed_too() {
        let dir = Path::new("/home/me/.local/bin");
        let text = "\n# Added by the leo installer\nfish_add_path /home/me/.local/bin\n";
        assert_eq!(without_installer_lines(text, dir).as_deref(), Some(""));
    }
}
