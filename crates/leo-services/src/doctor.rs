//! `leo doctor`: a full health scan of leo, the notes, the AI, recording and
//! backup, grouped so a problem is found under the thing it affects.
//!
//! `leo setup` answers "what do I install next?"; this answers "is everything
//! actually working?", so it goes further: it reads every note file, parses the
//! config, and — when asked to probe — sends one small request to each AI in
//! use, listens to the microphone, and asks the backup remote whether it
//! answers.

use std::path::Path;

use crate::config::secret::SecretStore;
use crate::config::Config;
use crate::health::{self, Check, State};

/// One group of checks, headed by what it is about.
#[derive(Debug, Clone)]
pub struct Section {
    pub title: &'static str,
    pub checks: Vec<Check>,
}

/// Which checks may reach outside the machine or use hardware. All on for
/// `leo doctor`; tests turn on only what they can run.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probe {
    /// Send one small request to each AI provider in use.
    pub ai: bool,
    /// Record a moment of audio to check the microphone is heard.
    pub microphone: bool,
    /// Ask the backup remote whether it answers.
    pub remote: bool,
}

impl Probe {
    pub fn all() -> Probe {
        Probe {
            ai: true,
            microphone: true,
            remote: true,
        }
    }
}

/// Run every check.
pub fn scan(
    config: &Config,
    secrets: &dyn SecretStore,
    notes_dir: &Path,
    config_path: &Path,
    probe: Probe,
) -> Vec<Section> {
    vec![
        Section {
            title: "leo",
            checks: leo_checks(config_path),
        },
        Section {
            title: "notes",
            checks: notes_checks(notes_dir),
        },
        Section {
            title: "AI",
            checks: ai_checks(config, secrets, probe),
        },
        Section {
            title: "recording",
            checks: recording_checks(probe),
        },
        Section {
            title: "backup",
            checks: backup_checks(config, notes_dir, probe),
        },
    ]
}

fn warn(what: &str, needed_for: &str, note: String) -> Check {
    Check {
        what: what.to_string(),
        needed_for: needed_for.to_string(),
        state: State::Warn { note },
        detail: None,
    }
}

fn leo_checks(config_path: &Path) -> Vec<Check> {
    let mut checks = vec![Check::ready(
        "leo",
        "everything",
        Some(format!("version {}", env!("CARGO_PKG_VERSION"))),
    )];

    checks.push(if health::on_path("leo") {
        Check::ready("leo on your PATH", "running `leo` from any terminal", None)
    } else {
        Check::missing(
            "leo on your PATH",
            "running `leo` from any terminal",
            "echo 'export PATH=\"$HOME/.cargo/bin:$PATH\"' >> ~/.zshrc && source ~/.zshrc\n\
             (bash on a Mac: ~/.bash_profile instead of ~/.zshrc)",
        )
    });

    checks.push(match std::fs::read_to_string(config_path) {
        Err(_) => Check::ready(
            "config file",
            "providers and settings",
            Some("not created yet; using the defaults".to_string()),
        ),
        Ok(text) => match Config::parse(&text) {
            Ok(_) => Check::ready(
                "config file",
                "providers and settings",
                Some(config_path.display().to_string()),
            ),
            Err(e) => {
                let reason = format!("{e:#}").lines().next().unwrap_or("").to_string();
                let mut c = Check::missing(
                    "config file",
                    "providers and settings",
                    &format!(
                        "fix it (Ctrl-S, then e), or move it aside to start again:\n{}",
                        config_path.display()
                    ),
                );
                c.detail = Some(reason);
                c
            }
        },
    });
    checks
}

fn notes_checks(notes_dir: &Path) -> Vec<Check> {
    let mut checks = Vec::new();

    let probe_file = notes_dir.join(".leo-doctor-probe");
    let writable = std::fs::create_dir_all(notes_dir).is_ok()
        && std::fs::write(&probe_file, b"").is_ok()
        && std::fs::remove_file(&probe_file).is_ok();
    checks.push(if writable {
        Check::ready(
            "notes directory",
            "saving notes",
            Some(notes_dir.display().to_string()),
        )
    } else {
        Check::missing(
            "notes directory",
            "saving notes",
            &format!(
                "leo cannot write to {} — check its permissions",
                notes_dir.display()
            ),
        )
    });

    let store = match leo_core::store::Store::load_from(notes_dir) {
        Ok(store) => store,
        Err(e) => {
            checks.push(Check::missing(
                "notes",
                "everything",
                &format!("could not be read: {e}"),
            ));
            return checks;
        }
    };
    let dirs = store.directories.len();
    checks.push(Check::ready(
        "notes",
        "everything",
        Some(format!(
            "{} note{} in {} director{}",
            store.notes.len(),
            if store.notes.len() == 1 { "" } else { "s" },
            dirs,
            if dirs == 1 { "y" } else { "ies" }
        )),
    ));

    if !store.unreadable.is_empty() {
        let list: String = store
            .unreadable
            .iter()
            .map(|(path, why)| format!("{} — {why}\n", path.display()))
            .collect();
        checks.push(Check::missing(
            "unreadable notes",
            "seeing every note",
            &format!(
                "leo skips these (it never deletes them). Open each in your editor and fix \
                 the header between the --- lines:\n{list}"
            ),
        ));
    }

    let dupes = store.duplicate_ids();
    if !dupes.is_empty() {
        checks.push(Check::missing(
            "duplicate note IDs",
            "finding and editing notes",
            &format!(
                "more than one note file has the id {}; change the id: line in one of them",
                dupes.join(", ")
            ),
        ));
    }
    checks
}

fn ai_checks(config: &Config, secrets: &dyn SecretStore, probe: Probe) -> Vec<Check> {
    let mut checks = vec![
        health::chain_check(config, health::Chain::Chat, secrets),
        health::chain_check(config, health::Chain::Transcribe, secrets),
        health::credentials_check(),
    ];
    if probe.ai {
        let chat = crate::ai::provider::build_chat_chain(config, secrets)
            .into_iter()
            .find(|p| p.available())
            .map(|p| p.name().to_string());
        let speech = crate::ai::provider::build_transcribe_chain(config, secrets)
            .into_iter()
            .find(|p| p.available())
            .map(|p| p.name().to_string());
        for (label, name) in [("writing", chat), ("speech", speech)] {
            let Some(name) = name else { continue };
            let what = format!("{name} answers");
            let needed_for = format!("AI for {label}");
            checks.push(match crate::providers::test_provider(&name) {
                Ok(report) => Check::ready(&what, &needed_for, Some(report)),
                Err(e) => {
                    let mut c = Check::missing(
                        &what,
                        &needed_for,
                        "check its key with Ctrl-S, or that the service is up",
                    );
                    c.detail = Some(e.to_string());
                    c
                }
            });
        }
    }
    checks
}

fn recording_checks(probe: Probe) -> Vec<Check> {
    let has_sox = health::on_path("rec");
    let mut checks = vec![if has_sox {
        Check::ready("sox", "recording audio", None)
    } else {
        Check::missing("sox", "recording audio", health::install_hint("sox"))
    }];
    if probe.microphone && has_sox {
        checks.push(health::microphone());
    }
    checks
}

fn backup_checks(config: &Config, notes_dir: &Path, probe: Probe) -> Vec<Check> {
    use leo_core::sync;

    if !health::on_path("git") {
        return vec![Check::missing(
            "git",
            "backing up",
            health::install_hint("git"),
        )];
    }
    let mut checks = vec![Check::ready("git", "backing up", None)];

    if !sync::is_initialized(notes_dir) {
        checks.push(warn(
            "backup",
            "keeping a copy on GitHub",
            "off — `leo sync` sets it up".to_string(),
        ));
        return checks;
    }
    checks.push(Check::ready(
        "backup",
        "keeping a copy on GitHub",
        Some("every save is committed".to_string()),
    ));

    match sync::remote_url(notes_dir) {
        None => checks.push(warn(
            "GitHub remote",
            "keeping a copy on GitHub",
            "none connected — `leo sync` connects one".to_string(),
        )),
        Some(url) if probe.remote => checks.push(match sync::remote_reachable(notes_dir) {
            Ok(()) => Check::ready("GitHub remote", "keeping a copy on GitHub", Some(url)),
            Err(e) => {
                let mut c = Check::missing(
                    "GitHub remote",
                    "keeping a copy on GitHub",
                    &format!(
                        "check the address, and that `git push` works from this terminal:\n{url}"
                    ),
                );
                c.detail = Some(e);
                c
            }
        }),
        Some(url) => checks.push(Check::ready(
            "GitHub remote",
            "keeping a copy on GitHub",
            Some(url),
        )),
    }

    if let Some(waiting) = sync::unpushed(notes_dir) {
        checks.push(if waiting == 0 {
            Check::ready(
                "pushed",
                "keeping a copy on GitHub",
                Some("GitHub has everything".to_string()),
            )
        } else {
            warn(
                "pushed",
                "keeping a copy on GitHub",
                format!(
                    "{waiting} change{} not on GitHub yet — `leo sync` pushes {}",
                    if waiting == 1 { "" } else { "s" },
                    if waiting == 1 { "it" } else { "them" }
                ),
            )
        });
    }

    if let Some(changed) = sync::uncommitted(notes_dir) {
        checks.push(if changed == 0 {
            Check::ready("uncommitted changes", "keeping a copy on GitHub", None)
        } else {
            warn(
                "uncommitted changes",
                "keeping a copy on GitHub",
                format!(
                    "{changed} file{} changed outside leo; the next save or `leo sync` commits {}",
                    if changed == 1 { "" } else { "s" },
                    if changed == 1 { "it" } else { "them" }
                ),
            )
        });
    }

    checks.push(Check::ready(
        "automatic backup",
        "keeping a copy on GitHub",
        Some(config.sync.auto_push.label().to_string()),
    ));
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;
    use tempfile::TempDir;

    fn quiet() -> Probe {
        Probe::default()
    }

    fn section<'a>(sections: &'a [Section], title: &str) -> &'a Section {
        sections
            .iter()
            .find(|s| s.title == title)
            .unwrap_or_else(|| panic!("no {title} section"))
    }

    fn check<'a>(section: &'a Section, what: &str) -> &'a Check {
        section
            .checks
            .iter()
            .find(|c| c.what == what)
            .unwrap_or_else(|| {
                panic!(
                    "no {what:?} check in {}: {:?}",
                    section.title, section.checks
                )
            })
    }

    fn setup() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let notes = tmp.path().join("notes");
        let config = tmp.path().join("config.toml");
        (tmp, notes, config)
    }

    #[test]
    fn every_part_of_leo_has_a_section_in_order() {
        let (_tmp, notes, config) = setup();
        let sections = scan(
            &Config::default(),
            &MemoryStore::default(),
            &notes,
            &config,
            quiet(),
        );
        let titles: Vec<&str> = sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["leo", "notes", "AI", "recording", "backup"]);
    }

    #[test]
    fn the_notes_are_counted_and_an_unreadable_one_is_named() {
        let (_tmp, notes, config) = setup();
        let mut store = leo_core::store::Store::load_from(&notes).unwrap();
        store.create_note("A", "a", vec![], "").unwrap();
        store.create_note("B", "b", vec![], "cs130").unwrap();
        store.create_dir("cs130");
        store.save().unwrap();
        std::fs::write(notes.join("broken.md"), "---\ntitle: [unclosed\n---\nx").unwrap();

        let sections = scan(
            &Config::default(),
            &MemoryStore::default(),
            &notes,
            &config,
            quiet(),
        );
        let notes_section = section(&sections, "notes");
        let count = check(notes_section, "notes");
        assert!(count.state.is_ready());
        assert!(
            count.detail.as_deref().unwrap_or("").contains("2 notes"),
            "{count:?}"
        );

        let unreadable = check(notes_section, "unreadable notes");
        match &unreadable.state {
            State::Missing { fix } => assert!(fix.contains("broken.md"), "{fix}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_config_file_that_does_not_parse_is_a_failure() {
        let (_tmp, notes, config) = setup();
        std::fs::write(&config, "[chat\nchain = [").unwrap();
        let sections = scan(
            &Config::default(),
            &MemoryStore::default(),
            &notes,
            &config,
            quiet(),
        );
        let c = check(section(&sections, "leo"), "config file");
        assert!(matches!(c.state, State::Missing { .. }), "{c:?}");
    }

    #[test]
    fn no_config_file_yet_is_fine() {
        let (_tmp, notes, config) = setup();
        let sections = scan(
            &Config::default(),
            &MemoryStore::default(),
            &notes,
            &config,
            quiet(),
        );
        assert!(check(section(&sections, "leo"), "config file")
            .state
            .is_ready());
    }

    /// Backup is optional, so not having it is worth a mention, not a failure.
    #[test]
    fn backup_being_off_is_a_note_not_a_failure() {
        let (_tmp, notes, config) = setup();
        std::fs::create_dir_all(&notes).unwrap();
        let sections = scan(
            &Config::default(),
            &MemoryStore::default(),
            &notes,
            &config,
            quiet(),
        );
        let b = check(section(&sections, "backup"), "backup");
        match &b.state {
            State::Warn { note } => assert!(note.contains("leo sync"), "{note}"),
            other => panic!("expected a note, got {other:?}"),
        }
    }

    #[test]
    fn a_backup_remote_is_checked_and_what_is_waiting_is_counted() {
        let (tmp, notes, config) = setup();
        let remote = tmp.path().join("remote.git");
        assert!(std::process::Command::new("git")
            .args(["init", "--bare", "-q"])
            .arg(&remote)
            .status()
            .unwrap()
            .success());
        std::fs::create_dir_all(&notes).unwrap();
        leo_core::sync::connect(&notes, remote.to_str().unwrap()).unwrap();
        std::fs::write(notes.join("new.md"), "x").unwrap();

        let probe = Probe {
            remote: true,
            ..Probe::default()
        };
        let sections = scan(
            &Config::default(),
            &MemoryStore::default(),
            &notes,
            &config,
            probe,
        );
        let backup = section(&sections, "backup");
        assert!(
            check(backup, "GitHub remote").state.is_ready(),
            "{:?}",
            backup.checks
        );
        assert!(
            matches!(
                check(backup, "uncommitted changes").state,
                State::Warn { .. }
            ),
            "{:?}",
            backup.checks
        );
    }
}
