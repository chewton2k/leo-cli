use serde::{Deserialize, Serialize};

/// Which wire protocol a provider speaks. Adding a provider that speaks an
/// existing protocol is a config-file change, not a code change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI-compatible chat completions: OpenRouter, Ollama, LM Studio, vLLM.
    Openai,
    #[serde(rename = "openai_transcribe", alias = "groq")]
    Transcriptions,
    /// Local whisper.cpp binary.
    WhisperCpp,
    ChatAudio,
    Parakeet,
}

/// One named provider from `[providers.<name>]`.
///
/// Fields are optional because they are kind-specific; validation happens when
/// the provider is constructed, so an unrelated missing field never blocks load.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub kind: Option<ProviderKind>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Name of the env var holding this provider's key. Never the key itself.
    #[serde(default)]
    pub key_env: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// whisper_cpp only: binary name or path.
    #[serde(default)]
    pub bin: Option<String>,
    /// whisper_cpp only: path to the ggml model file.
    #[serde(default)]
    pub model_path: Option<String>,
    #[serde(default)]
    pub key_from: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub reasoning: Option<bool>,
}

impl ProviderConfig {
    pub fn account<'a>(&'a self, name: &'a str) -> &'a str {
        self.key_from.as_deref().unwrap_or(name)
    }

    pub fn fill_from(&mut self, built_in: ProviderConfig) {
        if self.kind.is_some() && self.kind != built_in.kind {
            return;
        }
        self.kind = self.kind.or(built_in.kind);
        self.base_url = self.base_url.take().or(built_in.base_url);
        self.model = self.model.take().or(built_in.model);
        self.key_env = self.key_env.take().or(built_in.key_env);
        self.max_tokens = self.max_tokens.or(built_in.max_tokens);
        self.bin = self.bin.take().or(built_in.bin);
        self.model_path = self.model_path.take().or(built_in.model_path);
        self.key_from = self.key_from.take().or(built_in.key_from);
        self.path = self.path.take().or(built_in.path);
        self.reasoning = self.reasoning.or(built_in.reasoning);
    }
}

/// An ordered fallback chain for one task, from `[chat]` or `[transcribe]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaskChain {
    #[serde(default)]
    pub chain: Vec<String>,
}
