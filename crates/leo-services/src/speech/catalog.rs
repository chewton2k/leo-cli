use std::collections::BTreeMap;
use std::sync::LazyLock;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_MODEL: &str = "parakeet-tdt-0.6b-v3";
const MOST_MODEL_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Deserialize)]
pub struct Catalog {
    pub models: Vec<Model>,
    pub results: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub description: String,
    pub repository: String,
    pub revision: String,
    pub license: String,
    pub family: String,
    pub languages: Vec<String>,
    pub capabilities: Value,
    pub quant: String,
    pub role: String,
    pub curated: bool,
    pub files: BTreeMap<String, ModelFile>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ModelFile {
    pub filename: String,
    pub size: u64,
    pub sha256: String,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
        serde_json::from_str(include_str!("catalog.json"))
            .expect("the bundled model catalog is valid")
    });
    &CATALOG
}

pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 160
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

impl Catalog {
    pub fn curated(&self) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(|m| m.curated && m.role == "asr")
    }

    pub fn get(&self, id: &str) -> Result<&Model> {
        ensure!(
            safe_name(id),
            "Choose a speech model from Settings → Speech models."
        );
        let model = self.curated().find(|m| m.id == id).context(
            "That speech model is not in the library. Choose one in Settings → Speech models.",
        )?;
        let file = model
            .files
            .get(&model.quant)
            .context("This model has no supported download.")?;
        ensure!(
            safe_name(&file.filename)
                && file.size > 0
                && file.size <= MOST_MODEL_BYTES
                && file.sha256.len() == 64
                && file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && model.revision.len() == 40
                && model.revision.bytes().all(|b| b.is_ascii_hexdigit())
                && model.repository.split('/').count() == 2
                && model.repository.split('/').all(safe_name),
            "This speech model's download information is invalid. Update Leo and try again."
        );
        Ok(model)
    }

    pub fn speed(&self, id: &str) -> f64 {
        let backend = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "accelerated"
        } else {
            "cpu"
        };
        self.results
            .get(id)
            .and_then(|r| r.get("performance"))
            .and_then(|p| p.get(backend))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    }

    pub fn accuracy(&self, id: &str) -> f64 {
        let Some(scores) = self
            .results
            .get(id)
            .and_then(|r| r.get("accuracy"))
            .and_then(Value::as_object)
        else {
            return 0.0;
        };
        let errors: Vec<f64> = scores
            .values()
            .filter_map(|s| s.get("error").and_then(Value::as_f64))
            .collect();
        if errors.is_empty() {
            0.0
        } else {
            (100.0 - errors.iter().sum::<f64>() / errors.len() as f64).max(0.0)
        }
    }

    pub fn live(&self, id: &str) -> bool {
        self.speed(id) >= 2.0
    }
}

impl Model {
    pub fn file(&self) -> &ModelFile {
        &self.files[&self.quant]
    }
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository,
            self.revision,
            self.file().filename
        )
    }
    pub fn recommended(&self) -> bool {
        matches!(
            self.id.as_str(),
            DEFAULT_MODEL | "parakeet-unified-en-0.6b" | "canary-180m-flash"
        )
    }
    pub fn language(&self, language: &str) -> bool {
        (language == "auto" && self.languages.len() > 1)
            || self.languages.iter().any(|l| l == language)
    }
}
