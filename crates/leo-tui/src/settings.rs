//! The provider screen's behavior: building its rows from config plus the
//! keychain, and applying the actions the user takes on them.
//!
//! Kept out of `tui/mod.rs` so the row-building and chain-editing logic can be
//! tested against an in-memory secret store, with no terminal and no keychain.

use anyhow::Result;

use leo_services::config::edit::{self, Task};
use leo_services::config::provider::ProviderKind;
use leo_services::config::secret::{redact, SecretStore};
use leo_services::config::Config;
use crate::view::settings::{Credential, Row, SettingAction};

/// Which chain a provider kind can serve. A transcription provider in the chat
/// chain would be silently dropped by the chain builder, so the screen offers
/// the right one instead.
fn task_for(kind: Option<ProviderKind>) -> Task {
    match kind {
        Some(ProviderKind::Openai) => Task::Chat,
        _ => Task::Transcribe,
    }
}

/// Something that can be done to a provider row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderOp {
    Login,
    Add,
    Test,
}

/// What Enter does on a provider row: the one thing it most needs.
pub fn primary_action(credential: &Credential, in_chain: bool) -> ProviderOp {
    match (credential, in_chain) {
        (Credential::Missing, _) => ProviderOp::Login,
        (_, false) => ProviderOp::Add,
        (_, true) => ProviderOp::Test,
    }
}

/// Describe where a provider's credential comes from, without revealing it.
///
/// Asks whether a key exists rather than reading it: this runs for every
/// provider on the screen, and on a real keychain each *read* can cost a
/// permission dialog while an existence check does not.
fn credential_for(name: &str, key_env: Option<&str>, store: &dyn SecretStore) -> Credential {
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
    if store.has(name) {
        Credential::Stored
    } else {
        Credential::Missing
    }
}

/// Build the screen: both chains in order, then everything else that is
/// configured.
pub fn rows(cfg: &Config, store: &dyn SecretStore, notes_dir: &std::path::Path) -> Vec<Row> {
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
            Task::Chat => leo_services::ai::provider::build_chat_chain(cfg, store)
                .iter()
                .map(|p| p.available())
                .collect(),
            Task::Transcribe => leo_services::ai::provider::build_transcribe_chain(cfg, store)
                .iter()
                .map(|p| p.available())
                .collect(),
        };
        // The builder drops entries it cannot construct, so line readiness up by
        // name rather than by index.
        let ready_names: Vec<String> = match task {
            Task::Chat => leo_services::ai::provider::build_chat_chain(cfg, store)
                .iter()
                .map(|p| p.name().to_string())
                .collect(),
            Task::Transcribe => leo_services::ai::provider::build_transcribe_chain(cfg, store)
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

    let unused: Vec<(&String, &leo_services::config::provider::ProviderConfig)> = cfg
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

    rows.extend(appearance_rows(cfg));
    rows.extend(backup_rows(notes_dir, cfg));
    rows.extend(storage_rows(notes_dir));
    rows
}

/// The theme section: what colour the interface is, and how to change it.
fn appearance_rows(cfg: &Config) -> Vec<Row> {
    let palette = cfg.theme.palette();
    let named = leo_services::config::theme::presets()
        .into_iter()
        .find(|(_, rgb)| *rgb == palette.accent)
        .map(|(name, _)| name.to_string());

    vec![
        Row::Section("appearance".to_string()),
        Row::Setting {
            label: "colour".to_string(),
            // The hex is what the config holds, so showing it makes the row and
            // the file legible to each other.
            value: match named {
                Some(name) => format!("{name}  {}", palette.accent.to_hex()),
                None => palette.accent.to_hex(),
            },
            action: SettingAction::NextTheme,
        },
    ]
}

/// The GitHub backup section: whether notes are backed up, where to, and how far
/// behind.
///
/// Sync was previously only reachable by typing `:sync init`, then
/// `:sync connect <url>`, which meant the feature was invisible to anyone who had
/// not read the README.
fn backup_rows(notes_dir: &std::path::Path, cfg: &Config) -> Vec<Row> {
    let mut rows = vec![Row::Section("backup to github".to_string())];

    if !leo_core::sync::is_initialized(notes_dir) {
        rows.push(Row::Setting {
            label: "git backup".to_string(),
            value: "not set up".to_string(),
            action: SettingAction::SyncInit,
        });
        return rows;
    }

    match leo_core::sync::remote_url(notes_dir) {
        Some(url) => {
            // A setting, not a fact: a remote that cannot be changed from the
            // page that shows it is a dead end, and moving a repository is an
            // ordinary thing to do.
            rows.push(Row::Setting {
                label: "remote".to_string(),
                value: url.clone(),
                action: SettingAction::SyncConnect {
                    current: Some(url),
                },
            });
            let waiting = match leo_core::sync::unpushed(notes_dir) {
                Some(0) => "everything is pushed".to_string(),
                Some(1) => "1 commit to push".to_string(),
                Some(n) => format!("{n} commits to push"),
                None => "no upstream branch yet".to_string(),
            };
            rows.push(Row::Setting {
                label: "push".to_string(),
                value: waiting,
                action: SettingAction::SyncPush,
            });
            rows.push(Row::Setting {
                label: "pull".to_string(),
                value: "fetch and reload".to_string(),
                action: SettingAction::SyncPull,
            });
            // Only offered once there is somewhere to push to: the setting means
            // nothing without a remote.
            rows.push(Row::Setting {
                label: "automatically".to_string(),
                value: cfg.sync.auto_push.label().to_string(),
                action: SettingAction::NextAutoPush,
            });
        }
        None => rows.push(Row::Setting {
            label: "remote".to_string(),
            value: "none — connect one".to_string(),
            action: SettingAction::SyncConnect { current: None },
        }),
    }

    rows
}

/// Where things are on disk. Facts rather than settings: leo decides these, and
/// the user only needs to be able to find them.
fn storage_rows(notes_dir: &std::path::Path) -> Vec<Row> {
    let mut rows = vec![Row::Section("where things live".to_string())];
    rows.push(Row::Fact {
        label: "notes".to_string(),
        value: notes_dir.display().to_string(),
    });
    if let Ok(path) = Config::config_path() {
        rows.push(Row::Setting {
            label: "settings".to_string(),
            value: path.display().to_string(),
            action: SettingAction::EditConfig,
        });
    }
    if let Ok(file) = leo_services::config::file_store::FileStore::new() {
        rows.push(Row::Fact {
            label: "keys".to_string(),
            value: format!("{} (only you can read it)", file.path().display()),
        });
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

/// Switch to the next colour preset and write it to the config.
///
/// Cycling rather than offering a list: there are six, and pressing a key until
/// it looks right is faster than reading names. Writes `preset` and clears any
/// explicit `accent`, so the choice on screen and the file agree.
pub fn cycle_theme() -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;

    let presets: Vec<&str> = leo_services::config::theme::presets().keys().copied().collect();
    let current = doc
        .get("theme")
        .and_then(|t| t.get("preset"))
        .and_then(|p| p.as_str())
        .unwrap_or("orange");
    let next = presets
        .iter()
        .position(|p| *p == current)
        .map(|i| presets[(i + 1) % presets.len()])
        .unwrap_or(presets[0]);

    let theme = doc
        .entry("theme")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if let Some(table) = theme.as_table_mut() {
        table.insert("preset", toml_edit::value(next));
        // An explicit accent would win over the preset, which would make this
        // key appear to do nothing.
        table.remove("accent");
    }

    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "Colour set to {next}. Restart leo to see it."
    )))
}

/// Switch when leo pushes on its own, and write it to the config.
pub fn cycle_auto_push() -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;

    let current = doc
        .get("sync")
        .and_then(|t| t.get("auto_push"))
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "off" => Some(leo_services::config::sync::AutoPush::Off),
            "on_quit" => Some(leo_services::config::sync::AutoPush::OnQuit),
            "when_idle" => Some(leo_services::config::sync::AutoPush::WhenIdle),
            _ => None,
        })
        .unwrap_or_default();
    let next = current.next();

    let table = doc
        .entry("sync")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if let Some(table) = table.as_table_mut() {
        table.insert("auto_push", toml_edit::value(next.as_str()));
    }

    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!("Backing up {}.", next.label())))
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
    /// Enter does the one thing a provider row most needs: a key when it has
    /// none, joining a list when it is unused, otherwise a test.
    #[test]
    fn enter_on_a_provider_does_what_it_most_needs() {
        use crate::view::settings::Credential;
        assert_eq!(primary_action(&Credential::Missing, true), ProviderOp::Login);
        assert_eq!(primary_action(&Credential::Missing, false), ProviderOp::Login);
        assert_eq!(primary_action(&Credential::Stored, false), ProviderOp::Add);
        assert_eq!(primary_action(&Credential::NotNeeded, true), ProviderOp::Test);
    }

    use super::*;
    use leo_services::config::secret::MemoryStore;
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
        let rows = rows(&small_config(), &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));

        assert_eq!(rows[0], Row::Header(Task::Chat));
        assert_eq!(rows[1].provider_name(), Some("ollama"));
        assert_eq!(rows[2].provider_name(), Some("openrouter"));
        assert_eq!(rows[3], Row::Header(Task::Transcribe));
        assert_eq!(rows[4].provider_name(), Some("groq"));
        assert_eq!(rows[5], Row::AvailableHeader);
        // Everything configured but unchained shows up, so a user can see what
        // is available without opening the file.
        assert_eq!(rows[6].provider_name(), Some("cerebras"));
        // The provider part ends where the rest of the page begins.
        assert!(matches!(rows[7], Row::Section(_)), "{:?}", rows[7]);
    }

    #[test]
    fn a_chain_position_is_shown_as_its_priority() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(&small_config(), &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));
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
        let rows = rows(&small_config(), &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));
        match &rows[1] {
            Row::Member { credential, .. } => assert_eq!(*credential, Credential::NotNeeded),
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_stored_key_shows_as_stored_and_a_missing_one_says_so() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETTINGS_OR");
        std::env::remove_var("LEO_TEST_SETTINGS_GROQ");

        let store = MemoryStore::default();
        store.set("openrouter", "sk-or-v1-secret9999").unwrap();
        let rows = rows(&small_config(), &store, std::path::Path::new("/tmp/leo-test-notes"));

        match &rows[2] {
            Row::Member { credential, .. } => {
                assert_eq!(*credential, Credential::Stored);
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

        let rows = rows(&small_config(), &store, std::path::Path::new("/tmp/leo-test-notes"));
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
        let rows = rows(&cfg, &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));

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
        let rows = rows(&cfg, &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));

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
    fn an_empty_config_produces_only_headers_and_the_rest_of_the_page() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse("").unwrap();
        let rows = rows(&cfg, &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));

        // No providers, but both chain headers still say so.
        let providers: Vec<&Row> = rows
            .iter()
            .filter(|r| r.provider_name().is_some())
            .collect();
        assert!(providers.is_empty(), "{providers:?}");
        assert_eq!(rows[0], Row::Header(Task::Chat));
        assert_eq!(rows[1], Row::Header(Task::Transcribe));

        // And the page is more than providers: appearance, backup, storage.
        let sections: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Section(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sections, ["appearance", "backup to github", "where things live"]);
    }

    /// Everything on the page must either do something or be worth reading, and
    /// j/k must only stop on the rows that do something.
    #[test]
    fn only_actionable_rows_are_selectable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(&small_config(), &MemoryStore::default(), std::path::Path::new("/tmp/leo-test-notes"));

        for row in &rows {
            match row {
                Row::Header(_) | Row::AvailableHeader | Row::Section(_) | Row::Fact { .. } => {
                    assert!(!row.selectable(), "{row:?} should not be selectable")
                }
                Row::Member { .. } | Row::Unused { .. } | Row::Setting { .. } => {
                    assert!(row.selectable(), "{row:?} should be selectable")
                }
            }
        }
        // And every setting says what choosing it will do.
        for row in rows.iter().filter(|r| matches!(r, Row::Setting { .. })) {
            let action = row.action().expect("a setting with no action");
            assert!(!action.describe().is_empty());
        }
    }

    /// The colour row must name the preset when the accent matches one, since
    /// "orange" is more use than a hex string.
    #[test]
    fn the_appearance_row_names_the_current_colour() {
        let cfg = Config::parse("").unwrap();
        let rows = appearance_rows(&cfg);
        let Row::Setting { label, value, action } = &rows[1] else {
            panic!("expected a setting, got {:?}", rows[1]);
        };
        assert_eq!(label, "colour");
        assert!(value.contains("orange"), "{value}");
        assert!(value.contains("#d97757"), "{value}");
        assert_eq!(*action, SettingAction::NextTheme);
    }

    /// Sync was previously invisible unless the user had read the README.
    #[test]
    fn backup_offers_setup_when_there_is_no_repo() {
        let dir = tempfile::tempdir().unwrap();
        let rows = backup_rows(dir.path(), &Config::default());
        assert_eq!(rows[0], Row::Section("backup to github".to_string()));
        let Row::Setting { value, action, .. } = &rows[1] else {
            panic!("expected a setting, got {:?}", rows[1]);
        };
        assert_eq!(value, "not set up");
        assert_eq!(*action, SettingAction::SyncInit);
    }

    /// The gap this closes: a configured remote was shown as a fact, so the page
    /// that displayed where notes were backed up gave no way to change it.
    #[test]
    fn a_configured_remote_can_be_changed_from_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        if leo_core::sync::init(&notes).is_err() {
            return; // no git on this machine
        }
        let url = "https://github.com/example/notes.git";
        if leo_core::sync::connect(&notes, url).is_err() {
            return;
        }

        let rows = backup_rows(&notes, &Config::default());
        let remote = rows
            .iter()
            .find(|r| matches!(r, Row::Setting { label, .. } if label == "remote"))
            .expect("the remote row is not a setting, so it cannot be changed");

        let Row::Setting { value, action, .. } = remote else {
            unreachable!()
        };
        assert_eq!(value, url);
        // And it carries the current URL, so the prompt can prefill it rather
        // than making the user retype a URL to change one character.
        assert_eq!(
            *action,
            SettingAction::SyncConnect {
                current: Some(url.to_string())
            }
        );
        assert!(remote.selectable(), "the remote row cannot be selected");
    }

    /// The setting is only meaningful once there is a remote, and it has to be
    /// changeable from the page that shows it.
    #[test]
    fn automatic_backup_is_offered_once_a_remote_exists() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        if leo_core::sync::init(&notes).is_err() {
            return;
        }

        // No remote yet: nothing to automate.
        let rows = backup_rows(&notes, &Config::default());
        assert!(
            !rows.iter().any(|r| matches!(r, Row::Setting { action, .. }
                if *action == SettingAction::NextAutoPush)),
            "offered automatic backup with nowhere to push"
        );

        if leo_core::sync::connect(&notes, "https://github.com/example/n.git").is_err() {
            return;
        }
        let rows = backup_rows(&notes, &Config::default());
        let row = rows
            .iter()
            .find(|r| matches!(r, Row::Setting { action, .. }
                if *action == SettingAction::NextAutoPush))
            .expect("no automatic backup row");
        let Row::Setting { value, .. } = row else {
            unreachable!()
        };
        // The default is stated rather than left blank.
        assert_eq!(value, leo_services::config::sync::AutoPush::default().label());
    }

    #[test]
    fn storage_shows_where_notes_and_keys_live() {
        let rows = storage_rows(std::path::Path::new("/tmp/leo-test-notes"));
        let values: String = rows
            .iter()
            .map(|r| match r {
                Row::Fact { label, value } | Row::Setting { label, value, .. } => {
                    format!("{label}={value} ")
                }
                _ => String::new(),
            })
            .collect();
        assert!(values.contains("notes=/tmp/leo-test-notes"), "{values}");
        assert!(values.contains("config.toml"), "{values}");
        assert!(values.contains("credentials.json"), "{values}");
    }

    #[test]
    fn readiness_reflects_whether_the_chain_runner_would_use_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETTINGS_OR");
        let store = MemoryStore::default();

        // Without a key, openrouter is not ready.
        let rows_before = rows(&small_config(), &store, std::path::Path::new("/tmp/leo-test-notes"));
        let ready_before = matches!(&rows_before[2], Row::Member { ready: true, .. });
        assert!(!ready_before);

        // With one, it is.
        store.set("openrouter", "a-key").unwrap();
        let rows_after = rows(&small_config(), &store, std::path::Path::new("/tmp/leo-test-notes"));
        assert!(matches!(&rows_after[2], Row::Member { ready: true, .. }));
    }
}
