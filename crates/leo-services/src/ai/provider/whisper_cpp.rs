use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::audio::expand_tilde;
use crate::ai::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;

pub struct WhisperCppTranscribe {
    name: String,
    bin: String,
    model_path: PathBuf,
}

impl WhisperCppTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig) -> Self {
        WhisperCppTranscribe {
            name,
            bin: cfg
                .bin
                .clone()
                .filter(|b| !b.trim().is_empty())
                .unwrap_or_else(|| "whisper-cli".to_string()),
            model_path: expand_tilde(cfg.model_path.as_deref().unwrap_or("")),
        }
    }

    pub fn binary_on_path(bin: &str) -> bool {
        if bin.contains('/') {
            return Path::new(bin).is_file();
        }
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| {
            dir.join(bin).is_file() || (cfg!(windows) && dir.join(format!("{bin}.exe")).is_file())
        })
    }
}

impl TranscribeProvider for WhisperCppTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let output = Command::new(&self.bin)
            .arg("-m")
            .arg(&self.model_path)
            .arg("-f")
            .arg(audio_path)
            .arg("-nt")
            .arg("-np")
            .output()
            .map_err(|e| {
                ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Retryable(format!(
                "{}: {} exited with {}: {}",
                self.name,
                self.bin,
                output.status,
                stderr.trim()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err(ProviderError::Retryable(format!(
                "{}: produced no output",
                self.name
            )));
        }
        Ok(text)
    }

    fn max_bytes(&self) -> Option<u64> {
        None
    }

    fn available(&self) -> bool {
        self.model_path.is_file() && Self::binary_on_path(&self.bin)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        if !Self::binary_on_path(&self.bin) {
            format!("{}: `{}` not on PATH", self.name, self.bin)
        } else {
            format!(
                "{}: model file not found at {}",
                self.name,
                self.model_path.display()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_named_program_and_its_model_must_both_exist() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let with = |bin: &str, model: &str| ProviderConfig {
            bin: Some(bin.to_string()),
            model_path: Some(model.to_string()),
            ..Default::default()
        };
        let path = model.path().to_string_lossy().to_string();
        assert!(WhisperCppTranscribe::new("w".into(), &with("sh", &path)).available());
        let missing = WhisperCppTranscribe::new("w".into(), &with("leo-not-a-real-binary", &path));
        assert!(!missing.available());
        assert!(missing.unavailable_reason().contains("not on PATH"));
        let no_model = WhisperCppTranscribe::new("w".into(), &with("sh", "/nonexistent.bin"));
        assert!(!no_model.available());
        assert_eq!(no_model.max_bytes(), None);
    }
}
