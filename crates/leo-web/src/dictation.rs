use crate::AppState;
use anyhow::{ensure, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

pub trait DictationApi: Send + Sync {
    fn view(&self) -> Result<Value>;
    fn change(&self, change: Value) -> Result<Value>;
}

pub fn remember(data: &Path, text: &str) -> Result<()> {
    ensure!(
        !text.trim().is_empty() && text.len() <= 100_000,
        "This dictation is too long to keep in Recent dictations."
    );
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        let tx = db.transaction()?;
        tx.execute("INSERT INTO dictations(at,text) VALUES (?1,?2)", rusqlite::params![chrono::Utc::now().to_rfc3339(), text])?;
        tx.execute("DELETE FROM dictations WHERE id NOT IN (SELECT id FROM dictations ORDER BY id DESC LIMIT 20)", [])?;
        tx.commit()
    })
}
pub fn recent(data: &Path) -> Result<Vec<Value>> {
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.prepare("SELECT id,at,text FROM dictations ORDER BY id DESC LIMIT 20")?.query_map([], |r| Ok(json!({"id":r.get::<_,i64>(0)?, "at":r.get::<_,String>(1)?, "text":r.get::<_,String>(2)?})))?.collect()
    })
}
pub fn clear(data: &Path) -> Result<()> {
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.execute("DELETE FROM dictations", [])?;
        Ok(())
    })
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
async fn call(
    api: Option<Arc<dyn DictationApi>>,
    data: std::path::PathBuf,
    change: Option<Value>,
) -> Response {
    let Some(api) = api else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Dictation is available in the Leo app. Reopen Leo and try again.",
        );
    };
    let work = tokio::task::spawn_blocking(move || -> Result<Value> {
        if let Some(mut change) = change {
            match change["action"].as_str() {
                Some("clear") => clear(&data)?,
                Some("copy") => {
                    let id = change["id"].as_i64();
                    let item = recent(&data)?.into_iter().find(|v| v["id"].as_i64() == id);
                    ensure!(
                        item.is_some(),
                        "This dictation is no longer in Recent dictations."
                    );
                    change["text"] = item.unwrap()["text"].clone();
                    api.change(change)?;
                }
                _ => {
                    api.change(change)?;
                }
            }
        }
        let mut view = api.view()?;
        view["recent"] = json!(recent(&data)?);
        Ok(view)
    })
    .await;
    match work {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Dictation settings could not be opened. Try again.",
        ),
    }
}
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let data = crate::db::for_chats(&state.chats)
        .path()
        .parent()
        .unwrap()
        .to_path_buf();
    call(state.dictation, data, None).await
}
pub(crate) async fn change(
    State(state): State<AppState>,
    change: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(change)) = change else {
        return error(
            StatusCode::BAD_REQUEST,
            "This dictation change could not be read. Try again in Settings.",
        );
    };
    let data = crate::db::for_chats(&state.chats)
        .path()
        .parent()
        .unwrap()
        .to_path_buf();
    call(state.dictation, data, Some(change)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_dictations_are_local_bounded_and_clearable() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..23 {
            remember(dir.path(), &format!("text {n}")).unwrap();
        }
        let all = recent(dir.path()).unwrap();
        assert_eq!(all.len(), 20);
        assert_eq!(all[0]["text"], "text 22");
        assert_eq!(all[19]["text"], "text 3");
        assert!(!dir.path().join("notes").exists());
        clear(dir.path()).unwrap();
        assert!(recent(dir.path()).unwrap().is_empty());
    }
}
