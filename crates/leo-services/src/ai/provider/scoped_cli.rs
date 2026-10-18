use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{ensure, Context, Result};

use super::agent_cli::Agent;

const MOST_PROFILE_ENTRIES: usize = 10_000;
const MOST_PROFILE_DEPTH: usize = 32;

pub fn profile_root() -> Result<PathBuf> {
    let config = crate::config::Config::config_path()?;
    let parent = config
        .parent()
        .context("Leo needs a private settings folder")?;
    leo_core::paths::contained_path(parent, Path::new("agent-profiles"))
}

pub(crate) fn clean_environment(command: &mut Command) {
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_RUNTIME_DIR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TERM",
        "COLORTERM",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "SYSTEMDRIVE",
        "HOMEDRIVE",
        "HOMEPATH",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

pub(crate) fn plan_environment(command: &mut Command) {
    clean_environment(command);
    for name in [
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "CODEX_HOME",
        "CODEX_SQLITE_HOME",
        "CLAUDE_CONFIG_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "CODEX_CA_CERTIFICATE",
        "NODE_EXTRA_CA_CERTS",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn clear_profile_in(home: &Path) -> Result<()> {
    let _lock = crate::private_file::lock(home, ".leo-profile.lock", Duration::ZERO)?;
    fn checked_leaf(root: &Path, path: &Path) -> Result<PathBuf> {
        let parent = path.parent().context("The profile entry needs a folder")?;
        let checked = leo_core::paths::contained_path(root, parent.strip_prefix(root)?)?;
        Ok(checked.join(path.file_name().context("The profile entry needs a name")?))
    }
    fn collect(root: &Path, here: &Path, depth: usize, paths: &mut Vec<PathBuf>) -> Result<()> {
        ensure!(
            depth <= MOST_PROFILE_DEPTH,
            "The writing profile has too many folder levels; remove it on this computer"
        );
        for entry in std::fs::read_dir(here)? {
            ensure!(
                paths.len() < MOST_PROFILE_ENTRIES,
                "The writing profile is too large; remove it on this computer"
            );
            let entry = entry?;
            if entry.file_name() == ".leo-profile.lock" {
                continue;
            }
            let path = checked_leaf(root, &entry.path())?;
            if entry.file_type()?.is_dir() {
                collect(root, &path, depth + 1, paths)?;
            }
            ensure!(
                paths.len() < MOST_PROFILE_ENTRIES,
                "The writing profile is too large; remove it on this computer"
            );
            paths.push(path);
        }
        Ok(())
    }
    let mut paths = Vec::new();
    collect(home, home, 0, &mut paths)?;
    for path in paths {
        let checked = checked_leaf(home, &path)?;
        if std::fs::symlink_metadata(&checked)?.is_dir() {
            std::fs::remove_dir(checked)?;
        } else {
            std::fs::remove_file(checked)?;
        }
    }
    Ok(())
}

pub(crate) fn configure(command: &mut Command, _agent: Agent) -> Result<()> {
    plan_environment(command);
    Ok(())
}

pub fn clear_profile(name: &str) -> Result<()> {
    ensure!(
        matches!(name, "agy" | "copilot"),
        "Choose a legacy writing profile shown in Storage"
    );
    let root = profile_root()?;
    let home = leo_core::paths::contained_path(&root, Path::new(name))?;
    ensure!(
        std::fs::symlink_metadata(&home).is_ok_and(|meta| meta.is_dir()),
        "That legacy writing profile no longer exists. Refresh Storage."
    );
    clear_profile_in(&home)
}

#[cfg(test)]
mod tests {
    use super::super::agent_cli::probe_command;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn plan_writers_preserve_login_locations_without_inheriting_credentials_or_startup_options() {
        for agent in [Agent::Codex, Agent::ClaudeCode] {
            let mut command = Command::new("not-run");
            for name in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CODE_USE_BEDROCK",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
                "CODEX_API_KEY",
                "CODEX_ACCESS_TOKEN",
                "NODE_OPTIONS",
                "NODE_PATH",
                "BASH_ENV",
                "LD_PRELOAD",
                "DYLD_INSERT_LIBRARIES",
            ] {
                command.env(name, "fixture, never used");
            }
            configure(&mut command, agent).unwrap();
            let env: HashMap<_, _> = command.get_envs().collect();
            assert!(!env.contains_key(std::ffi::OsStr::new("ANTHROPIC_API_KEY")));
            assert!(!env.contains_key(std::ffi::OsStr::new("NODE_OPTIONS")));
            for name in [
                "HOME",
                "USERPROFILE",
                "PATH",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "CODEX_HOME",
                "CLAUDE_CONFIG_DIR",
                "DBUS_SESSION_BUS_ADDRESS",
                "CODEX_CA_CERTIFICATE",
                "NODE_EXTRA_CA_CERTS",
            ] {
                let expected = std::env::var_os(name);
                assert_eq!(
                    env.get(std::ffi::OsStr::new(name)).copied().flatten(),
                    expected.as_deref(),
                    "{name}"
                );
            }
            assert!(env.keys().all(|name| {
                !name.to_string_lossy().contains("API_KEY")
                    && !name.to_string_lossy().contains("TOKEN")
            }));
        }
    }

    #[cfg(unix)]
    #[test]
    fn probes_drop_explicit_runtime_and_provider_injection_without_parent_env_changes() {
        for agent in [None, Some(Agent::Codex), Some(Agent::ClaudeCode)] {
            let mut command = Command::new("sh");
            command.args(["-c", "test -z \"${ANTHROPIC_API_KEY+x}${ANTHROPIC_AUTH_TOKEN+x}${OPENAI_API_KEY+x}${CODEX_API_KEY+x}${NODE_OPTIONS+x}${BASH_ENV+x}${COPILOT_PROVIDER_API_KEY_COMMAND+x}${COPILOT_PROVIDERS_CONFIG+x}${COPILOT_SKILLS_DIRS+x}${GITHUB_COPILOT_PROMPT_MODE_EXTENSIONS+x}\" || exit 17; printf clean"]);
            for name in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "OPENAI_API_KEY",
                "CODEX_API_KEY",
                "NODE_OPTIONS",
                "BASH_ENV",
                "COPILOT_PROVIDER_API_KEY_COMMAND",
                "COPILOT_PROVIDERS_CONFIG",
                "COPILOT_SKILLS_DIRS",
                "GITHUB_COPILOT_PROMPT_MODE_EXTENSIONS",
            ] {
                command.env(name, "untrusted fixture, never executed");
            }
            if let Some(agent) = agent {
                configure(&mut command, agent).unwrap();
            } else {
                plan_environment(&mut command);
            }
            assert_eq!(probe_command(command).unwrap(), "clean");
        }
    }

    #[cfg(unix)]
    #[test]
    fn clearing_a_profile_unlinks_customizations_without_touching_their_targets() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("hooks")).unwrap();
        clear_profile_in(home.path()).unwrap();
        assert!(!home.path().join("hooks").exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    }

    #[test]
    fn legacy_profile_cleanup_stays_explicit_and_refuses_an_active_owner() {
        let home = tempfile::tempdir().unwrap();
        let private = home.path().join("token.json");
        std::fs::write(&private, "fake saved sign-in").unwrap();
        let guard =
            crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO).unwrap();
        assert!(clear_profile_in(home.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(&private).unwrap(),
            "fake saved sign-in"
        );
        drop(guard);
        clear_profile_in(home.path()).unwrap();
        assert!(!private.exists());
        assert!(home.path().join(".leo-profile.lock").is_file());
    }
}
