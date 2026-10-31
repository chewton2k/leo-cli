use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leo_core::attachments;
use serde::Deserialize;

use crate::routes::notes::{directory, save};
use crate::{store_now, AppState, Figure, Importer, Made, UploadFile};

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ImportJob {
    pub(crate) state: &'static str,
    pub(crate) label: String,
    pub(crate) dir: String,
    pub(crate) step: String,
    pub(crate) done: usize,
    pub(crate) total: usize,
    pub(crate) note: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) notes: Vec<String>,
    pub(crate) error: Option<String>,
}

pub(crate) const IMPORT_BYTES: usize = 120 * 1024 * 1024;
const MOST_UPLOAD_FILES: usize = 20;
const MOST_UPLOAD_BYTES: usize = 90 * 1024 * 1024;
const MOST_IMPORTS: usize = 2;
const KEPT_IMPORTS: usize = 40;
const IMPORT_READ_TIME: std::time::Duration = std::time::Duration::from_secs(30);
pub(crate) struct ImportSlots {
    readers: std::sync::Arc<tokio::sync::Semaphore>,
    checks: std::sync::Arc<tokio::sync::Semaphore>,
}

impl Default for ImportSlots {
    fn default() -> Self {
        Self {
            readers: std::sync::Arc::new(tokio::sync::Semaphore::new(MOST_IMPORTS)),
            checks: std::sync::Arc::new(tokio::sync::Semaphore::new(MOST_IMPORTS)),
        }
    }
}

fn import_busy() -> Response {
    (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "error": "Two uploads are already being prepared. Wait for one to finish, then try again." }))).into_response()
}

pub(crate) async fn import_limit(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Ok(_permit) = std::sync::Arc::clone(&state.import_slots.readers).try_acquire_owned() else {
        return import_busy();
    };
    match tokio::time::timeout(IMPORT_READ_TIME, next.run(request)).await {
        Ok(response) => response,
        Err(_) => (StatusCode::REQUEST_TIMEOUT, Json(serde_json::json!({ "error": "The upload took too long to arrive. Check your connection and try again." }))).into_response(),
    }
}

fn reserve_import(
    jobs: &mut std::collections::HashMap<String, ImportJob>,
    id: String,
    job: ImportJob,
) -> bool {
    if jobs.values().filter(|job| job.state == "working").count() >= MOST_IMPORTS {
        return false;
    }
    if jobs.len() >= KEPT_IMPORTS {
        let finished = jobs
            .iter()
            .find(|(_, job)| job.state != "working")
            .map(|(id, _)| id.clone());
        if let Some(id) = finished {
            jobs.remove(&id);
        } else {
            return false;
        }
    }
    jobs.insert(id, job);
    true
}

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
    #[serde(default)]
    pub(crate) wants: String,
    pub(crate) files: Vec<ImportFileBody>,
    #[serde(default)]
    pub(crate) each: bool,
}

pub(crate) const MOST_WANTS: usize = 2_000;

pub fn safe_file_name(name: &str) -> String {
    let cleaned = stored_file_name(name);
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        "upload".to_string()
    } else if leo_core::filename::reserved_on_windows(cleaned) {
        let (stem, rest) = cleaned.split_at(cleaned.find('.').unwrap_or(cleaned.len()));
        format!("{stem}-{rest}")
    } else {
        cleaned.to_string()
    }
}

fn stored_file_name(name: &str) -> String {
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
    let data = notes_dir.parent().unwrap_or(notes_dir);
    leo_core::paths::contained_path(data, &std::path::Path::new("attachments").join(note)).ok()
}

pub(crate) fn original_path(
    notes_dir: &std::path::Path,
    note: &str,
    name: &str,
) -> Option<std::path::PathBuf> {
    if name.is_empty() || name != stored_file_name(name) {
        return None;
    }
    originals_dir(notes_dir, note)?;
    let relative = std::path::Path::new("attachments").join(note).join(name);
    leo_core::paths::contained_path(notes_dir.parent().unwrap_or(notes_dir), &relative).ok()
}

fn stage_originals(
    files: &[UploadFile],
) -> std::io::Result<Vec<(String, tempfile::NamedTempFile)>> {
    use std::io::{Seek, Write};
    files
        .iter()
        .map(|file| {
            let mut staged = tempfile::NamedTempFile::new()?;
            staged.write_all(&file.bytes)?;
            staged.rewind()?;
            Ok((safe_file_name(&file.name), staged))
        })
        .collect()
}

fn save_original(
    notes_dir: &std::path::Path,
    note: &str,
    name: &str,
    staged: &mut tempfile::NamedTempFile,
) -> std::io::Result<()> {
    let invalid =
        || std::io::Error::other("Original files must stay inside the attachments folder");
    let folder = originals_dir(notes_dir, note).ok_or_else(invalid)?;
    std::fs::create_dir_all(&folder)?;
    let path = original_path(notes_dir, note, name).ok_or_else(invalid)?;
    let mut out = tempfile::NamedTempFile::new_in(&folder)?;
    std::io::copy(staged.as_file_mut(), &mut out)?;
    out.as_file().sync_all()?;
    original_path(notes_dir, note, name).ok_or_else(invalid)?;
    out.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn read_original(notes_dir: &std::path::Path, note: &str, name: &str) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let path = original_path(notes_dir, note, name).ok_or_else(|| {
        std::io::Error::other("Original files must stay inside the attachments folder")
    })?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > IMPORT_BYTES as u64 {
        return Err(std::io::Error::other(
            "The original file is too large or is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(IMPORT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > IMPORT_BYTES {
        return Err(std::io::Error::other("The original file is too large"));
    }
    Ok(bytes)
}

fn set_job(state: &AppState, id: &str, change: impl FnOnce(&mut ImportJob)) {
    if let Ok(mut jobs) = state.imports.lock() {
        if let Some(job) = jobs.get_mut(id) {
            change(job);
        }
    }
}

fn figure_number(target: &str) -> Option<usize> {
    target.strip_prefix("figure:")?.parse().ok()
}

pub(crate) fn settle_figures(
    body: &str,
    figures: &[Figure],
    save: &mut dyn FnMut(&Figure) -> Option<String>,
) -> String {
    let mut saved: Vec<Option<Option<String>>> = vec![None; figures.len()];
    let mut placed = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        let found = attachments::pictures_in(line);
        if !found.iter().any(|(_, s)| s.target.starts_with("figure:")) {
            lines.push(line.to_string());
            continue;
        }
        let mut out = String::new();
        let mut at = 0;
        for (range, shown) in found {
            out.push_str(&line[at..range.start]);
            at = range.end;
            let Some(n) = figure_number(&shown.target) else {
                out.push_str(&line[range]);
                continue;
            };
            let Some(figure) = n.checked_sub(1).and_then(|i| figures.get(i)) else {
                continue;
            };
            let slot = &mut saved[n - 1];
            if slot.is_some() || figure.photo {
                continue;
            }
            let path = save(figure);
            if let Some(path) = &path {
                out.push_str(&format!("![{}]({path})", shown.alt));
                placed = true;
            }
            *slot = Some(path);
        }
        out.push_str(&line[at..]);
        if !out.trim().is_empty() || line.trim().is_empty() {
            lines.push(out.trim_end().to_string());
        }
    }
    let mut body = lines.join("\n");
    let mut section = |heading: &str, which: &dyn Fn(&Figure) -> bool| {
        let shown: Vec<String> = figures
            .iter()
            .enumerate()
            .filter(|(i, f)| which(f) && saved[*i].is_none())
            .filter_map(|(_, f)| {
                let caption = if f.photo {
                    f.place.clone()
                } else {
                    format!("Figure from {}", f.place)
                };
                save(f).map(|path| format!("![{caption}]({path})"))
            })
            .collect();
        if !shown.is_empty() {
            body.push_str(&format!("\n\n## {heading}\n\n{}", shown.join("\n\n")));
        }
    };
    if !placed {
        section("Figures", &|f| !f.photo);
    }
    section("Photos", &|f| f.photo);
    body
}

struct Missed {
    note: Option<String>,
    error: String,
}

fn write_upload(
    state: &AppState,
    importer: &Importer,
    dir: &str,
    title: Option<String>,
    wants: &str,
    files: Vec<UploadFile>,
    step: &mut dyn FnMut(&str, usize, usize),
) -> Result<String, Missed> {
    let missed = |note: Option<String>, error: &str| Missed {
        note,
        error: error.to_string(),
    };
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    let mut originals = stage_originals(&files).map_err(|_| {
        missed(
            None,
            "The originals could not be kept. Check that there is free disk space and try again.",
        )
    })?;
    let Made {
        title: made_title,
        body,
        figures,
    } = importer(files, wants, step).map_err(|e| missed(None, &e.to_string()))?;
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
    let stem = names
        .first()
        .and_then(|n| n.rsplit_once('.').map(|(s, _)| s.to_string()))
        .unwrap_or_else(|| "upload".into());
    let made = store_now(state, |store| {
        if !store.dir_exists(dir) {
            store.create_dir(dir);
        }
        let notes_dir = store.notes_dir.clone();
        let body = settle_figures(&body, &figures, &mut |figure| {
            let kind = attachments::kind_of(&figure.bytes)?;
            let name = if figure.photo {
                let named = figure
                    .place
                    .rsplit_once('.')
                    .map_or(figure.place.as_str(), |(s, _)| s);
                format!("{named}.{kind}")
            } else {
                format!("{stem} {}.{kind}", figure.place)
            };
            attachments::save(&notes_dir, &name, &figure.bytes).ok()
        });
        let note = store
            .create_note(title, format!("{}{footer}", body.trim()), vec![], dir)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .id
            .clone();
        save(store)?;
        Ok((note, store.notes_dir.clone()))
    });
    let (note, notes_dir) = made.map_err(|_| missed(None, "The note could not be saved."))?;
    if originals
        .iter_mut()
        .any(|(name, staged)| save_original(&notes_dir, &note, name, staged).is_err())
    {
        return Err(missed(Some(note), "The note was saved, but its originals could not be kept. Check the attachments folder permissions and try again."));
    }
    Ok(note)
}

fn run_import(
    state: AppState,
    id: String,
    importer: Importer,
    dir: String,
    title: Option<String>,
    wants: String,
    files: Vec<UploadFile>,
) {
    let mut step = |text: &str, done: usize, total: usize| {
        set_job(&state, &id, |job| {
            job.step = text.to_string();
            job.done = done;
            job.total = total;
        })
    };
    let made = write_upload(&state, &importer, &dir, title, &wants, files, &mut step);
    set_job(&state, &id, |job| match made {
        Ok(note) => {
            job.state = "done";
            job.done = job.total.max(1);
            job.notes = vec![note.clone()];
            job.note = Some(note);
        }
        Err(missed) => {
            job.state = "failed";
            job.note = missed.note;
            job.error = Some(missed.error);
        }
    });
}

fn run_each(
    state: AppState,
    id: String,
    importer: Importer,
    dir: String,
    wants: String,
    files: Vec<UploadFile>,
) {
    let count = files.len();
    let mut notes = Vec::new();
    let mut problems = Vec::new();
    for (i, file) in files.into_iter().enumerate() {
        let name = file.name.clone();
        let mut step = |text: &str, _: usize, _: usize| {
            set_job(&state, &id, |job| {
                job.step = format!("{text} · {name} ({} of {count})", i + 1);
                job.done = i;
                job.total = count;
            })
        };
        match write_upload(&state, &importer, &dir, None, &wants, vec![file], &mut step) {
            Ok(note) => notes.push(note),
            Err(missed) => {
                notes.extend(missed.note);
                problems.push(format!("{name}: {}", missed.error));
            }
        }
    }
    set_job(&state, &id, |job| {
        job.done = count;
        job.total = count;
        job.state = if notes.is_empty() { "failed" } else { "done" };
        job.note = notes.first().cloned();
        job.notes = notes;
        job.error = (!problems.is_empty()).then(|| problems.join("\n"));
    });
}

pub(crate) async fn start_import(
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> Response {
    let Some(importer) = state.importer.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "This copy of leo has no AI for writing." })),
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
    if body.directory.len() > 1_024
        || body.files.len() > MOST_UPLOAD_FILES
        || body
            .files
            .iter()
            .any(|file| file.name.len() > 512 || file.mime.len() > 128)
        || body.title.as_ref().is_some_and(|title| title.len() > 4_000)
        || body
            .files
            .iter()
            .try_fold(0usize, |size, file| size.checked_add(file.data.len()))
            .is_none_or(|size| size > IMPORT_BYTES)
    {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(serde_json::json!({ "error": "Upload up to 20 files and 90 MB at a time, with short file names and a short title." }))).into_response();
    }
    let dir = body.directory.trim().to_string();
    let slots = std::sync::Arc::clone(&state.import_slots.checks);
    let Ok(permit) = slots.try_acquire_owned() else {
        return import_busy();
    };
    let checked = state.with_store({
        let dir = dir.clone();
        move |store| {
            let _permit = permit;
            directory(store, &dir)
        }
    });
    if checked.await.is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let id = uuid::Uuid::new_v4().to_string();
    let reserved = state
        .imports
        .lock()
        .map(|mut jobs| {
            reserve_import(
                &mut jobs,
                id.clone(),
                ImportJob {
                    state: "working",
                    label: "Preparing an upload".into(),
                    dir: dir.clone(),
                    step: "Uploading".into(),
                    done: 0,
                    total: 1,
                    note: None,
                    notes: Vec::new(),
                    error: None,
                },
            )
        })
        .unwrap_or(false);
    if !reserved {
        return import_busy();
    }
    let decoded = (|| -> Result<Vec<UploadFile>, (StatusCode, String)> {
        let mut files = Vec::new();
        let mut size = 0usize;
        for file in body.files {
            use base64::Engine;
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(file.data.as_bytes())
            else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("{} did not arrive intact; try again.", file.name),
                ));
            };
            size = size.saturating_add(bytes.len());
            if size > MOST_UPLOAD_BYTES {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Upload up to 90 MB at a time. Choose fewer files and try again.".into(),
                ));
            }
            files.push(UploadFile {
                name: safe_file_name(&file.name),
                mime: file.mime,
                bytes,
            });
        }
        Ok(files)
    })();
    let files = match decoded {
        Ok(files) => files,
        Err((code, error)) => {
            if let Ok(mut jobs) = state.imports.lock() {
                jobs.remove(&id);
            }
            return (code, Json(serde_json::json!({ "error": error }))).into_response();
        }
    };
    let each = body.each && files.len() > 1;
    set_job(&state, &id, |job| {
        job.label = if each {
            format!("Making {} notes, one from each file", files.len())
        } else {
            upload_label(&files)
        }
    });
    let worker = state.clone();
    let job = id.clone();
    let wants: String = body.wants.chars().take(MOST_WANTS).collect();
    if std::thread::Builder::new()
        .name("leo-import".into())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if each {
                    run_each(worker.clone(), job.clone(), importer, dir, wants, files)
                } else {
                    run_import(
                        worker.clone(),
                        job.clone(),
                        importer,
                        dir,
                        body.title,
                        wants,
                        files,
                    )
                }
            }));
            if outcome.is_err() {
                set_job(&worker, &job, |job| {
                    job.state = "failed";
                    job.error =
                        Some("The upload stopped unexpectedly. Try again with fewer files.".into());
                });
            }
        })
        .is_err()
    {
        if let Ok(mut jobs) = state.imports.lock() {
            jobs.remove(&id);
        }
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "The upload could not start. Close another busy task and try again." }))).into_response();
    }
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
                .take(MOST_UPLOAD_FILES * KEPT_IMPORTS)
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|kind| kind.is_file()))
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| original_path(&notes_dir, &id, name).is_some())
                })
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

#[derive(Deserialize, Default)]
pub(crate) struct OriginalView {
    #[serde(default)]
    pub(crate) view: Option<String>,
}

pub(crate) fn viewable(name: &str) -> bool {
    let mime = mime_for(name);
    mime == "application/pdf" || mime.starts_with("image/")
}

pub(crate) async fn get_original(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    Query(how): Query<OriginalView>,
) -> Response {
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    match read_original(&notes_dir, &id, &name) {
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
            let inline = how.view.is_some() && viewable(&name);
            let disposition = if inline { "inline" } else { "attachment" };
            if let Ok(value) =
                HeaderValue::from_str(&format!("{disposition}; filename=\"{ascii}\""))
            {
                headers.insert(header::CONTENT_DISPOSITION, value);
            }
            if inline {
                headers.insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));
                headers.insert(
                    "content-security-policy",
                    HeaderValue::from_static("default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; frame-ancestors 'self'"),
                );
            }
            response
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    fn job(state: &'static str) -> ImportJob {
        ImportJob {
            state,
            label: String::new(),
            dir: String::new(),
            step: String::new(),
            done: 0,
            total: 1,
            note: None,
            notes: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn import_admission_caps_running_and_retained_jobs() {
        let mut jobs = std::collections::HashMap::new();
        assert!(reserve_import(&mut jobs, "a".into(), job("working")));
        assert!(reserve_import(&mut jobs, "b".into(), job("working")));
        assert!(!reserve_import(&mut jobs, "c".into(), job("working")));
        jobs.get_mut("a").unwrap().state = "done";
        for n in 0..KEPT_IMPORTS * 3 {
            assert!(reserve_import(&mut jobs, format!("done-{n}"), job("done")));
        }
        assert!(jobs.len() <= KEPT_IMPORTS);
        assert_eq!(jobs.get("b").unwrap().state, "working");
    }
}
