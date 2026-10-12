use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{NaiveDate, NaiveTime, Utc};
use leo_core::notes::checkbox_line;
use leo_core::tasks::{self as core, Located};
use serde::{Deserialize, Deserializer};

use crate::routes::notes::save;
use crate::tasks::{self, Meta, Task};
use crate::AppState;

fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

fn error(code: StatusCode, message: &str) -> Response {
    (code, Json(serde_json::json!({ "error": message }))).into_response()
}

fn time_of(text: Option<&str>) -> Result<Option<NaiveTime>, ()> {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) => NaiveTime::parse_from_str(t, "%H:%M")
            .map(Some)
            .map_err(|_| ()),
    }
}

pub(crate) async fn list_tasks(State(state): State<AppState>) -> Response {
    match all_listed(&state).await {
        Ok(all) => Json(serde_json::json!({ "tasks": all })).into_response(),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn all_listed(state: &AppState) -> Result<Vec<tasks::Listed>, StatusCode> {
    let chats = state.chats.clone();
    state
        .with_store(move |store| {
            let metas = tasks::metas(&chats);
            let mut all = tasks::note_tasks(store, &metas);
            all.extend(
                tasks::load_all(&chats)
                    .iter()
                    .map(|t| tasks::standalone(t, Some(store))),
            );
            Ok(tasks::sorted(all))
        })
        .await
}

#[derive(Deserialize)]
pub(crate) struct NewTask {
    #[serde(default)]
    id: Option<String>,
    title: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    date: Option<NaiveDate>,
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    done: bool,
}

pub(crate) async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<NewTask>,
) -> Response {
    let title: String = body.title.trim().chars().take(tasks::MOST_TITLE).collect();
    if title.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Give the task a title.");
    }
    let (Ok(time), Ok(end)) = (time_of(body.time.as_deref()), time_of(body.end.as_deref())) else {
        return error(StatusCode::BAD_REQUEST, "Write times like 14:30.");
    };
    let id = match body.id {
        Some(id) if tasks::valid_id(&id) && !id.starts_with('n') => id,
        Some(_) => return error(StatusCode::BAD_REQUEST, "That task id is not valid."),
        None => format!("t{}", uuid::Uuid::new_v4().simple()),
    };
    let chats = state.chats.clone();
    if let Some(existing) = tasks::load(&chats, &id) {
        return Json(serde_json::json!(tasks::standalone(&existing, None))).into_response();
    }
    if tasks::count(&chats) >= tasks::MOST_TASKS {
        return error(
            StatusCode::CONFLICT,
            "leo keeps up to 10,000 tasks. Delete finished ones in Storage first.",
        );
    }
    let now = Utc::now();
    let task = Task {
        id,
        title,
        kind: tasks::clean_kind(body.kind.as_deref().unwrap_or("other")),
        done: body.done,
        date: body.date,
        time: body.date.and(time),
        end: body.date.and(time).and(end),
        note: body.note.filter(|n| !n.is_empty() && n.len() <= 64),
        sent_at: None,
        created_at: now,
        updated_at: now,
    };
    if tasks::put(&chats, &task).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!(tasks::standalone(&task, None))),
    )
        .into_response()
}

#[derive(Deserialize, Default)]
pub(crate) struct Change {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    done: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    date: Option<Option<NaiveDate>>,
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default, deserialize_with = "present")]
    note: Option<Option<String>>,
    #[serde(default)]
    sent: Option<bool>,
    #[serde(default)]
    confirm: bool,
}

pub(crate) async fn change_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<Change>,
) -> Response {
    if !tasks::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (Ok(time), Ok(end)) = (
        time_of(change.time.as_deref()),
        time_of(change.end.as_deref()),
    ) else {
        return error(StatusCode::BAD_REQUEST, "Write times like 14:30.");
    };
    let chats = state.chats.clone();
    if let Some(mut task) = tasks::load(&chats, &id) {
        if let Some(title) = change.title {
            let title: String = title.trim().chars().take(tasks::MOST_TITLE).collect();
            if title.is_empty() {
                return error(StatusCode::BAD_REQUEST, "Give the task a title.");
            }
            task.title = title;
        }
        if let Some(kind) = change.kind {
            task.kind = tasks::clean_kind(&kind);
        }
        if let Some(done) = change.done {
            task.done = done;
        }
        if let Some(date) = change.date {
            task.date = date;
            task.time = date.and(time);
            task.end = date.and(time).and(end);
        }
        if let Some(note) = change.note {
            task.note = note.filter(|n| !n.is_empty() && n.len() <= 64);
        }
        if let Some(sent) = change.sent {
            task.sent_at = sent.then(Utc::now);
        }
        task.updated_at = Utc::now();
        if tasks::put(&chats, &task).is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        return Json(serde_json::json!(tasks::standalone(&task, None))).into_response();
    }
    let found = state
        .with_store(move |store| {
            let metas = tasks::metas(&chats);
            let listed = tasks::note_tasks(store, &metas);
            let Some(task) = listed.into_iter().find(|t| t.id == id) else {
                return Err(StatusCode::NOT_FOUND);
            };
            let note_id = task.note.as_ref().map(|n| n.id.clone()).unwrap_or_default();
            let mut meta = metas.into_iter().find(|m| m.id == id).unwrap_or(Meta {
                id: id.clone(),
                note: note_id.clone(),
                text: task.title.clone(),
                ..Meta::default()
            });
            let note = store.find_note_mut(&note_id).ok_or(StatusCode::NOT_FOUND)?;
            let line = match core::locate(&note.body, &meta.text, &meta.before, &meta.after) {
                Located::At(n) => n,
                _ if change.kind.is_some() || change.sent.is_some() => usize::MAX,
                _ => return Err(StatusCode::CONFLICT),
            };
            let mut lines: Vec<String> = note.body.lines().map(str::to_string).collect();
            let mut preview = None;
            if line != usize::MAX {
                if let Some(done) = change.done {
                    if let Some(set) = core::set_box(&lines[line], done) {
                        lines[line] = set;
                    }
                }
                if let Some(date) = change.date {
                    let text = checkbox_line(&lines[line])
                        .map(|c| c.text.to_string())
                        .unwrap_or_default();
                    let redated = core::with_due(&text, date, date.and(time));
                    if !redated.exact && !change.confirm {
                        preview =
                            Some(serde_json::json!({ "before": text, "after": redated.text }));
                    } else {
                        let at = lines[line].len() - text.len();
                        lines[line] = format!("{}{}", &lines[line][..at], redated.text);
                        meta.text = redated.text;
                    }
                }
                let (before, after) = core::context_of(&lines.join("\n"), line);
                meta.before = before;
                meta.after = after;
            }
            if let Some(preview) = preview {
                return Ok(Err(preview));
            }
            if let Some(kind) = change.kind {
                meta.kind = Some(tasks::clean_kind(&kind));
            }
            if let Some(sent) = change.sent {
                meta.sent_at = sent.then(Utc::now);
            }
            let body = lines.join("\n") + if note.body.ends_with('\n') { "\n" } else { "" };
            if body != note.body {
                note.body = body;
                note.updated_at = Utc::now();
                save(store)?;
            }
            let wants_meta = meta.promoted
                || meta.kind.is_some()
                || meta.sent_at.is_some()
                || change.date.is_some();
            if wants_meta {
                tasks::put_meta(&chats, &meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            let metas = tasks::metas(&chats);
            Ok(Ok(tasks::note_tasks(store, &metas)
                .into_iter()
                .find(|t| t.id == id)))
        })
        .await;
    match found {
        Ok(Ok(Some(task))) => Json(serde_json::json!(task)).into_response(),
        Ok(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(preview)) => (StatusCode::CONFLICT, Json(serde_json::json!({ "error": "leo could not find the old date to replace. Check the change, then confirm it.", "preview": preview }))).into_response(),
        Err(StatusCode::CONFLICT) => error(StatusCode::CONFLICT, "leo could not find that checkbox in the note any more. Open the note to check it."),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn delete_task(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if !tasks::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if let Some(task) = tasks::load(&state.chats, &id) {
        tasks::remove(&state.chats, &id);
        return Json(serde_json::json!(task)).into_response();
    }
    let metas = tasks::metas(&state.chats);
    match metas.iter().find(|m| m.id == id) {
        Some(meta) if meta.promoted => {
            tasks::remove_meta(&state.chats, &id);
            StatusCode::NO_CONTENT.into_response()
        }
        Some(_) | None if id.starts_with('n') => error(
            StatusCode::CONFLICT,
            "This task comes from the note's Action items. Remove it from the note to drop it.",
        ),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct Promote {
    note: String,
    line: usize,
}

pub(crate) async fn promote_task(
    State(state): State<AppState>,
    Json(body): Json<Promote>,
) -> Response {
    let chats = state.chats.clone();
    let done = state
        .with_store(move |store| {
            let note = store
                .notes
                .iter()
                .find(|n| n.id == body.note)
                .ok_or(StatusCode::NOT_FOUND)?;
            let line = note
                .body
                .lines()
                .nth(body.line)
                .ok_or(StatusCode::BAD_REQUEST)?;
            let found = checkbox_line(line)
                .filter(|c| !c.text.trim().is_empty())
                .ok_or(StatusCode::BAD_REQUEST)?;
            let (before, after) = core::context_of(&note.body, body.line);
            let meta = Meta {
                id: tasks::note_task_id(&note.id, found.text),
                note: note.id.clone(),
                text: found.text.trim().to_string(),
                before,
                after,
                kind: None,
                promoted: true,
                sent_at: None,
            };
            tasks::put_meta(&chats, &meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let metas = tasks::metas(&chats);
            tasks::note_tasks(store, &metas)
                .into_iter()
                .find(|t| t.id == meta.id)
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
        })
        .await;
    match done {
        Ok(task) => (StatusCode::CREATED, Json(serde_json::json!(task))).into_response(),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn task_ics(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Ok(all) = all_listed(&state).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(task) = all.iter().find(|t| t.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if task.date.is_none() {
        return error(StatusCode::BAD_REQUEST, "Give the task a date first.");
    }
    let name: String = task
        .title
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .take(60)
        .collect();
    (
        [
            (
                header::CONTENT_TYPE,
                "text/calendar; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}.ics\"",
                    if name.trim().is_empty() {
                        "task"
                    } else {
                        name.trim()
                    }
                ),
            ),
        ],
        tasks::ics_of(task, Utc::now()),
    )
        .into_response()
}

#[derive(Deserialize)]
pub(crate) struct Import {
    tasks: Vec<Task>,
}

pub(crate) async fn import_tasks(
    State(state): State<AppState>,
    Json(body): Json<Import>,
) -> Response {
    let mut added = 0;
    let mut kept = 0;
    for mut task in body.tasks.into_iter().take(tasks::MOST_TASKS) {
        if !tasks::valid_id(&task.id) || task.id.starts_with('n') || task.title.trim().is_empty() {
            continue;
        }
        task.title = task.title.trim().chars().take(tasks::MOST_TITLE).collect();
        task.kind = tasks::clean_kind(&task.kind);
        match tasks::load(&state.chats, &task.id) {
            Some(mine) if mine.updated_at >= task.updated_at => kept += 1,
            _ => {
                if tasks::count(&state.chats) >= tasks::MOST_TASKS {
                    break;
                }
                if tasks::put(&state.chats, &task).is_ok() {
                    added += 1;
                }
            }
        }
    }
    Json(serde_json::json!({ "added": added, "kept": kept })).into_response()
}
