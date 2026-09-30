use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table};

use crate::ai::provider::whisper_cpp::model_file;
use crate::config::edit::{self, Task};
use crate::config::provider::ProviderKind;
use crate::config::secret::SecretStore;
use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub id: &'static str,
    pub price: &'static str,
}

const fn m(id: &'static str, price: &'static str) -> Model {
    Model { id, price }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub provider: &'static str,
    pub name: &'static str,
    pub models: &'static [Model],
}

impl Choice {
    pub fn local(&self) -> bool {
        self.models.is_empty()
    }

    pub fn label(&self) -> String {
        if self.local() {
            format!("{} ({})", this_computer(), self.name)
        } else {
            self.name.to_string()
        }
    }

    pub fn price(&self, model: &str) -> Option<&'static str> {
        if self.local() {
            return Some("free");
        }
        self.models.iter().find(|m| m.id == model).map(|m| m.price)
    }
}

pub fn this_computer() -> &'static str {
    if cfg!(target_os = "macos") {
        "this Mac"
    } else {
        "this computer"
    }
}

pub const WRITING: &[Choice] = &[
    Choice {
        provider: "ollama",
        name: "Ollama",
        models: &[],
    },
    Choice {
        provider: "openai",
        name: "OpenAI",
        models: &[
            m("gpt-5-nano", "$0.05 in, $0.40 out per 1M tokens"),
            m("gpt-6-luna", "$0.10 in, $0.50 out per 1M tokens"),
            m("gpt-5.4-nano", "$0.20 in, $1.25 out per 1M tokens"),
            m("gpt-5-mini", "$0.25 in, $2 out per 1M tokens"),
            m("gpt-5.4-mini", "$0.75 in, $4.50 out per 1M tokens"),
            m("gpt-6.1-sol", "$2 in, $10 out per 1M tokens"),
            m("gpt-5.4", "$2.50 in, $15 out per 1M tokens"),
            m("gpt-5.5", "$5 in, $30 out per 1M tokens"),
            m("gpt-6-astra", "$10 in, $50 out per 1M tokens"),
        ],
    },
    Choice {
        provider: "anthropic",
        name: "Anthropic",
        models: &[
            m("claude-haiku-4-5", "$1 in, $5 out per 1M tokens"),
            m("claude-sonnet-5-5", "$2 in, $10 out per 1M tokens"),
            m("claude-sonnet-5", "$2 in, $10 out per 1M tokens"),
            m("claude-opus-5-5", "$4 in, $20 out per 1M tokens"),
            m("claude-opus-5", "$5 in, $25 out per 1M tokens"),
        ],
    },
    Choice {
        provider: "gemini",
        name: "Gemini",
        models: &[
            m(
                "gemini-3.1-flash-lite",
                "free tier, then $0.25 in, $1.50 out per 1M tokens",
            ),
            m(
                "gemini-3.5-flash-lite",
                "free tier, then $0.30 in, $2.50 out per 1M tokens",
            ),
            m(
                "gemini-3.8-flash",
                "free tier, then $0.75 in, $3.75 out per 1M tokens",
            ),
            m(
                "gemini-3.5-flash",
                "free tier, then $1.50 in, $9 out per 1M tokens",
            ),
            m("gemini-3.1-pro-preview", "$2 in, $12 out per 1M tokens"),
        ],
    },
    Choice {
        provider: "xai",
        name: "xAI",
        models: &[
            m("grok-4.3", "$1.25 in, $2.50 out per 1M tokens"),
            m("grok-4.5", "$2 in, $6 out per 1M tokens"),
            m("grok-4.6", "$2 in, $6 out per 1M tokens"),
            m("grok-4.7", "$2 in, $6 out per 1M tokens"),
        ],
    },
    Choice {
        provider: "openrouter",
        name: "OpenRouter",
        models: &[
            m("openrouter/free", "free, with daily limits"),
            m("qwen/qwen3.8-27b:free", "free, with daily limits"),
            m("google/gemma-4-31b-it:free", "free, with daily limits"),
            m(
                "nvidia/nemotron-3-super-120b-a12b:free",
                "free, with daily limits",
            ),
            m(
                "deepseek/deepseek-v4-flash",
                "$0.08 in, $0.16 out per 1M tokens",
            ),
            m("openai/gpt-6-luna", "$0.10 in, $0.50 out per 1M tokens"),
            m(
                "google/gemini-3.1-flash-lite",
                "$0.25 in, $1.50 out per 1M tokens",
            ),
            m("anthropic/claude-haiku-4.5", "$1 in, $5 out per 1M tokens"),
            m(
                "anthropic/claude-sonnet-5.5",
                "$2 in, $10 out per 1M tokens",
            ),
        ],
    },
];

pub const SPEECH: &[Choice] = &[
    Choice {
        provider: "whisper_cpp",
        name: "whisper.cpp",
        models: &[],
    },
    Choice {
        provider: "openai_whisper",
        name: "OpenAI",
        models: &[
            m("gpt-4o-mini-transcribe", "$0.18 per hour"),
            m("gpt-transcribe", "$0.27 per hour"),
            m("whisper-1", "$0.36 per hour"),
        ],
    },
    Choice {
        provider: "gemini_speech",
        name: "Gemini",
        models: &[
            m("gemini-3.8-flash", "free tier"),
            m(
                "gemini-3.1-flash-lite",
                "free tier, then about $0.06 per hour",
            ),
        ],
    },
    Choice {
        provider: "xai_speech",
        name: "xAI",
        models: &[
            m("grok-voice-transcribe-2.0", "$0.10 per hour"),
            m("grok-voice-transcribe-1.0", "$0.10 per hour"),
        ],
    },
];

pub fn priced(task: Task, provider: &str, model: &str) -> String {
    match find(task, provider).and_then(|c| c.price(model)) {
        Some(price) => format!("{model} ({price})"),
        None => model.to_string(),
    }
}

pub const OLLAMA_STARTER: &str = "qwen3:8b";
pub const WHISPER_STARTER: &str = crate::ai::provider::whisper_cpp::STARTER;
pub const WHISPER_STARTER_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
pub const WHISPER_STARTER_SHA256: &str =
    "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002";

pub fn choices(task: Task) -> &'static [Choice] {
    match task {
        Task::Chat => WRITING,
        Task::Transcribe => SPEECH,
    }
}

pub fn find(task: Task, provider: &str) -> Option<Choice> {
    choices(task)
        .iter()
        .copied()
        .find(|c| c.provider == provider)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Local {
    pub ollama_running: bool,
    pub ollama: Vec<String>,
    pub whisper: Vec<PathBuf>,
}

pub fn local_models(cfg: &Config) -> Local {
    let url = cfg.provider("ollama").and_then(|p| p.base_url.clone());
    let ollama_running = url.as_deref().is_some_and(crate::health::port_open);
    let ollama = match (&url, ollama_running) {
        (Some(url), true) => ollama_models(url),
        _ => Vec::new(),
    };
    Local {
        ollama_running,
        ollama,
        whisper: whisper_models(cfg),
    }
}

pub fn ollama_models(base_url: &str) -> Vec<String> {
    if !crate::health::port_open(base_url) {
        return Vec::new();
    }
    let root = base_url
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .replace("://localhost", "://127.0.0.1");
    let Ok(client) = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(300))
        .timeout(std::time::Duration::from_millis(1500))
        .build()
    else {
        return Vec::new();
    };
    client
        .get(format!("{root}/api/tags"))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .map(|json| ollama_names(&json))
        .unwrap_or_default()
}

pub fn ollama_names(json: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = json["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m["name"].as_str().or(m["model"].as_str()))
                .filter(|n| !n.contains("embed"))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

pub fn models_dir() -> PathBuf {
    crate::ai::provider::whisper_cpp::models_dir()
}

pub fn whisper_models(cfg: &Config) -> Vec<PathBuf> {
    let mut dirs = vec![models_dir()];
    if let Some(parent) = cfg
        .provider("whisper_cpp")
        .map(model_file)
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        if !dirs.contains(&parent) {
            dirs.push(parent);
        }
    }
    let mut found: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|entries| entries.flatten().map(|e| e.path()))
        .filter(|p| is_whisper_model(p))
        .collect();
    found.sort();
    found.dedup();
    found
}

fn is_whisper_model(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("ggml-") && n.ends_with(".bin") && !n.contains("silero"))
}

pub fn whisper_label(path: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    name.trim_start_matches("ggml-")
        .trim_end_matches(".bin")
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub provider: String,
    pub choice: Option<Choice>,
    pub model: Option<String>,
    pub ready: bool,
}

pub fn selection(cfg: &Config, store: &dyn SecretStore, task: Task) -> Option<Selection> {
    let chain = match task {
        Task::Chat => &cfg.chat.chain,
        Task::Transcribe => &cfg.transcribe.chain,
    };
    let usable = chain
        .iter()
        .find(|n| crate::health::provider_usable(cfg, n, store));
    let provider = usable.or(chain.first())?.clone();
    let model = cfg.provider(&provider).and_then(|p| {
        if p.kind == Some(ProviderKind::WhisperCpp) {
            Some(model_file(p).display().to_string())
        } else {
            p.model.clone()
        }
    });
    Some(Selection {
        choice: find(task, &provider),
        ready: usable.is_some(),
        provider,
        model,
    })
}

pub fn model_options(task: Task, provider: &str, local: &Local) -> Vec<String> {
    match (task, provider) {
        (Task::Chat, "ollama") => local.ollama.clone(),
        (Task::Transcribe, "whisper_cpp") => local
            .whisper
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        _ => find(task, provider)
            .map(|c| c.models.iter().map(|m| m.id.to_string()).collect())
            .unwrap_or_default(),
    }
}

pub fn step<'a>(options: &'a [String], current: Option<&str>, delta: isize) -> Option<&'a String> {
    if options.is_empty() {
        return None;
    }
    let len = options.len() as isize;
    let at = current.and_then(|c| options.iter().position(|o| o == c));
    let next = match at {
        Some(i) => (i as isize + delta).rem_euclid(len),
        None if delta < 0 => len - 1,
        None => 0,
    };
    options.get(next as usize)
}

pub fn step_choice(task: Task, current: Option<&str>, delta: isize) -> Choice {
    let all = choices(task);
    let names: Vec<String> = all.iter().map(|c| c.provider.to_string()).collect();
    step(&names, current, delta)
        .and_then(|name| find(task, name))
        .unwrap_or(all[0])
}

pub fn write_choice(doc: &mut DocumentMut, task: Task, provider: &str) {
    edit::write_chain(doc, task, &[provider.to_string()]);
}

pub fn write_model(doc: &mut DocumentMut, provider: &str, model: &str) {
    let field = match Config::built_in_provider(provider) {
        Some(pc) if pc.kind == Some(ProviderKind::WhisperCpp) => "model_path",
        _ => "model",
    };
    let providers = doc
        .entry("providers")
        .or_insert(Item::Table(implicit_table()));
    let Some(providers) = providers.as_table_mut() else {
        return;
    };
    providers.set_implicit(true);
    let block = providers
        .entry(provider)
        .or_insert_with(|| Item::Table(Table::new()));
    if let Some(table) = block.as_table_mut() {
        table.insert(field, toml_edit::value(model));
    }
}

fn implicit_table() -> Table {
    let mut table = Table::new();
    table.set_implicit(true);
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;

    fn load(text: &str) -> Config {
        Config::parse_with_built_ins(text).unwrap()
    }

    #[test]
    fn every_choice_names_a_built_in_provider_that_serves_its_task() {
        use crate::config::provider::ProviderKind;
        for choice in WRITING {
            let pc = Config::built_in_provider(choice.provider).expect(choice.provider);
            assert_eq!(pc.kind, Some(ProviderKind::Openai), "{}", choice.provider);
            if !choice.local() {
                let default = pc.model.clone().unwrap();
                assert!(
                    choice.models.iter().any(|m| m.id == default),
                    "{} defaults to {default}, which is not offered",
                    choice.provider
                );
                assert!(
                    choice
                        .models
                        .windows(2)
                        .all(|w| cost(w[0].price) <= cost(w[1].price)),
                    "{} is not listed cheapest first",
                    choice.provider
                );
            }
        }
        for choice in SPEECH {
            let pc = Config::built_in_provider(choice.provider).expect(choice.provider);
            assert_ne!(pc.kind, Some(ProviderKind::Openai), "{}", choice.provider);
            if !choice.local() {
                let default = pc.model.clone().unwrap();
                assert!(
                    choice.models.iter().any(|m| m.id == default),
                    "{} defaults to {default}, which is not offered",
                    choice.provider
                );
                assert!(
                    choice
                        .models
                        .windows(2)
                        .all(|w| cost(w[0].price) <= cost(w[1].price)),
                    "{} is not listed cheapest first",
                    choice.provider
                );
            }
        }
    }

    fn cost(price: &str) -> f64 {
        price
            .split('$')
            .skip(1)
            .filter_map(|p| p.split(|c: char| !(c.is_ascii_digit() || c == '.')).next())
            .filter_map(|n| n.parse::<f64>().ok())
            .enumerate()
            .map(|(i, n)| if i == 0 { n } else { n * 4.0 })
            .sum()
    }

    #[test]
    fn every_cloud_model_has_a_price_and_local_ones_are_free() {
        for choice in WRITING.iter().chain(SPEECH) {
            for model in choice.models {
                assert!(
                    model.price.contains('$') || model.price.starts_with("free"),
                    "{}: {}",
                    model.id,
                    model.price
                );
            }
        }
        assert_eq!(
            priced(Task::Chat, "anthropic", "claude-opus-5-5"),
            "claude-opus-5-5 ($4 in, $20 out per 1M tokens)"
        );
        assert_eq!(priced(Task::Chat, "ollama", "qwen3:8b"), "qwen3:8b (free)");
        assert_eq!(priced(Task::Chat, "mine", "house"), "house");
        assert_eq!(
            priced(Task::Transcribe, "openai_whisper", "gpt-transcribe"),
            "gpt-transcribe ($0.27 per hour)"
        );
    }

    #[test]
    fn cheap_and_free_cloud_models_are_offered() {
        let ids = |p: &str| -> Vec<&str> {
            find(Task::Chat, p)
                .unwrap()
                .models
                .iter()
                .map(|m| m.id)
                .collect()
        };
        for cheap in ["gpt-5-nano", "gpt-6-luna", "gpt-5-mini", "gpt-5.4-mini"] {
            assert!(ids("openai").contains(&cheap), "{cheap}");
        }
        assert!(ids("gemini").contains(&"gemini-3.1-flash-lite"));
        assert!(find(Task::Chat, "gemini")
            .unwrap()
            .models
            .iter()
            .any(|m| m.price.starts_with("free tier")));
        assert_eq!(
            Config::built_in_provider("openai")
                .unwrap()
                .model
                .as_deref(),
            Some("gpt-6-luna")
        );
    }

    #[test]
    fn one_key_serves_writing_and_speech_for_each_cloud() {
        for (chat, speech) in [
            ("openai", "openai_whisper"),
            ("gemini", "gemini_speech"),
            ("xai", "xai_speech"),
        ] {
            let a = Config::built_in_provider(chat).unwrap();
            let b = Config::built_in_provider(speech).unwrap();
            assert_eq!(a.account(chat), b.account(speech));
            assert_eq!(a.key_env, b.key_env);
        }
    }

    #[test]
    fn the_anthropic_models_the_user_asked_for_are_offered() {
        let anthropic = find(Task::Chat, "anthropic").unwrap();
        for model in [
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-haiku-4-5",
            "claude-sonnet-5",
            "claude-opus-5",
        ] {
            assert!(anthropic.models.iter().any(|m| m.id == model), "{model}");
        }
    }

    #[test]
    fn stepping_wraps_both_ways_and_starts_from_an_unknown_value() {
        let options: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(step(&options, Some("a"), 1).unwrap(), "b");
        assert_eq!(step(&options, Some("c"), 1).unwrap(), "a");
        assert_eq!(step(&options, Some("a"), -1).unwrap(), "c");
        assert_eq!(step(&options, Some("zzz"), 1).unwrap(), "a");
        assert_eq!(step(&options, None, -1).unwrap(), "c");
        assert!(step(&[], Some("a"), 1).is_none());
    }

    #[test]
    fn stepping_a_choice_from_a_custom_chain_lands_on_the_first() {
        assert_eq!(
            step_choice(Task::Chat, Some("openrouter"), 1).provider,
            "ollama"
        );
        assert_eq!(
            step_choice(Task::Chat, Some("ollama"), 1).provider,
            "openai"
        );
        assert_eq!(
            step_choice(Task::Chat, Some("ollama"), -1).provider,
            "openrouter"
        );
        assert_eq!(
            step_choice(Task::Transcribe, Some("xai_speech"), 1).provider,
            "whisper_cpp"
        );
    }

    #[test]
    fn choosing_a_provider_makes_it_the_whole_chain() {
        let mut doc = Config::default_toml().parse::<DocumentMut>().unwrap();
        write_choice(&mut doc, Task::Chat, "anthropic");
        write_choice(&mut doc, Task::Transcribe, "gemini_speech");
        let cfg = load(&doc.to_string());
        assert_eq!(cfg.chat.chain, vec!["anthropic"]);
        assert_eq!(cfg.transcribe.chain, vec!["gemini_speech"]);
        assert!(doc.to_string().contains("Keys do NOT belong"));
    }

    #[test]
    fn a_model_written_for_a_built_in_keeps_everything_else_about_it() {
        let mut doc = Config::default_toml().parse::<DocumentMut>().unwrap();
        write_model(&mut doc, "anthropic", "claude-opus-5-5");
        write_model(&mut doc, "ollama", "llama3.3:70b");
        let text = doc.to_string();
        let cfg = load(&text);
        let anthropic = cfg.provider("anthropic").unwrap();
        assert_eq!(anthropic.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(
            anthropic.base_url.as_deref(),
            Some("https://api.anthropic.com/v1")
        );
        assert_eq!(anthropic.key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert_eq!(anthropic.reasoning, Some(true));
        let ollama = cfg.provider("ollama").unwrap();
        assert_eq!(ollama.model.as_deref(), Some("llama3.3:70b"));
        assert_eq!(
            ollama.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );

        write_model(&mut doc, "anthropic", "claude-haiku-4-5");
        let cfg = load(&doc.to_string());
        assert_eq!(
            cfg.provider("anthropic").unwrap().model.as_deref(),
            Some("claude-haiku-4-5")
        );
        assert_eq!(doc.to_string().matches("[providers.anthropic]").count(), 1);
        assert!(!doc.to_string().contains("[providers]\n"));
    }

    #[test]
    fn a_user_block_keeps_its_own_fields_when_the_model_changes() {
        let mut doc = "[providers.ollama]\nkind = \"openai\"\nbase_url = \"http://localhost:9999/v1\"\nmodel = \"old\"\n"
            .parse::<DocumentMut>()
            .unwrap();
        write_model(&mut doc, "ollama", "new");
        let cfg = load(&doc.to_string());
        let ollama = cfg.provider("ollama").unwrap();
        assert_eq!(ollama.model.as_deref(), Some("new"));
        assert_eq!(ollama.base_url.as_deref(), Some("http://localhost:9999/v1"));
    }

    #[test]
    fn a_whisper_model_is_a_path() {
        let mut doc = String::new().parse::<DocumentMut>().unwrap();
        write_model(&mut doc, "whisper_cpp", "/models/ggml-large-v3-turbo.bin");
        let cfg = load(&doc.to_string());
        let w = cfg.provider("whisper_cpp").unwrap();
        assert_eq!(
            w.model_path.as_deref(),
            Some("/models/ggml-large-v3-turbo.bin")
        );
        assert_eq!(w.bin, None);
        assert_eq!(w.model, None);
        assert_eq!(
            whisper_label("/models/ggml-large-v3-turbo.bin"),
            "large-v3-turbo"
        );
    }

    #[test]
    fn ollama_names_come_from_the_tags_listing_without_embedding_models() {
        let json = serde_json::json!({"models": [
            {"name": "qwen3:8b", "model": "qwen3:8b"},
            {"name": "nomic-embed-text:latest"},
            {"model": "llama3.3:70b"},
            {"name": "gemma3:4b"}
        ]});
        assert_eq!(
            ollama_names(&json),
            vec!["gemma3:4b", "llama3.3:70b", "qwen3:8b"]
        );
        assert!(ollama_names(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn a_closed_port_lists_no_ollama_models_quickly() {
        let started = std::time::Instant::now();
        assert!(ollama_models("http://127.0.0.1:9/v1").is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn whisper_models_are_found_beside_the_configured_one() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "ggml-base.en.bin",
            "ggml-large-v3-turbo.bin",
            "ggml-silero-v5.bin",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let configured = dir.path().join("ggml-base.en.bin");
        let cfg = load(&format!(
            "[providers.whisper_cpp]\nkind = \"whisper_cpp\"\nmodel_path = \"{}\"\n",
            configured.display()
        ));
        let found = whisper_models(&cfg);
        assert!(found.contains(&configured));
        assert!(found.contains(&dir.path().join("ggml-large-v3-turbo.bin")));
        assert!(!found.iter().any(|p| p.ends_with("ggml-silero-v5.bin")));
        assert!(!found.iter().any(|p| p.ends_with("notes.txt")));
    }

    #[test]
    fn model_options_are_local_lists_for_local_choices_and_curated_for_cloud() {
        let local = Local {
            ollama_running: true,
            ollama: vec!["qwen3:8b".to_string()],
            whisper: vec![PathBuf::from("/m/ggml-base.en.bin")],
        };
        assert_eq!(
            model_options(Task::Chat, "ollama", &local),
            vec!["qwen3:8b"]
        );
        assert_eq!(
            model_options(Task::Transcribe, "whisper_cpp", &local),
            vec!["/m/ggml-base.en.bin"]
        );
        assert_eq!(
            model_options(Task::Chat, "openai", &local)
                .first()
                .map(String::as_str),
            Some("gpt-5-nano")
        );
        assert!(model_options(Task::Chat, "mine", &local).is_empty());
        assert_eq!(
            model_options(Task::Chat, "openrouter", &local)
                .first()
                .map(String::as_str),
            Some("openrouter/free")
        );
    }

    #[test]
    fn the_selection_is_the_first_usable_provider_else_the_first_named() {
        let store = MemoryStore::default();
        let cfg = load("[chat]\nchain = [\"anthropic\", \"openai\"]\n[transcribe]\nchain = []\n");
        let sel = selection(&cfg, &store, Task::Chat).unwrap();
        assert_eq!(sel.provider, "anthropic");
        assert!(!sel.ready);
        assert_eq!(sel.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(sel.choice.map(|c| c.name), Some("Anthropic"));

        store.set("openai", "k").unwrap();
        let sel = selection(&cfg, &store, Task::Chat).unwrap();
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            assert_eq!(sel.provider, "openai");
            assert!(sel.ready);
        }
        assert!(selection(&cfg, &store, Task::Transcribe).is_none());
    }

    #[test]
    fn a_stored_openai_key_makes_openai_speech_usable_too() {
        let store = MemoryStore::default();
        store.set("openai", "k").unwrap();
        let cfg = load("[transcribe]\nchain = [\"openai_whisper\"]\n");
        if std::env::var("OPENAI_API_KEY").is_err() {
            let sel = selection(&cfg, &store, Task::Transcribe).unwrap();
            assert!(sel.ready);
        }
        let chain = crate::ai::provider::build_transcribe_chain(&cfg, &store);
        assert!(chain[0].available());
    }
}
