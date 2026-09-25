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
    std::env::var_os("LEO_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn choose(home: Option<PathBuf>, platform: Option<PathBuf>, what: &str) -> Result<PathBuf> {
    match home {
        Some(home) => Ok(home),
        None => platform
            .map(|p| p.join("leo"))
            .with_context(|| format!("could not determine a {what} directory for this platform")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leo_home_holds_everything() {
        let home = PathBuf::from("/tmp/leo-home");
        assert_eq!(choose(Some(home.clone()), Some("/p".into()), "data").unwrap(), home);
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
