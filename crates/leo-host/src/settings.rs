use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use leo_services::config::choice;
use leo_services::config::secret::{default_store, SecretStore};
use leo_services::config::Config;

pub struct WebSettings;

impl leo_web::SettingsApi for WebSettings {
    fn describe(&self, notes_dir: &Path) -> Value {
        let cfg = Config::load();
        let store = default_store();
        let local = choice::local_models(&cfg);
        let usage = leo_services::usage::load();
        let mut page = leo_services::web_settings::describe(
            &cfg,
            store.as_ref(),
            &local,
            &usage,
            Some(notes_dir),
        );
        page["plans"] = serde_json::json!(leo_services::web_settings::coding_plans(&cfg, &usage));
        page["reads_stay_here"] = serde_json::json!(leo_services::ai::reads_stay_here(&cfg));
        page
    }

    fn refresh_usage(&self) {
        leo_services::usage::refresh_codex(&Config::load());
    }

    fn apply(&self, change: &Value) -> Result<String> {
        let path = Config::config_path()?;
        let cfg = Config::load();
        let store = default_store();
        let local = choice::local_models(&cfg);
        leo_services::web_settings::apply(change, &path, store.as_ref(), &local)
    }

    fn setup_backup(&self, notes_dir: &Path, name: &str) -> Result<String> {
        let remote = leo_core::sync::remote_url(notes_dir);
        setup_backup_with(name, remote.as_deref(), || {
            leo_core::sync::github(notes_dir,name).map(|backup|backup.describe()).context("Backup setup could not finish. Install GitHub CLI and sign in with gh auth login, then try again.")
        })
    }

    fn backup(&self, notes_dir: &Path) -> Result<Option<std::process::Command>> {
        backup_command(notes_dir, &Config::load().sync)
    }

    fn test(&self, task: &str) -> Result<String> {
        let store = default_store();
        leo_services::web_settings::test(&Config::load(), store.as_ref(), task)
    }
}

fn setup_backup_with(
    name: &str,
    remote: Option<&str>,
    setup: impl FnOnce() -> Result<String>,
) -> Result<String> {
    let parts: Vec<&str> = name.split('/').collect();
    anyhow::ensure!(
        (1..=2).contains(&parts.len())
            && parts.iter().all(|part| !part.is_empty()
                && part.len() <= 100
                && !part.starts_with(['-', '.'])
                && !part.ends_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
        "Use a GitHub repository name such as leo-notes or your-name/leo-notes."
    );
    anyhow::ensure!(
        remote.is_none(),
        "Backup is already connected. Its existing remote has been kept."
    );
    setup()
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
    let mut command = leo_core::children::quiet_command("git");
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

#[cfg(test)]
mod backup_setup_tests {
    #[test]
    fn setup_validates_before_writing_and_never_replaces_an_existing_remote() {
        for name in ["", "--public", "../notes", "owner/repo/extra", "repo name"] {
            assert!(
                super::setup_backup_with(name, None, || panic!("must not contact GitHub")).is_err()
            );
        }
        assert!(super::setup_backup_with(
            "leo-notes",
            Some("https://github.com/me/existing"),
            || panic!("must not contact GitHub")
        )
        .is_err());
        assert_eq!(
            super::setup_backup_with("owner/leo-notes", None, || Ok("connected".into())).unwrap(),
            "connected"
        );
    }
}

#[cfg(all(test, unix))]
mod usage_tests {
    use leo_web::SettingsApi;

    #[test]
    fn settings_read_saved_usage_without_refreshing_the_cli() {
        if std::env::var_os("LEO_USAGE_TEST_CHILD").is_some() {
            let home = leo_core::paths::config_dir().unwrap();
            let notes = home.join("notes");
            std::fs::create_dir_all(&notes).unwrap();
            super::WebSettings.describe(&notes);
            assert!(!home.join("usage-called").exists());
            super::WebSettings.refresh_usage();
            assert!(home.join("usage-called").exists());
            std::fs::remove_file(home.join("usage-called")).unwrap();
            let saved = super::WebSettings.describe(&notes)["plans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|plan| plan["provider"] == "codex")
                .unwrap()["limits"]
                .clone();
            assert_eq!(saved["five_hour"]["used"], 0.12);
            assert!(!home.join("usage-called").exists());
            std::fs::write(home.join("codex"), "#!/bin/sh\nexit 1\n").unwrap();
            super::WebSettings.refresh_usage();
            let after = super::WebSettings.describe(&notes);
            assert_eq!(
                after["plans"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|plan| plan["provider"] == "codex")
                    .unwrap()["limits"],
                saved
            );
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("codex");
        std::fs::write(&bin, r#"#!/bin/sh
if [ "$1" = login ]; then echo 'Logged in using ChatGPT'; exit 0; fi
if [ "$1" = features ]; then echo 'shell_tool stable true'; exit 0; fi
if [ "$2" = --help ]; then echo '--ignore-user-config --ignore-rules'; exit 0; fi
if [ "$1" != app-server ]; then exit 0; fi
touch "${0%/*}/usage-called"
while IFS= read -r line; do
  case "$line" in
    *'"method":"account/rateLimits/read"'*)
      printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":300}}}}'
      exit 0 ;;
  esac
done
"#).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = format!("[chat]\nchain = [\"ollama\"]\n[providers.codex]\nbin = {}\n[providers.claude_code]\nbin = {}\n[providers.ollama]\nbase_url = \"\"\n", serde_json::to_string(&bin).unwrap(), serde_json::to_string(&dir.path().join("no-claude")).unwrap());
        std::fs::write(dir.path().join("config.toml"), config).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "settings::usage_tests::settings_read_saved_usage_without_refreshing_the_cli",
                "--nocapture",
            ])
            .env("LEO_USAGE_TEST_CHILD", "1")
            .env("LEO_HOME", dir.path())
            .env_remove("LEO_USE_KEYCHAIN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
