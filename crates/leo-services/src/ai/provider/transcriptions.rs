use std::path::Path;

use crate::ai::error::{
    classify_reqwest, classify_status, scrub_secret, ProviderError, ProviderResult,
};
use crate::ai::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

const MAX_BYTES: u64 = 20 * 1024 * 1024;
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct Transcriptions {
    name: String,
    base_url: String,
    model: String,
    path: String,
    key: Option<Secret>,
    needs_key: bool,
}

impl Transcriptions {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        Transcriptions {
            name,
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            model: cfg
                .model
                .clone()
                .unwrap_or_else(|| "gpt-transcribe".to_string()),
            path: cfg
                .path
                .clone()
                .unwrap_or_else(|| "audio/transcriptions".to_string()),
            key,
            needs_key: cfg.key_env.is_some(),
        }
    }

    fn url(&self) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            self.path.trim_start_matches('/')
        )
    }
}

impl TranscribeProvider for Transcriptions {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let file_name = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();
        let bytes = std::fs::read(audio_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: cannot read audio: {e}", self.name)))?;

        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .file_name(file_name)
            .mime_str("audio/wav")
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let form = reqwest::blocking::multipart::Form::new()
            .text("model", self.model.clone())
            .part("file", part);

        let mut request = client.post(self.url()).multipart(form);
        if let Some(key) = &self.key {
            request = request.header("Authorization", format!("Bearer {}", key.as_str()));
        }
        let resp = request
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            // Scrub defensively in case a misbehaving gateway reflects the
            // key back inside the body itself.
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, self.key.as_ref().map(|k| k.as_str()));
            return Err(classify_status(status, &self.name, &text));
        }

        let json: serde_json::Value = resp.json().map_err(|e| {
            ProviderError::Fatal(format!("{}: unreadable response: {e}", self.name))
        })?;

        json["text"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| {
                ProviderError::Fatal(format!("{}: unexpected response shape", self.name))
            })
    }

    fn max_bytes(&self) -> Option<u64> {
        Some(MAX_BYTES)
    }

    fn available(&self) -> bool {
        !self.needs_key || self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: no API key (run `leo doctor`, or type /settings in leo)",
            self.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::{resolve, MemoryStore, SecretStore};

    #[test]
    fn max_bytes_is_twenty_megabytes() {
        let cfg = ProviderConfig::default();
        let provider = Transcriptions::new("groq".to_string(), &cfg, None);
        assert_eq!(provider.max_bytes(), Some(20 * 1024 * 1024));
    }

    /// A key is required only when the provider declares a `key_env`. That is
    /// what lets this kind serve a local `/audio/transcriptions` server, which
    /// needs no credential, using the same code path as Groq.
    #[test]
    fn availability_follows_whether_a_key_is_declared() {
        let keyed = ProviderConfig {
            key_env: Some("GROQ_API_KEY".to_string()),
            ..ProviderConfig::default()
        };
        assert!(!Transcriptions::new("groq".to_string(), &keyed, None).available());

        let store = MemoryStore::default();
        store.set("groq", "a-key").unwrap();
        let key = resolve("groq", None, &store);
        assert!(Transcriptions::new("groq".to_string(), &keyed, key).available());

        // No key_env: a local server, available with no credential at all.
        let local = ProviderConfig {
            base_url: Some("http://localhost:8000/v1".to_string()),
            ..ProviderConfig::default()
        };
        assert!(Transcriptions::new("local".to_string(), &local, None).available());
    }

    #[test]
    fn the_endpoint_follows_base_url_so_any_host_works() {
        let cfg = ProviderConfig {
            base_url: Some("https://api.openai.com/v1/".to_string()),
            ..ProviderConfig::default()
        };
        let p = Transcriptions::new("openai_whisper".to_string(), &cfg, None);
        assert_eq!(p.url(), "https://api.openai.com/v1/audio/transcriptions");

        let bare = Transcriptions::new(
            "openai_whisper".to_string(),
            &ProviderConfig::default(),
            None,
        );
        assert_eq!(bare.url(), "https://api.openai.com/v1/audio/transcriptions");

        let xai = ProviderConfig {
            base_url: Some("https://api.x.ai/v1".to_string()),
            path: Some("/stt".to_string()),
            ..ProviderConfig::default()
        };
        let p = Transcriptions::new("xai_speech".to_string(), &xai, None);
        assert_eq!(p.url(), "https://api.x.ai/v1/stt");
    }

    #[test]
    fn unavailable_reason_never_contains_the_key_value() {
        let store = MemoryStore::default();
        store.set("groq", "sk-super-secret-value").unwrap();
        let key = resolve("groq", None, &store);
        let cfg = ProviderConfig::default();
        let provider = Transcriptions::new("groq".to_string(), &cfg, key);
        assert!(!provider
            .unavailable_reason()
            .contains("sk-super-secret-value"));
    }
}
