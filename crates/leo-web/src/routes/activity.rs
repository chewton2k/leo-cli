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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) ready: bool,
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
            ready: false,
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
            ready: false,
        });
    }
    if let Some((done, total)) = map {
        out.push(Task {
            kind: "map",
            label: "Connecting your notes in the knowledge graph".to_string(),
            step: if total > 0 {
                format!("{done} of {total} steps")
            } else {
                "Starting".to_string()
            },
            done,
            total,
            href: "#/map".to_string(),
            ready: false,
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
    let mut tasks = activity_tasks(&imports, recording.as_ref(), state.graphs.building());
    let jobs: Vec<_> = state
        .combines
        .lock()
        .map(|jobs| jobs.values().cloned().collect())
        .unwrap_or_default();
    tasks.extend(combine_tasks(&jobs, |id| {
        state
            .progress
            .view(&format!("combine:{id}"))
            .and_then(|v| v["fraction"].as_f64())
    }));
    Json(serde_json::json!({ "tasks": tasks }))
}

pub(crate) fn combine_tasks(
    jobs: &[crate::routes::combine::CombineJob],
    fraction: impl Fn(&str) -> Option<f64>,
) -> Vec<Task> {
    let mut out: Vec<Task> = jobs
        .iter()
        .map(|j| Task {
            kind: "combine",
            label: format!("Combining “{}” into “{}”", j.with_title, j.into_title),
            step: match j.state {
                "working" => "Felix is writing one note".into(),
                "ready" => "Ready to review".into(),
                _ => format!("Not combined: {}", j.error),
            },
            done: if j.state == "working" {
                fraction(&j.id).map_or(0, |f| (f * 100.0).round() as usize)
            } else {
                100
            },
            total: 100,
            href: format!("#combine/{}", j.id),
            ready: j.state != "working",
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}
