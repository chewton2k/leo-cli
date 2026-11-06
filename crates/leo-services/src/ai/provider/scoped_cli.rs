use std::process::Command;

use super::agent_cli::Agent;

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

pub(crate) fn configure(
    command: &mut Command,
    _agent: Agent,
) -> crate::ai::error::ProviderResult<()> {
    plan_environment(command);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
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
}
