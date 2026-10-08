use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::routes::notes::{directory, save, NoteResponse};
use crate::AppState;

#[derive(serde::Serialize)]
pub(crate) struct TrashResponse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) directory: String,
    pub(crate) deleted_at: String,
}

pub(crate) async fn list_trash(
    State(state): State<AppState>,
) -> Result<Json<Vec<TrashResponse>>, StatusCode> {
    state
        .with_store(|store| {
            Ok(Json(
                store
                    .trashed()
                    .into_iter()
                    .map(|t| TrashResponse {
                        id: t.id,
                        title: t.title,
                        directory: t.directory,
                        deleted_at: t.deleted_at.to_rfc3339(),
                    })
                    .collect(),
            ))
        })
        .await
}

pub(crate) async fn restore_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            store.restore(&id).ok_or(StatusCode::NOT_FOUND)?;
            save(store)?;
            Ok(Json(NoteResponse::from_note(
                store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?,
            )))
        })
        .await
}

#[derive(Deserialize)]
pub(crate) struct TrashChoice {
    #[serde(default)]
    pub(crate) ids: Vec<String>,
    #[serde(default)]
    pub(crate) all: bool,
}

#[derive(Deserialize)]
pub(crate) struct TrashMove {
    #[serde(default)]
    pub(crate) notes: Vec<String>,
    #[serde(default)]
    pub(crate) dirs: Vec<String>,
}

pub(crate) async fn move_to_trash(
    State(state): State<AppState>,
    Json(body): Json<TrashMove>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(move |store| {
            let mut folders = 0;
            let mut notes = 0;
            for dir in &body.dirs {
                let dir = dir.trim().trim_matches('/');
                if dir.is_empty() {
                    return Err(StatusCode::BAD_REQUEST);
                }
                directory(store, dir)?;
            }
            for dir in &body.dirs {
                let (gone_notes, gone_dirs) =
                    store.delete_dir_recursive(dir.trim().trim_matches('/'));
                notes += gone_notes;
                folders += usize::from(gone_dirs > 0);
            }
            let ids: Vec<String> = body
                .notes
                .iter()
                .filter_map(|id| {
                    store
                        .notes
                        .iter()
                        .find(|n| &n.id == id)
                        .map(|n| n.id.clone())
                })
                .collect();
            notes += ids.len();
            if !ids.is_empty() {
                store.delete_notes(&ids);
            }
            save(store)?;
            Ok(Json(
                serde_json::json!({ "notes": notes, "folders": folders }),
            ))
        })
        .await
}

pub(crate) async fn delete_from_trash(
    State(state): State<AppState>,
    Json(choice): Json<TrashChoice>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(move |store| {
            let gone = if choice.all {
                store.empty_trash()
            } else {
                store.delete_from_trash(&choice.ids)
            }
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Json(serde_json::json!({ "deleted": gone })))
        })
        .await
}

pub(crate) async fn restore_many(
    State(state): State<AppState>,
    Json(choice): Json<TrashChoice>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(move |store| {
            let restored = choice
                .ids
                .iter()
                .filter(|id| store.restore(id).is_some())
                .count();
            save(store)?;
            Ok(Json(serde_json::json!({ "restored": restored })))
        })
        .await
}
