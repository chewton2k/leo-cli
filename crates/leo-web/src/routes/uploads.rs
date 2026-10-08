use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::routes::notes::{directory, save};
use crate::{store_now, AppState, Importer, UploadFile};

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ImportJob {
    pub(crate) state: &'static str,
    pub(crate) label: String,
    pub(crate) dir: String,
    pub(crate) step: String,
    pub(crate) done: usize,
    pub(crate) total: usize,
    pub(crate) note: Option<String>,
    pub(crate) error: Option<String>,
}

pub(crate) const IMPORT_BYTES: usize = 120 * 1024 * 1024;

#[derive(Deserialize)]
pub(crate) struct ImportFileBody {
    pub(crate) name: String,
    #[serde(default, rename = "type")]
    pub(crate) mime: String,
    pub(crate) data: String,
}

#[derive(Deserialize)]
pub(crate) struct ImportBody {
    #[serde(default)]
    pub(crate) directory: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    pub(crate) files: Vec<ImportFileBody>,
}

pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    let cleaned: String = cleaned.chars().take(120).collect();
    if cleaned.is_empty() {
        "upload".to_string()
    } else {
        cleaned
    }
}

pub(crate) fn originals_dir(notes_dir: &std::path::Path, note: &str) -> Option<std::path::PathBuf> {
    if note.is_empty() || !note.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(
        notes_dir
            .parent()
            .unwrap_or(notes_dir)
            .join("attachments")
            .join(note),
    )
}

fn set_job(state: &AppState, id: &str, change: impl FnOnce(&mut ImportJob)) {
    if let Ok(mut jobs) = state.imports.lock() {
        if let Some(job) = jobs.get_mut(id) {
            change(job);
        }
    }
}

fn run_import(
    state: AppState,
    id: String,
    importer: Importer,
    dir: String,
    title: Option<String>,
    files: Vec<UploadFile>,
) {
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    let originals = files.clone();
    let written = importer(files, &mut |step, done, total| {
        set_job(&state, &id, |job| {
            job.step = step.to_string();
            job.done = done;
            job.total = total;
        })
    });
    let (made_title, body) = match written {
        Ok(found) => found,
        Err(e) => {
            set_job(&state, &id, |job| {
                job.state = "failed";
                job.error = Some(e.to_string());
            });
            return;
        }
    };
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| (!made_title.trim().is_empty()).then(|| made_title.trim().to_string()))
        .unwrap_or_else(|| names.first().cloned().unwrap_or_else(|| "Upload".into()));
    let footer = format!(
        "\n\n---\n*From {}, uploaded {}.*",
        names.join(", "),
        chrono::Local::now().format("%b %-d, %Y")
    );
    let made = store_now(&state, |store| {
        if !store.dir_exists(&dir) {
            store.create_dir(&dir);
        }
        let note = store
            .create_note(title, format!("{}{footer}", body.trim()), vec![], &dir)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .id
            .clone();
        save(store)?;
        Ok((note, store.notes_dir.clone()))
    });
    match made {
        Ok((note, notes_dir)) => {
            if let Some(folder) = originals_dir(&notes_dir, &note) {
                if std::fs::create_dir_all(&folder).is_ok() {
                    for file in &originals {
                        let _ =
                            std::fs::write(folder.join(safe_file_name(&file.name)), &file.bytes);
                    }
                }
            }
            set_job(&state, &id, |job| {
                job.state = "done";
                job.done = job.total.max(1);
                job.note = Some(note);
            });
        }
        Err(_) => set_job(&state, &id, |job| {
            job.state = "failed";
            job.error = Some("The note could not be saved.".into());
        }),
    }
}

pub(crate) async fn start_import(
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> Response {
    let Some(importer) = state.importer.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "leo serve was started without AI." })),
        )
            .into_response();
    };
    if body.files.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Choose a file first." })),
        )
            .into_response();
    }
    let dir = body.directory.trim().to_string();
    let checked = state.with_store({
        let dir = dir.clone();
        move |store| directory(store, &dir)
    });
    if checked.await.is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut files = Vec::new();
    for file in body.files {
        use base64::Engine;
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(file.data.as_bytes())
        else {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("{} did not arrive intact; try again.", file.name) }))).into_response();
        };
        files.push(UploadFile {
            name: safe_file_name(&file.name),
            mime: file.mime,
            bytes,
        });
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Ok(mut jobs) = state.imports.lock() {
        jobs.insert(
            id.clone(),
            ImportJob {
                state: "working",
                label: upload_label(&files),
                dir: dir.clone(),
                step: "Uploading".into(),
                done: 0,
                total: 1,
                note: None,
                error: None,
            },
        );
    }
    let worker = state.clone();
    let job = id.clone();
    std::thread::spawn(move || run_import(worker, job, importer, dir, body.title, files));
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "id": id }))).into_response()
}

pub(crate) fn upload_label(files: &[UploadFile]) -> String {
    match files {
        [] => "Making a note".to_string(),
        [one] => format!("Making a note from {}", one.name),
        [first, rest @ ..] => format!("Making a note from {} and {} more", first.name, rest.len()),
    }
}

pub(crate) async fn import_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let job = state
        .imports
        .lock()
        .ok()
        .and_then(|jobs| jobs.get(&id).cloned());
    match job {
        Some(job) => Json(job).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn list_originals(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let Some(folder) = originals_dir(&notes_dir, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut files: Vec<serde_json::Value> = std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| {
                    serde_json::json!({
                        "name": e.file_name().to_string_lossy(),
                        "size": e.metadata().map(|m| m.len()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Json(files).into_response()
}

fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

pub(crate) async fn get_original(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> Response {
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let Some(folder) = originals_dir(&notes_dir, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if name != safe_file_name(&name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match std::fs::read(folder.join(&name)) {
        Ok(bytes) => {
            let ascii: String = name
                .chars()
                .map(|c| {
                    if c.is_ascii_graphic() || c == ' ' {
                        c
                    } else {
                        '_'
                    }
                })
                .filter(|c| *c != '"')
                .collect();
            let mut response = bytes.into_response();
            let headers = response.headers_mut();
            if let Ok(value) = HeaderValue::from_str(mime_for(&name)) {
                headers.insert(header::CONTENT_TYPE, value);
            }
            if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"")) {
                headers.insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
