use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Hold,
    Tap,
    #[default]
    Both,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Popup {
    #[default]
    Live,
    Minimal,
    Off,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Writing {
    #[default]
    Off,
    Quick,
    Selected,
}
const RETIRED: &[&str] = &["closed_microphone", "trailing_space", "enter"];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub shortcut: String,
    pub mode: Mode,
    pub microphone: Option<String>,
    pub custom_words: Vec<String>,
    pub fillers: bool,
    pub sounds: bool,
    pub popup: Popup,
    pub top: bool,
    pub writing: Writing,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: if cfg!(target_os = "macos") {
                "option+space"
            } else {
                "ctrl+space"
            }
            .into(),
            mode: Mode::Both,
            microphone: None,
            custom_words: Vec::new(),
            fillers: true,
            sounds: false,
            popup: Popup::Live,
            top: false,
            writing: Writing::Off,
        }
    }
}
impl Settings {
    pub fn recover(path: &Path) -> (Self, Option<String>) {
        match Self::load(path) { Ok(settings)=>(settings,None), Err(_)=>(Self::default(),Some("Dictation settings could not be read. Review and save the defaults here to repair them.".into())) }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.shortcut.trim().is_empty() && self.shortcut.len() <= 200,
            "Record a shortcut in Dictation settings."
        );
        ensure!(
            self.custom_words.len() <= 200
                && self
                    .custom_words
                    .iter()
                    .all(|w| !w.trim().is_empty() && w.chars().count() <= 200),
            "Use at most 200 custom words, each up to 200 characters."
        );
        ensure!(
            self.microphone.as_ref().is_none_or(|n| n.len() <= 1024),
            "Choose a microphone from the list."
        );
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        ensure!(
            std::fs::symlink_metadata(path)?.is_file() && std::fs::metadata(path)?.len() <= 200_000,
            "Dictation settings could not be opened. Restore dictation.json from a backup."
        );
        let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?).context(
            "Dictation settings could not be read. Restore dictation.json from a backup.",
        )?;
        if let Some(fields) = value.as_object_mut() {
            for retired in RETIRED {
                fields.remove(*retired);
            }
        }
        let value: Self = serde_json::from_value(value).context(
            "Dictation settings could not be read. Restore dictation.json from a backup.",
        )?;
        value.validate()?;
        Ok(value)
    }
    pub fn commit(&self, path: &Path, apply: impl FnOnce() -> Result<()>) -> Result<()> {
        let folder = path.parent().context("Dictation settings need a folder.")?;
        let _lock = crate::private_file::lock(
            folder,
            ".dictation.lock",
            std::time::Duration::from_secs(5),
        )?;
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            ensure!(meta.is_file() && meta.len()<=200_000,"Dictation settings cannot be saved over this file. Restore dictation.json from a backup.");
        }
        let original = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        self.save(path)?;
        if let Err(error) = apply() {
            if let Some(bytes) = original {
                let mut file = tempfile::NamedTempFile::new_in(folder)?;
                file.write_all(&bytes)?;
                file.as_file().sync_all()?;
                file.persist(path).map_err(|e| e.error)?;
            } else {
                std::fs::remove_file(path)?;
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let folder = path.parent().context("Dictation settings need a folder.")?;
        std::fs::create_dir_all(folder)?;
        let mut tmp = tempfile::NamedTempFile::new_in(folder)?;
        tmp.write_all(&serde_json::to_vec_pretty(self)?)?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod recovery_tests {
    #[test]
    fn a_failed_live_rebind_restores_the_previous_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dictation.json");
        let original = super::Settings::default();
        original.save(&path).unwrap();
        let mut changed = original.clone();
        changed.shortcut = "fn".into();
        assert!(changed
            .commit(&path, || {
                assert_eq!(super::Settings::load(&path).unwrap(), changed);
                anyhow::bail!("shortcut unavailable")
            })
            .is_err());
        assert_eq!(super::Settings::load(&path).unwrap(), original);
    }
    #[test]
    fn damaged_preferences_do_not_prevent_the_app_opening_or_get_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dictation.json");
        std::fs::write(&path, "not valid json").unwrap();
        let (settings, problem) = super::Settings::recover(&path);
        assert_eq!(settings, super::Settings::default());
        assert!(problem.is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not valid json");
    }
    #[test]
    fn ai_writing_is_off_unless_chosen_and_older_files_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dictation.json");
        std::fs::write(&path, r#"{"shortcut":"fn","fillers":false}"#).unwrap();
        let settings = super::Settings::load(&path).unwrap();
        assert_eq!(settings.writing, super::Writing::Off);
        let chosen: super::Settings = serde_json::from_str(r#"{"writing":"quick"}"#).unwrap();
        assert_eq!(chosen.writing, super::Writing::Quick);
        assert!(serde_json::from_str::<super::Settings>(r#"{"writing":"loud"}"#).is_err());
    }
    #[test]
    fn files_with_retired_options_load_and_drop_them_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dictation.json");
        std::fs::write(
            &path,
            r#"{"shortcut":"fn","fillers":false,"closed_microphone":"Studio","trailing_space":true,"enter":true}"#,
        )
        .unwrap();
        let settings = super::Settings::load(&path).unwrap();
        assert_eq!(settings.shortcut, "fn");
        assert!(!settings.fillers);
        settings.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            !saved.contains("closed_microphone")
                && !saved.contains("trailing_space")
                && !saved.contains("\"enter\"")
        );
    }
}
