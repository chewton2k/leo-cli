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

    fn apply(&self, change: &Value, secure: bool) -> Result<String> {
        let path = Config::config_path()?;
        let cfg = Config::load();
        let store = default_store();
        let local = choice::local_models(&cfg);
        leo_services::web_settings::apply(change, &path, store.as_ref(), secure, &local)
    }

    fn test(&self, task: &str) -> Result<String> {
        let store = default_store();
        leo_services::web_settings::test(&Config::load(), store.as_ref(), task)
    }
}

impl leo_web::CalendarSecrets for WebSettings {
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
