use crate::reply::error;
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
        db.prepare("SELECT id,at,text,edited FROM dictations ORDER BY id DESC LIMIT 20")?.query_map([], |r| Ok(json!({"id":r.get::<_,i64>(0)?, "at":r.get::<_,String>(1)?, "text":r.get::<_,String>(2)?, "edited":r.get::<_,i64>(3)? != 0})))?.collect()
    })
}
pub fn edit(data: &Path, id: i64, text: &str) -> Result<()> {
    let text = text.trim();
    ensure!(
        !text.is_empty() && text.len() <= 100_000,
        "This dictation is too long to keep in Recent dictations."
    );
    let changed = crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.execute(
            "UPDATE dictations SET text=?1, edited=1 WHERE id=?2",
            rusqlite::params![text, id],
        )
    })?;
    ensure!(
        changed == 1,
        "This dictation is no longer in Recent dictations."
    );
    Ok(())
}
pub fn clear(data: &Path) -> Result<()> {
    crate::db::at(&data.join(crate::db::FILE)).with(|db| {
        db.execute("DELETE FROM dictations", [])?;
        Ok(())
    })
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
                Some("edit") => edit(
                    &data,
                    change["id"].as_i64().unwrap_or(0),
                    change["text"].as_str().unwrap_or(""),
                )?,
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
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, e.to_string()),
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
    call(state.dictation.clone(), data, None).await
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
    call(state.dictation.clone(), data, Some(change)).await
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
    #[test]
    fn editing_a_recent_dictation_keeps_the_row_and_refuses_unknown_ids() {
        let dir = tempfile::tempdir().unwrap();
        remember(dir.path(), "ship it to leo seal I users").unwrap();
        let id = recent(dir.path()).unwrap()[0]["id"].as_i64().unwrap();
        assert_eq!(recent(dir.path()).unwrap()[0]["edited"], false);
        edit(dir.path(), id, "  Ship it to Leo CLI users  ").unwrap();
        let all = recent(dir.path()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0]["text"], "Ship it to Leo CLI users");
        assert_eq!(all[0]["edited"], true);
        assert!(edit(dir.path(), id + 1, "x").is_err());
        assert!(edit(dir.path(), id, "   ").is_err());
        assert_eq!(
            recent(dir.path()).unwrap()[0]["text"],
            "Ship it to Leo CLI users"
        );
    }
}
