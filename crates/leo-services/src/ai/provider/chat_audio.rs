use std::path::Path;

use base64::Engine;

use crate::ai::error::{
    classify_reqwest, classify_status, scrub_secret, ProviderError, ProviderResult,
};
use crate::ai::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

const MAX_BYTES: u64 = 14 * 1024 * 1024;
const INSTRUCTION: &str = "Transcribe this recording word for word. Reply with the transcript only: no title, no notes, no timestamps. If nothing is said, reply with nothing.";

pub struct ChatAudioTranscribe {
    name: String,
    base_url: String,
    model: String,
    key: Option<Secret>,
}

impl ChatAudioTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        ChatAudioTranscribe {
            name,
            base_url: cfg.base_url.clone().unwrap_or_else(|| {
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
            }),
            model: cfg
                .model
                .clone()
                .unwrap_or_else(|| "gemini-3.8-flash".to_string()),
            key,
        }
    }

    fn url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn body(&self, audio: &[u8]) -> serde_json::Value {
        let data = base64::engine::general_purpose::STANDARD.encode(audio);
        serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": INSTRUCTION},
                    {"type": "input_audio", "input_audio": {"data": data, "format": "wav"}},
                ],
            }],
        })
    }
}

fn reply_text(json: &serde_json::Value) -> Option<String> {
    let content = &json["choices"][0]["message"]["content"];
    if let Some(text) = content.as_str() {
        return Some(text.trim().to_string());
    }
    if content.is_null() && json["choices"][0]["message"].is_object() {
        return Some(String::new());
    }
    None
}

impl TranscribeProvider for ChatAudioTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let bytes = std::fs::read(audio_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: cannot read audio: {e}", self.name)))?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;
        let mut request = client
            .post(self.url())
            .header("Content-Type", "application/json")
            .json(&self.body(&bytes));
        if let Some(key) = &self.key {
            request = request.header("Authorization", format!("Bearer {}", key.as_str()));
        }
        let resp = request
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, self.key.as_ref().map(|k| k.as_str()));
            return Err(classify_status(status, &self.name, &text));
        }
        let json: serde_json::Value = resp.json().map_err(|e| {
            ProviderError::Retryable(format!("{}: unreadable response: {e}", self.name))
        })?;
        reply_text(&json).ok_or_else(|| {
            ProviderError::Retryable(format!("{}: unexpected response shape", self.name))
        })
    }

    fn max_bytes(&self) -> Option<u64> {
        Some(MAX_BYTES)
    }

    fn available(&self) -> bool {
        self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: no API key (press Ctrl-S in leo, or run `leo doctor`)",
            self.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::{resolve, MemoryStore, SecretStore};

    #[test]
    fn the_audio_travels_inline_as_base64_wav_with_an_instruction() {
        let cfg = ProviderConfig {
            model: Some("gemini-x".to_string()),
            ..ProviderConfig::default()
        };
        let p = ChatAudioTranscribe::new("gemini_speech".to_string(), &cfg, None);
        let body = p.body(b"RIFFdata");
        assert_eq!(body["model"], "gemini-x");
        let parts = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "input_audio");
        assert_eq!(parts[1]["input_audio"]["format"], "wav");
        assert_eq!(parts[1]["input_audio"]["data"], "UklGRmRhdGE=");
    }

    #[test]
    fn the_endpoint_is_chat_completions_under_the_base_url() {
        let p = ChatAudioTranscribe::new(
            "gemini_speech".to_string(),
            &ProviderConfig::default(),
            None,
        );
        assert_eq!(
            p.url(),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
    }

    #[test]
    fn a_reply_is_read_from_the_first_choice_and_silence_is_empty() {
        let json = serde_json::json!({"choices": [{"message": {"content": "  hello there \n"}}]});
        assert_eq!(reply_text(&json).as_deref(), Some("hello there"));
        let silent = serde_json::json!({"choices": [{"message": {"role": "assistant"}}]});
        assert_eq!(reply_text(&silent).as_deref(), Some(""));
        assert_eq!(reply_text(&serde_json::json!({"error": "x"})), None);
    }

    #[test]
    fn it_needs_a_key_and_fits_a_five_minute_segment() {
        let cfg = ProviderConfig::default();
        assert!(!ChatAudioTranscribe::new("g".to_string(), &cfg, None).available());
        let store = MemoryStore::default();
        store.set("gemini", "k").unwrap();
        let key = resolve("gemini", None, &store);
        let p = ChatAudioTranscribe::new("g".to_string(), &cfg, key);
        assert!(p.available());
        let five_minutes = 300 * 16_000 * 2 + 44;
        assert!(p.max_bytes().unwrap() > five_minutes);
        assert!(p.max_bytes().unwrap() * 4 / 3 < 20 * 1024 * 1024);
    }
}
