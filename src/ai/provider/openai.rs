use crate::ai::error::{
    classify_reqwest, classify_status, scrub_secret, ProviderError, ProviderResult,
};
use crate::ai::provider::{ChatProvider, ChatRequest, Sink};
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Any endpoint speaking the OpenAI chat-completions protocol: OpenRouter,
/// Ollama, LM Studio, llama.cpp server, vLLM, Groq chat.
pub struct OpenAiChat {
    name: String,
    base_url: String,
    model: String,
    key: Option<Secret>,
    /// Whether this endpoint needs a key at all. Local servers do not.
    needs_key: bool,
    max_tokens: u32,
}

impl OpenAiChat {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        OpenAiChat {
            name,
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            model: cfg.model.clone().unwrap_or_else(|| "openrouter/free".to_string()),
            key,
            // A provider that names no key_env is a local server needing none.
            needs_key: cfg.key_env.is_some(),
            max_tokens: cfg.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        }
    }
}

impl OpenAiChat {
    /// The request body, shared by the streaming and non-streaming paths so they
    /// cannot drift apart on model or temperature.
    fn body(&self, req: &ChatRequest, stream: bool) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": req.prompt}],
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "stream": stream,
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn authorized(
        &self,
        builder: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        let builder = builder
            .header("Content-Type", "application/json")
            // OpenRouter attribution headers; harmless elsewhere.
            .header("HTTP-Referer", "https://github.com/leo-cli")
            .header("X-Title", "leo");
        match &self.key {
            Some(key) => builder.header("Authorization", format!("Bearer {}", key.as_str())),
            None => builder,
        }
    }
}

/// The two kinds of text a delta can carry.
///
/// Kept apart because they are not interchangeable: `content` is the answer,
/// while `reasoning` is the model thinking aloud. Merging them streamed a
/// model's deliberations into the user's note, which is the bug this split fixes.
#[derive(Debug, PartialEq, Eq)]
enum Delta {
    Content(String),
    Reasoning(String),
}

/// Pull the text out of one SSE `data:` payload.
///
/// Returns `None` for anything that is not text — keep-alives, the terminating
/// `[DONE]`, role-only first chunks — so the caller can ignore them without
/// knowing the shape of the protocol.
fn delta_text(payload: &str) -> Option<Delta> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    let delta = &json["choices"][0]["delta"];

    if let Some(text) = delta["content"].as_str().filter(|s| !s.is_empty()) {
        return Some(Delta::Content(text.to_string()));
    }
    delta["reasoning"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|text| Delta::Reasoning(text.to_string()))
}

impl ChatProvider for OpenAiChat {
    fn complete_streaming(&self, req: &ChatRequest, sink: Sink<'_>) -> ProviderResult<String> {
        use std::io::{BufRead, BufReader};

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let resp = self
            .authorized(client.post(self.endpoint()))
            .json(&self.body(req, true))
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, self.key.as_ref().map(Secret::as_str));
            return Err(classify_status(status, &self.name, &text));
        }

        let mut answer = String::new();
        // Held back rather than shown: only used if no content ever arrives,
        // which is how a reasoning model on a free tier sometimes replies.
        let mut reasoning = String::new();

        let reader = BufReader::new(resp);
        for line in reader.lines() {
            let line = line.map_err(|e| {
                // A stream that dies partway is retryable: the next provider may
                // manage it, and any text already delivered is still useful.
                ProviderError::Retryable(format!("{}: stream ended early: {e}", self.name))
            })?;
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            match delta_text(payload) {
                Some(Delta::Content(fragment)) => {
                    answer.push_str(&fragment);
                    sink(&fragment);
                }
                Some(Delta::Reasoning(fragment)) => reasoning.push_str(&fragment),
                None => {}
            }
        }

        if !answer.trim().is_empty() {
            return Ok(answer);
        }
        // No answer, but the model said something: better than nothing, and the
        // non-streaming path makes the same choice.
        if !reasoning.trim().is_empty() {
            sink(&reasoning);
            return Ok(reasoning);
        }
        Err(ProviderError::Retryable(format!(
            "{}: streamed an empty response",
            self.name
        )))
    }

    fn complete(&self, req: &ChatRequest) -> ProviderResult<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": req.prompt}],
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut request = client
            .post(&url)
            .header("Content-Type", "application/json")
            // OpenRouter attribution headers; harmless elsewhere.
            .header("HTTP-Referer", "https://github.com/leo-cli")
            .header("X-Title", "leo")
            .json(&body);

        if let Some(key) = &self.key {
            request = request.header("Authorization", format!("Bearer {}", key.as_str()));
        }

        let resp = request
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            // Only the response body is quoted — never our request headers,
            // so our own Authorization header cannot leak this way. Scrub
            // defensively in case a misbehaving gateway reflects the key
            // back inside the body itself.
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, self.key.as_ref().map(Secret::as_str));
            return Err(classify_status(status, &self.name, &text));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| ProviderError::Fatal(format!("{}: unreadable response: {e}", self.name)))?;

        let message = &json["choices"][0]["message"];
        let finish = json["choices"][0]["finish_reason"].as_str().unwrap_or("");

        // Prefer `content`. Reasoning models on OpenRouter's free tier often
        // return an empty or null `content` while putting text in `reasoning`,
        // especially when the token budget ran out mid-thought, so fall back to
        // that rather than discarding a usable answer.
        let text = message["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                message["reasoning"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
            });

        match text {
            Some(t) => Ok(t.to_string()),
            // Retryable, not fatal: an empty completion is this provider
            // failing to answer, and the next one in the chain may well do
            // better. Treating it as fatal would abort the whole chain over a
            // truncated reasoning trace.
            None if finish == "length" => Err(ProviderError::Retryable(format!(
                "{}: hit the token limit before producing any content",
                self.name
            ))),
            None => Err(ProviderError::Retryable(format!(
                "{}: returned no content",
                self.name
            ))),
        }
    }

    fn available(&self) -> bool {
        !self.needs_key || self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: no API key (run `leo model login {}`)",
            self.name, self.name
        )
    }

    fn max_tokens(&self) -> Option<u32> {
        Some(self.max_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::ChatProvider;
    use crate::config::secret::{resolve, MemoryStore, SecretStore};

    #[test]
    fn max_bytes_is_not_applicable_but_max_tokens_reflects_config() {
        let cfg = ProviderConfig {
            max_tokens: Some(777),
            ..Default::default()
        };
        let provider = OpenAiChat::new("ollama".to_string(), &cfg, None);
        assert_eq!(ChatProvider::max_tokens(&provider), Some(777));
    }

    #[test]
    fn unavailable_reason_never_contains_the_key_value() {
        let store = MemoryStore::default();
        store.set("openrouter", "sk-super-secret-value").unwrap();
        let key = resolve("openrouter", None, &store);
        let cfg = ProviderConfig {
            key_env: Some("LEO_TEST_UNUSED_KEY_ENV".to_string()),
            ..Default::default()
        };
        let provider = OpenAiChat::new("openrouter".to_string(), &cfg, key);
        assert!(!provider
            .unavailable_reason()
            .contains("sk-super-secret-value"));
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::delta_text;

    use super::Delta;

    #[test]
    fn a_content_delta_yields_its_text() {
        let payload = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        assert_eq!(delta_text(payload), Some(Delta::Content("Hello".into())));
    }

    /// The bug this guards: reasoning and content were merged, so a model's
    /// thinking-aloud was streamed into the user's note.
    #[test]
    fn reasoning_is_kept_separate_from_the_answer() {
        let payload = r#"{"choices":[{"delta":{"reasoning":"let me think"}}]}"#;
        assert_eq!(
            delta_text(payload),
            Some(Delta::Reasoning("let me think".into())),
            "reasoning was reported as answer text"
        );
    }

    /// Everything else in the protocol must be ignored rather than misread as
    /// text: keep-alives, the terminator, and the role-only opening chunk.
    #[test]
    fn protocol_noise_yields_nothing() {
        for payload in [
            "[DONE]",
            "",
            "   ",
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"{"choices":[{"delta":{}}]}"#,
            r#"{"choices":[{"delta":{"content":""}}]}"#,
            r#"{"choices":[]}"#,
            "not json at all",
        ] {
            assert_eq!(delta_text(payload), None, "misread {payload:?}");
        }
    }

    /// Content wins when both are present in one delta.
    #[test]
    fn content_wins_over_reasoning_in_the_same_delta() {
        let payload = r#"{"choices":[{"delta":{"content":"answer","reasoning":"thinking"}}]}"#;
        assert_eq!(delta_text(payload), Some(Delta::Content("answer".into())));
    }

    #[test]
    fn whitespace_only_content_is_kept_since_it_is_part_of_the_text() {
        let payload = r#"{"choices":[{"delta":{"content":" "}}]}"#;
        assert_eq!(delta_text(payload), Some(Delta::Content(" ".into())));
    }

    /// Only content reassembles into the answer; reasoning is discarded when
    /// there is an answer to give.
    #[test]
    fn only_content_deltas_form_the_answer() {
        let stream = [
            r#"{"choices":[{"delta":{"reasoning":"The user asks about moves."}}]}"#,
            r#"{"choices":[{"delta":{"reasoning":" I should be brief."}}]}"#,
            r#"{"choices":[{"delta":{"content":"Ownership"}}]}"#,
            r#"{"choices":[{"delta":{"content":" transfers."}}]}"#,
            "[DONE]",
        ];
        let answer: String = stream
            .iter()
            .filter_map(|p| match delta_text(p) {
                Some(Delta::Content(t)) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(answer, "Ownership transfers.");
        assert!(!answer.contains("user asks"), "reasoning leaked into the answer");
    }

    /// Fragments must concatenate into the whole answer, which is what the
    /// non-streaming path would have returned.
    #[test]
    fn fragments_reassemble_into_the_answer() {
        let stream = [
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"{"choices":[{"delta":{"content":"Own"}}]}"#,
            r#"{"choices":[{"delta":{"content":"ership"}}]}"#,
            r#"{"choices":[{"delta":{"content":" moves."}}]}"#,
            "[DONE]",
        ];
        let whole: String = stream
            .iter()
            .filter_map(|p| match delta_text(p) {
                Some(Delta::Content(t)) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(whole, "Ownership moves.");
    }
}
