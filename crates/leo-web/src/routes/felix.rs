use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::routes::notes::{save, NoteResponse};
use crate::routes::uploads::{safe_file_name, ImportFileBody};
use crate::{captions, chat, chat_files, chats, review, store_now, tools, AppState, UploadFile};

fn ndjson(value: serde_json::Value) -> String {
    format!("{value}\n")
}

pub(crate) async fn list_chat_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if !chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chat_files::list(&dir, &id)).await {
        Ok(docs) => Json(docs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn add_chat_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ImportFileBody>,
) -> Response {
    let refuse = |status: StatusCode, message: String| {
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    };
    if !chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(reader) = state.reader.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "leo serve was started without a way to read files.".into(),
        );
    };
    let bytes = {
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(body.data.as_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return refuse(
                    StatusCode::BAD_REQUEST,
                    format!("{} did not arrive intact; try again.", body.name),
                )
            }
        }
    };
    let name = safe_file_name(&body.name);
    let dir = state.chats.clone();
    let file = UploadFile {
        name: name.clone(),
        mime: body.mime,
        bytes,
    };
    let done = tokio::task::spawn_blocking(move || -> Result<chat_files::Doc> {
        if chat_files::list(&dir, &id).len() >= chat_files::MOST_FILES {
            anyhow::bail!(
                "a chat holds up to {} documents; remove one first",
                chat_files::MOST_FILES
            );
        }
        let text = reader(file, &mut |_| {})?;
        chat_files::add(&dir, &id, &name, &text, chrono::Utc::now())
    })
    .await;
    match done {
        Ok(Ok(doc)) => (StatusCode::CREATED, Json(doc)).into_response(),
        Ok(Err(e)) => refuse(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn remove_chat_file(
    State(state): State<AppState>,
    Path((id, doc)): Path<(String, String)>,
) -> StatusCode {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chat_files::remove(&dir, &id, &doc)).await {
        Ok(true) => StatusCode::NO_CONTENT,
        _ => StatusCode::NOT_FOUND,
    }
}

pub(crate) async fn get_review(State(state): State<AppState>) -> Response {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || review::missed(&dir, chrono::Utc::now())).await {
        Ok(missed) => Json(missed).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct ReviewDone {
    #[serde(default)]
    pub(crate) done: Vec<String>,
}

pub(crate) async fn mark_reviewed(
    State(state): State<AppState>,
    Json(body): Json<ReviewDone>,
) -> Response {
    if body.done.iter().any(|key| !review::valid_key(key)) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || review::mark_reviewed(&dir, &body.done)).await {
        Ok(Ok(added)) => Json(serde_json::json!({ "reviewed": added })).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct ChatQuery {
    #[serde(default)]
    pub(crate) q: Option<String>,
}

pub(crate) async fn list_chats(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ChatQuery>,
) -> Response {
    let dir = state.chats.clone();
    if let Some(q) = query.q.filter(|q| !q.trim().is_empty()) {
        return match tokio::task::spawn_blocking(move || chats::search(&dir, &q)).await {
            Ok(found) => Json(found).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
    }
    let days = state
        .with_store(|store| Ok(leo_core::keep::load(&store.notes_dir).chat_days))
        .await
        .unwrap_or(None);
    match tokio::task::spawn_blocking(move || {
        let now = chrono::Utc::now();
        chats::tidy(&dir, days, now);
        chat_files::tidy_orphans(&dir, now);
        chats::list(&dir)
    })
    .await
    {
        Ok(list) => Json(list).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn get_chat(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chats::load(&dir, &id)).await {
        Ok(Some(chat)) => Json(chat).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn put_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<chats::Saving>,
) -> Response {
    if !chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || {
        let mut chat = chats::save(&dir, &id, body, chrono::Utc::now())?;
        if chat.wants_name() && chats::rename(&dir, &id, &chat.title) {
            chat.named = true;
        }
        anyhow::Ok(chat)
    })
    .await
    {
        Ok(Ok(chat)) => Json(chat.summary()).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

const MOST_BACKGROUND: usize = 8;

struct BusyGuard {
    set: &'static std::sync::LazyLock<std::sync::Mutex<std::collections::BTreeSet<String>>>,
    id: String,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Ok(mut busy) = self.set.lock() {
            busy.remove(&self.id);
        }
    }
}

fn claim_busy(
    set: &'static std::sync::LazyLock<std::sync::Mutex<std::collections::BTreeSet<String>>>,
    id: &str,
) -> Option<BusyGuard> {
    let mut busy = set.lock().ok()?;
    if busy.len() >= MOST_BACKGROUND || !busy.insert(id.to_string()) {
        return None;
    }
    Some(BusyGuard {
        set,
        id: id.to_string(),
    })
}

static REMEMBERING: std::sync::LazyLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
    std::sync::LazyLock::new(Default::default);

pub(crate) fn remember_in_background(
    dir: std::path::PathBuf,
    id: String,
    old: Option<chats::Memory>,
    upto: Vec<chat::Turn>,
    from: usize,
    writer: crate::graph::Writer,
) {
    if !chats::valid_id(&id) {
        return;
    }
    let Some(guard) = claim_busy(&REMEMBERING, &id) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("leo-chat-memory".into())
        .spawn(move || {
            let _guard = guard;
            let (system, user, most) = chat::memory_prompt(old.as_ref(), &upto[from..]);
            if let Ok(text) = writer(&system, &user, most) {
                let text = text.trim();
                if !text.is_empty() {
                    chats::set_memory(
                        &dir,
                        &id,
                        chats::Memory {
                            upto: upto.len(),
                            hash: chat::hash_of(&upto),
                            text: text.to_string(),
                        },
                    );
                }
            }
        });
}

pub(crate) async fn delete_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let dir = state.chats.clone();
    let warm = Arc::clone(&state.warm);
    match tokio::task::spawn_blocking(move || {
        warm.forget(&id);
        chats::remove(&dir, &id)
    })
    .await
    {
        Ok(true) => StatusCode::NO_CONTENT,
        _ => StatusCode::NOT_FOUND,
    }
}

struct Continued {
    warm: crate::warm::Warm,
    text: String,
    sources: Vec<chat::SourceRef>,
    notes: std::collections::BTreeMap<String, u64>,
}

struct Remember {
    key: u64,
    chat: Option<String>,
    seen: usize,
    history: u64,
    notes: std::collections::BTreeMap<String, u64>,
    documents: std::collections::BTreeSet<u64>,
    material: Option<u64>,
    limit: usize,
}

struct Ask {
    system: String,
    conversation: String,
    continued: Option<Continued>,
    remember: Remember,
    sources: Vec<chat::SourceRef>,
    room: usize,
    access: tools::Access,
    documents: Vec<(String, String)>,
    steer: crate::steer::Steer,
    marking: Option<String>,
    interview: Option<String>,
    interview_problem: Option<String>,
    session: Option<tools::Session>,
}

pub(crate) const MARK: &str = "You mark a student's answer to a practice question against the model answer. Reply with only JSON: {\"correct\": true or false, \"feedback\": \"...\"}. Judge what the answer means, not how it is written: the same value or idea in other words or notation is right (sin x and sin(x), 1/2 and 0.5). A right result reached by wrong or missing reasoning, or an answer too vague to show understanding, is incorrect. When the question has more than one good answer, a well-argued different answer is correct. feedback: two to four short sentences in plain language. If right: why it is right, and any gap a complete answer would add. If wrong: the right answer, where the answer went wrong, and a quick tip for remembering it. Math uses $...$.";
const MARK_TOKENS: u32 = 600;

#[derive(serde::Deserialize)]
pub(crate) struct MarkBody {
    question: String,
    answer: String,
    given: String,
    #[serde(default)]
    explain: String,
}

pub(crate) fn mark_with(writer: &crate::graph::Writer, body: &MarkBody) -> Option<(bool, String)> {
    let user = format!(
        "<question>\n{}\n</question>\n<model_answer>\n{}\n</model_answer>\n{}<student_answer>\n{}\n</student_answer>",
        body.question.trim(),
        body.answer.trim(),
        if body.explain.trim().is_empty() { String::new() } else { format!("<why>\n{}\n</why>\n", body.explain.trim()) },
        body.given.trim()
    );
    for _ in 0..2 {
        let Ok(reply) = writer(MARK, &user, MARK_TOKENS) else {
            continue;
        };
        let Some(value) = crate::graph::json_in(&reply) else {
            continue;
        };
        let (Some(correct), Some(feedback)) =
            (value["correct"].as_bool(), value["feedback"].as_str())
        else {
            continue;
        };
        let feedback = feedback.trim();
        if !feedback.is_empty() {
            return Some((correct, feedback.chars().take(2_000).collect()));
        }
    }
    None
}

pub(crate) async fn mark_quiz(
    State(state): State<AppState>,
    Json(body): Json<MarkBody>,
) -> Response {
    let fits = |text: &str, most: usize| text.chars().count() <= most;
    if body.given.trim().is_empty()
        || body.question.trim().is_empty()
        || !fits(&body.question, 12_000)
        || !fits(&body.answer, 6_000)
        || !fits(&body.given, 6_000)
        || !fits(&body.explain, 3_000)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(writer) = state
        .quick
        .clone()
        .or_else(|| state.graphs.writer())
        .map(|w| state.usage.counted(w, crate::usage::Purpose::QuickReview))
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Choose a writing AI in Settings to mark answers."})),
        )
            .into_response();
    };
    match tokio::task::spawn_blocking(move || mark_with(&writer, &body)).await {
        Ok(Some((correct, feedback))) => Json(serde_json::json!({"correct": correct, "feedback": feedback})).into_response(),
        _ => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "The quick check could not finish. Ask Felix instead."})),
        )
            .into_response(),
    }
}

pub(crate) const JUDGE: &str = "You check a student's answer to a practice question against the model answer given. Reply with exactly one word: correct if the answer gets the main idea right and its reasoning holds, incorrect if it is wrong, misses the main idea, skips a step the question asked for, or reaches the result by wrong reasoning. When the question has more than one good answer, a different answer that makes a defensible case with the relevant principles is correct. Use interpretable language.";

pub(crate) fn verdict_in(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    let near = |mark: &str| {
        lower
            .find(mark)
            .is_some_and(|at| at <= crate::review::VERDICT_WITHIN)
    };
    if near("[[incorrect]]") {
        Some("incorrect")
    } else if near("[[correct]]") {
        Some("correct")
    } else {
        None
    }
}

fn judged(streamer: &chat::Streamer, asked: &str) -> (Option<&'static str>, Option<chat::Spent>) {
    let user = format!("{asked}\n\nReply with one word: correct or incorrect.");
    let Ok(reply) = streamer(JUDGE, &user, 20, &mut |_| {}, &mut || {}) else {
        return (None, None);
    };
    let word = reply.text.trim().to_lowercase();
    let verdict = match word.as_str() {
        "incorrect" => Some("incorrect"),
        "correct" => Some("correct"),
        _ => None,
    };
    (verdict, reply.spent)
}

#[derive(serde::Deserialize)]
pub(crate) struct Steering {
    pub(crate) text: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct Permission {
    pub(crate) access: String,
}

pub(crate) async fn change_answer_access(
    State(state): State<AppState>,
    Path(answer): Path<String>,
    Json(body): Json<Permission>,
) -> Response {
    if !["ask", "auto", "read"].contains(&body.access.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"Choose Ask first, Auto or Read only."})),
        )
            .into_response();
    }
    match state
        .steering
        .set_access(&answer, tools::Access::named(&body.access))
    {
        Ok(()) => Json(serde_json::json!({"access":body.access})).into_response(),
        Err(_) => (
            StatusCode::GONE,
            Json(serde_json::json!({"error":"That answer has finished."})),
        )
            .into_response(),
    }
}

pub(crate) async fn steer_answer(
    State(state): State<AppState>,
    Path(answer): Path<String>,
    Json(body): Json<Steering>,
) -> Response {
    match state.steering.add(&answer, &body.text) {
        Ok(waiting) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "waiting": waiting })),
        )
            .into_response(),
        Err("empty") => StatusCode::BAD_REQUEST.into_response(),
        Err("full") => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "Felix already has several messages waiting." })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::GONE,
            Json(serde_json::json!({ "error": "That answer has finished." })),
        )
            .into_response(),
    }
}

fn interact(
    desk: &mut tools::Desk,
    call: &tools::Call,
    send: &dyn Fn(serde_json::Value),
) -> tools::Done {
    let quiet = |result: String| tools::Done {
        step: "Tried to ask you something".into(),
        result,
        proposal: None,
        found: Vec::new(),
    };
    if desk.asked > 0 {
        return quiet(tools::ALREADY_ASKED.into());
    }
    if call.name == "quiz" && desk.session.is_some() {
        if desk.session_over || desk.session.as_ref().is_some_and(tools::Session::over) {
            return quiet(tools::SESSION_DONE.into());
        }
        let idea = tools::quiz_from(call).ok().and_then(|q| q.idea);
        if let Some(Err(why)) = desk.session.as_ref().map(|s| s.check(idea)) {
            return quiet(format!("That did not work: {why}."));
        }
    }
    let (step, shown) = if call.name == "quiz" {
        match tools::quiz_from(call) {
            Ok(quiz) => (
                "Asked you a practice question",
                serde_json::json!({ "quiz": quiz }),
            ),
            Err(why) => return quiet(format!("That did not work: {why}.")),
        }
    } else {
        match tools::asked_from(call) {
            Ok(asked) => ("Asked you a question", serde_json::json!({ "ask": asked })),
            Err(why) => return quiet(format!("That did not work: {why}.")),
        }
    };
    desk.asked += 1;
    send(shown);
    tools::Done {
        step: step.into(),
        result: tools::ASKED.into(),
        proposal: None,
        found: Vec::new(),
    }
}

fn run_tool(
    state: &AppState,
    desk: &mut tools::Desk,
    call: &tools::Call,
    send: &dyn Fn(serde_json::Value),
) -> Result<tools::Done> {
    let result = execute_tool(state, desk, call, send);
    state.usage.tool(
        result
            .as_ref()
            .map_or(true, |done| done.result.starts_with("That did not work:")),
        result
            .as_ref()
            .is_ok_and(|done| done.result.starts_with("Unchanged:")),
    );
    result
}

fn lowered(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_lowercase().collect::<String>() + chars.as_str()
    })
}

fn execute_tool(
    state: &AppState,
    desk: &mut tools::Desk,
    call: &tools::Call,
    send: &dyn Fn(serde_json::Value),
) -> Result<tools::Done> {
    if desk.reply_closed() {
        return Ok(tools::Done {
            step: "Stopped working".into(),
            result: "The reply was stopped. Do not call more tools or make changes.".into(),
            proposal: None,
            found: Vec::new(),
        });
    }
    let access = desk.current_access();
    let before = desk.sources.len();
    let files_before = desk.files.len();
    let mut done = if call.name == "invalid" {
        tools::Done {
            step: format!(
                "Rewrote a tool call: {}",
                lowered(&tools::why_it_failed(&call.text("problem")))
            ),
            result: format!(
                "That did not work: {}. Write the call as one line: <tool>{{\"name\": \"search_notes\", \"query\": \"...\"}}</tool>. Inside a JSON string write a line break as \\n and a quote as \\\".",
                call.text("problem")
            ),
            proposal: None,
            found: Vec::new(),
        }
    } else if let Err(problem) = tools::check(call) {
        tools::Done {
            step: format!(
                "Retried {}: {}",
                call.name.replace('_', " "),
                lowered(&tools::why_it_failed(&problem))
            ),
            result: format!("That did not work: {problem}"),
            proposal: None,
            found: Vec::new(),
        }
    } else if tools::is_interaction(&call.name) {
        interact(desk, call, send)
    } else if desk.asked > 0 {
        tools::Done {
            step: "Waited for your answer".into(),
            result: tools::ALREADY_ASKED.into(),
            proposal: None,
            found: Vec::new(),
        }
    } else if call.name == "study_plan" {
        match tools::plan_from(call) {
            Ok(ideas) if desk.session.as_ref().is_none_or(|s| s.over()) => {
                send(serde_json::json!({ "plan": { "ideas": ideas } }));
                desk.session = Some(tools::Session {
                    closed: vec![false; ideas.len()],
                    ideas,
                    asked: 0,
                });
                desk.session_over = false;
                tools::Done {
                    step: "Planned a study session".into(),
                    result: tools::PLANNED.into(),
                    proposal: None,
                    found: Vec::new(),
                }
            }
            Ok(_) => tools::Done {
                step: "Tried to plan a study session".into(),
                result: "That did not work: a study session is already running. Keep asking about its open ideas, or call end_session first.".into(),
                proposal: None,
                found: Vec::new(),
            },
            Err(why) => tools::Done {
                step: "Tried to plan a study session".into(),
                result: format!("That did not work: {why}."),
                proposal: None,
                found: Vec::new(),
            },
        }
    } else if call.name == "end_session" {
        if desk.session.is_some() && !desk.session_over {
            send(serde_json::json!({ "session_end": true }));
        }
        desk.session_over = true;
        tools::Done {
            step: "Ended the study session".into(),
            result: tools::ENDED.into(),
            proposal: None,
            found: Vec::new(),
        }
    } else if matches!(
        call.name.as_str(),
        "list_schedule" | "list_trash" | "search_chats"
    ) || tools::organizes(&call.name)
    {
        crate::routes::organize::run(state, desk, access, call)
    } else if call.name == "calculate" {
        let steps = call.text("steps");
        let lines = steps.lines().filter(|l| !l.trim().is_empty()).count();
        tools::Done {
            step: format!(
                "Calculated {lines} step{}",
                if lines == 1 { "" } else { "s" }
            ),
            result: crate::calc::run(&steps),
            proposal: None,
            found: Vec::new(),
        }
    } else if crate::interview_tools::is_tool(&call.name) {
        if matches!(
            call.name.as_str(),
            "interview_code_edit"
                | "interview_test"
                | "interview_problem_edit"
                | "interview_problem_delete"
                | "interview_topics"
        ) && !access.changes()
        {
            return Ok(tools::Done {
                step: "Kept your code unchanged".into(),
                result:
                    "Read only is active. Explain your observations without proposing code changes."
                        .into(),
                proposal: None,
                found: vec![],
            });
        }
        let (mut done, updated) = crate::interview_tools::run(
            &state.chats,
            desk.interview.as_deref(),
            call,
            desk.interview_problem.as_deref(),
        );
        if done.proposal.is_some() {
            if desk.proposals >= tools::MOST_PROPOSALS {
                done.proposal = None;
                done.result = format!(
                    "At most {} changes fit in one answer.",
                    tools::MOST_PROPOSALS
                );
            } else {
                desk.proposals += 1;
            }
        }
        if let Some(item) = updated {
            if matches!(
                call.name.as_str(),
                "interview_challenge" | "interview_problem"
            ) {
                desk.interview_problem = item.challenge.as_ref().map(|c| c.id.clone());
            }
            if call.name == "interview_challenge" {
                crate::interview_keys::prepare(
                    &state.chats,
                    crate::routes::interviews::key_writer(state, &item),
                    &item,
                    false,
                );
            }
            send(serde_json::json!({"interview": {"id": item.id, "revision": item.revision}}));
        }
        done
    } else if tools::is_web(&call.name) {
        desk.run_web(call)
    } else if call.name == "look_at_picture" {
        look_at_picture(state, desk, call)?
    } else if call.name == "read_original" {
        read_original(state, desk, call)?
    } else {
        let graphs = Arc::clone(&state.graphs);
        store_now(state, |store| {
            let cache = graphs.load();
            Ok(desk.run(store, &cache, call))
        })
        .map_err(|_| anyhow::anyhow!("leo could not read the notes"))?
    };
    if desk.marking
        && matches!(call.name.as_str(), "quiz" | "end_session")
        && matches!(done.result.as_str(), tools::ASKED | tools::ENDED)
    {
        desk.verdict = match call.text("verdict").as_str() {
            "correct" => Some("correct"),
            "incorrect" => Some("incorrect"),
            _ => desk.verdict,
        };
    }
    let mut read_args = call.args.clone();
    if let Some(args) = read_args.as_object_mut() {
        args.remove("fresh");
    }
    if let Some(reused) = desk
        .reads
        .compact(&call.name, &read_args.to_string(), &done.result)
    {
        if call.text("fresh") != "yes" {
            done.result = reused;
            done.step = format!("{} again (unchanged)", done.step);
        }
    }
    send(serde_json::json!({ "step": done.step, "tool": call.name, "found": done.found }));
    let added = desk.take_steering();
    if !added.is_empty() {
        send(serde_json::json!({ "steered": added }));
        done.result.push_str(&crate::steer::added(&added));
    }
    if desk.sources.len() != before {
        send(serde_json::json!({ "sources": desk.sources }));
    }
    if desk.files.len() != files_before {
        send(serde_json::json!({ "files": desk.files }));
    }
    let latest_access = desk.current_access();
    if !latest_access.changes() && done.proposal.is_some() {
        done.proposal = None;
        done.result = "Read only is active. No change was made or proposed.".into();
    }
    if let Some(proposal) = &done.proposal {
        let shown = match latest_access {
            tools::Access::Auto
                if matches!(proposal, tools::Proposal::InterviewCodeEdit { .. }) =>
            {
                done.result = "Auto is on: leo applies this change in the user's editor now, merging it with their typing; they can undo it. It has not been run.".into();
                let mut shown = serde_json::json!(proposal);
                shown["auto"] = serde_json::json!(true);
                shown
            }
            tools::Access::Auto if !matches!(proposal, tools::Proposal::DeleteFolder { .. }) => {
                match apply_now(state, proposal) {
                    Ok(applied) => {
                        if let tools::Proposal::DeleteInterviewProblem { interview, .. } = proposal
                        {
                            if desk.interview.as_deref() == Some(interview.as_str()) {
                                desk.interview_problem = applied["item"]["challenge"]["id"]
                                    .as_str()
                                    .map(str::to_string);
                                send(
                                    serde_json::json!({"interview": {"id": interview, "revision": applied["revision"]}}),
                                );
                            }
                        }
                        done.result = match proposal {
                            tools::Proposal::Edit { .. } => {
                                "Changed. leo applied it; the user can undo it."
                            }
                            tools::Proposal::Create { .. } => {
                                "Made. leo made the note; the user can undo it."
                            }
                            _ => "Done. leo did it; the user can undo it.",
                        }
                        .into();
                        applied
                    }
                    Err(why) => {
                        done.result = format!(
                        "Suggested, but leo could not apply it ({why}); the user can press Apply."
                    );
                        serde_json::json!(proposal)
                    }
                }
            }
            _ => serde_json::json!(proposal),
        };
        send(serde_json::json!({ "proposal": shown }));
    }
    Ok(done)
}

const MOST_PICTURES_LOOKED_AT: usize = 4;

fn read_original(
    state: &AppState,
    desk: &mut tools::Desk,
    call: &tools::Call,
) -> Result<tools::Done> {
    let wanted = call.text("note");
    let fail = |step: String, why: String| tools::Done {
        step,
        result: format!("That did not work: {why}."),
        proposal: None,
        found: Vec::new(),
    };
    let chosen = call.text("file");
    let named = desk.file_named(&chosen).cloned();
    let looked = store_now(state, |store| {
        let note = match &named {
            Some(f) => desk.note_for(store, &f.note),
            None => desk.note_for(store, &wanted),
        };
        Ok(note.map(|n| (n, store.notes_dir.clone())))
    })
    .map_err(|_| anyhow::anyhow!("leo could not read the notes"))?;
    let (note, notes_dir) = match looked {
        Ok(found) => found,
        Err(why) => return Ok(fail(format!("Looked for “{wanted}”"), why)),
    };
    let files = crate::originals::files(&notes_dir, &note.id);
    let step = format!("Looked at the file behind “{}”", note.title);
    if files.is_empty() {
        return Ok(fail(
            step,
            format!(
                "\"{}\" was not made from an uploaded file, so there is nothing behind it to read",
                note.title
            ),
        ));
    }
    let name = match &named {
        Some(f) => Some(f.name.clone()),
        None if chosen.trim().is_empty() && files.len() == 1 => Some(files[0].0.clone()),
        None => files
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(chosen.trim()))
            .map(|(n, _)| n.clone()),
    };
    let Some(name) = name else {
        let names: Vec<String> = files.iter().map(|(n, _)| format!("\"{n}\"")).collect();
        return Ok(fail(
            step,
            format!(
                "say which file: \"{}\" has {}",
                note.title,
                names.join(", ")
            ),
        ));
    };
    let Some(reader) = state.reader.clone() else {
        return Ok(fail(step, "this copy of leo cannot read files".into()));
    };
    let Some(path) = crate::originals::path_of(&notes_dir, &note.id, &name) else {
        return Ok(fail(step, format!("{name} is not there any more")));
    };
    let text = match state.originals.text(&reader, &path, &name) {
        Ok(text) => text,
        Err(e) => return Ok(fail(step, format!("{name} could not be read ({e})"))),
    };
    let tag = desk.file_tag(&note, &name);
    let step = format!("Read {name}");
    let size = desk.part_chars();
    let lines: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(i, l)| format!("{}| {l}", i + 1))
        .collect();
    let find = call.text("find");
    let words: Vec<String> = find
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_string)
        .collect();
    let cite = format!("Quote it word for word. Cite a page or slide like [{tag} p4], or lines like [{tag} L120-124].");
    if !words.is_empty() {
        let mut place = String::new();
        let mut hits: Vec<(usize, usize, String)> = Vec::new();
        for (i, l) in text.lines().enumerate() {
            if let Some(p) = crate::originals::place_of(l) {
                place = p;
            }
            let lower = l.to_lowercase();
            let score = words.iter().filter(|w| lower.contains(w.as_str())).count();
            if score > 0 {
                hits.push((i, score, place.clone()));
            }
        }
        if hits.is_empty() {
            return Ok(tools::Done {
                step: format!("Looked for “{}” in {name}", chat::clip(&find, 40)),
                result: format!("Nothing in {name} mentions \"{}\".", chat::clip(&find, 80)),
                proposal: None,
                found: Vec::new(),
            });
        }
        hits.sort_by_key(|(i, score, _)| (std::cmp::Reverse(*score), *i));
        let mut keep = Vec::new();
        let mut used = 0;
        for (i, _, place) in hits {
            if used >= size {
                break;
            }
            let from = i.saturating_sub(1);
            let to = (i + 2).min(lines.len());
            let piece = lines[from..to].join("\n");
            used += piece.chars().count();
            keep.push((i, place, piece));
        }
        keep.sort_by_key(|(i, _, _)| *i);
        let shown: Vec<String> = keep
            .into_iter()
            .map(|(_, place, piece)| {
                if place.is_empty() {
                    piece
                } else {
                    format!("({place})\n{piece}")
                }
            })
            .collect();
        return Ok(tools::Done {
            step: format!("Looked for “{}” in {name}", chat::clip(&find, 40)),
            result: format!(
                "<file id=\"{tag}\" name=\"{}\" find=\"{}\">\n{}\n</file>\n{cite}",
                chat::attribute(&name),
                chat::attribute(&find),
                shown.join("\n…\n")
            ),
            proposal: None,
            found: vec![name],
        });
    }
    let mut parts: Vec<String> = vec![String::new()];
    for line in lines {
        let last = parts.last_mut().expect("one part");
        if !last.is_empty() && last.chars().count() + line.chars().count() > size {
            parts.push(String::new());
        }
        let last = parts.last_mut().expect("one part");
        if !last.is_empty() {
            last.push('\n');
        }
        last.push_str(&chat::clip(&line, size));
    }
    let total = parts.len();
    let part = call
        .text("part")
        .trim()
        .parse::<usize>()
        .unwrap_or(1)
        .max(1);
    if part > total {
        return Ok(fail(
            step,
            format!(
                "{name} has {total} part{}",
                if total == 1 { "" } else { "s" }
            ),
        ));
    }
    Ok(tools::Done {
        step: if total > 1 {
            format!("Read part {part} of {total} of {name}")
        } else {
            step
        },
        result: format!(
            "<file id=\"{tag}\" name=\"{}\" part=\"{part}\" of=\"{total}\">\n{}\n</file>\n{cite}",
            chat::attribute(&name),
            parts[part - 1]
        ),
        proposal: None,
        found: vec![name],
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct PassageQuery {
    at: String,
}

pub(crate) async fn original_passage(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<PassageQuery>,
) -> Response {
    let Some(reader) = state.reader.clone() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let originals = Arc::clone(&state.originals);
    let found = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, StatusCode> {
        let path =
            crate::originals::path_of(&notes_dir, &id, &name).ok_or(StatusCode::NOT_FOUND)?;
        let text = originals
            .text(&reader, &path, &name)
            .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
        let lines: Vec<&str> = text.lines().collect();
        let (from, to, label) =
            crate::originals::passage(&text, &query.at).ok_or(StatusCode::NOT_FOUND)?;
        let shown: Vec<serde_json::Value> = (from..=to.min(lines.len()))
            .filter_map(|n| {
                lines
                    .get(n - 1)
                    .map(|l| serde_json::json!({ "n": n, "text": l }))
            })
            .collect();
        let page = label
            .strip_prefix("page ")
            .and_then(|n| n.parse::<u32>().ok());
        Ok(serde_json::json!({ "name": name, "label": label, "lines": shown, "page": page }))
    })
    .await
    .unwrap_or(Err(StatusCode::INTERNAL_SERVER_ERROR));
    match found {
        Ok(value) => Json(value).into_response(),
        Err(code) => code.into_response(),
    }
}

fn look_at_picture(
    state: &AppState,
    desk: &mut tools::Desk,
    call: &tools::Call,
) -> Result<tools::Done> {
    let wanted = call.text("note");
    let fail = |step: String, why: String| tools::Done {
        step,
        result: format!("That did not work: {why}."),
        proposal: None,
        found: Vec::new(),
    };
    let found = store_now(state, |store| Ok(desk.pictures_for(store, &wanted)))
        .map_err(|_| anyhow::anyhow!("leo could not read the notes"))?;
    let (title, pictures) = match found {
        Ok(found) => found,
        Err(why) => return Ok(fail(format!("Looked for “{wanted}”"), why)),
    };
    let step = format!("Looked at the pictures in “{title}”");
    if pictures.is_empty() {
        return Ok(fail(step, format!("\"{title}\" has no pictures")));
    }
    let Some(seer) = state.seer.clone() else {
        return Ok(fail(
            step,
            "no AI that can see pictures is set up for writing".into(),
        ));
    };
    let question = call.text("question");
    let mut lines = Vec::new();
    for (i, (alt, path)) in pictures.iter().take(MOST_PICTURES_LOOKED_AT).enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let label = if alt.is_empty() {
            name.clone()
        } else {
            format!("{alt} ({name})")
        };
        match captions::describe(path, &seer, &question) {
            Ok(said) => {
                if question.trim().is_empty() {
                    if let Some(key) = captions::key_of(path) {
                        state.captions.put(key, said.clone());
                    }
                }
                lines.push(format!("Picture {}: {label}: {said}", i + 1));
            }
            Err(e) => lines.push(format!(
                "Picture {}: {label}: could not be looked at ({e})",
                i + 1
            )),
        }
    }
    if pictures.len() > MOST_PICTURES_LOOKED_AT {
        lines.push(format!(
            "…and {} more pictures not looked at.",
            pictures.len() - MOST_PICTURES_LOOKED_AT
        ));
    }
    Ok(tools::Done {
        step,
        result: lines.join("\n"),
        proposal: None,
        found: vec![title],
    })
}

fn counted(spent: &mut Option<chat::Spent>, more: Option<chat::Spent>) {
    if let Some(more) = more {
        let more = chat::Spent {
            steps: more.steps.max(1),
            ..more
        };
        *spent = Some(match spent.take() {
            Some(so_far) => so_far.plus(more),
            None => more,
        });
    }
}

fn answer(
    state: &AppState,
    streamer: &chat::Streamer,
    ask: Ask,
    tx: &crate::reply::Sender,
) -> Result<Option<chat::Spent>> {
    let Ask {
        system,
        conversation,
        continued,
        remember,
        sources,
        room,
        access,
        documents,
        steer,
        marking,
        interview,
        interview_problem,
        session,
    } = ask;
    let said = std::cell::RefCell::new(String::new());
    let send = |value: serde_json::Value| {
        if value.get("restart").is_some() || value.get("reset").is_some() {
            said.borrow_mut().clear();
        }
        if let Some(t) = value["t"].as_str() {
            said.borrow_mut().push_str(t);
        }
        let _ = tx.send(ndjson(value));
    };
    let finished = |mut spent: Option<chat::Spent>, structured: Option<&'static str>| {
        if tx.is_closed() {
            return spent;
        }
        if let Some(asked) = &marking {
            let visible = verdict_in(&said.borrow());
            if visible.is_none() || structured.is_some_and(|verdict| Some(verdict) != visible) {
                let (verdict, judged_spent) = if visible.is_none() && structured.is_some() {
                    (structured, None)
                } else {
                    let started = std::time::Instant::now();
                    let result = judged(streamer, asked);
                    state.usage.record(
                        crate::usage::Purpose::StudyGrading,
                        result.1.as_ref(),
                        started.elapsed(),
                        result.0.is_none(),
                    );
                    result
                };
                counted(&mut spent, judged_spent);
                if let Some(verdict) = verdict {
                    let _ = tx.send(ndjson(serde_json::json!({ "verdict": verdict })));
                } else if visible.is_some() && structured.is_some() {
                    let _ = tx.send(ndjson(serde_json::json!({ "verdict": "ungraded" })));
                }
                if visible.is_some() && structured.is_some() {
                    if let Some(id) = remember.chat.as_deref() {
                        state.warm.forget(id);
                    }
                }
            }
        }
        spent
    };
    let web = state.web.clone().filter(|w| (w.needed)());
    let web_on = web.is_some();
    let interview_manual = if interview.is_some() {
        crate::interview_tools::manual()
    } else {
        String::new()
    };
    let text = format!(
        "{system}\n\n{}\n\n{interview_manual}",
        tools::manual_for(web_on, access)
    );
    let last = format!("{system}\n\n{}", tools::NO_MORE_TOOLS);
    let native = format!("{system}\n\n{}", tools::guidance_for(web_on, access));
    let mut desk = tools::Desk::new(
        continued
            .as_ref()
            .map_or_else(|| sources.clone(), |c| c.sources.clone()),
        room,
    )
    .with_web(web)
    .with_access(access)
    .with_documents(documents)
    .with_captions(Arc::clone(&state.captions))
    .with_meaning(state.meaning.clone(), Arc::clone(&state.vectors))
    .with_reply(tx.clone())
    .with_steer(steer);
    desk.interview = interview.clone();
    desk.interview_problem = interview_problem;
    desk.session = session;
    desk.marking = marking.is_some();
    desk.reads = continued
        .as_ref()
        .map(|c| c.warm.reads.clone())
        .unwrap_or_default();
    let mut specs = tools::native_specs(web_on, access);
    if interview.is_some() {
        specs.extend(tools::native_from(
            crate::interview_tools::SPECS.iter().copied(),
        ));
    }
    let keep = |talk: Box<dyn chat::Conversation>,
                sent: usize,
                sources: Vec<chat::SourceRef>,
                notes: std::collections::BTreeMap<String, u64>,
                documents: std::collections::BTreeSet<u64>,
                final_access: tools::Access,
                reads: crate::read_cache::Reads| {
        let sent = sent.max(talk.context_chars().unwrap_or(0));
        if let Some(chat) = remember
            .chat
            .as_ref()
            .filter(|_| !tx.is_closed() && final_access == access)
        {
            state.warm.keep(crate::warm::Warm {
                key: remember.key,
                chat: chat.clone(),
                seen: remember.seen,
                history: remember.history,
                sources,
                notes,
                documents,
                material: remember.material,
                reads,
                sent,
                limit: remember.limit,
                talk,
                used: std::time::Instant::now(),
            });
        }
    };
    if let Some(Continued {
        warm,
        text: follow,
        notes,
        ..
    }) = continued
    {
        let crate::warm::Warm {
            talk: mut warm_talk,
            sent,
            mut documents,
            ..
        } = warm;
        let shown = std::cell::Cell::new(false);
        let tried = if warm_talk.native() {
            native_answer(state, warm_talk.as_mut(), &mut desk, &follow, &send, &shown)
        } else {
            text_answer(
                state,
                warm_talk.as_mut(),
                &mut desk,
                access,
                &follow,
                &send,
                &shown,
            )
        };
        match tried {
            Ok(spent) => {
                documents.extend(remember.documents.iter().copied());
                let used = sent + follow.len() + said.borrow().len();
                let final_access = desk.current_access();
                keep(
                    warm_talk,
                    used,
                    desk.sources.clone(),
                    notes,
                    documents,
                    final_access,
                    desk.reads.clone(),
                );
                return Ok(finished(spent, desk.verdict));
            }
            Err(_) => {
                drop(warm_talk);
                send(serde_json::json!({ "reset": true }));
                desk.asked = 0;
                desk.verdict = None;
                desk.reads.clear();
                desk.sources = sources.clone();
                send(serde_json::json!({ "sources": sources }));
            }
        }
    }
    if tx.is_closed() {
        return Ok(None);
    }
    let chosen = state.converse.as_ref().and_then(|converse| {
        converse(
            &chat::Instructions {
                native: &native,
                text: &text,
                last: &last,
            },
            &specs,
        )
    });
    if let Some(mut talk) = chosen {
        let shown = std::cell::Cell::new(false);
        let tried = if talk.native() {
            native_answer(
                state,
                talk.as_mut(),
                &mut desk,
                &conversation,
                &send,
                &shown,
            )
        } else {
            text_answer(
                state,
                talk.as_mut(),
                &mut desk,
                access,
                &conversation,
                &send,
                &shown,
            )
        };
        match tried {
            Ok(spent) => {
                let used = conversation.len() + said.borrow().len();
                let final_access = desk.current_access();
                keep(
                    talk,
                    used,
                    desk.sources.clone(),
                    remember.notes.clone(),
                    remember.documents.clone(),
                    final_access,
                    desk.reads.clone(),
                );
                return Ok(finished(spent, desk.verdict));
            }
            Err(_) => {
                send(serde_json::json!({ "reset": true }));
                desk.asked = 0;
                desk.verdict = None;
                desk.reads.clear();
            }
        }
    }
    let mut failure = None;
    for attempt in 0..FALLBACK_TRIES {
        if tx.is_closed() {
            return Ok(None);
        }
        if attempt > 0 {
            send(serde_json::json!({ "reset": true }));
            desk.asked = 0;
        }
        desk.reads.clear();
        desk.verdict = None;
        let mut restated = chat::Restated::new(Arc::clone(streamer), text.clone(), last.clone());
        let shown = std::cell::Cell::new(false);
        match text_answer(
            state,
            &mut restated,
            &mut desk,
            access,
            &conversation,
            &send,
            &shown,
        ) {
            Ok(spent) => return Ok(finished(spent, desk.verdict)),
            Err(e) => failure = Some(e),
        }
    }
    Err(failure.unwrap_or_else(|| anyhow::anyhow!("Felix could not answer")))
}

const FALLBACK_TRIES: usize = 2;
const CODE_RETRIES: usize = 1;

#[allow(clippy::too_many_arguments)]
fn native_answer(
    state: &AppState,
    talk: &mut dyn chat::Conversation,
    desk: &mut tools::Desk,
    conversation: &str,
    send: &dyn Fn(serde_json::Value),
    shown: &std::cell::Cell<bool>,
) -> Result<Option<chat::Spent>> {
    let mut spent = None;
    let mut calls = 0usize;
    let mut failures = crate::work::Failures::default();
    let exhausted = std::cell::Cell::new(false);
    let mut reads_only = true;
    let mut failure: Option<anyhow::Error> = None;
    let gap = std::cell::Cell::new(false);
    let code_context = desk.interview.is_some();
    let mut message = conversation.to_string();
    let connection = desk.reply_sender();
    let stopped = || {
        connection
            .as_ref()
            .is_some_and(crate::reply::Sender::is_closed)
    };
    for attempt in 0..=CODE_RETRIES {
        if desk.reply_closed() {
            return Ok(spent);
        }
        let head = std::cell::RefCell::new(crate::code_reply::Head::new(
            code_context
                && reads_only
                && calls < tools::MOST_STEPS
                && desk.current_access().changes(),
        ));
        let show = |t: &str| {
            if gap.replace(false) && shown.get() {
                send(serde_json::json!({ "t": "\n\n" }));
            }
            shown.set(true);
            send(serde_json::json!({ "t": t }));
        };
        let remaining = tools::MOST_STEPS.saturating_sub(calls);
        let purpose = if desk.requests == 0 {
            crate::usage::Purpose::Answer
        } else {
            crate::usage::Purpose::Retry
        };
        desk.requests += 1;
        let started = std::time::Instant::now();
        let outcome = {
            let mut piece = |t: &str| head.borrow_mut().push(t, &mut |t| show(t));
            let mut restart = || gap.set(true);
            let mut call = |name: &str, args: &serde_json::Value| -> String {
                if desk.reply_closed() {
                    return "The reply was stopped. Do not call more tools or make changes.".into();
                }
                if exhausted.get() {
                    return tools::NO_MORE_TOOLS.into();
                }
                head.borrow_mut().release(&mut |t| show(t));
                calls += 1;
                reads_only &= crate::code_reply::read(name);
                if calls > tools::MOST_STEPS {
                    return format!("That did not work: {}", tools::NO_MORE_TOOLS);
                }
                let mut call = tools::Call {
                    name: name.to_string(),
                    args: args.clone(),
                };
                if !call.args.is_object() {
                    call.args = serde_json::json!({});
                }
                match run_tool(state, desk, &call, send) {
                    Ok(done) => {
                        exhausted.set(failures.record(name, &call.args.to_string(), &done.result));
                        if code_context && reads_only && calls < tools::MOST_STEPS {
                            *head.borrow_mut() =
                                crate::code_reply::Head::new(desk.current_access().changes());
                        }
                        if exhausted.get() {
                            format!("{}\n{}", done.result, tools::NO_MORE_TOOLS)
                        } else {
                            done.result
                        }
                    }
                    Err(e) => {
                        let said = e.to_string();
                        failure = Some(e);
                        format!("That did not work: {said}")
                    }
                }
            };
            talk.say(
                &message,
                chat::Exchange {
                    tail: "",
                    last: false,
                    max_tokens: chat::REPLY_TOKENS,
                    most_calls: remaining,
                    stopped: &stopped,
                    tools_done: &|| exhausted.get(),
                    piece: &mut piece,
                    restart: &mut restart,
                    call: &mut call,
                },
            )
        };
        state.usage.record(
            purpose,
            outcome.as_ref().ok().and_then(|reply| reply.spent.as_ref()),
            started.elapsed(),
            outcome.is_err(),
        );
        let reply = outcome?;
        if let Some(e) = failure.take() {
            return Err(e);
        }
        counted(&mut spent, reply.spent);
        if desk.reply_closed() {
            return Ok(spent);
        }
        if code_context
            && reads_only
            && !exhausted.get()
            && calls < tools::MOST_STEPS
            && head.borrow().refused()
        {
            if !desk.current_access().changes() {
                show("Read only is active. Switch Felix to Ask or Auto to change the code.");
            } else if attempt < CODE_RETRIES {
                message = crate::interview_tools::CODE_RETRY_NATIVE.into();
                continue;
            } else {
                show(crate::interview_tools::CODE_RETRY_FAILED);
            }
            return Ok(spent);
        }
        head.borrow_mut().release(&mut |t| show(t));
        return Ok(spent);
    }
    Ok(spent)
}

#[allow(clippy::too_many_arguments)]
fn text_answer(
    state: &AppState,
    talk: &mut dyn chat::Conversation,
    desk: &mut tools::Desk,
    access: tools::Access,
    conversation: &str,
    send: &dyn Fn(serde_json::Value),
    shown: &std::cell::Cell<bool>,
) -> Result<Option<chat::Spent>> {
    let mut unstuck = false;
    let mut failures = crate::work::Failures::default();
    let mut exhausted = false;
    let mut repairing = false;
    let mut reads_only = true;
    let mut spent: Option<chat::Spent> = None;
    let mut message = conversation.to_string();
    let code_context = desk.interview.is_some();
    let connection = desk.reply_sender();
    let stopped = || {
        connection
            .as_ref()
            .is_some_and(crate::reply::Sender::is_closed)
    };
    let gap = std::cell::Cell::new(false);
    let shown_once = |t: &str| {
        if gap.replace(false) && shown.get() {
            send(serde_json::json!({ "t": "\n\n" }));
        }
        shown.set(true);
        send(serde_json::json!({ "t": t }))
    };
    for step in 0..=tools::MOST_STEPS {
        if desk.reply_closed() {
            return Ok(spent);
        }
        let last = step == tools::MOST_STEPS || desk.asked > 0 || exhausted;
        let gate = std::cell::RefCell::new(tools::Gate::default());
        let check_code = code_context && reads_only && !last && desk.current_access().changes();
        let head = std::cell::RefCell::new(crate::code_reply::Head::new(check_code));
        let emit = |t: &str| head.borrow_mut().push(t, &mut |t| shown_once(t));
        let purpose = if repairing || (step == 0 && desk.requests > 0) {
            crate::usage::Purpose::Retry
        } else if step == 0 {
            crate::usage::Purpose::Answer
        } else {
            crate::usage::Purpose::ToolContinuation
        };
        desk.requests += 1;
        let started = std::time::Instant::now();
        let outcome = talk.say(
            &message,
            chat::Exchange {
                tail: if last {
                    tools::NO_MORE_TOOLS
                } else {
                    tools::reminder(access)
                },
                last,
                max_tokens: chat::REPLY_TOKENS,
                most_calls: 0,
                stopped: &stopped,
                tools_done: &|| last,
                piece: &mut |piece| gate.borrow_mut().push(piece, &mut |t| emit(t)),
                restart: &mut || {
                    gate.borrow_mut().reset();
                    *head.borrow_mut() = crate::code_reply::Head::new(check_code);
                    send(serde_json::json!({ "restart": true }));
                },
                call: &mut |_, _| String::new(),
            },
        );
        state.usage.record(
            purpose,
            outcome.as_ref().ok().and_then(|reply| reply.spent.as_ref()),
            started.elapsed(),
            outcome.is_err(),
        );
        let reply = outcome?;
        counted(&mut spent, reply.spent);
        if desk.reply_closed() {
            return Ok(spent);
        }
        let reply = reply.text;
        let mut gate = gate.into_inner();
        let call = if last { None } else { tools::find_call(&reply) };
        if call.is_none() {
            gate.finish(&mut |t| emit(t));
            if code_context && reads_only && !last && head.borrow().refused() {
                if !desk.current_access().changes() {
                    shown_once(
                        "Read only is active. Switch Felix to Ask or Auto to change the code.",
                    );
                } else if !unstuck {
                    unstuck = true;
                    repairing = true;
                    message = crate::interview_tools::CODE_RETRY_TEXT.into();
                    continue;
                } else {
                    shown_once(crate::interview_tools::CODE_RETRY_FAILED);
                }
                return Ok(spent);
            }
        }
        let call = match call {
            None if !code_context && !last && !unstuck && tools::claims_no_tools(&reply) => {
                unstuck = true;
                repairing = true;
                if gate.shown {
                    send(serde_json::json!({ "restart": true }));
                }
                message = tools::UNSTUCK.to_string();
                continue;
            }
            None => {
                head.borrow_mut().release(&mut |t| shown_once(t));
                return Ok(spent);
            }
            Some(Ok(call)) => call,
            Some(Err(problem)) => tools::Call {
                name: "invalid".into(),
                args: serde_json::json!({ "problem": problem }),
            },
        };
        head.borrow_mut().release(&mut |t| shown_once(t));
        if gate.shown {
            gap.set(true);
        }
        reads_only &= crate::code_reply::read(&call.name);
        let done = run_tool(state, desk, &call, send)?;
        exhausted = failures.record(&call.name, &call.args.to_string(), &done.result);
        repairing = done.result.starts_with("That did not work:");
        message = tools::result_message(&call.name, &done.result);
    }
    Ok(spent)
}

#[derive(serde::Deserialize)]
pub(crate) struct Suggestion {
    #[serde(default)]
    pub(crate) find: String,
    pub(crate) replace: String,
}

#[derive(serde::Serialize)]
pub(crate) struct Applied {
    #[serde(flatten)]
    pub(crate) note: NoteResponse,
    pub(crate) before: String,
}

pub(crate) fn change_note(
    store: &mut leo_core::store::Store,
    id: &str,
    find: &str,
    replace: &str,
) -> Result<Applied, StatusCode> {
    let note = store.find_note_mut(id).ok_or(StatusCode::NOT_FOUND)?;
    let body = if find.is_empty() {
        let base = note.body.trim_end();
        if base.is_empty() {
            replace.to_string()
        } else {
            format!("{base}\n\n{}", replace.trim_start())
        }
    } else if note.body.matches(find).count() == 1 {
        note.body.replacen(find, replace, 1)
    } else {
        return Err(StatusCode::CONFLICT);
    };
    let before = std::mem::replace(&mut note.body, body);
    note.updated_at = chrono::Utc::now();
    Ok(Applied {
        note: NoteResponse::from_note(note),
        before,
    })
}

fn apply_now(state: &AppState, proposal: &tools::Proposal) -> Result<serde_json::Value, String> {
    if !matches!(
        proposal,
        tools::Proposal::Edit { .. } | tools::Proposal::Create { .. }
    ) {
        return crate::routes::organize::act_now(state, proposal);
    }
    let mut shown = serde_json::json!(proposal);
    store_now(state, |store| {
        match proposal {
            tools::Proposal::Edit {
                note,
                find,
                replace,
                ..
            } => {
                let applied = change_note(store, note, find, replace)?;
                save(store)?;
                shown["before"] = serde_json::json!(applied.before);
                shown["after"] = serde_json::json!(applied.note.version);
            }
            tools::Proposal::Create {
                title,
                body,
                folder,
            } => {
                let made = crate::routes::notes::make_note(store, title, body, folder)?;
                save(store)?;
                shown["made"] = serde_json::json!(made.id);
                shown["folder"] = serde_json::json!(made.directory);
            }
            _ => {}
        }
        Ok(())
    })
    .map_err(|code| match code {
        StatusCode::CONFLICT => {
            "the text it would replace is not in the note exactly once".to_string()
        }
        StatusCode::NOT_FOUND => "the note is not there any more".to_string(),
        _ => "leo could not save it".to_string(),
    })?;
    shown["state"] = serde_json::json!("applied");
    Ok(shown)
}

pub(crate) async fn apply_suggestion(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<Suggestion>,
) -> Response {
    let applied = state
        .with_store(move |store| {
            let applied = change_note(store, &id, &change.find, &change.replace)?;
            save(store)?;
            Ok(applied)
        })
        .await;
    match applied {
        Ok(applied) => Json(applied).into_response(),
        Err(StatusCode::CONFLICT) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "The note changed since Felix suggested this, so the text it would replace is not there any more." })),
        )
            .into_response(),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn chat_reply(
    State(state): State<AppState>,
    Json(body): Json<chat::ChatBody>,
) -> Response {
    let Some(streamer) = state.chat.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "leo serve was started without AI." })),
        )
            .into_response();
    };
    if body
        .messages
        .last()
        .is_none_or(|t| t.role != "user" || t.text.trim().is_empty())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(foreground) = state.work.foreground() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error":"Felix is already working on several answers. Wait for one to finish or stop an answer, then try again."})),
        )
            .into_response();
    };
    let mode = chat::mode_of(body.mode.as_deref());
    let access = if body.practice {
        tools::Access::Read
    } else {
        tools::Access::named(body.access.as_deref().unwrap_or(""))
    };
    let question = chat::question_of(&body.messages);
    let graphs = Arc::clone(&state.graphs);
    let note = body.note.clone();
    let attached = body.refs.clone();
    let recent = body.recent.clone();
    let seen = Arc::clone(&state.captions);
    let room = state
        .room
        .as_ref()
        .map_or(chat::ROOM, |measure| measure())
        .min(chat::MOST_ROOM);
    let close = {
        let meaning = state.meaning.clone();
        let vectors = Arc::clone(&state.vectors);
        let asked = question.clone();
        tokio::task::spawn_blocking(move || {
            crate::vectors::close_to(meaning.as_ref(), &vectors, &asked, 24)
        })
        .await
        .unwrap_or_default()
    };
    let note_room = chat::notes_room(mode, body.practice, room);
    let gathered = state
        .with_store(move |store| {
            let cache = graphs.load();
            Ok(chat::gather_seeing(
                store,
                &cache,
                note.as_deref(),
                &attached,
                &recent,
                &question,
                note_room,
                Some(&seen),
                &close,
            ))
        })
        .await;
    let (sources, notes) = match gathered {
        Ok(found) => found,
        Err(code) => return code.into_response(),
    };
    let documents = match &body.chat {
        Some(chat) if !body.files.is_empty() => chat_files::texts(&state.chats, chat, &body.files),
        _ => Vec::new(),
    };
    let start = chat::first_kept(&body.messages, room);
    let memory = body
        .chat
        .as_deref()
        .and_then(|id| chats::load(&state.chats, id))
        .and_then(|c| c.memory);
    let documents_room = if body.practice { note_room } else { room };
    let (mut system, mut user) = chat::prompt_material(
        mode,
        &notes,
        &documents,
        &body.messages,
        room,
        documents_room,
        memory.as_ref(),
    );
    system.push_str("\n\n");
    system.push_str(tools::MARKING);
    let practice = body.chat.clone().filter(|id| chats::valid_id(id));
    let saved_practice = if let Some(id) = practice.as_ref() {
        let path = state.chats.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || crate::interview::load(&path, &id))
            .await
            .ok()
            .and_then(|item| item.ok())
    } else {
        None
    };
    let interview = practice.filter(|_| mode == "interview" || saved_practice.is_some());
    if interview.is_some() {
        system.push_str("\n\n");
        system.push_str(crate::interview_tools::WORKSPACE_RULES);
        system.push('\n');
        system.push_str(crate::interview_tools::workspace_access(access));
    }
    if body.interview_problem.is_some() && saved_practice.is_none() {
        return (StatusCode::CONFLICT, Json(serde_json::json!({"error":"That practice is no longer here. Reopen Interview practice before asking Felix to change its code."}))).into_response();
    }
    let interview_problem = body
        .interview_problem
        .clone()
        .filter(|_| interview.is_some());
    if let Some(item) = saved_practice.as_ref() {
        if interview_problem.as_ref().is_some_and(|wanted| {
            !chats::valid_id(wanted) || !item.problems.iter().any(|p| &p.challenge.id == wanted)
        }) {
            return (StatusCode::CONFLICT, Json(serde_json::json!({"error":"That practice problem is no longer here. Reopen your practice and choose a saved problem."}))).into_response();
        }
    }
    let warm_chat = body
        .chat
        .clone()
        .filter(|id| chats::valid_id(id) && state.converse.is_some());
    let warm_key = crate::warm::hash_of(&(
        mode,
        format!("{access:?}"),
        state.web.as_ref().is_some_and(|w| (w.needed)()),
        interview.is_some(),
        documents_room,
        crate::warm::config_stamp(),
    ));
    let documents_seen: std::collections::BTreeSet<u64> =
        documents.iter().map(crate::warm::hash_of).collect();
    let mut remember = Remember {
        key: warm_key,
        chat: warm_chat.clone(),
        seen: body.messages.len(),
        history: crate::warm::history_of(&body.messages),
        notes: crate::warm::fingerprints(&notes, &sources),
        documents: documents_seen,
        material: None,
        limit: crate::warm::context_limit(room, state.context.as_ref().map(|measure| measure())),
    };
    let retained = if let Some(id) = warm_chat {
        let warm = Arc::clone(&state.warm);
        let messages = body.messages.clone();
        tokio::task::spawn_blocking(move || warm.take(warm_key, &id, &messages))
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let mut continued = retained.map(|warm| {
        let renumbered = crate::warm::renumber(&notes, &sources, &warm.sources, &warm.notes);
        let fresh_documents: Vec<(String, String)> = documents
            .iter()
            .filter(|d| !warm.documents.contains(&crate::warm::hash_of(d)))
            .cloned()
            .collect();
        let message = body.messages.last().map_or("", |t| t.text.as_str());
        Continued {
            text: chat::follow_up(&fresh_documents, &renumbered.text, message, documents_room),
            sources: renumbered.sources,
            notes: renumbered.notes,
            warm,
        }
    });
    if let Some(item) = saved_practice {
        let mut context = crate::interview::context_problem(&item, interview_problem.as_deref());
        if let Some(active) = interview_problem
            .clone()
            .or_else(|| item.challenge.as_ref().map(|c| c.id.clone()))
        {
            let path = state.chats.clone();
            let checks = crate::interview_tools::latest_checks(&item, &active).map(<[_]>::to_vec);
            let interview_id = item.id.clone();
            if let Ok(tests) = tokio::task::spawn_blocking(move || {
                crate::interview_keys::tests_material(
                    &path,
                    &interview_id,
                    &active,
                    checks.as_deref(),
                )
            })
            .await
            {
                context.push_str(&tests);
            }
        }
        let material = crate::warm::hash_of(&context);
        remember.material = Some(material);
        user = chat::before_reply(&user, &context);
        if let Some(c) = continued.as_mut() {
            if c.warm.material != Some(material) {
                c.text = chat::before_reply(&c.text, &context);
            }
        }
    }
    if let Some(block) = body.selection.as_ref().and_then(chat::passage_block) {
        user = format!("{block}\n{user}");
        if let Some(c) = continued.as_mut() {
            c.text = format!("{block}\n{}", c.text);
        }
    }
    if let (Some(id), Some(writer)) = (
        body.chat.clone(),
        state.housekeeping.clone().or_else(|| state.quick.clone()),
    ) {
        let from = memory
            .as_ref()
            .filter(|m| chat::memory_fits(m, &body.messages, start))
            .map_or(0, |m| m.upto);
        if start >= from + chat::MEMORY_EVERY {
            remember_in_background(
                state.chats.clone(),
                id,
                memory.filter(|m| m.upto == from && from > 0),
                body.messages[..start].to_vec(),
                from,
                writer,
            );
        }
    }
    let marking = body
        .mark
        .then(|| body.messages.last().map(|t| t.text.clone()))
        .flatten();
    let body_session = body.session.clone().filter(|s| {
        !s.ideas.is_empty()
            && s.ideas.len() <= tools::MOST_IDEAS
            && s.ideas
                .iter()
                .all(|i| i.chars().count() <= tools::MOST_IDEA_CHARS)
    });
    let (tx, rx) = crate::reply::channel();
    let steer = state.steering.open();
    let _ = state.steering.set_access(steer.id(), access);
    let _ = tx.try_send(ndjson(serde_json::json!({ "answer": steer.id() })));
    let _ = tx.try_send(ndjson(
        serde_json::json!({ "sources": continued.as_ref().map_or(&sources, |c| &c.sources) }),
    ));
    let worker = state.clone();
    tokio::task::spawn_blocking(move || {
        let _foreground = foreground;
        let ask = Ask {
            system,
            conversation: user,
            continued,
            remember,
            sources,
            room,
            access,
            documents,
            steer,
            marking,
            interview,
            interview_problem,
            session: body_session,
        };
        let end = match answer(&worker, &streamer, ask, &tx) {
            Ok(spent) => {
                if let Some(spent) = spent {
                    let _ = tx.send(ndjson(serde_json::json!({ "spent": spent })));
                }
                serde_json::json!({ "done": true })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        let _ = tx.send(ndjson(end));
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|line| (Ok::<_, std::io::Error>(line), rx))
    });
    let mut response = axum::body::Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

#[cfg(test)]
mod background_guards {
    use super::*;

    static BUSY: std::sync::LazyLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
        std::sync::LazyLock::new(Default::default);

    #[test]
    fn background_admission_is_bounded_and_released_after_failure() {
        let held: Vec<_> = (0..MOST_BACKGROUND)
            .map(|index| claim_busy(&BUSY, &index.to_string()).unwrap())
            .collect();
        assert!(claim_busy(&BUSY, "extra").is_none());
        assert!(claim_busy(&BUSY, "0").is_none());
        drop(held);
        assert!(std::panic::catch_unwind(|| {
            let _guard = claim_busy(&BUSY, "failed").unwrap();
            panic!("fake background worker panic");
        })
        .is_err());
        assert!(claim_busy(&BUSY, "failed").is_some());
        assert!(BUSY.lock().unwrap().is_empty());
    }
}
