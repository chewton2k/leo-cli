//! The provider screen's behavior: building its rows from config plus the
//! keychain, and applying the actions the user takes on them.
//!
//! Kept out of `tui/mod.rs` so the row-building and chain-editing logic can be
//! tested against an in-memory secret store, with no terminal and no keychain.

use anyhow::Result;

use crate::config::edit::{self, Task};
use crate::config::provider::ProviderKind;
use crate::config::secret::{redact, resolve, SecretStore};
use crate::config::Config;
use crate::tui::view::settings::{Credential, Row};

/// Which chain a provider kind can serve. A transcription provider in the chat
/// chain would be silently dropped by the chain builder, so the screen offers
/// the right one instead.
fn task_for(kind: Option<ProviderKind>) -> Task {
    match kind {
        Some(ProviderKind::Openai) => Task::Chat,
        _ => Task::Transcribe,
    }
}

/// Describe where a provider's credential comes from, without revealing it.
fn credential_for(
    name: &str,
    key_env: Option<&str>,
    store: &dyn SecretStore,
) -> Credential {
    let Some(var) = key_env else {
        return Credential::NotNeeded;
    };
    if let Ok(value) = std::env::var(var) {
        if !value.trim().is_empty() {
            return Credential::Env {
                var: var.to_string(),
                redacted: redact(&value),
            };
        }
    }
    // `None` for the env name on purpose: it was just checked, so this asks
    // only the keychain.
    match resolve(name, None, store) {
        Some(secret) => Credential::Keychain(redact(secret.as_str())),
        None => Credential::Missing,
    }
}

/// Build the screen: both chains in order, then everything else that is
/// configured.
pub fn rows(cfg: &Config, store: &dyn SecretStore) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut in_a_chain: Vec<&String> = Vec::new();

    for task in [Task::Chat, Task::Transcribe] {
        let chain = match task {
            Task::Chat => &cfg.chat.chain,
            Task::Transcribe => &cfg.transcribe.chain,
        };
        rows.push(Row::Header(task));

        // Availability comes from the real provider implementations, so the
        // marker means "the chain runner would use this now", not a guess.
        let ready: Vec<bool> = match task {
            Task::Chat => crate::ai::provider::build_chat_chain(cfg, store)
                .iter()
                .map(|p| p.available())
                .collect(),
            Task::Transcribe => crate::ai::provider::build_transcribe_chain(cfg, store)
                .iter()
                .map(|p| p.available())
                .collect(),
        };
        // The builder drops entries it cannot construct, so line readiness up by
        // name rather than by index.
        let ready_names: Vec<String> = match task {
            Task::Chat => crate::ai::provider::build_chat_chain(cfg, store)
                .iter()
                .map(|p| p.name().to_string())
                .collect(),
            Task::Transcribe => crate::ai::provider::build_transcribe_chain(cfg, store)
                .iter()
                .map(|p| p.name().to_string())
                .collect(),
        };

        for (i, name) in chain.iter().enumerate() {
            in_a_chain.push(name);
            let (model, credential) = match cfg.provider(name) {
                Some(pc) => (
                    pc.model.clone().unwrap_or_else(|| "(default)".to_string()),
                    credential_for(name, pc.key_env.as_deref(), store),
                ),
                // Named in a chain but never defined: worth showing rather than
                // hiding, since it is a config typo the user should see.
                None => ("(not configured)".to_string(), Credential::Missing),
            };
            let is_ready = ready_names
                .iter()
                .position(|n| n == name)
                .and_then(|p| ready.get(p).copied())
                .unwrap_or(false);

            rows.push(Row::Member {
                task,
                position: i + 1,
                name: name.clone(),
                model,
                credential,
                ready: is_ready,
            });
        }
    }

    let unused: Vec<(&String, &crate::config::provider::ProviderConfig)> = cfg
        .providers
        .iter()
        .filter(|(name, _)| !in_a_chain.contains(name))
        .collect();

    if !unused.is_empty() {
        rows.push(Row::AvailableHeader);
        for (name, pc) in unused {
            rows.push(Row::Unused {
                name: name.clone(),
                model: pc.model.clone().unwrap_or_else(|| "(default)".to_string()),
                credential: credential_for(name, pc.key_env.as_deref(), store),
                task: task_for(pc.kind),
            });
        }
    }

    rows
}

/// What the App should do after an action that changed the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// Nothing was written.
    No,
    /// The file changed; reload config and rebuild the rows. Carries a message.
    Yes(String),
}

/// Move a chain member up or down and persist it. `delta` is -1 or 1.
pub fn reorder(task: Task, name: &str, delta: isize) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let mut chain = edit::read_chain(&doc, task);

    let Some(index) = chain.iter().position(|n| n == name) else {
        return Ok(Changed::No);
    };
    let moved = if delta < 0 {
        edit::move_up(&mut chain, index)
    } else {
        edit::move_down(&mut chain, index)
    };
    if moved == index {
        return Ok(Changed::No);
    }

    edit::write_chain(&mut doc, task, &chain);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "{name} is now {} in the {} chain",
        moved + 1,
        task.label()
    )))
}

/// Add a provider to the chain its kind can serve.
pub fn add_to_chain(task: Task, name: &str) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let mut chain = edit::read_chain(&doc, task);
    if !edit::add(&mut chain, name) {
        return Ok(Changed::No);
    }
    edit::write_chain(&mut doc, task, &chain);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "{name} added to the {} chain",
        task.label()
    )))
}

/// Remove a provider from a chain. Its definition stays in the file, so this is
/// reversible with one keypress.
pub fn remove_from_chain(task: Task, name: &str) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let mut chain = edit::read_chain(&doc, task);
    if !edit::remove(&mut chain, name) {
        return Ok(Changed::No);
    }
    edit::write_chain(&mut doc, task, &chain);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "{name} removed from the {} chain (still configured)",
        task.label()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn small_config() -> Config {
        Config::parse(
            r#"
[chat]
chain = ["ollama", "openrouter"]

[transcribe]
chain = ["groq"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "LEO_TEST_SETTINGS_OR"

[providers.groq]
kind = "groq"
model = "whisper-large-v3-turbo"
key_env = "LEO_TEST_SETTINGS_GROQ"

[providers.cerebras]
kind = "openai"
base_url = "https://api.cerebras.ai/v1"
model = "llama-3.3-70b"
key_env = "LEO_TEST_SETTINGS_CB"
"#,
        )
        .unwrap()
    }

    #[test]
    fn rows_list_both_chains_in_order_then_the_rest() {
        let _guard = ENV_LOCK.lock().unwrap();
        for v in ["LEO_TEST_SETTINGS_OR", "LEO_TEST_SETTINGS_GROQ", "LEO_TEST_SETTINGS_CB"] {
            std::env::remove_var(v);
        }
        let rows = rows(&small_config(), &MemoryStore::default());

        assert_eq!(rows[0], Row::Header(Task::Chat));
        assert_eq!(rows[1].provider_name(), Some("ollama"));
        assert_eq!(rows[2].provider_name(), Some("openrouter"));
        assert_eq!(rows[3], Row::Header(Task::Transcribe));
        assert_eq!(rows[4].provider_name(), Some("groq"));
        assert_eq!(rows[5], Row::AvailableHeader);
        // Everything configured but unchained shows up, so a user can see what
        // is available without opening the file.
        assert_eq!(rows[6].provider_name(), Some("cerebras"));
        assert_eq!(rows.len(), 7);
    }

    #[test]
    fn a_chain_position_is_shown_as_its_priority() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(&small_config(), &MemoryStore::default());
        match &rows[2] {
            Row::Member { position, name, .. } => {
                assert_eq!(*position, 2);
                assert_eq!(name, "openrouter");
            }
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_keyless_local_provider_needs_no_credential() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(&small_config(), &MemoryStore::default());
        match &rows[1] {
            Row::Member { credential, .. } => assert_eq!(*credential, Credential::NotNeeded),
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_stored_key_shows_as_keychain_and_a_missing_one_says_so() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETTINGS_OR");
        std::env::remove_var("LEO_TEST_SETTINGS_GROQ");

        let store = MemoryStore::default();
        store.set("openrouter", "sk-or-v1-secret9999").unwrap();
        let rows = rows(&small_config(), &store);

        match &rows[2] {
            Row::Member { credential, .. } => {
                assert_eq!(*credential, Credential::Keychain("…9999".to_string()));
            }
            other => panic!("expected a member, got {other:?}"),
        }
        match &rows[4] {
            Row::Member { credential, .. } => assert_eq!(*credential, Credential::Missing),
            other => panic!("expected a member, got {other:?}"),
        }
    }

    /// Env wins over the keychain, and the screen has to say so — otherwise a
    /// user who logs in and sees no change has no way to understand why.
    #[test]
    fn an_env_var_is_reported_as_env_even_when_the_keychain_has_one() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_SETTINGS_OR", "from-env-1234");
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain-9999").unwrap();

        let rows = rows(&small_config(), &store);
        std::env::remove_var("LEO_TEST_SETTINGS_OR");

        match &rows[2] {
            Row::Member { credential, .. } => match credential {
                Credential::Env { var, redacted } => {
                    assert_eq!(var, "LEO_TEST_SETTINGS_OR");
                    assert_eq!(redacted, "…1234");
                }
                other => panic!("expected env, got {other:?}"),
            },
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_chain_entry_with_no_provider_block_is_shown_rather_than_hidden() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse("[chat]\nchain = [\"ghost\"]\n").unwrap();
        let rows = rows(&cfg, &MemoryStore::default());

        match &rows[1] {
            Row::Member { name, model, ready, .. } => {
                assert_eq!(name, "ghost");
                assert_eq!(model, "(not configured)");
                assert!(!ready);
            }
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn an_unused_provider_is_offered_to_the_chain_its_kind_can_serve() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse(
            r#"
[providers.some_chat]
kind = "openai"
base_url = "https://x/v1"
model = "m"
key_env = "A_API_KEY"

[providers.some_whisper]
kind = "groq"
model = "w"
key_env = "B_API_KEY"

[providers.local_binary]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "/nope"
"#,
        )
        .unwrap();
        let rows = rows(&cfg, &MemoryStore::default());

        let task_of = |name: &str| {
            rows.iter()
                .find(|r| r.provider_name() == Some(name))
                .and_then(|r| r.task())
                .unwrap()
        };
        assert_eq!(task_of("some_chat"), Task::Chat);
        assert_eq!(task_of("some_whisper"), Task::Transcribe);
        assert_eq!(task_of("local_binary"), Task::Transcribe);
    }

    #[test]
    fn an_empty_config_produces_only_headers() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse("").unwrap();
        let rows = rows(&cfg, &MemoryStore::default());
        assert_eq!(rows, vec![Row::Header(Task::Chat), Row::Header(Task::Transcribe)]);
    }

    #[test]
    fn readiness_reflects_whether_the_chain_runner_would_use_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETTINGS_OR");
        let store = MemoryStore::default();

        // Without a key, openrouter is not ready.
        let rows_before = rows(&small_config(), &store);
        let ready_before = matches!(&rows_before[2], Row::Member { ready: true, .. });
        assert!(!ready_before);

        // With one, it is.
        store.set("openrouter", "a-key").unwrap();
        let rows_after = rows(&small_config(), &store);
        assert!(matches!(&rows_after[2], Row::Member { ready: true, .. }));
    }
}
