//! `leo uninstall`: remove the program and the PATH line the installer added.
//! Notes, settings and keys stay, since they are the user's, not the program's.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use leo_core::store::Store;
use leo_services::config::Config;

/// The comment install.sh writes above its PATH line.
const MARKER: &str = "# Added by the leo installer";

pub fn run(yes: bool) -> Result<()> {
    let exe = std::env::current_exe().context("could not tell where leo is installed")?;
    // The directory as the shell file names it may be the resolved path or not
    // (macOS reports /var as /private/var), so a PATH line naming either counts.
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

    if !yes {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("nothing removed; run `leo uninstall --yes` to confirm");
        }
        let answer = super::prompt::ask(&format!(
            "  Remove leo from {}? Your notes stay. [y/N] ",
            pretty(&exe, &home)
        ))?;
        if !answer.eq_ignore_ascii_case("y") && !answer.eq_ignore_ascii_case("yes") {
            println!("  Nothing removed.");
            return Ok(());
        }
    }

    std::fs::remove_file(&exe).with_context(|| format!("could not remove {}", exe.display()))?;
    println!();
    println!("  Removed {}", pretty(&exe, &home));

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
    if let Ok(notes) = Store::notes_dir() {
        println!("  Your notes are still in {}", pretty(&notes, &home));
    }
    if let Ok(config) = Config::config_path() {
        println!("  and your settings in {}", pretty(&config, &home));
    }
    println!("  Delete those folders too if you want them gone. Thank you for using leo!");
    println!();
    Ok(())
}

/// `text` without the installer's block for `dir`: its comment, its PATH line
/// and the blank line before them. `None` when there is no such block, so an
/// untouched file is not rewritten.
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

/// A path with the home directory shown as `~`.
fn pretty(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Another directory's block belongs to another install.
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
