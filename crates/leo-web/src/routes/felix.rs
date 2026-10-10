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
    let writer = state.graphs.writer();
    match tokio::task::spawn_blocking(move || chats::save(&dir, &id, body, chrono::Utc::now()))
        .await
    {
        Ok(Ok(chat)) => {
            if let Some(writer) = writer.filter(|_| chat.wants_name()) {
                name_in_background(state.chats.clone(), &chat, writer);
            }
            Json(chat.summary()).into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

static NAMING: std::sync::LazyLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
    std::sync::LazyLock::new(Default::default);

pub(crate) fn name_in_background(
    dir: std::path::PathBuf,
    chat: &chats::Chat,
    writer: crate::graph::Writer,
) {
    let id = chat.id.clone();
    if !NAMING
        .lock()
        .is_ok_and(|mut naming| naming.insert(id.clone()))
    {
        return;
    }
    let (system, user) = chats::name_prompt(chat);
    std::thread::spawn(move || {
        if let Some(name) = writer(&system, &user, 40)
            .ok()
            .and_then(|reply| chats::clean_name(&reply))
        {
            chats::rename(&dir, &id, &name);
        }
        if let Ok(mut naming) = NAMING.lock() {
            naming.remove(&id);
        }
    });
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
    if !chats::valid_id(&id)
        || !REMEMBERING
            .lock()
            .is_ok_and(|mut busy| busy.insert(id.clone()))
    {
        return;
    }
    std::thread::spawn(move || {
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
        if let Ok(mut busy) = REMEMBERING.lock() {
            busy.remove(&id);
        }
    });
}

pub(crate) async fn delete_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chats::remove(&dir, &id)).await {
        Ok(true) => StatusCode::NO_CONTENT,
        _ => StatusCode::NOT_FOUND,
    }
}

struct Ask {
    system: String,
    conversation: String,
    sources: Vec<chat::SourceRef>,
    room: usize,
    wanted: String,
    access: tools::Access,
    documents: Vec<(String, String)>,
    steer: crate::steer::Steer,
}

#[derive(serde::Deserialize)]
pub(crate) struct Steering {
    pub(crate) text: String,
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
    access: tools::Access,
    call: &tools::Call,
    send: &dyn Fn(serde_json::Value),
) -> Result<tools::Done> {
    let before = desk.sources.len();
    let mut done = if call.name == "invalid" {
        tools::Done {
            step: "Tried to use a tool".into(),
            result: format!(
                "That did not work: {}. Write the call as one line: <tool>{{\"name\": \"search_notes\", \"query\": \"...\"}}</tool>",
                call.text("problem")
            ),
            proposal: None,
            found: Vec::new(),
        }
    } else if let Err(problem) = tools::check(call) {
        tools::Done {
            step: "Tried to use a tool".into(),
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
    } else if tools::is_web(&call.name) {
        desk.run_web(call)
    } else if call.name == "look_at_picture" {
        look_at_picture(state, desk, call)?
    } else {
        let graphs = Arc::clone(&state.graphs);
        store_now(state, |store| {
            let cache = graphs.load();
            Ok(desk.run(store, &cache, call))
        })
        .map_err(|_| anyhow::anyhow!("leo could not read the notes"))?
    };
    send(serde_json::json!({ "step": done.step, "tool": call.name, "found": done.found }));
    let added = desk.take_steering();
    if !added.is_empty() {
        send(serde_json::json!({ "steered": added }));
        done.result.push_str(&crate::steer::added(&added));
    }
    if desk.sources.len() != before {
        send(serde_json::json!({ "sources": desk.sources }));
    }
    if let Some(proposal) = &done.proposal {
        let shown = match access {
            tools::Access::Auto => match apply_now(state, proposal) {
                Ok(applied) => {
                    done.result = match proposal {
                        tools::Proposal::Edit { .. } => {
                            "Changed. leo applied it; the user can undo it."
                        }
                        tools::Proposal::Create { .. } => {
                            "Made. leo made the note; the user can undo it."
                        }
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
            },
            _ => serde_json::json!(proposal),
        };
        send(serde_json::json!({ "proposal": shown }));
    }
    Ok(done)
}

const MOST_PICTURES_LOOKED_AT: usize = 4;

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
    tx: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<Option<chat::Spent>> {
    let Ask {
        system,
        conversation,
        sources,
        room,
        wanted,
        access,
        documents,
        steer,
    } = ask;
    let send = |value: serde_json::Value| {
        let _ = tx.send(ndjson(value));
    };
    let web = state.web.clone().filter(|w| (w.needed)());
    let web_on = web.is_some();
    let system = if tools::wants_plan(&wanted) {
        format!("{system}\n\n{}", tools::PLAN)
    } else {
        system
    };
    let text = format!("{system}\n\n{}", tools::manual_for(web_on, access));
    let last = format!("{system}\n\n{}", tools::NO_MORE_TOOLS);
    let native = format!("{system}\n\n{}", tools::guidance_for(web_on, access));
    let mut desk = tools::Desk::new(sources, room)
        .with_web(web)
        .with_access(access)
        .with_documents(documents)
        .with_captions(Arc::clone(&state.captions))
        .with_meaning(state.meaning.clone(), Arc::clone(&state.vectors))
        .with_steer(steer);
    let chosen = state.converse.as_ref().and_then(|converse| {
        converse(
            &chat::Instructions {
                native: &native,
                text: &text,
                last: &last,
            },
            &tools::native_specs(web_on, access),
        )
    });
    if let Some(mut talk) = chosen {
        let shown = std::cell::Cell::new(false);
        let tried = if talk.native() {
            native_answer(
                state,
                talk.as_mut(),
                &mut desk,
                access,
                &wanted,
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
                &wanted,
                &conversation,
                &send,
                &shown,
            )
        };
        match tried {
            Ok(spent) => return Ok(spent),
            Err(e) if shown.get() => return Err(e),
            Err(_) => send(serde_json::json!({ "restart": true })),
        }
    }
    let mut restated = chat::Restated::new(Arc::clone(streamer), text, last);
    let shown = std::cell::Cell::new(false);
    text_answer(
        state,
        &mut restated,
        &mut desk,
        access,
        &wanted,
        &conversation,
        &send,
        &shown,
    )
}

#[allow(clippy::too_many_arguments)]
fn native_answer(
    state: &AppState,
    talk: &mut dyn chat::Conversation,
    desk: &mut tools::Desk,
    access: tools::Access,
    wanted: &str,
    conversation: &str,
    send: &dyn Fn(serde_json::Value),
    shown: &std::cell::Cell<bool>,
) -> Result<Option<chat::Spent>> {
    let mut spent = None;
    let mut calls = 0usize;
    let mut failure: Option<anyhow::Error> = None;
    let mut message = conversation.to_string();
    let mut nudged = !access.changes() || !tools::wants_change(wanted);
    let gap = std::cell::Cell::new(false);
    loop {
        let reply = {
            let mut piece = |t: &str| {
                if gap.replace(false) && shown.get() {
                    send(serde_json::json!({ "t": "\n\n" }));
                }
                shown.set(true);
                send(serde_json::json!({ "t": t }));
            };
            let mut restart = || gap.set(true);
            let mut call = |name: &str, args: &serde_json::Value| -> String {
                calls += 1;
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
                match run_tool(state, desk, access, &call, send) {
                    Ok(done) => done.result,
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
                    most_calls: tools::MOST_STEPS,
                    piece: &mut piece,
                    restart: &mut restart,
                    call: &mut call,
                },
            )?
        };
        if let Some(e) = failure.take() {
            return Err(e);
        }
        counted(&mut spent, reply.spent);
        if nudged || desk.proposals > 0 {
            return Ok(spent);
        }
        nudged = true;
        send(serde_json::json!({ "restart": true }));
        message = tools::NUDGE.to_string();
    }
}

#[allow(clippy::too_many_arguments)]
fn text_answer(
    state: &AppState,
    talk: &mut dyn chat::Conversation,
    desk: &mut tools::Desk,
    access: tools::Access,
    wanted: &str,
    conversation: &str,
    send: &dyn Fn(serde_json::Value),
    shown: &std::cell::Cell<bool>,
) -> Result<Option<chat::Spent>> {
    let mut nudged = !access.changes() || !tools::wants_change(wanted);
    let mut unstuck = false;
    let mut spent: Option<chat::Spent> = None;
    let mut message = conversation.to_string();
    let gap = std::cell::Cell::new(false);
    let shown_once = |t: &str| {
        if gap.replace(false) && shown.get() {
            send(serde_json::json!({ "t": "\n\n" }));
        }
        shown.set(true);
        send(serde_json::json!({ "t": t }))
    };
    for step in 0..=tools::MOST_STEPS {
        let last = step == tools::MOST_STEPS || desk.asked > 0;
        let gate = std::cell::RefCell::new(tools::Gate::default());
        let reply = talk.say(
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
                piece: &mut |piece| gate.borrow_mut().push(piece, &mut |t| shown_once(t)),
                restart: &mut || {
                    gate.borrow_mut().reset();
                    send(serde_json::json!({ "restart": true }));
                },
                call: &mut |_, _| String::new(),
            },
        )?;
        counted(&mut spent, reply.spent);
        let reply = reply.text;
        let mut gate = gate.into_inner();
        let call = if last { None } else { tools::find_call(&reply) };
        let call = match call {
            None if !last && !unstuck && tools::claims_no_tools(&reply) => {
                unstuck = true;
                if gate.shown {
                    send(serde_json::json!({ "restart": true }));
                }
                message = tools::UNSTUCK.to_string();
                continue;
            }
            None if !last && !nudged && desk.proposals == 0 => {
                nudged = true;
                if gate.shown {
                    send(serde_json::json!({ "restart": true }));
                }
                message = tools::NUDGE.to_string();
                continue;
            }
            None => {
                gate.finish(&mut |t| shown_once(t));
                return Ok(spent);
            }
            Some(Ok(call)) => call,
            Some(Err(problem)) => tools::Call {
                name: "invalid".into(),
                args: serde_json::json!({ "problem": problem }),
            },
        };
        if gate.shown {
            gap.set(true);
        }
        let done = run_tool(state, desk, access, &call, send)?;
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
        .clamp(chat::LEAST_ROOM, chat::MOST_ROOM);
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
                room,
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
    let (system, user) = chat::prompt_within(
        mode,
        &notes,
        &documents,
        &body.messages,
        room,
        memory.as_ref(),
    );
    if let (Some(id), Some(writer)) = (body.chat.clone(), state.graphs.writer()) {
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
    let wanted = if body.practice {
        String::new()
    } else {
        body.messages
            .last()
            .map(|t| t.text.clone())
            .unwrap_or_default()
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let steer = state.steering.open();
    let _ = tx.send(ndjson(serde_json::json!({ "answer": steer.id() })));
    let _ = tx.send(ndjson(serde_json::json!({ "sources": sources })));
    let worker = state.clone();
    tokio::task::spawn_blocking(move || {
        let ask = Ask {
            system,
            conversation: user,
            sources,
            room,
            wanted,
            access,
            documents,
            steer,
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
