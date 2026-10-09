use std::path::PathBuf;

use toml_edit::{DocumentMut, Item, Table};

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
        provider: "codex",
        name: "Codex",
        models: &[
            m("gpt-6-luna", "included in your ChatGPT plan"),
            m("gpt-6-sol", "included in your ChatGPT plan"),
            m("gpt-6.1-sol", "included in your ChatGPT plan"),
            m("gpt-6-astra", "included in your ChatGPT plan"),
        ],
    },
    Choice {
        provider: "anthropic",
        name: "Anthropic",
        models: &[
            m(
                "claude-haiku-5-5",
                "$0.10 in, $0.50 out per 1M tokens (more above 100K-token prompts)",
            ),
            m("claude-sonnet-5-5", "$2 in, $10 out per 1M tokens"),
            m("claude-sonnet-5", "$2 in, $10 out per 1M tokens"),
            m("claude-opus-5-5", "$4 in, $20 out per 1M tokens"),
            m("claude-opus-5", "$5 in, $25 out per 1M tokens"),
        ],
    },
    Choice {
        provider: "claude_code",
        name: "Claude Code",
        models: &[
            m("claude-haiku-5-5", "included in your Claude plan"),
            m("claude-sonnet-5-5", "included in your Claude plan"),
            m("claude-opus-5-5", "included in your Claude plan"),
            m(
                "claude-fable-5-1",
                "needs usage credits on your Claude plan",
            ),
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
            m(
                "anthropic/claude-sonnet-5.5",
                "$2 in, $10 out per 1M tokens",
            ),
        ],
    },
];

pub const SPEECH: &[Choice] = &[
    Choice {
        provider: "parakeet",
        name: "Parakeet",
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
    pub speech_ready: bool,
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
        speech_ready: cfg
            .provider("parakeet")
            .map(crate::ai::provider::parakeet::model_dir)
            .is_some_and(|dir| crate::ai::provider::parakeet::present(&dir)),
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
    crate::ai::provider::audio::models_dir()
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
        if p.kind == Some(ProviderKind::Parakeet) {
            Some(crate::ai::provider::parakeet::MODEL_NAME.to_string())
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

pub fn efforts(provider: &str, model: &str) -> &'static [&'static str] {
    let model = model.to_ascii_lowercase();
    match provider {
        "claude_code" => &["low", "medium", "high", "xhigh", "max"],
        "codex" => &["low", "medium", "high", "xhigh"],
        "openai"
            if model.starts_with("gpt-5")
                || model.starts_with("gpt-6")
                || model.starts_with('o') =>
        {
            &["minimal", "low", "medium", "high"]
        }
        "gemini" => &["low", "medium", "high"],
        _ => &[],
    }
}

pub fn step_effort(
    provider: &str,
    model: &str,
    current: Option<&str>,
    delta: isize,
) -> Option<Option<&'static str>> {
    let levels = efforts(provider, model);
    if levels.is_empty() {
        return None;
    }
    let ring: Vec<Option<&'static str>> = std::iter::once(None)
        .chain(levels.iter().map(|l| Some(*l)))
        .collect();
    let at = ring.iter().position(|l| *l == current).unwrap_or(0) as isize;
    let next = (at + delta).rem_euclid(ring.len() as isize) as usize;
    Some(ring[next])
}

pub fn write_effort(doc: &mut DocumentMut, provider: &str, effort: Option<&str>) {
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
        match effort {
            Some(effort) => {
                table.insert("effort", toml_edit::value(effort));
            }
            None => {
                table.remove("effort");
            }
        }
    }
}

pub fn write_model(doc: &mut DocumentMut, provider: &str, model: &str) {
    let field = match Config::built_in_provider(provider) {
        Some(pc) if pc.kind == Some(ProviderKind::Parakeet) => "model_path",
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

    #[test]
    fn effort_levels_follow_the_program_and_model_and_default_is_one_of_them() {
        assert_eq!(
            efforts("claude_code", "claude-sonnet-5-5"),
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            efforts("openai", "gpt-6-luna"),
            ["minimal", "low", "medium", "high"]
        );
        assert!(efforts("openai", "gpt-4.1").is_empty());
        assert!(efforts("ollama", "qwen3:8b").is_empty());
        assert_eq!(step_effort("codex", "", None, 1), Some(Some("low")));
        assert_eq!(
            step_effort("codex", "", Some("xhigh"), 1),
            Some(None),
            "past the last comes back to default"
        );
        assert_eq!(step_effort("codex", "", None, -1), Some(Some("xhigh")));
        assert_eq!(step_effort("ollama", "", None, 1), None);
        let mut doc: DocumentMut = "".parse().unwrap();
        write_effort(&mut doc, "codex", Some("high"));
        assert!(
            doc.to_string()
                .contains("[providers.codex]\neffort = \"high\""),
            "{doc}"
        );
        write_effort(&mut doc, "codex", None);
        assert!(!doc.to_string().contains("effort"), "{doc}");
    }

    fn load(text: &str) -> Config {
        Config::parse_with_built_ins(text).unwrap()
    }

    #[test]
    fn every_choice_names_a_built_in_provider_that_serves_its_task() {
        use crate::config::provider::ProviderKind;
        for choice in WRITING {
            let pc = Config::built_in_provider(choice.provider).expect(choice.provider);
            assert!(
                matches!(
                    pc.kind,
                    Some(ProviderKind::Openai | ProviderKind::ClaudeCode | ProviderKind::Codex)
                ),
                "{}",
                choice.provider
            );
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
                    model.price.contains('$')
                        || model.price.starts_with("free")
                        || model.price.starts_with("included in your")
                        || model.price.contains("usage credits"),
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
    fn signed_in_agents_offer_the_newest_models_and_keep_the_older_ones() {
        let has = |provider: &str, model: &str| {
            find(Task::Chat, provider)
                .unwrap()
                .models
                .iter()
                .any(|m| m.id == model)
        };
        for model in [
            "claude-haiku-5-5",
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-fable-5-1",
        ] {
            assert!(has("claude_code", model), "{model}");
        }
        for model in ["gpt-6-luna", "gpt-6-sol", "gpt-6.1-sol", "gpt-6-astra"] {
            assert!(has("codex", model), "{model}");
        }
    }

    #[test]
    fn the_anthropic_models_the_user_asked_for_are_offered() {
        let anthropic = find(Task::Chat, "anthropic").unwrap();
        for model in [
            "claude-haiku-5-5",
            "claude-sonnet-5-5",
            "claude-opus-5-5",
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
            "parakeet"
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

        write_model(&mut doc, "anthropic", "claude-sonnet-5");
        let cfg = load(&doc.to_string());
        assert_eq!(
            cfg.provider("anthropic").unwrap().model.as_deref(),
            Some("claude-sonnet-5")
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
    fn model_options_are_local_lists_for_local_choices_and_curated_for_cloud() {
        let local = Local {
            ollama_running: true,
            ollama: vec!["qwen3:8b".to_string()],
            speech_ready: true,
        };
        assert_eq!(
            model_options(Task::Chat, "ollama", &local),
            vec!["qwen3:8b"]
        );
        assert!(model_options(Task::Transcribe, "parakeet", &local).is_empty());
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
