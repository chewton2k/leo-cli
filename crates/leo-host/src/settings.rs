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

    fn test(&self, task: &str) -> Result<String> {
        let store = default_store();
        leo_services::web_settings::test(&Config::load(), store.as_ref(), task)
    }
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
        let output = leo_core::children::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "settings::usage_tests::settings_read_saved_usage_without_refreshing_the_cli",
                "--nocapture",
            ])
            .env("LEO_USAGE_TEST_CHILD", "1")
            .env("LEO_HOME", dir.path())
            .env("CODEX_HOME", dir.path().join("codex-profile"))
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
