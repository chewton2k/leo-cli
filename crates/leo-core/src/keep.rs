use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const TRASH_CHOICES: [Option<u32>; 5] = [Some(7), Some(30), Some(90), Some(365), None];
pub const CHAT_CHOICES: [Option<u32>; 4] = [Some(30), Some(90), Some(365), None];
pub const DEFAULT_TRASH_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keep {
    #[serde(default = "default_trash_days")]
    pub trash_days: Option<u32>,
    #[serde(default)]
    pub chat_days: Option<u32>,
}

fn default_trash_days() -> Option<u32> {
    Some(DEFAULT_TRASH_DAYS)
}

impl Default for Keep {
    fn default() -> Self {
        Keep {
            trash_days: default_trash_days(),
            chat_days: None,
        }
    }
}

pub fn path_for(notes_dir: &Path) -> PathBuf {
    notes_dir.parent().unwrap_or(notes_dir).join("keep.json")
}

pub fn load(notes_dir: &Path) -> Keep {
    std::fs::read_to_string(path_for(notes_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(notes_dir: &Path, keep: &Keep) -> Result<()> {
    if !TRASH_CHOICES.contains(&keep.trash_days) || !CHAT_CHOICES.contains(&keep.chat_days) {
        anyhow::bail!("that is not one of the choices");
    }
    let path = path_for(notes_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(keep)?)
        .with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn kept_for(days: Option<u32>) -> String {
    match days {
        None => "until you empty it".to_string(),
        Some(_) => format!("for {}", describe(days)),
    }
}

pub fn describe(days: Option<u32>) -> String {
    match days {
        None => "forever".to_string(),
        Some(365) => "a year".to_string(),
        Some(1) => "a day".to_string(),
        Some(n) => format!("{n} days"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_saved_means_thirty_days_of_trash_and_chats_kept_forever() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        assert_eq!(load(&notes), Keep::default());
        assert_eq!(Keep::default().trash_days, Some(30));
        assert_eq!(Keep::default().chat_days, None);
        std::fs::write(path_for(&notes), "{}").unwrap();
        assert_eq!(
            load(&notes),
            Keep::default(),
            "missing keys take the defaults"
        );
        std::fs::write(path_for(&notes), "not json").unwrap();
        assert_eq!(load(&notes), Keep::default());
    }

    #[test]
    fn a_choice_is_saved_beside_the_notes_folder_and_odd_values_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        let keep = Keep {
            trash_days: None,
            chat_days: Some(90),
        };
        save(&notes, &keep).unwrap();
        assert_eq!(load(&notes), keep);
        assert_eq!(path_for(&notes), tmp.path().join("keep.json"));
        assert!(save(
            &notes,
            &Keep {
                trash_days: Some(3),
                chat_days: None
            }
        )
        .is_err());
        assert!(save(
            &notes,
            &Keep {
                trash_days: Some(30),
                chat_days: Some(7)
            }
        )
        .is_err());
        assert_eq!(load(&notes), keep);
    }

    #[test]
    fn choices_read_like_speech() {
        assert_eq!(describe(None), "forever");
        assert_eq!(describe(Some(365)), "a year");
        assert_eq!(describe(Some(30)), "30 days");
        assert_eq!(kept_for(Some(30)), "for 30 days");
        assert_eq!(kept_for(None), "until you empty it");
    }
}
