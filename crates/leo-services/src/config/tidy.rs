use toml_edit::{DocumentMut, Item};

use crate::config::edit::{self, Task};
use crate::config::Config;

pub const RETIRED: &[&str] = &[
    "lmstudio",
    "llamacpp",
    "vllm",
    "groq_chat",
    "cerebras",
    "mistral",
    "deepseek",
    "together",
    "groq",
    "hf",
    "local_whisper_server",
];

const KNOWN_KINDS: &[&str] = &[
    "openai",
    "openai_transcribe",
    "groq",
    "whisper_cpp",
    "chat_audio",
    "parakeet",
];

fn stale(provider: &str, field: &str, value: &toml_edit::Value) -> bool {
    match (field, value) {
        ("model", v) => matches!(
            (provider, v.as_str()),
            ("openai", Some("gpt-4o-mini"))
                | ("gemini", Some("gemini-2.5-flash" | "gemini-2.0-flash"))
                | ("xai", Some("grok-3-mini"))
                | ("openai_whisper", Some("whisper-1"))
                | (
                    "anthropic",
                    Some("claude-haiku-4-5" | "claude-haiku-4-5-20251001")
                )
                | ("openrouter", Some("anthropic/claude-haiku-4.5"))
        ),
        ("max_tokens", v) => provider != "ollama" && matches!(v.as_integer(), Some(4096 | 8192)),
        ("kind", v) => v.as_str() == Some("groq"),
        ("bin", v) => v.as_str() == Some("whisper-cli"),
        ("model_path", v) => v.as_str() == Some("~/.leo/models/ggml-base.en.bin"),
        _ => false,
    }
}

fn same(item: &toml_edit::Value, built_in: &toml::Value) -> bool {
    match built_in {
        toml::Value::String(s) => item.as_str() == Some(s.as_str()),
        toml::Value::Integer(i) => item.as_integer() == Some(*i),
        toml::Value::Boolean(b) => item.as_bool() == Some(*b),
        _ => false,
    }
}

pub fn tidy(doc: &mut DocumentMut) -> bool {
    let mut changed = false;

    for (task, default) in [
        (Task::Chat, &crate::config::DEFAULT_CHAT_CHAIN[..]),
        (
            Task::Transcribe,
            &crate::config::DEFAULT_TRANSCRIBE_CHAIN[..],
        ),
    ] {
        let before = edit::read_chain(doc, task);
        if doc.get(task.table()).and_then(|t| t.get("chain")).is_none() {
            continue;
        }
        let mut kept: Vec<String> = before
            .iter()
            .filter(|n| !RETIRED.contains(&n.as_str()))
            .cloned()
            .collect();
        if kept != before {
            if kept.is_empty() {
                kept = default.iter().map(|s| s.to_string()).collect();
            }
            edit::write_chain(doc, task, &kept);
            changed = true;
        }
    }

    let Some(providers) = doc.get_mut("providers").and_then(Item::as_table_mut) else {
        return replace_old_engine(doc) || changed;
    };
    let names: Vec<String> = providers.iter().map(|(k, _)| k.to_string()).collect();
    for name in names {
        let kind = providers
            .get(&name)
            .and_then(|b| b.get("kind"))
            .and_then(|k| k.as_str())
            .map(str::to_string);
        let unusable_kind = kind.as_deref().is_some_and(|k| !KNOWN_KINDS.contains(&k));
        let own_program = providers
            .get(&name)
            .and_then(|b| b.get("bin"))
            .and_then(|b| b.as_str())
            .is_some_and(|b| b != "whisper-cli");
        let old_engine = (kind.as_deref() == Some("whisper_cpp")
            || (kind.is_none() && name == "whisper_cpp"))
            && !own_program;
        if RETIRED.contains(&name.as_str()) || unusable_kind || old_engine {
            providers.remove(&name);
            changed = true;
            continue;
        }
        let Some(built_in) = Config::built_in_provider(&name)
            .and_then(|pc| toml::Value::try_from(pc).ok())
            .and_then(|v| v.as_table().cloned())
        else {
            continue;
        };
        let Some(block) = providers.get_mut(&name).and_then(Item::as_table_like_mut) else {
            continue;
        };
        let fields: Vec<String> = block.iter().map(|(k, _)| k.to_string()).collect();
        for field in fields {
            let redundant = block.get(&field).and_then(Item::as_value).is_some_and(|v| {
                built_in.get(&field).is_some_and(|b| same(v, b)) || stale(&name, &field, v)
            });
            if redundant {
                block.remove(&field);
                changed = true;
            }
        }
        if block.is_empty() {
            providers.remove(&name);
            changed = true;
        }
    }
    if providers.is_empty() {
        doc.remove("providers");
    }
    changed |= replace_old_engine(doc);
    changed
}

fn replace_old_engine(doc: &mut DocumentMut) -> bool {
    let defined = doc
        .get("providers")
        .and_then(|p| p.get("whisper_cpp"))
        .is_some();
    if defined {
        return false;
    }
    let mut changed = false;
    for task in [Task::Chat, Task::Transcribe] {
        let before = edit::read_chain(doc, task);
        if !before.iter().any(|n| n == "whisper_cpp") {
            continue;
        }
        let mut after: Vec<String> = Vec::new();
        for name in before {
            let name = if name == "whisper_cpp" {
                "parakeet".to_string()
            } else {
                name
            };
            if !after.contains(&name) {
                after.push(name);
            }
        }
        edit::write_chain(doc, task, &after);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r#"# leo configuration.

[chat]
chain = ["openrouter", "mistral"]

[transcribe]
chain = ["whisper_cpp", "groq", "hf"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
max_tokens = 4096

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "OPENROUTER_API_KEY"
max_tokens = 8192

[providers.gemini]
kind = "openai"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
model = "gemini-2.5-flash"
key_env = "GEMINI_API_KEY"
max_tokens = 4096

[providers.openai]
kind = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
key_env = "OPENAI_API_KEY"
max_tokens = 4096

[providers.xai]
kind = "openai"
base_url = "https://api.x.ai/v1"
model = "grok-3-mini"
key_env = "XAI_API_KEY"
max_tokens = 4096

[providers.whisper_cpp]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "~/.leo/models/ggml-base.en.bin"

[providers.hf]
kind = "hf"
model = "openai/whisper-large-v3-turbo"
key_env = "HF_API_KEY"

[providers.openai_whisper]
kind = "groq"
base_url = "https://api.openai.com/v1"
model = "whisper-1"
key_env = "OPENAI_API_KEY"

[theme]
preset = "mono"
"#;

    #[test]
    fn an_old_inventory_becomes_the_two_chains_and_the_theme() {
        let mut doc = OLD.parse::<DocumentMut>().unwrap();
        assert!(tidy(&mut doc));
        let text = doc.to_string();
        assert!(!text.contains("[providers"), "{text}");
        assert!(text.contains("chain = [\"openrouter\"]"), "{text}");
        assert!(text.contains("chain = [\"parakeet\"]"), "{text}");
        assert!(text.contains("preset = \"mono\""));
        assert!(text.starts_with("# leo configuration."));
        assert!(!tidy(&mut doc), "a tidy file must stay as it is");

        let cfg = Config::parse_with_built_ins(&text).unwrap();
        assert_eq!(
            cfg.provider("openai").unwrap().model.as_deref(),
            Some("gpt-6-luna")
        );
        assert_eq!(
            cfg.provider("xai").unwrap().model.as_deref(),
            Some("grok-4.7")
        );
        assert!(cfg.provider("whisper_cpp").is_none());
        assert_eq!(
            cfg.provider("openrouter").unwrap().model.as_deref(),
            Some("openrouter/free")
        );
        assert!(cfg.provider("mistral").is_none());
    }

    #[test]
    fn a_task_whose_only_provider_was_retired_goes_back_to_this_computer() {
        let mut doc = "[chat]\nchain = [\"mistral\"]\n"
            .parse::<DocumentMut>()
            .unwrap();
        assert!(tidy(&mut doc));
        assert_eq!(edit::read_chain(&doc, Task::Chat), vec!["ollama"]);
    }

    #[test]
    fn choices_the_user_made_survive() {
        let mut doc = r#"[chat]
chain = ["anthropic", "mistral"]

[transcribe]
chain = ["gemini_speech"]

[providers.anthropic]
model = "claude-opus-5-5"

[providers.ollama]
base_url = "http://10.0.0.5:11434/v1"
max_tokens = 16000

[providers.whisper_cpp]
kind = "whisper_cpp"
bin = "/opt/whisper/main"
model_path = "/models/ggml-large-v3-turbo.bin"

[providers.mine]
kind = "openai"
base_url = "https://example.com/v1"
model = "house-model"
"#
        .parse::<DocumentMut>()
        .unwrap();
        assert!(tidy(&mut doc));
        let cfg = Config::parse_with_built_ins(&doc.to_string()).unwrap();
        assert_eq!(cfg.chat.chain, vec!["anthropic"]);
        assert_eq!(cfg.transcribe.chain, vec!["gemini_speech"]);
        assert_eq!(
            cfg.provider("anthropic").unwrap().model.as_deref(),
            Some("claude-opus-5-5")
        );
        let ollama = cfg.provider("ollama").unwrap();
        assert_eq!(ollama.base_url.as_deref(), Some("http://10.0.0.5:11434/v1"));
        assert_eq!(ollama.max_tokens, Some(16000));
        let whisper = cfg.provider("whisper_cpp").unwrap();
        assert_eq!(whisper.bin.as_deref(), Some("/opt/whisper/main"));
        assert_eq!(
            whisper.model_path.as_deref(),
            Some("/models/ggml-large-v3-turbo.bin")
        );
        assert_eq!(
            cfg.provider("mine").unwrap().model.as_deref(),
            Some("house-model")
        );
    }

    #[test]
    fn a_retired_claude_model_goes_back_to_the_default() {
        let mut doc = "[providers.anthropic]\nmodel = \"claude-haiku-4-5\"\n[providers.openrouter]\nmodel = \"anthropic/claude-haiku-4.5\"\n"
            .parse::<DocumentMut>()
            .unwrap();
        assert!(tidy(&mut doc));
        let cfg = Config::parse_with_built_ins(&doc.to_string()).unwrap();
        assert_eq!(
            cfg.provider("anthropic").unwrap().model.as_deref(),
            Some("claude-sonnet-5-5")
        );
        assert_eq!(
            cfg.provider("openrouter").unwrap().model.as_deref(),
            Some("openrouter/free")
        );
    }

    #[test]
    fn a_file_with_nothing_retired_is_left_byte_for_byte() {
        let text = Config::default_toml();
        let mut doc = text.parse::<DocumentMut>().unwrap();
        assert!(!tidy(&mut doc));
        assert_eq!(doc.to_string(), text);
    }

    #[test]
    fn an_empty_or_missing_chain_is_not_invented() {
        let mut doc = "[chat]\nchain = []\n".parse::<DocumentMut>().unwrap();
        assert!(!tidy(&mut doc));
        assert_eq!(doc.to_string(), "[chat]\nchain = []\n");
    }

    #[test]
    fn loading_an_old_file_rewrites_it_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, OLD).unwrap();
        let cfg = Config::load_from(&path);
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(!on_disk.contains("[providers"), "{on_disk}");
        assert!(!on_disk.contains("mistral"), "{on_disk}");
        assert!(cfg.provider("hf").is_none());
        assert!(cfg.provider("whisper_cpp").is_none());
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        Config::load_from(&path);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
    }
}
