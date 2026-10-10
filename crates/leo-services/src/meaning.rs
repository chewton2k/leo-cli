use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

use crate::providers::{download_verified, files_state, model_state, ModelState};

pub const MODEL_URL: &str =
    "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a";
pub const MODEL_MANIFEST: &str = "config.json=094f8e891b932f2000c92cfc663bac4c62069f5d8af5b5278c4306aef3084750 tokenizer.json=d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66 model.safetensors=3c9f31665447c8911517620762200d2245a2518d6e7208acc78cd9db317e21ad";
pub const MODEL_MB: u32 = 134;
pub const DIMENSIONS: usize = 384;
pub const QUERY: &str = "Represent this sentence for searching relevant passages: ";
const MOST_TOKENS: usize = 512;
const BATCH: usize = 16;

pub fn manifest() -> Vec<(String, String)> {
    let text = std::env::var("LEO_MEANING_MANIFEST").unwrap_or_else(|_| MODEL_MANIFEST.to_string());
    text.split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, sha)| (name.to_string(), sha.to_string()))
        .collect()
}

pub fn dir() -> PathBuf {
    crate::config::choice::models_dir().join("bge-small-en-v1.5")
}

pub fn state() -> ModelState {
    files_state(&dir(), &manifest())
}

pub fn wanted() -> bool {
    std::env::var_os("LEO_INSTALL_NO_MODEL").is_none()
        && std::env::var_os("LEO_NO_MEANING_MODEL").is_none()
        && state() != ModelState::Ready
}

pub fn fetch(quiet: bool) -> Result<PathBuf> {
    let base = std::env::var("LEO_MEANING_URL").unwrap_or_else(|_| MODEL_URL.to_string());
    let dir = dir();
    for (name, sha) in manifest() {
        if model_state(&dir.join(&name), &sha) == ModelState::Ready {
            continue;
        }
        if !quiet {
            println!("  {name}");
        }
        download_verified(&format!("{base}/{name}"), &sha, &dir, &name, quiet)?;
    }
    Ok(dir)
}

pub struct Model {
    bert: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl Model {
    pub fn load(dir: &Path) -> Result<Model> {
        let device = Device::Cpu;
        let config: Config = serde_json::from_str(
            &std::fs::read_to_string(dir.join("config.json"))
                .context("the meaning model has no config.json")?,
        )?;
        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("the meaning model's tokenizer could not be read: {e}"))?;
        tokenizer.with_padding(Some(PaddingParams::default()));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MOST_TOKENS,
                ..TruncationParams::default()
            }))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let weights = std::fs::read(dir.join("model.safetensors"))
            .context("the meaning model has no weights")?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DTYPE, &device)?;
        let bert = BertModel::load(vb, &config)?;
        Ok(Model {
            bert,
            tokenizer,
            device,
        })
    }

    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            let encodings = self
                .tokenizer
                .encode_batch(batch.to_vec(), true)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let ids: Vec<Tensor> = encodings
                .iter()
                .map(|e| Tensor::new(e.get_ids(), &self.device))
                .collect::<candle_core::Result<_>>()?;
            let masks: Vec<Tensor> = encodings
                .iter()
                .map(|e| Tensor::new(e.get_attention_mask(), &self.device))
                .collect::<candle_core::Result<_>>()?;
            let ids = Tensor::stack(&ids, 0)?;
            let masks = Tensor::stack(&masks, 0)?;
            let types = ids.zeros_like()?;
            let hidden = self.bert.forward(&ids, &types, Some(&masks))?;
            let first = hidden.narrow(1, 0, 1)?.squeeze(1)?;
            let norms = first.sqr()?.sum_keepdim(1)?.sqrt()?;
            let unit = first.broadcast_div(&norms)?;
            out.extend(unit.to_vec2::<f32>()?);
        }
        Ok(out)
    }
}

static LOADED: OnceLock<Mutex<Loaded>> = OnceLock::new();
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Default)]
struct Loaded {
    model: Option<Arc<Model>>,
    missed: Option<std::time::Instant>,
}

pub fn model() -> Option<Arc<Model>> {
    let slot = LOADED.get_or_init(|| Mutex::new(Loaded::default()));
    let mut held = slot.lock().ok()?;
    if let Some(model) = held.model.as_ref() {
        return Some(Arc::clone(model));
    }
    if held.missed.is_some_and(|at| at.elapsed() < LOOK_AGAIN) {
        return None;
    }
    let loaded = (state() == ModelState::Ready)
        .then(|| Model::load(&dir()).ok())
        .flatten()
        .map(Arc::new);
    match loaded {
        Some(model) => {
            held.model = Some(Arc::clone(&model));
            Some(model)
        }
        None => {
            held.missed = Some(std::time::Instant::now());
            None
        }
    }
}

pub fn embed(texts: &[String], query: bool) -> Result<Vec<Vec<f32>>> {
    let model = model().context("the meaning model is not downloaded yet")?;
    if query {
        let asked: Vec<String> = texts.iter().map(|t| format!("{QUERY}{t}")).collect();
        model.embed(&asked)
    } else {
        model.embed(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_is_pinned_to_one_commit_and_every_file_has_a_checksum() {
        assert!(MODEL_URL.contains("/resolve/5c38ec7c"));
        let files: Vec<String> = manifest()
            .into_iter()
            .map(|(name, sha)| {
                assert_eq!(sha.len(), 64);
                name
            })
            .collect();
        assert_eq!(
            files,
            ["config.json", "tokenizer.json", "model.safetensors"]
        );
    }

    #[test]
    #[ignore = "needs the downloaded model: leo update fetches it"]
    fn sentences_that_mean_the_same_are_closer_than_ones_that_do_not() {
        let model = Model::load(&dir()).unwrap();
        let v = model
            .embed(&[
                format!("{QUERY}how does breadth first search pick the next vertex?"),
                "BFS takes the oldest discovered node from a FIFO queue.".into(),
                "Photosynthesis turns light into chemical energy in plants.".into(),
            ])
            .unwrap();
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        assert_eq!(v[0].len(), DIMENSIONS);
        assert!((dot(&v[1], &v[1]) - 1.0).abs() < 1e-4);
        assert!(
            dot(&v[0], &v[1]) > dot(&v[0], &v[2]) + 0.1,
            "{} vs {}",
            dot(&v[0], &v[1]),
            dot(&v[0], &v[2])
        );
    }
}
