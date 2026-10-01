pub mod choice;
pub mod edit;
pub mod file_store;
pub mod provider;
pub mod secret;
pub mod sync;
pub mod theme;
pub mod tidy;

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use provider::{ProviderConfig, TaskChain};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub chat: TaskChain,
    #[serde(default)]
    pub transcribe: TaskChain,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub theme: theme::ThemeConfig,
    #[serde(default)]
    pub sync: sync::SyncConfig,
}

pub const DEFAULT_CHAT_CHAIN: [&str; 1] = ["ollama"];
pub const DEFAULT_TRANSCRIBE_CHAIN: [&str; 1] = ["parakeet"];

impl Default for Config {
    fn default() -> Self {
        Config::parse_with_built_ins(&Config::default_toml())
            .expect("built-in default config must parse")
    }
}

impl Config {
    /// Parse exactly what the text says, with no built-ins added. `load_from`
    /// is what a user's config goes through; this stays literal so a caller can
    /// reason about one file in isolation.
    pub fn parse(text: &str) -> Result<Config> {
        toml::from_str(text).context("failed to parse leo config")
    }

    /// Parse a user's config and add the providers leo ships with.
    pub(crate) fn parse_with_built_ins(text: &str) -> Result<Config> {
        let mut config = Config::parse(text)?;
        config.merge_built_in_providers();
        Ok(config)
    }

    /// Add every provider leo ships with that the file does not already define.
    ///
    /// The file wins on a name collision, so overriding a built-in is a matter
    /// of writing a block with the same name — no need to copy the other
    /// seventeen to keep them.
    fn merge_built_in_providers(&mut self) {
        let built_in: Config = match toml::from_str(&Config::built_in_toml()) {
            Ok(config) => config,
            // Unreachable in a shipped binary: a test asserts it parses.
            Err(e) => {
                leo_core::diag::warn(format!("built-in providers are unparsable: {e}"));
                return;
            }
        };
        for (name, provider) in built_in.providers {
            match self.providers.get_mut(&name) {
                Some(own) => own.fill_from(provider),
                None => {
                    self.providers.insert(name, provider);
                }
            }
        }
    }

    /// The file written on first run: the two lines a user actually tunes, and
    /// a pointer to where everything else lives.
    ///
    /// Short on purpose. Every provider leo supports is available whether or not
    /// it appears here, so the file holds decisions rather than an inventory.
    pub fn default_toml() -> String {
        format!(
            r#"# leo configuration.
#
# Keys do NOT belong in this file. Type /settings inside leo and press Enter on
# the key row, or run `leo doctor`.
#
# Type /settings to choose what writes and what listens: this computer (free and
# private) or OpenAI, Anthropic, Gemini, xAI or OpenRouter with one key, or Claude Code
# or Codex with the plan you already pay for, and which model each uses. Settings
# writes these two lines for you.

[chat]
chain = [{chat}]

[transcribe]
chain = [{transcribe}]

#
# Backing up to git happens on every save once `leo backup` has set it up. Pushing is
# separate, because it needs the network:
#
#   [sync]
#   auto_push = "on_quit"    # off, on_quit, when_idle
#   idle_secs = 45           # for when_idle: how long the notes must be quiet
#
# To add your own provider, or to override one of the above, name it here:
#
#   [providers.my-provider]
#   kind = "openai"                        # or openai_transcribe
#   base_url = "https://api.example.com/v1"
#   model = "some-model-id"
#   key_env = "EXAMPLE_API_KEY"            # omit entirely for a local server
"#,
            chat = quoted_list(&DEFAULT_CHAT_CHAIN),
            transcribe = quoted_list(&DEFAULT_TRANSCRIBE_CHAIN),
        )
    }

    fn built_in_toml() -> String {
        r#"
[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.openai]
kind = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-6-luna"
key_env = "OPENAI_API_KEY"
max_tokens = 32000
reasoning = true

[providers.anthropic]
kind = "openai"
base_url = "https://api.anthropic.com/v1"
model = "claude-sonnet-5-5"
key_env = "ANTHROPIC_API_KEY"
max_tokens = 32000
reasoning = true

[providers.gemini]
kind = "openai"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
model = "gemini-3.8-flash"
key_env = "GEMINI_API_KEY"
max_tokens = 32000

[providers.xai]
kind = "openai"
base_url = "https://api.x.ai/v1"
model = "grok-4.7"
key_env = "XAI_API_KEY"
max_tokens = 32000
reasoning = true

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "OPENROUTER_API_KEY"
max_tokens = 8192

[providers.claude_code]
kind = "claude_code"
model = "claude-sonnet-5-5"

[providers.codex]
kind = "codex"
model = "gpt-6-luna"

[providers.parakeet]
kind = "parakeet"

[providers.openai_whisper]
kind = "openai_transcribe"
base_url = "https://api.openai.com/v1"
model = "gpt-transcribe"
key_env = "OPENAI_API_KEY"
key_from = "openai"

[providers.gemini_speech]
kind = "chat_audio"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
model = "gemini-3.8-flash"
key_env = "GEMINI_API_KEY"
key_from = "gemini"

[providers.xai_speech]
kind = "openai_transcribe"
base_url = "https://api.x.ai/v1"
path = "stt"
model = "grok-voice-transcribe-2.0"
key_env = "XAI_API_KEY"
key_from = "xai"
"#
        .to_string()
    }

    /// `~/.config/leo/config.toml`. Deliberately not beside the notes
    /// directory: notes are git-synced by sync.rs, and this file holds
    /// machine-local values (ports, model paths).
    pub fn config_path() -> Result<std::path::PathBuf> {
        Ok(leo_core::paths::config_dir()?.join("config.toml"))
    }

    /// Load from the standard path. Never fails: a missing or malformed file
    /// falls back to built-in defaults so the app always starts.
    pub fn load() -> Config {
        match Config::config_path() {
            Ok(path) => Config::load_from(&path),
            Err(e) => {
                leo_core::diag::warn(format!("config: {e}; using defaults"));
                let mut cfg = Config::default();
                cfg.apply_env_overrides();
                cfg
            }
        }
    }

    pub fn load_from(path: &std::path::Path) -> Config {
        let mut cfg = match std::fs::read_to_string(path).map(|text| tidied(path, text)) {
            Ok(text) => match Config::parse_with_built_ins(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    leo_core::diag::warn(format!(
                        "config: {} is invalid ({e}); using defaults",
                        path.display()
                    ));
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        };
        cfg.apply_env_overrides();
        cfg
    }

    /// Write the commented default config if none exists yet. Returns the path
    /// and whether it was newly created.
    pub fn ensure_exists() -> Result<(std::path::PathBuf, bool)> {
        let path = Config::config_path()?;
        if path.exists() {
            return Ok((path, false));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        std::fs::write(&path, Config::default_toml())
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok((path, true))
    }

    /// Env vars win over the file, so a one-off override needs no file edit.
    ///
    /// An override that names a provider with no `[providers]` block is left
    /// unapplied (with a warning) rather than fabricating a bare, kind-less
    /// entry: such an entry is unconstructable and gets silently skipped
    /// downstream, turning a typo into a silent no-op instead of a visible one.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(name) = std::env::var("LEO_CHAT_PROVIDER") {
            let name = name.trim();
            if !name.is_empty() {
                if self.providers.contains_key(name) {
                    self.chat.chain = vec![name.to_string()];
                } else {
                    leo_core::diag::warn(format!(
                        "config: LEO_CHAT_PROVIDER names unknown provider \"{name}\"; ignoring"
                    ));
                }
            }
        }
        if let Ok(name) = std::env::var("LEO_TRANSCRIBE_PROVIDER") {
            let name = name.trim();
            if !name.is_empty() {
                if self.providers.contains_key(name) {
                    self.transcribe.chain = vec![name.to_string()];
                } else {
                    leo_core::diag::warn(format!(
                        "config: LEO_TRANSCRIBE_PROVIDER names unknown provider \"{name}\"; ignoring"
                    ));
                }
            }
        }
        if let Ok(model) = std::env::var("LEO_CHAT_MODEL") {
            if let Some(first) = self.chat.chain.first().cloned() {
                if let Some(provider) = self.providers.get_mut(&first) {
                    provider.model = Some(model);
                } else {
                    leo_core::diag::warn(format!(
                        "config: LEO_CHAT_MODEL set but chat provider \"{first}\" has no [providers] block; ignoring"
                    ));
                }
            }
        }
    }

    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    pub fn built_in_provider(name: &str) -> Option<ProviderConfig> {
        let built_in: Config = toml::from_str(&Config::built_in_toml()).ok()?;
        built_in.providers.get(name).cloned()
    }
}

fn tidied(path: &std::path::Path, text: String) -> String {
    let Ok(mut doc) = text.parse::<toml_edit::DocumentMut>() else {
        return text;
    };
    if !tidy::tidy(&mut doc) {
        return text;
    }
    let _ = edit::save_document(path, &doc);
    doc.to_string()
}

fn quoted_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::ProviderKind;

    #[test]
    fn parses_a_full_config() {
        let toml = r#"
[chat]
chain = ["ollama", "anthropic"]

[transcribe]
chain = ["whisper_cpp", "openai_whisper"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.whisper_cpp]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "~/.leo/models/ggml-base.en.bin"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.chat.chain, vec!["ollama", "anthropic"]);
        assert_eq!(cfg.transcribe.chain, vec!["whisper_cpp", "openai_whisper"]);
        assert_eq!(cfg.providers["ollama"].kind, Some(ProviderKind::Openai));
        assert_eq!(
            cfg.providers["ollama"].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(
            cfg.providers["whisper_cpp"].kind,
            Some(ProviderKind::WhisperCpp)
        );
    }

    /// An empty file means "no opinion about chains", not "no providers": every
    /// provider leo ships with is still known, which is what lets the shipped
    /// file stay short.
    #[test]
    fn an_empty_config_has_no_chains_but_still_knows_every_provider() {
        // `load_from` applies env overrides, and other tests set them; without
        // this the chain can arrive non-empty depending on interleaving.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.chat.chain.is_empty());
        assert_eq!(cfg.providers.len(), 12, "{:?}", cfg.providers.keys());
        assert!(cfg.providers.contains_key("ollama"));
        assert!(cfg.providers.contains_key("parakeet"));
    }

    /// The file the user gets must be short enough to read. The inventory of
    /// providers lives in code precisely so this stays small.
    #[test]
    fn the_shipped_config_is_short() {
        let text = Config::default_toml();
        let lines = text.lines().count();
        assert!(lines < 40, "the shipped config grew to {lines} lines");
        // And it must not have turned back into an inventory. The commented
        // example of how to add one is fine; a real block is not.
        let real_blocks = text
            .lines()
            .filter(|l| l.trim_start().starts_with("[providers."))
            .count();
        assert_eq!(
            real_blocks, 0,
            "provider blocks are back in the shipped file"
        );
        // It still has to be a working config.
        let cfg = Config::parse(&text).unwrap();
        assert_eq!(cfg.chat.chain, DEFAULT_CHAT_CHAIN);
        assert_eq!(cfg.transcribe.chain, DEFAULT_TRANSCRIBE_CHAIN);
    }

    /// A user's own block must win over the built-in of the same name, so
    /// overriding one provider does not mean copying the other seventeen.
    #[test]
    fn a_user_block_overrides_the_built_in_of_the_same_name() {
        let cfg = Config::parse_with_built_ins(
            r#"
[chat]
chain = ["ollama"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:9999/v1"
model = "my-own-model"
"#,
        )
        .unwrap();
        let ollama = cfg.provider("ollama").expect("ollama");
        assert_eq!(ollama.model.as_deref(), Some("my-own-model"));
        assert_eq!(
            ollama.base_url.as_deref(),
            Some("http://localhost:9999/v1"),
            "the built-in overwrote the user's block"
        );
        assert!(cfg.providers.contains_key("anthropic"));
    }

    #[test]
    fn a_block_naming_only_a_model_keeps_the_rest_of_the_built_in() {
        let cfg =
            Config::parse_with_built_ins("[providers.openai]\nmodel = \"gpt-6-luna\"\n").unwrap();
        let openai = cfg.provider("openai").unwrap();
        assert_eq!(openai.model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(openai.kind, Some(ProviderKind::Openai));
        assert_eq!(
            openai.base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(openai.key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert_eq!(openai.reasoning, Some(true));
    }

    #[test]
    fn a_block_of_another_kind_under_a_built_in_name_takes_nothing_from_it() {
        let cfg = Config::parse_with_built_ins(
            "[providers.gemini]\nkind = \"openai_transcribe\"\nbase_url = \"http://localhost:8000/v1\"\n",
        )
        .unwrap();
        let gemini = cfg.provider("gemini").unwrap();
        assert_eq!(gemini.kind, Some(ProviderKind::Transcriptions));
        assert_eq!(gemini.key_env, None);
        assert_eq!(gemini.model, None);
    }

    /// The built-in table has to parse, since every load path merges it.
    #[test]
    fn the_built_in_provider_table_parses() {
        let built_in: Config = toml::from_str(&Config::built_in_toml()).unwrap();
        let mut names: Vec<&String> = built_in.providers.keys().collect();
        names.sort();
        assert_eq!(
            names,
            [
                "anthropic",
                "claude_code",
                "codex",
                "gemini",
                "gemini_speech",
                "ollama",
                "openai",
                "openai_whisper",
                "openrouter",
                "parakeet",
                "xai",
                "xai_speech"
            ]
        );
    }

    #[test]
    fn malformed_toml_is_an_error_naming_the_problem() {
        let err = Config::parse("[chat\nchain = ]").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("config"), "unhelpful error: {msg}");
    }

    #[test]
    fn unknown_provider_kind_is_an_error() {
        let toml = r#"
[providers.weird]
kind = "telepathy"
"#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn defaults_run_on_this_computer() {
        let cfg = Config::default();
        assert_eq!(cfg.chat.chain, vec!["ollama"]);
        assert_eq!(cfg.transcribe.chain, vec!["parakeet"]);
        assert_eq!(cfg.providers["parakeet"].bin, None);
    }

    #[test]
    fn the_old_transcription_kind_name_still_parses() {
        let cfg = Config::parse("[providers.mine]\nkind = \"groq\"\n").unwrap();
        assert_eq!(
            cfg.providers["mine"].kind,
            Some(ProviderKind::Transcriptions)
        );
    }

    /// The shipped config is the primary UI for the model layer, so every
    /// provider in it must be usable as written, not just parseable.
    #[test]
    fn every_shipped_provider_is_complete_enough_to_build() {
        let cfg = Config::default();
        for (name, p) in &cfg.providers {
            let kind = p.kind.unwrap_or_else(|| panic!("{name} has no kind"));
            match kind {
                ProviderKind::Openai => {
                    assert!(p.base_url.is_some(), "{name} has no base_url");
                    assert!(p.model.is_some(), "{name} has no model");
                    // A remote endpoint without a key_env could never
                    // authenticate; a local one must not demand a key.
                    let local = p
                        .base_url
                        .as_deref()
                        .map(|u| u.contains("localhost") || u.contains("127.0.0.1"))
                        .unwrap_or(false);
                    assert_eq!(
                        p.key_env.is_none(),
                        local,
                        "{name}: key_env presence should match whether it is local"
                    );
                }
                ProviderKind::Transcriptions | ProviderKind::ChatAudio => {
                    assert!(p.model.is_some(), "{name} has no model");
                    assert!(p.key_from.is_some(), "{name} should share its chat key");
                }
                ProviderKind::WhisperCpp => panic!("{name}: no built-in runs an external program"),
                ProviderKind::ClaudeCode | ProviderKind::Codex => {
                    assert!(
                        p.key_env.is_none(),
                        "{name} signs in through its own program"
                    );
                    assert!(p.base_url.is_none(), "{name} is a program, not a server");
                }
                ProviderKind::Parakeet => {
                    assert!(
                        p.model_path.is_none(),
                        "{name} should use the models folder"
                    );
                    assert!(p.key_env.is_none(), "{name} runs on this computer");
                }
            }
        }
    }

    /// Every provider named in a chain must exist, or the chain silently
    /// shortens and the user gets a mysterious "no provider available".
    #[test]
    fn every_chain_entry_names_a_defined_provider() {
        let cfg = Config::default();
        for name in cfg.chat.chain.iter().chain(cfg.transcribe.chain.iter()) {
            assert!(
                cfg.providers.contains_key(name),
                "chain names unknown {name}"
            );
        }
    }

    /// The defaults must stay free. A paid provider is offered in the file but
    /// never wired into a chain without the user asking.
    #[test]
    fn no_paid_provider_is_enabled_by_default() {
        let cfg = Config::default();
        for paid in [
            "openai",
            "anthropic",
            "gemini",
            "xai",
            "openai_whisper",
            "gemini_speech",
            "xai_speech",
        ] {
            assert!(
                cfg.providers.contains_key(paid),
                "{paid} should be offered in the file"
            );
            assert!(
                !cfg.chat.chain.contains(&paid.to_string())
                    && !cfg.transcribe.chain.contains(&paid.to_string()),
                "{paid} bills, so it must not be in a default chain"
            );
        }
    }

    /// Two providers sharing one key_env is fine and intentional (groq chat and
    /// groq transcription), but a typo'd variable name is not detectable later,
    /// so pin the spellings.
    #[test]
    fn key_env_names_follow_the_provider_convention() {
        let cfg = Config::default();
        for (name, p) in &cfg.providers {
            if let Some(var) = &p.key_env {
                assert!(
                    var.chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                    "{name}: {var} is not a conventional env var name"
                );
                assert!(
                    var.ends_with("_API_KEY"),
                    "{name}: {var} should end in _API_KEY"
                );
            }
        }
    }

    /// An hour of lecture makes a long note; cloud models can write one, and
    /// a 4096-token cap cut it off mid-sentence. Local servers keep the lower
    /// cap, since small local models have small contexts.
    #[test]
    fn cloud_chat_providers_allow_long_notes_and_local_ones_stay_modest() {
        let cfg = Config::default();
        for thinking in ["openai", "anthropic", "gemini", "xai"] {
            assert_eq!(
                cfg.provider(thinking).and_then(|p| p.max_tokens),
                Some(32000),
                "{thinking}"
            );
        }
        assert_eq!(
            cfg.provider("ollama").and_then(|p| p.max_tokens),
            Some(4096)
        );
    }

    #[test]
    fn the_shipped_file_explains_how_to_add_a_provider() {
        let text = Config::default_toml();
        // The file is the documentation, so these have to be present.
        assert!(text.contains("leo doctor"));
        assert!(text.contains("kind = \"openai\""));
        assert!(text.contains("[chat]"));
        assert!(text.contains("[transcribe]"));
        // And it must warn that keys do not belong in it.
        assert!(text.to_lowercase().contains("keys do not belong"));
    }

    #[test]
    fn default_config_round_trips_through_toml() {
        let text = Config::default_toml();
        let parsed = Config::parse_with_built_ins(&text).unwrap();
        assert_eq!(parsed.chat.chain, Config::default().chat.chain);
        assert_eq!(
            parsed.providers["anthropic"].model,
            Config::default().providers["anthropic"].model
        );
    }

    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn load_from_missing_path_returns_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml"));
        assert_eq!(cfg.chat.chain, Config::default().chat.chain);
    }

    #[test]
    fn load_from_malformed_file_falls_back_to_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[chat\nbroken").unwrap();
        // Must not panic and must not refuse to start.
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, Config::default().chat.chain);
    }

    #[test]
    fn file_values_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[chat]\nchain = [\"only_this\"]\n").unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, vec!["only_this"]);
    }

    #[test]
    fn leo_chat_model_overrides_first_chain_provider() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_CHAT_MODEL", "some/other-model");
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_MODEL");

        let first = &cfg.chat.chain[0];
        assert_eq!(
            cfg.providers[first].model.as_deref(),
            Some("some/other-model")
        );
    }

    #[test]
    fn leo_chat_provider_replaces_the_chain() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_CHAT_PROVIDER", "anthropic");
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_PROVIDER");

        assert_eq!(cfg.chat.chain, vec!["anthropic"]);
    }

    #[test]
    fn leo_chat_provider_naming_unknown_provider_is_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_CHAT_PROVIDER", "typo_provider");
        let mut cfg = Config::default();
        let original_chain = cfg.chat.chain.clone();
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_PROVIDER");

        assert_eq!(cfg.chat.chain, original_chain);
        assert!(!cfg.providers.contains_key("typo_provider"));
    }

    #[test]
    fn leo_chat_model_for_provider_without_block_is_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut cfg = Config::default();
        cfg.chat.chain = vec!["no_such_provider".to_string()];
        std::env::set_var("LEO_CHAT_MODEL", "some/other-model");
        cfg.apply_env_overrides();
        std::env::remove_var("LEO_CHAT_MODEL");

        assert!(!cfg.providers.contains_key("no_such_provider"));
    }
}
