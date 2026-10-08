use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use leo_core::store::Store;
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct ListParams {
    pub(crate) tag: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) dir: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SearchParams {
    pub(crate) q: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ToggleParams {
    pub(crate) checkbox: usize,
}

#[derive(Deserialize)]
pub(crate) struct DirParams {
    pub(crate) parent: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CreateBody {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) directory: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateBody {
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) pinned: Option<bool>,
    pub(crate) base: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CreateDirBody {
    pub(crate) path: String,
}

#[derive(Deserialize)]
pub(crate) struct MoveBody {
    pub(crate) directory: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct NoteResponse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) tags: Vec<String>,
    pub(crate) directory: String,
    pub(crate) pinned: bool,
    pub(crate) version: String,
}

fn version_of(note: &leo_core::notes::Note) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let parts = [
        note.title.as_str(),
        note.body.as_str(),
        &note.tags.join("\u{1f}"),
    ];
    for part in parts {
        for byte in part.bytes().chain([0u8]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

impl NoteResponse {
    pub(crate) fn from_note(n: &leo_core::notes::Note) -> Self {
        NoteResponse {
            id: n.id.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            created_at: n.created_at.to_rfc3339(),
            updated_at: n.updated_at.to_rfc3339(),
            tags: n.tags.clone(),
            directory: n.directory.clone(),
            pinned: n.pinned,
            version: version_of(n),
        }
    }
}

pub(crate) fn save(store: &Store) -> Result<(), StatusCode> {
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn directory(store: &Store, path: &str) -> Result<(), StatusCode> {
    store
        .validate_directory(path)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

pub(crate) async fn list_notes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<NoteResponse>>, StatusCode> {
    state
        .with_store(move |store| {
            let limit = params.limit.unwrap_or(100).min(1000);
            let notes = if let Some(ref dir) = params.dir {
                directory(store, dir)?;
                store.list_notes_in_dir(dir, params.tag.as_deref(), limit)
            } else {
                store.list_notes(params.tag.as_deref(), limit)
            };
            Ok(Json(
                notes.iter().map(|n| NoteResponse::from_note(n)).collect(),
            ))
        })
        .await
}

pub(crate) async fn get_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
            Ok(Json(NoteResponse::from_note(note)))
        })
        .await
}

pub(crate) async fn create_note(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, StatusCode> {
    state
        .with_store(move |store| {
            let dir = body.directory.unwrap_or_default();
            directory(store, &dir)?;
            if !store.dir_exists(&dir) {
                store.create_dir(&dir);
            }
            let note = store
                .create_note(
                    body.title,
                    body.body.unwrap_or_default(),
                    body.tags.unwrap_or_default(),
                    &dir,
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let resp = NoteResponse::from_note(note);
            save(store)?;
            Ok((StatusCode::CREATED, Json(resp)))
        })
        .await
}

pub(crate) async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            let note = store.find_note_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
            let edited = body.title.is_some() || body.body.is_some() || body.tags.is_some();
            if edited
                && body
                    .base
                    .as_deref()
                    .is_some_and(|base| base != version_of(note))
            {
                return Err(StatusCode::CONFLICT);
            }
            if let Some(title) = body.title {
                note.title = title;
            }
            if let Some(text) = body.body {
                note.body = text;
            }
            if let Some(tags) = body.tags {
                note.tags = tags;
            }
            if let Some(pinned) = body.pinned {
                note.pinned = pinned;
            }
            if edited {
                note.updated_at = chrono::Utc::now();
            }
            let resp = NoteResponse::from_note(note);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

pub(crate) async fn delete_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    state
        .with_store(move |store| {
            let full = store
                .find_note(&id)
                .map(|n| n.id.clone())
                .ok_or(StatusCode::NOT_FOUND)?;
            store.delete_notes(&[full]);
            save(store)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
        .unwrap_or_else(|status| status)
}

pub(crate) async fn toggle_checkbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ToggleParams>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            store
                .toggle_checkbox(&id, params.checkbox)
                .ok_or(StatusCode::NOT_FOUND)?;
            let resp = NoteResponse::from_note(store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

pub(crate) async fn move_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            directory(store, &body.directory)?;
            if !store.dir_exists(&body.directory) {
                return Err(StatusCode::NOT_FOUND);
            }
            store
                .move_note(&id, &body.directory)
                .ok_or(StatusCode::NOT_FOUND)?;
            let resp = NoteResponse::from_note(store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

#[derive(serde::Serialize)]
pub(crate) struct SearchHit {
    #[serde(flatten)]
    pub(crate) note: NoteResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) why: Option<crate::search::Why>,
}

pub(crate) async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<SearchHit>>, StatusCode> {
    let graphs = state.graphs.clone();
    state
        .with_store(move |store| {
            let q = params.q.unwrap_or_default();
            let cache = graphs.load();
            Ok(Json(
                crate::search::search(store, &cache, &q)
                    .into_iter()
                    .map(|hit| SearchHit {
                        note: NoteResponse::from_note(hit.note),
                        why: hit.why,
                    })
                    .collect(),
            ))
        })
        .await
}

#[derive(serde::Serialize)]
pub(crate) struct DirResponse {
    pub(crate) name: String,
    pub(crate) notes: usize,
}

pub(crate) async fn list_dirs(
    State(state): State<AppState>,
    Query(params): Query<DirParams>,
) -> Result<Json<Vec<DirResponse>>, StatusCode> {
    state
        .with_store(move |store| {
            let parent = params.parent.unwrap_or_default();
            directory(store, &parent)?;
            Ok(Json(
                store
                    .subdirs(&parent)
                    .into_iter()
                    .map(|name| {
                        let full = if parent.is_empty() {
                            name.clone()
                        } else {
                            format!("{parent}/{name}")
                        };
                        DirResponse {
                            name,
                            notes: store.dir_contents(&full).0,
                        }
                    })
                    .collect(),
            ))
        })
        .await
}

pub(crate) async fn list_folders(
    State(state): State<AppState>,
) -> Result<Json<Vec<DirResponse>>, StatusCode> {
    state
        .with_store(|store| {
            let mut all = store.directories.clone();
            all.sort_by_key(|d| d.to_lowercase());
            Ok(Json(
                all.into_iter()
                    .map(|name| DirResponse {
                        notes: store.dir_contents(&name).0,
                        name,
                    })
                    .collect(),
            ))
        })
        .await
}

pub(crate) async fn create_dir(
    State(state): State<AppState>,
    Json(body): Json<CreateDirBody>,
) -> StatusCode {
    state
        .with_store(move |store| {
            directory(store, &body.path)?;
            if !store.create_dir(&body.path) {
                return Err(StatusCode::CONFLICT);
            }
            save(store)?;
            Ok(StatusCode::CREATED)
        })
        .await
        .unwrap_or_else(|status| status)
}
