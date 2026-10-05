//! Where leo keeps things on disk.
//!
//! `LEO_HOME`, when set, holds everything — notes, config, keys — in one
//! directory. That is what the end-to-end tests use to run the real binary
//! without touching the user's notes, and it lets someone keep leo's data
//! somewhere of their choosing. Unset, leo follows the platform: notes under the
//! data directory, config under the config directory, kept apart so that
//! machine-local settings are never pushed along with the notes.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// The directory holding `notes/`.
pub fn data_dir() -> Result<PathBuf> {
    choose(home(), dirs::data_dir(), "data")
}

/// The directory holding `config.toml`, keys, and recent notes.
pub fn config_dir() -> Result<PathBuf> {
    choose(home(), dirs::config_dir(), "config")
}

fn home() -> Option<PathBuf> {
    std::env::var_os("LEO_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn choose(home: Option<PathBuf>, platform: Option<PathBuf>, what: &str) -> Result<PathBuf> {
    match home {
        Some(home) => Ok(home),
        None => platform
            .map(|p| p.join("leo"))
            .with_context(|| format!("could not determine a {what} directory for this platform")),
    }
}

/// Whether `binary` is on the PATH. A plain scan with no subprocess, cheap
/// enough to call on every startup.
pub fn on_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        dir.join(binary).is_file()
            // Windows resolves a bare name through PATHEXT; `.exe` covers the
            // common case without reading the full list.
            || (cfg!(windows) && dir.join(format!("{binary}.exe")).is_file())
    })
}

pub fn validate_directory(directory: &str) -> Result<()> {
    if directory.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        !directory.starts_with('/')
            && !directory.contains('\\')
            && !directory.contains(':')
            && !directory.chars().any(char::is_control)
            && directory
                .split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.')),
        "Folders must stay inside your notes: use a relative folder name without . or .."
    );
    Ok(())
}

pub fn contained_path(root: &std::path::Path, relative: &std::path::Path) -> Result<PathBuf> {
    anyhow::ensure!(
        relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "Path must stay inside your notes"
    );
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => anyhow::ensure!(
                !meta.file_type().is_symlink(),
                "Notes cannot use symbolic links: {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leo_home_holds_everything() {
        let home = PathBuf::from("/tmp/leo-home");
        assert_eq!(
            choose(Some(home.clone()), Some("/p".into()), "data").unwrap(),
            home
        );
    }

    #[test]
    fn without_it_the_platform_directory_is_used() {
        assert_eq!(
            choose(None, Some("/p".into()), "data").unwrap(),
            PathBuf::from("/p/leo")
        );
    }

    #[test]
    fn no_directory_at_all_is_an_error_not_a_panic() {
        assert!(choose(None, None, "data").is_err());
    }
}
