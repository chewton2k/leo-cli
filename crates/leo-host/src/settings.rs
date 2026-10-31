use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use leo_services::config::choice;
use leo_services::config::secret::{default_store, SecretStore};
use leo_services::config::Config;

pub struct WebSettings;

impl leo_web::SettingsApi for WebSettings {
    fn describe(&self, notes_dir: &Path) -> Value {
        let cfg = Config::load();
        if cfg.chat.chain.first().map(String::as_str) == Some("codex") {
            leo_services::usage::refresh_codex(&cfg);
        }
        let store = default_store();
        let local = choice::local_models(&cfg);
        let usage = leo_services::usage::load();
        leo_services::web_settings::describe(&cfg, store.as_ref(), &local, &usage, Some(notes_dir))
    }

    fn apply(&self, change: &Value) -> Result<String> {
        let path = Config::config_path()?;
        let cfg = Config::load();
        let store = default_store();
        let local = choice::local_models(&cfg);
        leo_services::web_settings::apply(change, &path, store.as_ref(), &local)
    }

    fn backup(&self, notes_dir: &Path) -> Result<Option<std::process::Command>> {
        backup_command(notes_dir, &Config::load().sync)
    }

    fn test(&self, task: &str) -> Result<String> {
        let store = default_store();
        leo_services::web_settings::test(&Config::load(), store.as_ref(), task)
    }
}

fn backup_command(
    notes_dir: &Path,
    config: &leo_services::config::sync::SyncConfig,
) -> Result<Option<std::process::Command>> {
    if config.auto_push == leo_services::config::sync::AutoPush::Off
        || !leo_core::sync::is_initialized(notes_dir)
        || leo_core::sync::remote_url(notes_dir).is_none()
    {
        return Ok(None);
    }
    leo_core::sync::auto_commit(notes_dir)?;
    if !leo_services::config::sync::should_push_on_quit(config, leo_core::sync::unpushed(notes_dir))
    {
        return Ok(None);
    }
    let branch = leo_core::sync::current_branch(notes_dir)?;
    let mut command = std::process::Command::new("git");
    command
        .args(["push", "origin", &branch])
        .current_dir(notes_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=2")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(Some(command))
}

impl leo_web::CalendarAccess for WebSettings {
    fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        leo_services::web::fetch_public(url, leo_web::CALENDAR_BYTES)
    }
    fn fetch_since(
        &self,
        url: &str,
        etag: Option<&str>,
        modified: Option<&str>,
    ) -> Result<leo_web::Fetched> {
        let got =
            leo_services::web::fetch_public_since(url, leo_web::CALENDAR_BYTES, etag, modified)?;
        Ok(leo_web::Fetched {
            body: got.body,
            etag: got.etag,
            modified: got.modified,
        })
    }
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(default_store()
            .get(account)?
            .map(|s| s.as_str().to_string()))
    }
    fn set(&self, account: &str, value: &str) -> Result<()> {
        default_store().set(account, value)
    }
    fn delete(&self, account: &str) -> Result<()> {
        default_store().delete(account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leo_services::config::sync::{AutoPush, SyncConfig};

    fn git(dir: &Path, args: &[&str]) -> String {
        let result = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn website_backup_respects_off_and_pushes_note_and_task_changes_only_to_an_existing_remote() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        let mut config = SyncConfig {
            auto_push: AutoPush::OnQuit,
            ..SyncConfig::default()
        };
        assert!(backup_command(&notes, &config).unwrap().is_none());
        git(&notes, &["init", "-b", "main"]);
        for (name, value) in [
            ("user.name", "Leo test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
            ("credential.helper", ""),
        ] {
            git(&notes, &["config", name, value]);
        }
        let hooks = dir.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        git(
            &notes,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        let note = notes.join("Test.md");
        std::fs::write(&note, "Original").unwrap();
        leo_core::sync::auto_commit(&notes).unwrap();
        assert!(backup_command(&notes, &config).unwrap().is_none());
        let remote = dir.path().join("remote.git");
        git(dir.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(
            &notes,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&notes, &["push", "-u", "origin", "main"]);
        let head = git(&notes, &["rev-parse", "HEAD"]);
        std::fs::write(&note, "Edited").unwrap();
        let tasks = notes.join(".leo/tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(tasks.join("task-test.json"), "{\"title\":\"Study\"}").unwrap();
        config.auto_push = AutoPush::Off;
        assert!(backup_command(&notes, &config).unwrap().is_none());
        assert_eq!(git(&notes, &["rev-parse", "HEAD"]), head);
        for policy in [AutoPush::OnQuit, AutoPush::WhenIdle] {
            config.auto_push = policy;
            std::fs::write(&note, format!("Edited for {}", policy.as_str())).unwrap();
            let mut command = backup_command(&notes, &config).unwrap().unwrap();
            assert_eq!(command.get_program(), "git");
            assert!(command.status().unwrap().success());
            assert_eq!(
                git(&remote, &["rev-parse", "refs/heads/main"]),
                git(&notes, &["rev-parse", "HEAD"])
            );
            assert!(backup_command(&notes, &config).unwrap().is_none());
        }
        assert_eq!(
            git(&remote, &["show", "main:.leo/tasks/task-test.json"]),
            "{\"title\":\"Study\"}"
        );
    }
}
