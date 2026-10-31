use super::secret::SecretStore;
use anyhow::Result;
use std::collections::BTreeSet;

pub fn retired(name: &str, kind: Option<&str>) -> bool {
    matches!(name, "openai_whisper" | "gemini_speech" | "xai_speech")
        || matches!(kind, Some("openai_transcribe" | "groq" | "chat_audio"))
}

pub fn retire_speech_keys(text: &str, store: &dyn SecretStore) -> Result<()> {
    let document: toml::Value = toml::from_str(text)?;
    let mut accounts: BTreeSet<String> = [
        "openai",
        "gemini",
        "xai",
        "openai_whisper",
        "gemini_speech",
        "xai_speech",
    ]
    .map(str::to_owned)
    .into();
    let providers = document.get("providers").and_then(toml::Value::as_table);
    if let Some(providers) = providers {
        for (name, block) in providers {
            if retired(name, block.get("kind").and_then(toml::Value::as_str)) {
                accounts.insert(
                    block
                        .get("key_from")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(name)
                        .to_owned(),
                );
                accounts.insert(name.clone());
            }
        }
        if let Some(writers) = document
            .get("chat")
            .and_then(|t| t.get("chain"))
            .and_then(toml::Value::as_array)
        {
            for name in writers.iter().filter_map(toml::Value::as_str) {
                if let Some(block) = providers.get(name) {
                    if !retired(name, block.get("kind").and_then(toml::Value::as_str))
                        && !matches!(
                            name,
                            "openai" | "gemini" | "xai" | "anthropic" | "openrouter"
                        )
                    {
                        accounts.remove(
                            block
                                .get("key_from")
                                .and_then(toml::Value::as_str)
                                .unwrap_or(name),
                        );
                    }
                }
            }
        }
    }
    for account in accounts {
        if store.has(&account) {
            store.delete(&account)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::{
        secret::{MemoryStore, SecretStore},
        Config,
    };

    #[test]
    fn old_cloud_speech_names_and_aliases_load_as_local() {
        for (name, kind) in [
            ("openai_whisper", "openai_transcribe"),
            ("gemini_speech", "chat_audio"),
            ("xai_speech", "openai_transcribe"),
            ("mine", "groq"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, format!("[chat]\nchain = [\"codex\"]\n[transcribe]\nchain = [\"{name}\"]\n[providers.{name}]\nkind = \"{kind}\"\nkey_from = \"old-key\"\n")).unwrap();
            let cfg = Config::load_from(&path);
            assert_eq!(cfg.chat.chain, ["codex"]);
            assert_eq!(cfg.transcribe.chain, ["parakeet"]);
            assert!(cfg.provider(name).is_none());
            assert!(!std::fs::read_to_string(path).unwrap().contains("old-key"));
        }
    }

    #[test]
    fn speech_key_retirement_keeps_calendar_and_writing_accounts() {
        let store = MemoryStore::default();
        for key in [
            "openai",
            "gemini",
            "xai",
            "custom-speech",
            "calendar-1",
            "writer",
        ] {
            store.set(key, "secret").unwrap();
        }
        super::retire_speech_keys(
            r#"[providers.old]
kind = "chat_audio"
key_from = "custom-speech"
[chat]
chain = ["my-writer"]
[providers.my-writer]
kind = "openai"
key_from = "writer"
"#,
            &store,
        )
        .unwrap();
        for key in ["openai", "gemini", "xai", "custom-speech"] {
            assert!(!store.has(key), "{key}");
        }
        for key in ["calendar-1", "writer"] {
            assert!(store.has(key), "{key}");
        }
    }
}

#[cfg(test)]
mod shared_key_tests {
    use super::*;
    use crate::config::secret::MemoryStore;
    #[test]
    fn retired_named_writers_do_not_keep_the_old_speech_keys_alive() {
        for name in ["openai", "gemini", "xai"] {
            let store = MemoryStore::default();
            store.set(name, "old-key").unwrap();
            let text =
                format!("[chat]\nchain = [\"{name}\"]\n[providers.{name}]\nkind = \"openai\"\n");
            retire_speech_keys(&text, &store).unwrap();
            assert!(!store.has(name));
        }
    }
}
