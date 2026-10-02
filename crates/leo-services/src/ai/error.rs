use std::fmt;

/// Whether a provider failure should advance the chain or stop it.
///
/// Retryable means "this provider cannot serve the request right now" — try
/// the next one. Fatal means the request itself is wrong, so trying another
/// provider would fail identically and hide the real cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    Retryable(String),
    Fatal(String),
}

pub type ProviderResult<T> = std::result::Result<T, ProviderError>;

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Retryable(m) | ProviderError::Fatal(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl ProviderError {
    /// Used by this module's tests and by the chain-status UI in the follow-on
    /// TUI plan; the chain runner itself matches on the variants directly, so
    /// these accessors are not called in the current binary.
    #[allow(dead_code)]
    pub fn is_retryable(&self) -> bool {
        matches!(self, ProviderError::Retryable(_))
    }

    #[allow(dead_code)]
    pub fn message(&self) -> &str {
        match self {
            ProviderError::Retryable(m) | ProviderError::Fatal(m) => m,
        }
    }
}

/// Cap on how much of a provider's response body is quoted in an error.
const MAX_BODY_CHARS: usize = 400;

fn truncate(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX_BODY_CHARS {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(MAX_BODY_CHARS).collect();
    format!("{head}…")
}

/// Classify an HTTP status into retryable or fatal.
///
/// Only the response body is quoted — never a request header, so *our own*
/// Authorization header can never reach an error message this way. That is
/// narrower than "a key cannot leak": a gateway that reflects request
/// headers into a debug body could still put a key in the text passed here.
/// Callers holding a key scrub it from the body via `scrub_secret` before
/// calling this function, as defense in depth against that case.
pub fn classify_status(status: u16, provider: &str, body: &str) -> ProviderError {
    let msg = format!("{provider} returned {status}: {}", truncate(body));
    match status {
        402 | 408 | 429 => ProviderError::Retryable(msg),
        500..=599 => ProviderError::Retryable(msg),
        _ => ProviderError::Fatal(msg),
    }
}

/// A transport-level failure. A refused connection is the normal signal that a
/// local provider (Ollama, LM Studio) simply is not running, so it is
/// retryable rather than fatal.
pub fn classify_reqwest(provider: &str, e: &reqwest::Error) -> ProviderError {
    let msg = format!("{provider}: {e}");
    if e.is_timeout() || e.is_connect() || e.is_request() {
        ProviderError::Retryable(msg)
    } else {
        ProviderError::Fatal(msg)
    }
}

fn key_place(provider: &str) -> (String, Option<String>) {
    use crate::config::edit::Task;
    match crate::config::choice::find(Task::Chat, provider)
        .or_else(|| crate::config::choice::find(Task::Transcribe, provider))
    {
        Some(choice) => (
            choice.name.to_string(),
            Some(format!(
                "/settings, then Enter on the {} key row",
                choice.name
            )),
        ),
        None => (provider.to_string(), None),
    }
}

fn about_the_key(status: u16, body: &str) -> bool {
    let body = body.to_lowercase();
    match status {
        401 | 403 => true,
        400 => body.contains("api key") || body.contains("api_key"),
        _ => false,
    }
}

pub fn classify_status_with_key(status: u16, provider: &str, body: &str) -> ProviderError {
    if !about_the_key(status, body) {
        return classify_status(status, provider, body);
    }
    let (name, place) = key_place(provider);
    let what = if status == 403 {
        format!(
            "{name} refused the key stored in leo ({status}): it may be wrong, out of credit, or not allowed this model."
        )
    } else {
        format!("{name} rejected the key stored in leo ({status}).")
    };
    let fix = match place {
        Some(place) => format!(" To replace it: {place}."),
        None => String::new(),
    };
    ProviderError::Fatal(format!("{what}{fix} It said: {}", truncate(body)))
}

/// Defense in depth for `classify_status`: replace every occurrence of
/// `secret` in `body` with `…` before it is quoted in an error. Our own
/// request headers are never echoed back as `body` (see `classify_status`),
/// but a misbehaving gateway that reflects request headers into a 4xx debug
/// payload could still put the key text in the response body itself — this
/// is the guard against that. A pure string op; makes no network call.
pub fn scrub_secret(body: &str, secret: Option<&str>) -> String {
    match secret {
        Some(s) if !s.is_empty() => body.replace(s, "…"),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_and_rate_limit_errors_are_retryable() {
        // 402 is the HF credits-exhausted case that today's hardcoded
        // fallback special-cases.
        assert!(classify_status(402, "hf", "no credits").is_retryable());
        assert!(classify_status(429, "groq", "slow down").is_retryable());
        assert!(classify_status(408, "hf", "timeout").is_retryable());
    }

    #[test]
    fn server_errors_are_retryable() {
        for status in [500, 502, 503, 504] {
            assert!(
                classify_status(status, "openrouter", "oops").is_retryable(),
                "{status} should be retryable"
            );
        }
    }

    #[test]
    fn client_errors_are_fatal() {
        // A malformed request fails identically on every provider, so
        // advancing the chain would only hide the cause.
        assert!(!classify_status(400, "openrouter", "bad request").is_retryable());
        assert!(!classify_status(401, "openrouter", "bad key").is_retryable());
        assert!(!classify_status(403, "openrouter", "forbidden").is_retryable());
        assert!(!classify_status(404, "openrouter", "no model").is_retryable());
    }

    #[test]
    fn error_message_names_the_provider_and_status() {
        let e = classify_status(401, "openrouter", "invalid api key");
        let msg = e.message();
        assert!(msg.contains("openrouter"), "missing provider: {msg}");
        assert!(msg.contains("401"), "missing status: {msg}");
    }

    #[test]
    fn a_rejected_key_says_where_to_replace_it() {
        let openrouter = classify_status_with_key(
            401,
            "openrouter",
            r#"{"error":{"message":"Missing Authentication header","code":401}}"#,
        );
        assert!(!openrouter.is_retryable());
        assert!(
            openrouter.message().starts_with(
                "OpenRouter rejected the key stored in leo (401). To replace it: /settings, then Enter on the OpenRouter key row. It said: "
            ),
            "{}",
            openrouter.message()
        );
        let gemini = classify_status_with_key(
            400,
            "gemini_speech",
            r#"{"error":{"code":400,"message":"Please pass a valid API key","status":"INVALID_ARGUMENT"}}"#,
        );
        assert!(gemini
            .message()
            .starts_with("Gemini rejected the key stored in leo (400)."));
        assert!(gemini.message().contains("Gemini key row"));
        let xai = classify_status_with_key(
            400,
            "xai",
            r#"{"code":"invalid-argument","error":"Incorrect API key provided."}"#,
        );
        assert!(xai.message().starts_with("xAI rejected the key"));
        let whisper = classify_status_with_key(401, "openai_whisper", "Incorrect API key provided");
        assert!(
            whisper.message().contains("OpenAI key row"),
            "{}",
            whisper.message()
        );
        let forbidden = classify_status_with_key(403, "anthropic", "forbidden");
        assert!(forbidden.message().contains("out of credit"));
    }

    #[test]
    fn other_refusals_and_custom_providers_keep_the_plain_message() {
        let bad_model = classify_status_with_key(400, "openai", "model `x` does not exist");
        assert_eq!(
            bad_model,
            classify_status(400, "openai", "model `x` does not exist")
        );
        let busy = classify_status_with_key(429, "openrouter", "slow down");
        assert!(busy.is_retryable());
        let mine = classify_status_with_key(401, "my-provider", "bad key");
        assert!(mine
            .message()
            .starts_with("my-provider rejected the key stored in leo (401). It said:"));
    }

    #[test]
    fn long_bodies_are_truncated() {
        let body = "x".repeat(5000);
        let e = classify_status(500, "groq", &body);
        assert!(
            e.message().len() < 1000,
            "body should be truncated, got {} chars",
            e.message().len()
        );
    }

    #[test]
    fn scrub_secret_removes_the_key_from_a_reflected_body() {
        let body = "debug: header Authorization: Bearer sk-secret-value was sent upstream";
        let scrubbed = scrub_secret(body, Some("sk-secret-value"));
        assert!(!scrubbed.contains("sk-secret-value"), "got: {scrubbed}");
        assert!(scrubbed.contains('…'));
    }

    #[test]
    fn scrub_secret_is_a_no_op_without_a_key() {
        assert_eq!(scrub_secret("plain body", None), "plain body");
        assert_eq!(scrub_secret("plain body", Some("")), "plain body");
    }
}
