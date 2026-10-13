use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use crate::{interview, AppState};

fn failed(why: interview::Error) -> Response {
    let code = match &why {
        interview::Error::Invalid(_) => StatusCode::BAD_REQUEST,
        interview::Error::Conflict => StatusCode::CONFLICT,
        interview::Error::Missing => StatusCode::NOT_FOUND,
        interview::Error::Storage => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (code, Json(serde_json::json!({"error": why.message()}))).into_response()
}

pub(crate) async fn list(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || interview::list(&state.chats)).await {
        Ok(Ok(items)) => Json(items).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn get(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || interview::load(&state.chats, &id)).await {
        Ok(Ok(item)) => Json(item).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn put(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<interview::Change>,
) -> Response {
    match tokio::task::spawn_blocking(move || interview::change(&state.chats, &id, change)).await {
        Ok(Ok(item)) => Json(item).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || interview::remove(&state.chats, &id)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct ChallengeQuery {
    challenge: String,
}

fn key_view(state: &AppState, id: &str, challenge: &str) -> serde_json::Value {
    match crate::interview_keys::status(&state.chats, id, challenge) {
        Some((status, Some(key))) if status == "ready" => {
            serde_json::json!({"status": status, "key": key.public()})
        }
        Some((status, _)) => serde_json::json!({"status": status}),
        None => serde_json::json!({"status": "none"}),
    }
}

pub(crate) async fn key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ChallengeQuery>,
) -> Response {
    if !crate::interview_keys::valid(&id, &query.challenge) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match tokio::task::spawn_blocking(move || key_view(&state, &id, &query.challenge)).await {
        Ok(view) => Json(view).into_response(),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn prepare_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(query): Json<ChallengeQuery>,
) -> Response {
    if !crate::interview_keys::valid(&id, &query.challenge) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let done = tokio::task::spawn_blocking(move || {
        let item = interview::load(&state.chats, &id)?;
        if item
            .challenge
            .as_ref()
            .is_none_or(|c| c.id != query.challenge)
        {
            return Err(interview::Error::Invalid(
                "Open this problem before preparing its checks.".into(),
            ));
        }
        let again = crate::interview_keys::status(&state.chats, &id, &query.challenge).is_some();
        crate::interview_keys::prepare(&state.chats, state.graphs.writer(), &item, again);
        Ok(key_view(&state, &id, &query.challenge))
    })
    .await;
    match done {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn hidden_tests(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(query): Json<ChallengeQuery>,
) -> Response {
    if !crate::interview_keys::valid(&id, &query.challenge) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let tests = tokio::task::spawn_blocking(move || {
        crate::interview_keys::ready(&state.chats, &id, &query.challenge)
            .map(|key| {
                key.tests
                    .into_iter()
                    .filter(|t| t.hidden)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    Json(serde_json::json!({ "tests": tests })).into_response()
}

#[derive(serde::Deserialize)]
pub(crate) struct GradeBody {
    attempt: String,
}

fn quick_of(state: &AppState) -> Option<crate::graph::Writer> {
    state.quick.clone().or_else(|| state.graphs.writer())
}

pub(crate) async fn grade(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<GradeBody>,
) -> Response {
    if !crate::chats::valid_id(&id) || !crate::chats::valid_id(&body.attempt) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(quick) = quick_of(&state) else {
        return failed(interview::Error::Invalid(
            "Choose a writing AI in Settings to grade answers.".into(),
        ));
    };
    let done = tokio::task::spawn_blocking(move || {
        let item = interview::load(&state.chats, &id)?;
        let attempt = item
            .attempts
            .iter()
            .find(|a| a.id == body.attempt)
            .ok_or(interview::Error::Missing)?;
        if attempt.grade.is_some() {
            return Ok(item);
        }
        let key = crate::interview_keys::ready(&state.chats, &id, &attempt.challenge.id)
            .ok_or_else(|| {
                interview::Error::Invalid(
                    "This problem has no answer key yet, so Felix reviews it instead.".into(),
                )
            })?;
        let graded = crate::interview_keys::grade(
            &quick,
            &key,
            &attempt.challenge,
            &attempt.workspace,
            &attempt.checks,
        )
        .map_err(|_| {
            interview::Error::Invalid(
                "The quick check could not finish. Felix's review still covers it.".into(),
            )
        })?;
        interview::graded(&state.chats, &id, &body.attempt, graded)
    })
    .await;
    match done {
        Ok(Ok(item)) => Json(item).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct CoverageBody {
    challenge: String,
    workspace: interview::Workspace,
}

pub(crate) async fn coverage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CoverageBody>,
) -> Response {
    if !crate::interview_keys::valid(&id, &body.challenge) || body.workspace.validate().is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(quick) = quick_of(&state) else {
        return failed(interview::Error::Invalid(
            "Choose a writing AI in Settings to check coverage.".into(),
        ));
    };
    let done = tokio::task::spawn_blocking(move || {
        let item = interview::load(&state.chats, &id)?;
        let challenge = item
            .challenge
            .as_ref()
            .filter(|c| c.id == body.challenge)
            .or_else(|| {
                item.problems
                    .iter()
                    .map(|p| &p.challenge)
                    .find(|c| c.id == body.challenge)
            })
            .cloned()
            .ok_or(interview::Error::Missing)?;
        let key = crate::interview_keys::ready(&state.chats, &id, &challenge.id).ok_or_else(|| {
            interview::Error::Invalid("The checks for this problem are still being prepared.".into())
        })?;
        let sections: Vec<_> = crate::interview_keys::instant(&key, &body.workspace, &[])
            .into_iter()
            .filter(|c| c.kind == "section")
            .map(|c| serde_json::json!({"id": c.id, "text": c.label, "covered": if c.pass { "yes" } else { "no" }}))
            .collect();
        let items = crate::interview_keys::coverage(&quick, &key, &challenge, &body.workspace)
            .map_err(|_| interview::Error::Invalid("The coverage check could not finish. Try again in a moment.".into()))?;
        Ok(serde_json::json!({"sections": sections, "criteria": items}))
    })
    .await;
    match done {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}
