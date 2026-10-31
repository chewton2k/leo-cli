use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::catalog::{catalog, Catalog, DEFAULT_MODEL};

const MOST_DOWNLOADS: usize = 2;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3600);
const LOCK_WAIT: Duration = Duration::from_secs(5);
const CANCEL_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Selection {
    pub id: String,
    pub language: String,
}

#[derive(Clone, Debug)]
pub struct Selected {
    pub id: String,
    pub language: String,
    pub path: PathBuf,
    pub streaming: bool,
    pub live: bool,
}

#[derive(Clone)]
struct Download {
    cancel: Arc<AtomicBool>,
    bytes: u64,
    total: u64,
    state: &'static str,
    error: Option<String>,
}

type Verified = BTreeMap<PathBuf, (u64, SystemTime)>;

#[derive(Clone)]
pub struct Library {
    models: PathBuf,
    config: PathBuf,
    catalog: Arc<Catalog>,
    jobs: Arc<Mutex<BTreeMap<String, Download>>>,
    verified: Arc<Mutex<Verified>>,
}

pub fn library() -> Result<&'static Library> {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    if let Some(library) = LIBRARY.get() {
        return Ok(library);
    }
    let value = Library::new(
        crate::config::choice::models_dir().join("speech"),
        leo_core::paths::config_dir()?,
    );
    Ok(LIBRARY.get_or_init(|| value))
}

fn safe_root(path: &Path) -> Result<()> {
    let home = dirs::home_dir();
    let temporary = std::env::temp_dir();
    for ancestor in path.ancestors().take(64) {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            ensure!(
                !meta.file_type().is_symlink() && meta.is_dir(),
                "The speech model and settings folders cannot use symbolic links."
            );
        }
        if home.as_deref() == Some(ancestor) || ancestor == temporary {
            break;
        }
    }
    Ok(())
}

impl Library {
    pub fn new(models: PathBuf, config: PathBuf) -> Self {
        Self::with_catalog(models, config, Arc::new(catalog().clone()))
    }

    pub(crate) fn with_catalog(models: PathBuf, config: PathBuf, catalog: Arc<Catalog>) -> Self {
        Self {
            models,
            config,
            catalog,
            jobs: Default::default(),
            verified: Default::default(),
        }
    }

    pub fn models_dir(&self) -> &Path {
        &self.models
    }

    fn folders(&self) -> Result<()> {
        safe_root(&self.models)?;
        safe_root(&self.config)?;
        crate::private_file::directory(&self.models)?;
        crate::private_file::directory(&self.config)?;
        Ok(())
    }

    pub fn model_path(&self, id: &str) -> Result<PathBuf> {
        safe_root(&self.models)?;
        let model = self.catalog.get(id)?;
        if let Ok(meta) = std::fs::symlink_metadata(&self.models) {
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "The speech model folder cannot be a symbolic link."
            );
        }
        leo_core::paths::contained_path(&self.models, Path::new(&model.file().filename))
    }

    fn selection_path(&self) -> Result<PathBuf> {
        safe_root(&self.config)?;
        leo_core::paths::contained_path(&self.config, Path::new("speech-model.json"))
    }

    pub fn selection(&self) -> Result<Selection> {
        let path = self.selection_path()?;
        if !path.exists() {
            return Ok(Selection {
                id: DEFAULT_MODEL.into(),
                language: "auto".into(),
            });
        }
        let meta = std::fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_file() && meta.len() <= 4096,
            "The speech model choice could not be read. Choose a model again in Settings."
        );
        let selected: Selection = serde_json::from_reader(std::fs::File::open(&path)?.take(4097))
            .context(
            "The speech model choice could not be read. Choose a model again in Settings.",
        )?;
        if selected.id.is_empty() {
            return Ok(selected);
        }
        let model = self.catalog.get(&selected.id)?;
        ensure!(
            model.language(&selected.language),
            "Choose a language supported by this speech model."
        );
        Ok(selected)
    }

    fn verify(&self, id: &str) -> Result<PathBuf> {
        let model = self.catalog.get(id)?;
        let path = self.model_path(id)?;
        let meta = std::fs::symlink_metadata(&path)
            .context("Download a model in Settings → Speech models. Your recorded audio is kept until one is ready.")?;
        ensure!(meta.is_file() && meta.len() == model.file().size,
            "This speech model is incomplete. Delete it and download it again in Settings → Speech models.");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                meta.nlink() == 1,
                "The speech model cannot have hard links. Download it again."
            );
        }
        let stamp = (meta.len(), meta.modified()?);
        if self
            .verified
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path)
            == Some(&stamp)
        {
            return Ok(path);
        }
        let actual = crate::providers::sha256_of(&path)?;
        ensure!(actual.eq_ignore_ascii_case(&model.file().sha256), "This speech model is damaged. Delete it and download it again in Settings → Speech models.");
        self.verified
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.clone(), stamp);
        Ok(path)
    }

    pub fn selected(&self) -> Result<Selected> {
        let selection = self.selection()?;
        ensure!(!selection.id.is_empty(), "Choose and download a model in Settings → Speech models. Your audio is kept until one is ready.");
        let model = self.catalog.get(&selection.id)?;
        Ok(Selected {
            path: self.verify(&selection.id)?,
            live: self.catalog.live(&selection.id),
            streaming: model.capabilities["streaming"].as_bool().unwrap_or(false),
            id: selection.id,
            language: selection.language,
        })
    }

    pub fn use_model(&self, id: &str, language: &str) -> Result<()> {
        let model = self.catalog.get(id)?;
        ensure!(
            model.language(language),
            "Choose a language supported by this speech model."
        );
        self.folders()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        self.verify(id)?;
        let selection = Selection {
            id: id.into(),
            language: language.into(),
        };
        let mut tmp = tempfile::NamedTempFile::new_in(&self.config)?;
        serde_json::to_writer(&mut tmp, &selection)?;
        tmp.as_file().sync_all()?;
        tmp.persist(self.selection_path()?).map_err(|e| e.error)?;
        self.remove_legacy()?;
        Ok(())
    }

    fn remove_legacy(&self) -> Result<()> {
        const MOST_LEGACY_FILES: usize = 64;
        let old = self
            .models
            .parent()
            .unwrap_or(&self.models)
            .join("parakeet-tdt-0.6b-v3-int8");
        if let Ok(meta) = std::fs::symlink_metadata(&old) {
            if meta.is_dir() && !meta.file_type().is_symlink() {
                let files = std::fs::read_dir(&old)?
                    .take(MOST_LEGACY_FILES + 1)
                    .collect::<std::io::Result<Vec<_>>>()?;
                ensure!(files.len() <= MOST_LEGACY_FILES && files.iter().all(|e| e.file_type().is_ok_and(|t| t.is_file())),
                    "The new model is ready, but the old speech folder contains unexpected files. Remove it in Storage.");
                for file in files {
                    std::fs::remove_file(file.path())?;
                }
                std::fs::remove_dir(old)?;
            }
        }
        Ok(())
    }

    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        self.catalog.get(id)?;
        self.folders()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        ensure!(
            !self
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .is_some_and(|j| j.state == "downloading"),
            "Cancel this model's download before deleting it."
        );
        let path = self.model_path(id)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        self.verified
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&path);
        if self.selection()?.id == id {
            let mut tmp = tempfile::NamedTempFile::new_in(&self.config)?;
            serde_json::to_writer(
                &mut tmp,
                &Selection {
                    id: String::new(),
                    language: "auto".into(),
                },
            )?;
            tmp.as_file().sync_all()?;
            tmp.persist(self.selection_path()?).map_err(|e| e.error)?;
        }
        Ok(())
    }

    pub fn view(&self) -> Result<Value> {
        let selection = self.selection()?;
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut disk_bytes = 0;
        let mut models = Vec::new();
        for model in self.catalog.curated() {
            let path = self.model_path(&model.id)?;
            let bytes = std::fs::symlink_metadata(&path)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .unwrap_or(0);
            disk_bytes += bytes;
            let downloaded = bytes > 0;
            let verified = downloaded && self.verify(&model.id).is_ok();
            let job = jobs.get(&model.id);
            models.push(json!({ "id": model.id, "name": model.name, "description": model.description,
                "languages": model.languages, "size": model.file().size, "bytes": bytes, "downloaded": downloaded, "verified": verified,
                "in_use": verified && selection.id == model.id, "streaming": model.capabilities["streaming"],
                "live": self.catalog.live(&model.id), "recommended": model.recommended(), "license": model.license,
                "speed": self.catalog.speed(&model.id), "accuracy": self.catalog.accuracy(&model.id),
                "progress": job.map(|j| json!({ "bytes": j.bytes, "total": j.total, "state": j.state, "error": j.error })) }));
        }
        Ok(
            json!({ "selected": selection, "ready": self.selected().is_ok(), "models": models, "disk_bytes": disk_bytes }),
        )
    }

    pub fn download(&self, id: &str) -> Result<()> {
        let model = self.catalog.get(id)?;
        let url = model.url();
        let total = model.file().size;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(
                !jobs.get(id).is_some_and(|j| j.state == "downloading"),
                "This model is already downloading."
            );
            ensure!(
                jobs.values().filter(|j| j.state == "downloading").count() < MOST_DOWNLOADS,
                "Let a model download finish or cancel one first."
            );
            jobs.insert(
                id.into(),
                Download {
                    cancel: cancel.clone(),
                    bytes: 0,
                    total,
                    state: "downloading",
                    error: None,
                },
            );
        }
        let library = self.clone();
        let id = id.to_owned();
        std::thread::spawn(move || {
            let result = library.fetch(&id, &url, &cancel, &|bytes, _| {
                if let Some(job) = library
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&id)
                {
                    job.bytes = bytes;
                }
            });
            let mut jobs = library.jobs.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(job) = jobs.get_mut(&id) {
                job.state = if result.is_ok() {
                    "ready"
                } else if cancel.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "failed"
                };
                job.error = result.err().map(|e| e.to_string());
            }
        });
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        self.catalog.get(id)?;
        if let Some(job) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            job.cancel.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub(crate) fn fetch(
        &self,
        id: &str,
        url: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64, u64),
    ) -> Result<()> {
        let model = self.catalog.get(id)?;
        self.folders()?;
        let target = self.model_path(id)?;
        let mut tmp = tempfile::NamedTempFile::new_in(&self.models)?;
        tokio::runtime::Runtime::new()?.block_on(async {
            #[cfg(not(test))]
            let client = crate::web::model_client(DOWNLOAD_TIMEOUT)?;
            #[cfg(test)]
            let client = reqwest::Client::builder().no_proxy().timeout(DOWNLOAD_TIMEOUT).build()?;
            let transfer = async {
                let mut response = client.get(url).send().await.context("The model download could not start. Check your connection and try again.")?
                    .error_for_status().context("The model download failed. Check your connection and try again.")?;
                ensure!(response.content_length().is_none_or(|n| n == model.file().size), "The model download has the wrong size. Try again.");
                let mut hash = Sha256::new();
                let mut bytes = 0u64;
                while let Some(chunk) = response.chunk().await.context("The model download was interrupted. Try again.")? {
                    ensure!(!cancel.load(Ordering::Acquire), "The model download was cancelled.");
                    bytes += chunk.len() as u64;
                    ensure!(bytes <= model.file().size, "The model download is too large. Try again.");
                    tmp.write_all(&chunk)?;
                    hash.update(&chunk);
                    progress(bytes, model.file().size);
                }
                ensure!(bytes == model.file().size && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&model.file().sha256),
                    "The model download was damaged. Try downloading it again.");
                Ok::<(), anyhow::Error>(())
            };
            tokio::select! {
                result = transfer => result,
                _ = async { loop { if cancel.load(Ordering::Acquire) { break; } tokio::time::sleep(CANCEL_POLL).await; } } => bail!("The model download was cancelled."),
            }
        })?;
        tmp.as_file().sync_all()?;
        let _lock = crate::private_file::lock(&self.config, ".speech.lock", LOCK_WAIT)?;
        ensure!(
            !cancel.load(Ordering::Acquire),
            "The model download was cancelled."
        );
        ensure!(
            target == self.model_path(id)?,
            "The model download location changed. Try again."
        );
        tmp.persist(&target).map_err(|e| e.error)?;
        self.verified
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&target);
        self.remove_legacy()?;
        Ok(())
    }

    pub fn change(&self, change: &Value) -> Result<Value> {
        let id = change["id"].as_str().context("Choose a speech model.")?;
        match change["action"].as_str() {
            Some("download") => self.download(id)?,
            Some("cancel") => self.cancel(id)?,
            Some("use") => self.use_model(id, change["language"].as_str().unwrap_or("auto"))?,
            Some("delete") => {
                let library = self.clone();
                let id = id.to_owned();
                let path = self.model_path(&id)?;
                return super::engine::engine().edit_model(
                    path,
                    self.selection()?.id == id,
                    move || {
                        library.delete(&id)?;
                        library.view()
                    },
                );
            }
            _ => bail!("Choose Download, Cancel, Use or Delete."),
        }
        self.view()
    }
}
