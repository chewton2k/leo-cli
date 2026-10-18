use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use crate::{interview, progress, AppState};

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
    if let interview::Change::DeleteProblem {
        revision,
        challenge,
    } = &change
    {
        let (revision, challenge) = (*revision, challenge.clone());
        return match tokio::task::spawn_blocking(move || {
            let before = interview::load(&state.chats, &id)?;
            let title = interview::problem(&before, &challenge)
                .ok_or_else(|| {
                    interview::Error::Invalid("Choose a saved problem from this practice.".into())
                })?
                .title;
            let (item, token) =
                crate::interview_delete::delete(&state.chats, &id, revision, &challenge)?;
            let undo = crate::tools::Proposal::RestoreInterviewProblem {
                title,
                summary: "Restore the deleted problem and its work".into(),
                interview: id,
                token,
            };
            Ok::<_, interview::Error>(serde_json::json!({"item": item, "undo": undo}))
        })
        .await
        {
            Ok(Ok(reply)) => Json(reply).into_response(),
            Ok(Err(why)) => failed(why),
            Err(_) => failed(interview::Error::Storage),
        };
    }
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

pub(crate) fn key_writer(
    state: &AppState,
    item: &interview::Interview,
) -> Option<crate::graph::Writer> {
    let writer = state.graphs.writer()?;
    let challenge = item.challenge.as_ref()?;
    let Some(ticket) = state
        .progress
        .start(format!("key:{}:{}", item.id, challenge.id), progress::KEY)
    else {
        return Some(writer);
    };
    let inner = progress::metered(
        writer,
        state.chat.clone(),
        std::sync::Arc::clone(&ticket.meter),
    );
    Some(std::sync::Arc::new(
        move |system: &str, user: &str, most: u32| {
            let _held = &ticket;
            inner(system, user, most)
        },
    ))
}

pub(crate) async fn progress(State(state): State<AppState>, Path(key): Path<String>) -> Response {
    if !progress::valid_key(&key) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(
        state
            .progress
            .view(&key)
            .unwrap_or_else(|| serde_json::json!({ "fraction": null })),
    )
    .into_response()
}

fn key_view(state: &AppState, id: &str, challenge: &str) -> serde_json::Value {
    match crate::interview_keys::status(&state.chats, id, challenge) {
        Some((status, Some(key))) if status == "ready" => {
            let revealed = interview::load(&state.chats, id)
                .is_ok_and(|item| item.attempts.iter().any(|a| a.challenge.id == challenge));
            serde_json::json!({"status": status, "key": key.public(revealed)})
        }
        Some((status, _)) if status == "preparing" => serde_json::json!({
            "status": status,
            "progress": state.progress.view(&format!("key:{id}:{challenge}")),
        }),
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
        crate::interview_keys::prepare(&state.chats, key_writer(&state, &item), &item, again);
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
pub(crate) struct RunBody {
    challenge: String,
    #[serde(default)]
    stamp: String,
    #[serde(default)]
    reports: Vec<crate::interview_keys::TestReport>,
    #[serde(default)]
    error: String,
}

fn too_many(body: &RunBody) -> bool {
    body.reports.len() > crate::interview_keys::MOST_TESTS
        || body.error.len() > 4_000
        || body
            .reports
            .iter()
            .any(|r| r.id.len() > 40 || r.got.len() > 4_000)
}

pub(crate) async fn check_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(query): Json<ChallengeQuery>,
) -> Response {
    if !crate::interview_keys::valid(&id, &query.challenge) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let job = tokio::task::spawn_blocking(move || {
        crate::interview_keys::verify_job(&state.chats, &id, &query.challenge)
    })
    .await
    .ok()
    .flatten();
    Json(job.unwrap_or_else(|| serde_json::json!({"done": true}))).into_response()
}

pub(crate) async fn key_checked(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RunBody>,
) -> Response {
    if !crate::interview_keys::valid(&id, &body.challenge) || too_many(&body) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let done = tokio::task::spawn_blocking(move || {
        crate::interview_keys::verified(
            &state.chats,
            &id,
            &body.challenge,
            &body.stamp,
            &body.reports,
            &body.error,
        )
        .map(|dropped| (dropped, key_view(&state, &id, &body.challenge)))
    })
    .await;
    match done {
        Ok(Ok((dropped, mut view))) => {
            view["dropped"] = serde_json::json!(dropped);
            Json(view).into_response()
        }
        Ok(Err(why)) => failed(interview::Error::Invalid(why)),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn save_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RunBody>,
) -> Response {
    if !crate::interview_keys::valid(&id, &body.challenge) || too_many(&body) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let done = tokio::task::spawn_blocking(move || {
        crate::interview_keys::save_run(
            &state.chats,
            &id,
            &body.challenge,
            body.reports,
            &body.error,
        )
    })
    .await;
    match done {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(why)) => failed(interview::Error::Invalid(why)),
        Err(_) => failed(interview::Error::Storage),
    }
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
    let Some(job) = state.work.grade(format!("{id}:{}", body.attempt)) else {
        return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({"error":"Several answers are being reviewed. Wait for one to finish, then try again."}))).into_response();
    };
    if !job.leader {
        if tokio::time::timeout(std::time::Duration::from_secs(120), job.wait())
            .await
            .is_err()
        {
            return (StatusCode::REQUEST_TIMEOUT, Json(serde_json::json!({"error":"That answer is still being reviewed. Wait a moment, then try again."}))).into_response();
        }
        return match tokio::task::spawn_blocking(move || {
            let item = interview::load(&state.chats, &id)?;
            if item
                .attempts
                .iter()
                .any(|a| a.id == body.attempt && a.grade.is_some())
            {
                Ok(item)
            } else {
                Err(interview::Error::Invalid(
                    "The review could not finish. Try submitting it for review again.".into(),
                ))
            }
        })
        .await
        {
            Ok(Ok(item)) => Json(item).into_response(),
            Ok(Err(why)) => failed(why),
            Err(_) => failed(interview::Error::Storage),
        };
    }
    let ticket = state
        .progress
        .start(format!("grade:{}", body.attempt), progress::GRADE);
    let quick = match &ticket {
        Some(t) => progress::metered(quick, None, std::sync::Arc::clone(&t.meter)),
        None => quick,
    };
    let done = tokio::task::spawn_blocking(move || {
        let _grade = job;
        let _held = ticket;
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
    let ticket = state.progress.start(
        format!("coverage:{id}:{}", body.challenge),
        progress::COVERAGE,
    );
    let quick = match &ticket {
        Some(t) => progress::metered(quick, None, std::sync::Arc::clone(&t.meter)),
        None => quick,
    };
    let done = tokio::task::spawn_blocking(move || {
        let _held = ticket;
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

#[derive(serde::Deserialize)]
pub(crate) struct LanguageBody {
    challenge: String,
    language: String,
    #[serde(default)]
    restore: Option<Restore>,
}

#[derive(serde::Deserialize)]
pub(crate) struct Restore {
    from: String,
    starter: String,
    answer: String,
}

pub(crate) async fn language(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<LanguageBody>,
) -> Response {
    let language = body.language.trim().to_string();
    if !crate::interview_keys::valid(&id, &body.challenge)
        || language.is_empty()
        || language.chars().count() > 80
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let quick = quick_of(&state);
    let ticket = match (&body.restore, &quick) {
        (None, Some(_)) => state.progress.start(
            format!("language:{id}:{}", body.challenge),
            progress::LANGUAGE,
        ),
        _ => None,
    };
    let quick = match (&ticket, quick) {
        (Some(t), Some(q)) => Some(progress::metered(q, None, std::sync::Arc::clone(&t.meter))),
        (_, q) => q,
    };
    let done = tokio::task::spawn_blocking(move || {
        let _held = ticket;
        let item = interview::load(&state.chats, &id)?;
        let before = item
            .challenge
            .clone()
            .filter(|c| c.id == body.challenge)
            .ok_or_else(|| interview::Error::Invalid("Open this problem to change its language.".into()))?;
        let coding = before
            .coding
            .clone()
            .ok_or_else(|| interview::Error::Invalid("Only coding problems have a language.".into()))?;
        if coding.language.trim().eq_ignore_ascii_case(&language) && body.restore.is_none() {
            return Ok(serde_json::json!({"item": item, "changed": false}));
        }
        let mut after = before.clone();
        let old_starter = coding.starter.clone();
        let (starter, answer) = match body.restore {
            Some(restore) => {
                if coding.starter != restore.from {
                    return Err(interview::Error::Conflict);
                }
                (restore.starter, Some(restore.answer))
            }
            None => {
                if crate::interview_keys::is_sql(&coding.language) || crate::interview_keys::is_sql(&language) {
                    return Err(interview::Error::Invalid(format!(
                        "This problem stays in {}: SQL and other languages are different problems. New problems will use {language}.",
                        coding.language
                    )));
                }
                let quick = quick.ok_or_else(|| {
                    interview::Error::Invalid("Choose a writing AI in Settings to change the problem's language.".into())
                })?;
                let key = crate::interview_keys::ready(&state.chats, &id, &before.id);
                let starter = crate::interview_keys::translate_starter(&quick, &before, &language, key.as_ref())
                    .map_err(|_| {
                        interview::Error::Invalid(format!(
                            "The starting code could not be rewritten in {language}. Try again in a moment."
                        ))
                    })?;
                (starter, None)
            }
        };
        if let Some(c) = after.coding.as_mut() {
            c.language = language.clone();
            c.starter = starter.clone();
        }
        let (item, previous) = interview::relanguage(&state.chats, &id, &before, after, |_| {
            Some(answer.unwrap_or_else(|| starter.clone()))
        })?;
        let replaced = !previous.trim().is_empty() && previous.trim() != old_starter.trim();
        Ok(serde_json::json!({
            "item": item,
            "changed": true,
            "previous": {"language": coding.language, "starter": old_starter, "answer": previous},
            "replaced": replaced,
        }))
    })
    .await;
    match done {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}
