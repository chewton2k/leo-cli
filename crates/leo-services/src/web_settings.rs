use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item, Table};

use crate::ai::provider::agent_cli::{self, Agent};
use crate::config::choice::{self, Local};
use crate::config::edit::{self, Task};
use crate::config::secret::SecretStore;
use crate::config::sync::AutoPush;
use crate::config::Config;

const AUTO_PUSH: [AutoPush; 3] = [AutoPush::Off, AutoPush::OnQuit, AutoPush::WhenIdle];

fn task_named(name: &str) -> Option<Task> {
    match name {
        "writing" => Some(Task::Chat),
        "speech" => Some(Task::Transcribe),
        _ => None,
    }
}

fn task_name(task: Task) -> &'static str {
    match task {
        Task::Chat => "writing",
        Task::Transcribe => "speech",
    }
}

fn accounts(cfg: &Config) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for task in [Task::Chat, Task::Transcribe] {
        for c in choice::choices(task) {
            let Some(pc) = cfg.provider(c.provider) else {
                continue;
            };
            if pc.key_env.is_none() || Agent::of(pc).is_some() {
                continue;
            }
            let account = pc.account(c.provider).to_string();
            if !out.iter().any(|(a, _)| *a == account) {
                out.push((account, c.name.to_string()));
            }
        }
    }
    out
}

fn models(task: Task, provider: &str, local: &Local) -> Vec<Value> {
    match (task, provider) {
        (Task::Chat, "ollama") => local
            .ollama
            .iter()
            .map(|m| json!({ "id": m, "price": "free" }))
            .collect(),
        _ => choice::find(task, provider)
            .map(|c| {
                c.models
                    .iter()
                    .map(|m| json!({ "id": m.id, "price": m.price }))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn task_view(
    cfg: &Config,
    store: &dyn SecretStore,
    local: &Local,
    usage: &crate::usage::Seen,
    task: Task,
) -> Value {
    let selection = choice::selection(cfg, store, task);
    let provider = selection
        .as_ref()
        .map(|s| s.provider.clone())
        .unwrap_or_default();
    let chosen = choice::find(task, &provider);
    let pc = cfg.provider(&provider);
    let agent = pc.and_then(Agent::of);
    let key = match (pc, agent) {
        (Some(pc), None) if pc.key_env.is_some() => {
            let account = pc.account(&provider).to_string();
            let stored = store.has(&account);
            let ignored = pc
                .key_env
                .as_deref()
                .filter(|var| !stored && std::env::var(var).is_ok_and(|v| !v.trim().is_empty()));
            json!({
                "account": account,
                "name": chosen.map(|c| c.name).unwrap_or(provider.as_str()),
                "stored": stored,
                "ignored": ignored,
            })
        }
        _ => Value::Null,
    };
    let signin = agent.map(|agent| {
        let bin = pc.and_then(|p| p.bin.as_deref()).unwrap_or(agent.program());
        let installed = agent_cli::locate(bin).is_some();
        json!({
            "installed": installed,
            "text": if installed {
                format!("Uses {}, no key needed. If it asks, {}.", agent.plan(), agent.sign_in())
            } else {
                format!("`{bin}` is not installed: {}.", agent.install())
            },
        })
    });
    let note = match (task, provider.as_str()) {
        (Task::Chat, "ollama") if !local.ollama_running => Some(if crate::health::on_path("ollama") {
            "Ollama is not running: open the Ollama app on this computer."
        } else {
            "Ollama is not installed on this computer."
        }),
        (Task::Chat, "ollama") if local.ollama.is_empty() => Some("Ollama has no models yet: run `ollama pull qwen3:8b` on this computer."),
        (Task::Transcribe, "parakeet") if !local.speech_ready => Some("The speech model is not downloaded yet: leo downloads it when the app starts, or run `leo update`."),
        _ => None,
    };
    let usage = usage
        .get(&provider)
        .filter(|_| agent.is_some())
        .map(|seen| crate::usage::label(seen, chrono::Utc::now()));
    json!({
        "task": task_name(task),
        "provider": provider,
        "ready": selection.as_ref().is_some_and(|s| s.ready),
        "custom": chosen.is_none() && !provider.is_empty(),
        "choices": choice::choices(task).iter().map(|c| json!({ "id": c.provider, "label": c.label() })).collect::<Vec<_>>(),
        "model": selection.as_ref().and_then(|s| s.model.clone()),
        "models": models(task, &provider, local),
        "fixed_model": chosen.is_some_and(|c| c.local()) && task == Task::Transcribe,
        "key": key,
        "signin": signin,
        "usage": usage,
        "note": note,
    })
}

pub fn describe(
    cfg: &Config,
    store: &dyn SecretStore,
    local: &Local,
    usage: &crate::usage::Seen,
    notes_dir: Option<&Path>,
) -> Value {
    let remote = notes_dir.and_then(leo_core::sync::remote_url);
    json!({
        "tasks": [
            task_view(cfg, store, local, usage, Task::Chat),
            task_view(cfg, store, local, usage, Task::Transcribe),
        ],
        "backup": {
            "auto_push": cfg.sync.auto_push.as_str(),
            "options": AUTO_PUSH.iter().map(|a| json!({ "id": a.as_str(), "label": a.label() })).collect::<Vec<_>>(),
            "remote": remote,
        },
        "paths": {
            "notes": notes_dir.map(|p| p.display().to_string()),
            "config": Config::config_path().ok().map(|p| p.display().to_string()),
        },
    })
}

fn open(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, Config::default_toml())?;
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    text.parse::<DocumentMut>()
        .with_context(|| format!("{} is not valid TOML", path.display()))
}

fn text<'a>(change: &'a Value, field: &str) -> Result<&'a str> {
    change
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("the change has no {field}"))
}

pub fn apply(
    change: &Value,
    config_path: &Path,
    store: &dyn SecretStore,
    secure: bool,
    local: &Local,
) -> Result<String> {
    match text(change, "set")? {
        "provider" => {
            let task = task_named(text(change, "task")?).context("no such task")?;
            let value = text(change, "value")?;
            let Some(chosen) = choice::find(task, value) else {
                bail!("leo does not offer {value} there");
            };
            let mut doc = open(config_path)?;
            choice::write_choice(&mut doc, task, chosen.provider);
            edit::save_document(config_path, &doc)?;
            Ok(format!(
                "{} now uses {}.",
                if task == Task::Chat {
                    "Writing"
                } else {
                    "Speech"
                },
                chosen.label()
            ))
        }
        "model" => {
            let task = task_named(text(change, "task")?).context("no such task")?;
            let value = text(change, "value")?;
            let cfg = Config::load_from(config_path);
            let Some(selection) = choice::selection(&cfg, store, task) else {
                bail!("choose an AI first");
            };
            let options = choice::model_options(task, &selection.provider, local);
            if !options.iter().any(|o| o == value) {
                bail!(
                    "{value} is not one of the models offered for {}",
                    selection.provider
                );
            }
            let mut doc = open(config_path)?;
            choice::write_model(&mut doc, &selection.provider, value);
            edit::save_document(config_path, &doc)?;
            Ok(format!(
                "Model set to {}.",
                choice::priced(task, &selection.provider, value)
            ))
        }
        "key" => {
            let account = text(change, "account")?;
            let cfg = Config::load_from(config_path);
            let Some((_, name)) = accounts(&cfg).into_iter().find(|(a, _)| a == account) else {
                bail!("there is no key called {account}");
            };
            match change.get("value").and_then(Value::as_str).map(str::trim) {
                None | Some("") => {
                    store.delete(account)?;
                    Ok(format!("The {name} key is removed."))
                }
                Some(key) => {
                    if !secure {
                        bail!("keys can only be added over the https link or on this computer, so they never cross Wi-Fi unencrypted");
                    }
                    if key.len() > 512 || key.chars().any(char::is_whitespace) {
                        bail!("that does not look like an API key");
                    }
                    store.set(account, key)?;
                    Ok(format!("The {name} key is stored on this computer."))
                }
            }
        }
        "auto_push" => {
            let value = text(change, "value")?;
            let Some(chosen) = AUTO_PUSH.iter().find(|a| a.as_str() == value) else {
                bail!("{value} is not a backup setting");
            };
            let mut doc = open(config_path)?;
            let sync = doc.entry("sync").or_insert(Item::Table(Table::new()));
            if let Some(table) = sync.as_table_mut() {
                table.insert("auto_push", toml_edit::value(chosen.as_str()));
            }
            edit::save_document(config_path, &doc)?;
            Ok(format!("Backing up {}.", chosen.label()))
        }
        other => bail!("{other} is not a setting"),
    }
}

pub fn test(cfg: &Config, store: &dyn SecretStore, task: &str) -> Result<String> {
    let task = task_named(task).context("no such task")?;
    let Some(selection) = choice::selection(cfg, store, task) else {
        bail!("choose an AI first");
    };
    crate::providers::test_provider(&selection.provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;

    fn setup(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn view(path: &Path, store: &dyn SecretStore) -> Value {
        describe(
            &Config::load_from(path),
            store,
            &Local::default(),
            &Default::default(),
            None,
        )
    }

    #[test]
    fn the_page_shows_each_task_with_its_choices_models_and_key() {
        let (_d, path) =
            setup("[chat]\nchain = [\"openai\"]\n[transcribe]\nchain = [\"openai_whisper\"]\n");
        let store = MemoryStore::default();
        store.set("openai", "sk-secret-value").unwrap();
        let page = view(&path, &store);
        let writing = &page["tasks"][0];
        assert_eq!(writing["task"], "writing");
        assert_eq!(writing["provider"], "openai");
        assert_eq!(writing["ready"], true);
        assert!(writing["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "claude_code"));
        assert!(writing["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == "gpt-6-luna" && m["price"].as_str().unwrap().contains('$')));
        assert_eq!(writing["key"]["account"], "openai");
        assert_eq!(writing["key"]["stored"], true);
        assert_eq!(
            page["tasks"][1]["key"]["account"], "openai",
            "speech shares the OpenAI key"
        );
        assert!(
            !page.to_string().contains("sk-secret-value"),
            "a key never leaves the computer"
        );
        assert_eq!(page["backup"]["auto_push"], "on_quit");
        assert_eq!(page["backup"]["options"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_coding_plan_shows_its_sign_in_instead_of_a_key() {
        let (_d, path) = setup("[chat]\nchain = [\"codex\"]\n[providers.codex]\nbin = \"leo-no-such-codex\"\n[transcribe]\nchain = []\n");
        let page = view(&path, &MemoryStore::default());
        let writing = &page["tasks"][0];
        assert!(writing["key"].is_null());
        assert_eq!(writing["signin"]["installed"], false);
        assert!(writing["signin"]["text"]
            .as_str()
            .unwrap()
            .contains("npm install -g @openai/codex"));
    }

    #[test]
    fn choosing_a_provider_and_model_writes_the_config() {
        let (_d, path) = setup("[chat]\nchain = [\"openai\"]\n[transcribe]\nchain = []\n");
        let store = MemoryStore::default();
        let local = Local::default();
        let said = apply(
            &json!({"set": "provider", "task": "writing", "value": "anthropic"}),
            &path,
            &store,
            false,
            &local,
        )
        .unwrap();
        assert_eq!(said, "Writing now uses Anthropic.");
        let said = apply(
            &json!({"set": "model", "task": "writing", "value": "claude-opus-5-5"}),
            &path,
            &store,
            false,
            &local,
        )
        .unwrap();
        assert!(
            said.starts_with("Model set to claude-opus-5-5 ($"),
            "{said}"
        );
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, ["anthropic"]);
        assert_eq!(
            cfg.provider("anthropic").unwrap().model.as_deref(),
            Some("claude-opus-5-5")
        );
        assert!(apply(
            &json!({"set": "model", "task": "writing", "value": "gpt-made-up"}),
            &path,
            &store,
            false,
            &local
        )
        .is_err());
        assert!(
            apply(
                &json!({"set": "provider", "task": "writing", "value": "parakeet"}),
                &path,
                &store,
                false,
                &local
            )
            .is_err(),
            "speech engines cannot write"
        );
        assert!(apply(
            &json!({"set": "provider", "task": "cooking", "value": "openai"}),
            &path,
            &store,
            false,
            &local
        )
        .is_err());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[transcribe]"),
            "the rest of the file is kept"
        );
    }

    #[test]
    fn keys_are_stored_only_over_a_secure_link_and_can_be_removed() {
        let (_d, path) = setup("[chat]\nchain = [\"openrouter\"]\n[transcribe]\nchain = []\n");
        let store = MemoryStore::default();
        let local = Local::default();
        let add = json!({"set": "key", "account": "openrouter", "value": " sk-or-123 "});
        let refused = apply(&add, &path, &store, false, &local)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("https"), "{refused}");
        assert!(!store.has("openrouter"));
        assert_eq!(
            apply(&add, &path, &store, true, &local).unwrap(),
            "The OpenRouter key is stored on this computer."
        );
        assert_eq!(
            store.get("openrouter").unwrap().unwrap().as_str(),
            "sk-or-123"
        );
        assert!(apply(
            &json!({"set": "key", "account": "keychain-of-someone-else", "value": "x"}),
            &path,
            &store,
            true,
            &local
        )
        .is_err());
        assert!(apply(
            &json!({"set": "key", "account": "openrouter", "value": "two words"}),
            &path,
            &store,
            true,
            &local
        )
        .is_err());
        assert_eq!(
            apply(
                &json!({"set": "key", "account": "openrouter", "value": null}),
                &path,
                &store,
                false,
                &local
            )
            .unwrap(),
            "The OpenRouter key is removed."
        );
        assert!(!store.has("openrouter"));
    }

    #[test]
    fn backup_timing_is_one_of_three() {
        let (_d, path) = setup("[chat]\nchain = []\n");
        let store = MemoryStore::default();
        let local = Local::default();
        assert_eq!(
            apply(
                &json!({"set": "auto_push", "value": "when_idle"}),
                &path,
                &store,
                false,
                &local
            )
            .unwrap(),
            "Backing up when idle, and on quit."
        );
        assert_eq!(Config::load_from(&path).sync.auto_push, AutoPush::WhenIdle);
        assert!(apply(
            &json!({"set": "auto_push", "value": "hourly"}),
            &path,
            &store,
            false,
            &local
        )
        .is_err());
        assert!(apply(&json!({"set": "theme"}), &path, &store, false, &local).is_err());
    }

    #[test]
    fn a_missing_config_file_is_created_with_its_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leo/config.toml");
        apply(
            &json!({"set": "provider", "task": "writing", "value": "gemini"}),
            &path,
            &MemoryStore::default(),
            false,
            &Local::default(),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# leo configuration."));
        assert_eq!(Config::load_from(&path).chat.chain, ["gemini"]);
    }
}
