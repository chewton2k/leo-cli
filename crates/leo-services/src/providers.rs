//! Everything that touches AI providers from outside the chains: describing a
//! credential, storing and removing keys, testing one provider, and opening the
//! config file. The CLI and the TUI's profile screen both call these, so there
//! is one implementation of each.

use anyhow::Result;
use colored::Colorize;

use crate::ai;
use crate::config::{
    self,
    secret::{redact, resolve, SecretStore},
    Config,
};

/// What a model command asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAction {
    Login { name: String },
    Logout { name: String },
}

/// What a config command asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAction {
    Edit,
}

/// Build the single transcription provider named by `name`, regardless of
/// whether it appears in the `[transcribe]` chain — `leo model test <name>`
/// must work for a provider a user is still setting up.
pub fn build_one_transcriber(
    name: &str,
    pc: &config::provider::ProviderConfig,
    store: &dyn SecretStore,
) -> Option<Box<dyn ai::provider::TranscribeProvider>> {
    use config::provider::ProviderKind;
    match pc.kind {
        Some(ProviderKind::Hf) => Some(Box::new(ai::provider::hf::HfTranscribe::new(
            name.to_string(),
            pc,
            resolve(name, pc.key_env.as_deref(), store),
        ))),
        Some(ProviderKind::Groq) => Some(Box::new(ai::provider::groq::GroqTranscribe::new(
            name.to_string(),
            pc,
            resolve(name, pc.key_env.as_deref(), store),
        ))),
        Some(ProviderKind::WhisperCpp) => Some(Box::new(
            ai::provider::whisper_cpp::WhisperCppTranscribe::new(name.to_string(), pc),
        )),
        _ => None,
    }
}

/// Send one minimal request to a provider and report what happened.
///
/// Returns the report rather than printing it, so the CLI can print it and the
/// provider screen can show it in its status line. Transcription providers are
/// only checked for reachability: exercising one needs audio, and a test that
/// records from the microphone is not a test anyone wants to run twice.
pub fn test_provider(name: &str) -> Result<String> {
    let cfg = Config::load();
    let store = config::secret::default_store();
    let Some(pc) = cfg.provider(name) else {
        anyhow::bail!("no provider named '{name}' in your config");
    };

    let started = std::time::Instant::now();
    match pc.kind {
        Some(config::provider::ProviderKind::Openai) => {
            let key = resolve(name, pc.key_env.as_deref(), &store);
            let p = ai::provider::openai::OpenAiChat::new(name.to_string(), pc, key);
            if !p.available() {
                anyhow::bail!("{}", p.unavailable_reason());
            }
            use ai::provider::ChatProvider;
            let reply = p
                .complete(&ai::provider::ChatRequest {
                    system: None,
                    prompt: "Reply with the single word: ok".to_string(),
                    temperature: 0.0,
                    max_tokens: 16,
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let reply = reply.trim().replace('\n', " ");
            // A reasoning model can answer at length; one line is enough here.
            let reply: String = reply.chars().take(60).collect();
            Ok(format!(
                "{name} responded in {:?}: {reply}",
                started.elapsed()
            ))
        }
        Some(_) => match build_one_transcriber(name, pc, &store) {
            Some(p) if p.available() => Ok(format!("{name} is reachable")),
            Some(p) => anyhow::bail!("{}", p.unavailable_reason()),
            None => anyhow::bail!("provider '{name}' could not be built"),
        },
        None => anyhow::bail!("provider '{name}' has no `kind`"),
    }
}

/// Providers in a chain that need a key and have none, in chain order.
pub fn providers_missing_keys(cfg: &Config, store: &dyn SecretStore) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in cfg.chat.chain.iter().chain(&cfg.transcribe.chain) {
        let Some(var) = cfg.provider(name).and_then(|p| p.key_env.as_deref()) else {
            continue;
        };
        let in_env = std::env::var(var)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if !in_env && !store.has(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

pub fn model(command: ModelAction) -> Result<()> {
    let cfg = Config::load();
    let store = config::secret::default_store();

    match command {
        ModelAction::Login { name } => {
            if cfg.provider(&name).is_none() {
                println!(
                    "  {} no [providers.{name}] block in your config — storing the key anyway.",
                    "note".yellow()
                );
                println!("  Press Ctrl-S in leo to add it, or run `leo doctor` to see what is configured.");
            }
            let key_env = cfg.provider(&name).and_then(|p| p.key_env.clone());

            // Offer to import an existing .env value rather than making the
            // user paste it again.
            if let Some(var) = key_env.as_deref() {
                if let Ok(existing) = std::env::var(var) {
                    if !existing.trim().is_empty() {
                        println!("  Found {var} in your environment ({}).", redact(&existing));
                        print!("  Store it for you? [Y/n] ");
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                        let mut answer = String::new();
                        std::io::stdin().read_line(&mut answer)?;
                        if !answer.trim().eq_ignore_ascii_case("n") {
                            store.set(&name, existing.trim())?;
                            println!("  {} stored for {name}.", "ok".green());
                            println!(
                                "  You can now remove this line from your .env:\n    {var}=..."
                            );
                            return Ok(());
                        }
                    }
                }
            }

            // Read with echo disabled: never on screen, never in shell history,
            // never an argv value visible to other processes.
            let secret = rpassword::prompt_password(format!("  API key for {name}: "))?;
            if secret.trim().is_empty() {
                anyhow::bail!("no key entered");
            }
            store.set(&name, secret.trim())?;
            println!("  {} stored for {name}.", "ok".green());
            Ok(())
        }

        ModelAction::Logout { name } => {
            store.delete(&name)?;
            println!("  {} removed key for {name}.", "ok".green());
            Ok(())
        }
    }
}

pub fn config_file(command: ConfigAction) -> Result<()> {
    match command {
        ConfigAction::Edit => {
            let (path, created) = Config::ensure_exists()?;
            if created {
                println!("  Created {}", path.display());
            }
            leo_core::editor::open(&path)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;
    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// need one, and have none.
    #[test]
    fn doctor_offers_keys_only_where_one_is_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETUP_A");
        std::env::remove_var("LEO_TEST_SETUP_B");
        let cfg = Config::parse(
            r#"
[chat]
chain = ["local", "cloud_a", "cloud_b"]
[transcribe]
chain = []
[providers.local]
kind = "openai"
base_url = "http://localhost:11434/v1"
[providers.cloud_a]
kind = "openai"
base_url = "https://a.example/v1"
key_env = "LEO_TEST_SETUP_A"
[providers.cloud_b]
kind = "openai"
base_url = "https://b.example/v1"
key_env = "LEO_TEST_SETUP_B"
[providers.unused]
kind = "openai"
base_url = "https://c.example/v1"
key_env = "LEO_TEST_SETUP_C"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        store.set("cloud_b", "k").unwrap();
        assert_eq!(
            providers_missing_keys(&cfg, &store),
            vec!["cloud_a".to_string()]
        );
    }
}
