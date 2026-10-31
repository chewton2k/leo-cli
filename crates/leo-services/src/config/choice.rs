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
];

pub const SPEECH: &[Choice] = &[Choice {
    provider: "parakeet",
    name: "Local speech",
    models: &[],
}];

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
        speech_ready: crate::speech::library::library().is_ok_and(|l| l.selected().is_ok()),
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
            crate::speech::library::library()
                .ok()
                .and_then(|l| l.selection().ok())
                .map(|s| s.id)
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

pub const DEFAULT_EFFORT: &str = "medium";

pub fn efforts(provider: &str, model: &str) -> &'static [&'static str] {
    let model = model.to_ascii_lowercase();
    match provider {
        "claude_code" | "codex" => &["low", "medium", "high", "xhigh"],
        "anthropic"
            if model.starts_with("claude-") && !model.contains("-4-5") && !model.contains("-3") =>
        {
            &["low", "medium", "high", "xhigh"]
        }
        "openai"
            if model.starts_with("gpt-5")
                || model.starts_with("gpt-6")
                || model.starts_with('o') =>
        {
            &["low", "medium", "high"]
        }
        "gemini" => &["low", "medium", "high"],
        _ => &[],
    }
}

pub fn effort_for(provider: &str, model: &str, configured: Option<&str>) -> Option<String> {
    let levels = efforts(provider, model);
    if levels.is_empty() {
        return None;
    }
    let chosen = configured.map(|e| e.trim().to_lowercase());
    Some(
        chosen
            .filter(|e| levels.contains(&e.as_str()))
            .unwrap_or_else(|| DEFAULT_EFFORT.to_string()),
    )
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
    let ring: Vec<Option<&'static str>> = levels.iter().map(|l| Some(*l)).collect();
    let current = current
        .filter(|c| levels.contains(c))
        .unwrap_or(DEFAULT_EFFORT);
    let at = ring.iter().position(|l| *l == Some(current)).unwrap_or(0) as isize;
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
    fn effort_levels_follow_the_program_and_model_and_unset_means_medium() {
        assert_eq!(
            efforts("claude_code", "claude-sonnet-5-5"),
            ["low", "medium", "high", "xhigh"]
        );
        assert_eq!(efforts("openai", "gpt-6-luna"), ["low", "medium", "high"]);
        assert!(efforts("openai", "gpt-4.1").is_empty());
        assert!(efforts("ollama", "qwen3:8b").is_empty());
        assert_eq!(effort_for("codex", "", None).as_deref(), Some("medium"));
        assert_eq!(
            effort_for("codex", "", Some(" XHigh ")).as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            effort_for("codex", "", Some("max")).as_deref(),
            Some("medium"),
            "a level it does not offer means medium"
        );
        assert_eq!(effort_for("ollama", "qwen3:8b", Some("high")), None);
        assert_eq!(
            step_effort("codex", "", None, 1),
            Some(Some("high")),
            "unset counts as medium"
        );
        assert_eq!(
            step_effort("codex", "", Some("xhigh"), 1),
            Some(Some("low")),
            "past the last comes back to low"
        );
        assert_eq!(
            step_effort("codex", "", Some("low"), -1),
            Some(Some("xhigh"))
        );
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
            priced(Task::Chat, "codex", "gpt-6-luna"),
            "gpt-6-luna (included in your ChatGPT plan)"
        );
        assert_eq!(priced(Task::Chat, "ollama", "qwen3:8b"), "qwen3:8b (free)");
        assert_eq!(priced(Task::Chat, "mine", "house"), "house");
        assert_eq!(
            priced(Task::Transcribe, "openai_whisper", "gpt-transcribe"),
            "gpt-transcribe"
        );
    }

    #[test]
    fn writing_choices_are_local_or_signed_in_agents_and_speech_is_local() {
        assert_eq!(
            WRITING.iter().map(|c| c.provider).collect::<Vec<_>>(),
            ["ollama", "codex", "claude_code"]
        );
        assert_eq!(
            SPEECH.iter().map(|c| c.provider).collect::<Vec<_>>(),
            ["parakeet"]
        );
        for retired in [
            "openai",
            "anthropic",
            "openrouter",
            "gemini",
            "xai",
            "agy",
            "copilot",
        ] {
            assert!(find(Task::Chat, retired).is_none());
        }
        assert!(efforts("agy", "default").is_empty());
        assert!(efforts("copilot", "auto").is_empty());
    }

    #[test]
    fn speech_offers_only_the_local_model_and_needs_no_key() {
        assert_eq!(SPEECH.len(), 1);
        assert_eq!(SPEECH[0].provider, "parakeet");
        assert!(Config::built_in_provider("parakeet")
            .unwrap()
            .key_env
            .is_none());
        for name in ["openai_whisper", "gemini_speech", "xai_speech"] {
            assert!(Config::built_in_provider(name).is_none());
            assert!(find(Task::Transcribe, name).is_none());
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
        assert_eq!(step_choice(Task::Chat, Some("ollama"), 1).provider, "codex");
        assert_eq!(
            step_choice(Task::Chat, Some("ollama"), -1).provider,
            "claude_code"
        );
        assert_eq!(
            step_choice(Task::Transcribe, Some("xai_speech"), 1).provider,
            "parakeet"
        );
    }

    #[test]
    fn choosing_a_provider_makes_it_the_whole_chain() {
        let mut doc = Config::default_toml().parse::<DocumentMut>().unwrap();
        write_choice(&mut doc, Task::Chat, "codex");
        write_choice(&mut doc, Task::Transcribe, "parakeet");
        let cfg = load(&doc.to_string());
        assert_eq!(cfg.chat.chain, vec!["codex"]);
        assert_eq!(cfg.transcribe.chain, vec!["parakeet"]);
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
    fn model_options_are_local_lists_for_local_choices_and_curated_for_agents() {
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
            model_options(Task::Chat, "codex", &local)
                .first()
                .map(String::as_str),
            Some("gpt-6-luna")
        );
        assert!(model_options(Task::Chat, "mine", &local).is_empty());
        for retired in [
            "openai",
            "anthropic",
            "openrouter",
            "gemini",
            "xai",
            "agy",
            "copilot",
        ] {
            assert!(model_options(Task::Chat, retired, &local).is_empty());
        }
        assert_eq!(
            model_options(Task::Chat, "claude_code", &local)
                .first()
                .map(String::as_str),
            Some("claude-haiku-5-5")
        );
    }

    #[test]
    fn the_selection_is_the_first_usable_provider_else_the_first_named() {
        let store = MemoryStore::default();
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("missing-claude");
        let codex = dir.path().join("fake-codex");
        let mut cfg =
            load("[chat]\nchain = [\"claude_code\", \"codex\"]\n[transcribe]\nchain = []\n");
        cfg.providers.get_mut("claude_code").unwrap().bin = Some(claude.display().to_string());
        cfg.providers.get_mut("codex").unwrap().bin = Some(codex.display().to_string());
        let sel = selection(&cfg, &store, Task::Chat).unwrap();
        assert_eq!(sel.provider, "claude_code");
        assert!(!sel.ready);
        assert_eq!(sel.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(sel.choice.map(|c| c.name), Some("Claude Code"));

        std::fs::write(&codex, "fixture, never executed").unwrap();
        let sel = selection(&cfg, &store, Task::Chat).unwrap();
        assert_eq!(sel.provider, "codex");
        assert!(sel.ready);
        assert_eq!(sel.choice.map(|c| c.name), Some("Codex"));
        assert!(selection(&cfg, &store, Task::Transcribe).is_none());
    }

    #[test]
    fn old_cloud_speech_keys_cannot_create_a_speech_provider() {
        let store = MemoryStore::default();
        store.set("openai", "fake-key").unwrap();
        let cfg = load("[transcribe]\nchain = [\"openai_whisper\"]\n");
        let chain = crate::ai::provider::build_transcribe_chain(&cfg, &store);
        assert!(chain.is_empty());
    }
}
