use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::{graph, AppState};

#[derive(serde::Serialize)]
pub(crate) struct GraphResponse {
    pub(crate) graph: graph::Graph,
    pub(crate) status: graph::Status,
}

async fn note_sources(state: &AppState) -> Result<Vec<graph::Source>, StatusCode> {
    state
        .with_store(|store| Ok(graph::sources(&store.notes)))
        .await
}

pub(crate) async fn get_graph(
    State(state): State<AppState>,
) -> Result<Json<GraphResponse>, StatusCode> {
    let sources = note_sources(&state).await?;
    let graphs = Arc::clone(&state.graphs);
    tokio::task::spawn_blocking(move || {
        let cache = graphs.load();
        Json(GraphResponse {
            graph: graph::assemble(&sources, &cache),
            status: graphs.status(&sources),
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn graph_status(
    State(state): State<AppState>,
) -> Result<Json<graph::Status>, StatusCode> {
    let sources = note_sources(&state).await?;
    let graphs = Arc::clone(&state.graphs);
    tokio::task::spawn_blocking(move || Json(graphs.status(&sources)))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub(crate) struct BuildParams {
    #[serde(default)]
    pub(crate) fresh: bool,
}

pub(crate) async fn build_graph(
    State(state): State<AppState>,
    Query(params): Query<BuildParams>,
) -> Result<(StatusCode, Json<graph::Status>), StatusCode> {
    let sources = note_sources(&state).await?;
    let graphs = Arc::clone(&state.graphs);
    tokio::task::spawn_blocking(move || {
        if params.fresh && !graphs.clear().unwrap_or(false) {
            return (StatusCode::CONFLICT, Json(graphs.status(&sources)));
        }
        graphs.start(sources.clone());
        (StatusCode::ACCEPTED, Json(graphs.status(&sources)))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
