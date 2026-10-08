use axum::extract::State;
use axum::Json;

use crate::routes::uploads::ImportJob;
use crate::{record, AppState};

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub(crate) struct Task {
    pub(crate) kind: &'static str,
    pub(crate) label: String,
    pub(crate) step: String,
    pub(crate) done: usize,
    pub(crate) total: usize,
    pub(crate) href: String,
}

pub(crate) fn activity_tasks(
    imports: &std::collections::HashMap<String, ImportJob>,
    recording: Option<&record::RecordView>,
    map: Option<(usize, usize)>,
) -> Vec<Task> {
    let mut out: Vec<Task> = imports
        .values()
        .filter(|job| job.state == "working")
        .map(|job| Task {
            kind: "upload",
            label: job.label.clone(),
            step: job.step.clone(),
            done: job.done,
            total: job.total,
            href: if job.dir.is_empty() {
                "#/".to_string()
            } else {
                format!("#/f/{}", job.dir)
            },
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    if let Some(view) = recording.filter(|v| v.state == "writing") {
        let (done, total) = view.steps.unwrap_or((0, 0));
        out.push(Task {
            kind: "recording",
            label: "Writing the notes from a recording".to_string(),
            step: view.step.clone(),
            done,
            total,
            href: "#/record".to_string(),
        });
    }
    if let Some((done, total)) = map {
        out.push(Task {
            kind: "map",
            label: "Connecting your notes on the map".to_string(),
            step: if total > 0 {
                format!("{done} of {total} steps")
            } else {
                "Starting".to_string()
            },
            done,
            total,
            href: "#/map".to_string(),
        });
    }
    out
}

pub(crate) async fn activity(State(state): State<AppState>) -> Json<serde_json::Value> {
    let imports = state.imports.lock().map(|j| j.clone()).unwrap_or_default();
    let recording = state
        .recording
        .lock()
        .ok()
        .and_then(|held| held.as_ref().map(|j| j.view()));
    let tasks = activity_tasks(&imports, recording.as_ref(), state.graphs.building());
    Json(serde_json::json!({ "tasks": tasks }))
}
