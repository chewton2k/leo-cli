use axum::response::{IntoResponse, Response};
use axum::Json;

pub(crate) async fn list() -> Response {
    let read = tokio::task::spawn_blocking(leo_core::children::own_groups)
        .await
        .ok()
        .flatten();
    let Some(groups) = read else {
        return Json(serde_json::json!({ "total": 0, "groups": [], "unavailable": true }))
            .into_response();
    };
    let total: usize = groups.iter().map(|g| g.processes.len()).sum();
    let groups: Vec<_> = groups
        .iter()
        .map(|g| {
            serde_json::json!({
                "name": g.name,
                "count": g.processes.len(),
                "pids": g.processes.iter().map(|(pid, name)| serde_json::json!({ "pid": pid, "name": name })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(serde_json::json!({ "total": total, "groups": groups })).into_response()
}
