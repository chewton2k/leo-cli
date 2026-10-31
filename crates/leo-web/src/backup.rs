use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{AppState, SettingsApi};

const SETTLE: Duration = Duration::from_secs(2);
const PUSH_GAP: Duration = Duration::from_secs(20);
const PUSH_LIMIT: Duration = Duration::from_secs(60);

#[derive(Default, Clone, Serialize)]
pub(crate) struct Status {
    state: &'static str,
    message: &'static str,
    at: i64,
}

#[derive(Default)]
pub(crate) struct Backups {
    sender: Mutex<Option<mpsc::Sender<()>>>,
    status: Arc<Mutex<Status>>,
}

impl Backups {
    fn set(status: &Mutex<Status>, state: &'static str, message: &'static str) {
        if let Ok(mut status) = status.lock() {
            *status = Status {
                state,
                message,
                at: chrono::Utc::now().timestamp_millis(),
            };
        }
    }

    pub(crate) fn queue(
        &self,
        settings: Arc<dyn SettingsApi>,
        notes: std::path::PathBuf,
    ) -> Result<(), ()> {
        let mut sender = self.sender.lock().map_err(|_| ())?;
        if sender.as_ref().is_none_or(|s| s.is_closed()) {
            let (tx, mut rx) = mpsc::channel(1);
            *sender = Some(tx);
            let status = Arc::clone(&self.status);
            tokio::spawn(async move {
                let mut last: Option<tokio::time::Instant> = None;
                while rx.recv().await.is_some() {
                    let delay = last.map_or(SETTLE, |at| {
                        PUSH_GAP.saturating_sub(at.elapsed()).max(SETTLE)
                    });
                    tokio::time::sleep(delay).await;
                    let _ = rx.try_recv();
                    Self::set(&status, "running", "Backing up notes and tasks…");
                    let settings = Arc::clone(&settings);
                    let notes = notes.clone();
                    let prepared =
                        tokio::task::spawn_blocking(move || settings.backup(&notes)).await;
                    let success = match prepared {
                        Ok(Ok(Some(command))) => {
                            let mut command = tokio::process::Command::from(command);
                            command.kill_on_drop(true);
                            matches!(tokio::time::timeout(PUSH_LIMIT, command.status()).await, Ok(Ok(code)) if code.success())
                        }
                        Ok(Ok(None)) => {
                            Self::set(&status, "idle", "");
                            continue;
                        }
                        _ => false,
                    };
                    last = Some(tokio::time::Instant::now());
                    if success {
                        Self::set(&status, "done", "Notes and tasks backed up.");
                    } else {
                        Self::set(&status, "failed", "Backup could not finish. Your saved work stays on this computer. Run leo backup on your computer to retry.");
                        leo_core::diag::warn("Website backup could not finish. Saved work stays on this computer; run leo backup to retry.");
                    }
                }
            });
        }
        match sender.as_ref().ok_or(())?.try_send(()) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(())) => Ok(()),
            Err(_) => Err(()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Leave {}

pub(crate) async fn leave(
    State(state): State<AppState>,
    Json(_): Json<Leave>,
) -> Result<StatusCode, StatusCode> {
    let Some(settings) = state.settings.clone() else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let notes = state
        .with_store(|store| Ok(store.notes_dir.clone()))
        .await?;
    state
        .backups
        .queue(settings, notes)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn status(State(state): State<AppState>) -> Result<Json<Status>, StatusCode> {
    state
        .backups
        .status
        .lock()
        .map(|s| Json(s.clone()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
