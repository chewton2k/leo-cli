use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};

use crate::routes::notes::{save, CreateDirBody, MoveBody, MoveDirBody};
use crate::tasks::{self, Listed};
use crate::tools::{self, Access, Call, Done, Proposal};
use crate::{store_now, AppState};

const MOST_DAYS: i64 = 42;
const MOST_LINES: usize = 300;

fn fail(step: &str, why: impl std::fmt::Display) -> Done {
    Done {
        step: step.into(),
        result: format!("That did not work: {why}."),
        proposal: None,
        found: Vec::new(),
    }
}

fn day_label(date: NaiveDate) -> String {
    format!(
        "{}, {} {}",
        date.format("%a"),
        date.format("%b"),
        date.day()
    )
}

fn time_label(time: NaiveTime) -> String {
    let (pm, hour) = time
        .format("%p %I")
        .to_string()
        .split_once(' ')
        .map(|(p, h)| (p.to_lowercase(), h.trim_start_matches('0').to_string()))
        .unwrap_or_default();
    format!("{hour}:{} {pm}", time.format("%M"))
}

fn when_label(date: Option<NaiveDate>, time: Option<NaiveTime>) -> String {
    match (date, time) {
        (Some(d), Some(t)) => format!("{} at {}", day_label(d), time_label(t)),
        (Some(d), None) => day_label(d),
        _ => "no date".into(),
    }
}

fn folder_label(path: &str) -> String {
    if path.is_empty() {
        "the top level".into()
    } else {
        path.to_string()
    }
}

fn date_of(text: &str) -> Result<Option<NaiveDate>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| "write the date as YYYY-MM-DD".to_string())
}

fn time_of(text: &str) -> Result<Option<NaiveTime>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    NaiveTime::parse_from_str(text, "%H:%M")
        .map(Some)
        .map_err(|_| "write the time as HH:MM, like 14:30".to_string())
}

fn kind_of(text: &str) -> Result<Option<String>, String> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return Ok(None);
    }
    if tasks::KINDS.contains(&text.as_str()) {
        Ok(Some(text))
    } else {
        Err(format!(
            "\"type\" must be one of {}",
            tasks::KINDS.join(", ")
        ))
    }
}

pub(crate) fn listed_now(state: &AppState) -> Result<Vec<Listed>, String> {
    let chats = state.chats.clone();
    store_now(state, |store| {
        let metas = tasks::metas(&chats);
        let mut all = tasks::note_tasks(store, &metas);
        all.extend(
            tasks::load_all(&chats)
                .iter()
                .map(|t| tasks::standalone(t, Some(store))),
        );
        Ok(tasks::sorted(all))
    })
    .map_err(|_| "leo could not read the tasks".to_string())
}

fn find_task<'a>(all: &'a [Listed], wanted: &str) -> Result<&'a Listed, String> {
    let wanted = wanted.trim();
    if let Some(task) = all.iter().find(|t| t.id == wanted) {
        return Ok(task);
    }
    let lower = wanted.to_lowercase();
    let same: Vec<&Listed> = all
        .iter()
        .filter(|t| t.title.to_lowercase() == lower)
        .collect();
    match same.len() {
        1 => Ok(same[0]),
        0 => Err(format!(
            "no task is called \"{wanted}\"; use list_schedule to see their ids"
        )),
        _ => Err(format!(
            "more than one task is called \"{wanted}\"; use its id from list_schedule"
        )),
    }
}

pub(crate) fn run(state: &AppState, desk: &mut tools::Desk, access: Access, call: &Call) -> Done {
    match call.name.as_str() {
        "list_schedule" => return schedule(state, call),
        "list_trash" => return trash_list(state),
        "search_chats" => return chat_search(state, call),
        _ => {}
    }
    let step = match call.name.as_str() {
        "add_task" => "Tried to add a task",
        "update_task" => "Tried to change a task",
        "move_note" => "Tried to move a note",
        "make_folder" => "Tried to make a folder",
        "move_folder" => "Tried to move a folder",
        "rename_note" => "Tried to rename a note",
        "rename_folder" => "Tried to rename a folder",
        "pin_note" => "Tried to pin a note",
        "restore_note" => "Tried to bring a note back",
        "delete_folder" => "Tried to delete a folder",
        _ => "Tried to delete a note",
    };
    if !access.changes() {
        return fail(
            step,
            "Felix is set to read only, so he cannot change notes, folders or tasks",
        );
    }
    if desk.proposals >= tools::MOST_PROPOSALS {
        return fail(
            step,
            format!(
                "at most {} changes fit in one answer",
                tools::MOST_PROPOSALS
            ),
        );
    }
    let made = match call.name.as_str() {
        "add_task" => add_task(call),
        "update_task" => update_task(state, call),
        "move_note" => move_note(state, desk, call),
        "make_folder" => make_folder(state, call),
        "move_folder" => move_folder(state, call),
        "rename_note" => rename_note(state, desk, call),
        "rename_folder" => rename_folder(state, call),
        "pin_note" => pin_note(state, desk, call),
        "restore_note" => restore_note(state, call),
        "delete_folder" => delete_folder(state, call),
        _ => delete_note(state, desk, call),
    };
    match made {
        Ok(proposal) => {
            desk.proposals += 1;
            let summary = summary_of(&proposal);
            Done {
                step: summary.clone(),
                result:
                    "Suggested. The user sees it and decides whether to do it; it is not done yet."
                        .into(),
                proposal: Some(proposal),
                found: vec![summary],
            }
        }
        Err(why) => fail(step, why),
    }
}

pub(crate) fn summary_of(p: &Proposal) -> String {
    match p {
        Proposal::AddTask { summary, .. }
        | Proposal::UpdateTask { summary, .. }
        | Proposal::DeleteTask { summary, .. }
        | Proposal::MoveNote { summary, .. }
        | Proposal::MakeFolder { summary, .. }
        | Proposal::RemoveFolder { summary, .. }
        | Proposal::MoveFolder { summary, .. }
        | Proposal::DeleteNote { summary, .. }
        | Proposal::RestoreNote { summary, .. }
        | Proposal::RenameNote { summary, .. }
        | Proposal::RenameFolder { summary, .. }
        | Proposal::PinNote { summary, .. }
        | Proposal::DeleteFolder { summary, .. }
        | Proposal::RestoreFolder { summary, .. } => summary.clone(),
        Proposal::Edit { title, .. } => format!("Change “{title}”"),
        Proposal::Create { title, .. } => format!("Make the note “{title}”"),
    }
}

const MOST_TITLE_CHARS: usize = 300;
const MOST_CHATS_SHOWN: usize = 5;
const MOST_CHAT_LINES: usize = 8;

fn rename_note(state: &AppState, desk: &tools::Desk, call: &Call) -> Result<Proposal, String> {
    let title: String = call
        .text("title")
        .trim()
        .chars()
        .take(MOST_TITLE_CHARS)
        .collect();
    if title.is_empty() {
        return Err("give the new title".into());
    }
    store_now(state, |store| {
        let note = desk
            .note_for(store, &call.text("note"))
            .map_err(|_| StatusCode::NOT_FOUND)?;
        Ok(Proposal::RenameNote {
            summary: format!("Rename “{}” to “{title}”", note.title),
            note: note.id.clone(),
            from: note.title.clone(),
            title: title.clone(),
        })
    })
    .map_err(|_| "leo could not find that note; use its id like n3 or its exact title".into())
}

fn rename_folder(state: &AppState, call: &Call) -> Result<Proposal, String> {
    let from = call.text("folder").trim().trim_matches('/').to_string();
    let name = call.text("new_name").trim().to_string();
    if name.is_empty()
        || name.contains(['/', '\\'])
        || name.chars().count() > leo_core::store::MOST_FOLDER_NAME_CHARS
    {
        return Err("give a new name without slashes".into());
    }
    store_now(state, |store| {
        if from.is_empty() || !store.dir_exists(&from) {
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(Proposal::RenameFolder {
            summary: format!("Rename the folder {from} to {name}"),
            from: from.clone(),
            name: name.clone(),
        })
    })
    .map_err(|_| format!("there is no folder \"{from}\""))
}

fn pin_note(state: &AppState, desk: &tools::Desk, call: &Call) -> Result<Proposal, String> {
    let pinned = !matches!(
        call.text("pinned").trim().to_lowercase().as_str(),
        "no" | "false"
    );
    store_now(state, |store| {
        let note = desk
            .note_for(store, &call.text("note"))
            .map_err(|_| StatusCode::NOT_FOUND)?;
        if note.pinned == pinned {
            return Err(StatusCode::CONFLICT);
        }
        Ok(Proposal::PinNote {
            summary: format!("{} “{}”", if pinned { "Pin" } else { "Unpin" }, note.title),
            note: note.id.clone(),
            title: note.title.clone(),
            pinned,
        })
    })
    .map_err(|code| {
        if code == StatusCode::CONFLICT {
            format!(
                "the note is already {}",
                if pinned { "pinned" } else { "unpinned" }
            )
        } else {
            "leo could not find that note; use its id like n3 or its exact title".into()
        }
    })
}

fn restore_note(state: &AppState, call: &Call) -> Result<Proposal, String> {
    let wanted = call.text("note").trim().to_string();
    let trashed = store_now(state, |store| Ok(store.trashed()))
        .map_err(|_| "leo could not read the trash".to_string())?;
    let lower = wanted.to_lowercase();
    let found: Vec<_> = trashed
        .iter()
        .filter(|t| t.id == wanted || t.title.to_lowercase() == lower)
        .collect();
    match found.as_slice() {
        [one] => Ok(Proposal::RestoreNote {
            summary: format!("Bring “{}” back from the trash", one.title),
            note: one.id.clone(),
            title: one.title.clone(),
        }),
        [] => Err(format!(
            "no note in the trash is called \"{wanted}\"; use list_trash"
        )),
        _ => Err(format!(
            "more than one trashed note is called \"{wanted}\"; use its id from list_trash"
        )),
    }
}

fn delete_folder(state: &AppState, call: &Call) -> Result<Proposal, String> {
    let path = call.text("folder").trim().trim_matches('/').to_string();
    store_now(state, |store| {
        if path.is_empty() || store.validate_directory(&path).is_err() || !store.dir_exists(&path) {
            return Err(StatusCode::NOT_FOUND);
        }
        let inside = format!("{path}/");
        let notes = store
            .notes
            .iter()
            .filter(|n| n.directory == path || n.directory.starts_with(&inside))
            .count();
        Ok(Proposal::DeleteFolder {
            summary: format!(
                "Move the folder {path} and its {notes} {} to the trash",
                if notes == 1 { "note" } else { "notes" }
            ),
            path: path.clone(),
        })
    })
    .map_err(|_| format!("there is no folder \"{path}\""))
}

fn trash_list(state: &AppState) -> Done {
    let step = "Looked in the trash";
    let trashed = match store_now(state, |store| Ok(store.trashed())) {
        Ok(t) => t,
        Err(_) => return fail(step, "leo could not read the trash"),
    };
    let mut trashed = trashed;
    trashed.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    let lines: Vec<String> = trashed
        .iter()
        .take(100)
        .map(|t| {
            format!(
                "- [{}] “{}” from {} · deleted {}",
                t.id,
                t.title,
                folder_label(&t.directory),
                t.deleted_at.with_timezone(&Local).format("%Y-%m-%d %H:%M")
            )
        })
        .collect();
    Done {
        step: step.into(),
        result: if lines.is_empty() {
            "The trash is empty.".into()
        } else {
            format!("Notes in the trash, newest first:\n{}", lines.join("\n"))
        },
        proposal: None,
        found: Vec::new(),
    }
}

fn chat_search(state: &AppState, call: &Call) -> Done {
    let query = call.text("query");
    let step = format!(
        "Searched your earlier chats for “{}”",
        query.chars().take(60).collect::<String>()
    );
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 2)
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() {
        return fail(&step, "give some words to look for");
    }
    let found = crate::chats::search(&state.chats, &query);
    let mut out = Vec::new();
    for summary in found.iter().take(MOST_CHATS_SHOWN) {
        let Some(chat) = crate::chats::load(&state.chats, &summary.id) else {
            continue;
        };
        let mut hits = Vec::new();
        for message in &chat.messages {
            let who = if message["role"] == "user" {
                "User"
            } else {
                "Felix"
            };
            let text = message["text"].as_str().unwrap_or_default();
            for line in text.lines() {
                let lower = line.to_lowercase();
                if words.iter().any(|w| lower.contains(w.as_str())) && hits.len() < MOST_CHAT_LINES
                {
                    hits.push(format!(
                        "  {who}: {}",
                        line.trim().chars().take(240).collect::<String>()
                    ));
                }
            }
        }
        out.push(format!(
            "- “{}” ({}, {} messages)\n{}",
            summary.title,
            summary.updated_at.with_timezone(&Local).format("%Y-%m-%d"),
            summary.count,
            hits.join("\n")
        ));
    }
    Done {
        step,
        result: if out.is_empty() {
            "No earlier chat mentions that.".into()
        } else {
            format!("Earlier chats that match:\n{}", out.join("\n"))
        },
        proposal: None,
        found: found
            .iter()
            .take(MOST_CHATS_SHOWN)
            .map(|s| s.title.clone())
            .collect(),
    }
}

fn add_task(call: &Call) -> Result<Proposal, String> {
    let title: String = call
        .text("title")
        .trim()
        .chars()
        .take(tasks::MOST_TITLE)
        .collect();
    if title.is_empty() {
        return Err("give the task a title".into());
    }
    let date = date_of(&call.text("date"))?;
    let time = if date.is_some() {
        time_of(&call.text("time"))?
    } else {
        None
    };
    let kind = kind_of(&call.text("type"))?;
    Ok(Proposal::AddTask {
        summary: format!(
            "Add the task “{title}”{}",
            if date.is_some() {
                format!(" · {}", when_label(date, time))
            } else {
                String::new()
            }
        ),
        title,
        date: date.map(|d| d.to_string()),
        time: time.map(|t| t.format("%H:%M").to_string()),
        task_kind: kind,
    })
}

fn update_task(state: &AppState, call: &Call) -> Result<Proposal, String> {
    let all = listed_now(state)?;
    let task = find_task(&all, &call.text("task"))?;
    let done = match call.text("done").trim().to_lowercase().as_str() {
        "" => None,
        "yes" | "true" | "done" => Some(true),
        "no" | "false" | "not done" => Some(false),
        _ => return Err("\"done\" must be yes or no".into()),
    };
    let date_text = call.text("date");
    let clear_date = date_text.trim().eq_ignore_ascii_case("none");
    let date = if clear_date {
        None
    } else {
        date_of(&date_text)?
    };
    let time = time_of(&call.text("time"))?;
    let kind = kind_of(&call.text("type"))?;
    if done.is_none() && date.is_none() && !clear_date && time.is_none() && kind.is_none() {
        return Err("say what to change: done, date, time or type".into());
    }
    if time.is_some() && date.is_none() && task.date.is_none() {
        return Err("give the task a date along with the time".into());
    }
    let mut parts = Vec::new();
    if let Some(done) = done {
        parts.push(if done {
            "mark it done".to_string()
        } else {
            "mark it not done".to_string()
        });
    }
    if clear_date {
        parts.push("clear its date".into());
    } else if date.is_some() || time.is_some() {
        parts.push(format!(
            "move it to {}",
            when_label(date.or(task.date), time.or(task.time))
        ));
    }
    if let Some(kind) = &kind {
        parts.push(format!(
            "make it {} {kind}",
            if kind.starts_with(['a', 'e', 'o']) {
                "an"
            } else {
                "a"
            }
        ));
    }
    Ok(Proposal::UpdateTask {
        summary: format!("“{}”: {}", task.title, parts.join(", ")),
        id: task.id.clone(),
        done,
        date: date.map(|d| d.to_string()),
        clear_date,
        time: time.map(|t| t.format("%H:%M").to_string()),
        task_kind: kind,
    })
}

fn move_note(state: &AppState, desk: &tools::Desk, call: &Call) -> Result<Proposal, String> {
    let to = call.text("folder").trim().trim_matches('/').to_string();
    store_now(state, |store| {
        let note = desk
            .note_for(store, &call.text("note"))
            .map_err(|_| StatusCode::NOT_FOUND)?;
        if store.validate_directory(&to).is_err() || !store.dir_exists(&to) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if note.directory == to {
            return Err(StatusCode::CONFLICT);
        }
        Ok(Proposal::MoveNote {
            summary: format!("Move “{}” to {}", note.title, folder_label(&to)),
            note: note.id.clone(),
            title: note.title.clone(),
            to: to.clone(),
        })
    })
    .map_err(|code| match code {
        StatusCode::NOT_FOUND => {
            "leo could not find that note; use its id like n3 or its exact title".into()
        }
        StatusCode::CONFLICT => "the note is already in that folder".into(),
        _ => format!("there is no folder \"{to}\"; make it first with make_folder"),
    })
}

fn make_folder(state: &AppState, call: &Call) -> Result<Proposal, String> {
    let path = call.text("folder").trim().trim_matches('/').to_string();
    store_now(state, |store| {
        if path.is_empty() || store.validate_directory(&path).is_err() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if store.dir_exists(&path) {
            return Err(StatusCode::CONFLICT);
        }
        Ok(Proposal::MakeFolder {
            summary: format!("Make the folder {path}"),
            path: path.clone(),
        })
    })
    .map_err(|code| match code {
        StatusCode::CONFLICT => format!("the folder \"{path}\" is there already"),
        _ => format!("\"{path}\" is not a folder name leo can use"),
    })
}

fn move_folder(state: &AppState, call: &Call) -> Result<Proposal, String> {
    let from = call.text("folder").trim().trim_matches('/').to_string();
    let into = call.text("into").trim().trim_matches('/').to_string();
    store_now(state, |store| {
        if from.is_empty() || !store.dir_exists(&from) {
            return Err(StatusCode::NOT_FOUND);
        }
        if store.validate_directory(&into).is_err() || !store.dir_exists(&into) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if into == from || into.starts_with(&format!("{from}/")) {
            return Err(StatusCode::CONFLICT);
        }
        Ok(Proposal::MoveFolder {
            summary: format!("Move the folder {from} into {}", folder_label(&into)),
            from: from.clone(),
            into: into.clone(),
        })
    })
    .map_err(|code| match code {
        StatusCode::NOT_FOUND => format!("there is no folder \"{from}\""),
        StatusCode::CONFLICT => "a folder cannot go inside itself".into(),
        _ => format!("there is no folder \"{into}\" to move it into"),
    })
}

fn delete_note(state: &AppState, desk: &tools::Desk, call: &Call) -> Result<Proposal, String> {
    store_now(state, |store| {
        let note = desk
            .note_for(store, &call.text("note"))
            .map_err(|_| StatusCode::NOT_FOUND)?;
        Ok(Proposal::DeleteNote {
            summary: format!("Move “{}” to the trash", note.title),
            note: note.id.clone(),
            title: note.title.clone(),
        })
    })
    .map_err(|_| "leo could not find that note; use its id like n3 or its exact title".into())
}

fn schedule(state: &AppState, call: &Call) -> Done {
    let step = "Looked at your calendar";
    let today = Local::now().date_naive();
    let from = match date_of(&call.text("from")) {
        Ok(d) => d.unwrap_or(today),
        Err(why) => return fail(step, why),
    };
    let days = call
        .text("days")
        .trim()
        .parse::<i64>()
        .unwrap_or(7)
        .clamp(1, MOST_DAYS);
    let to = from + Duration::days(days);
    let midnight = |d: NaiveDate| {
        d.and_hms_opt(0, 0, 0)
            .and_then(|t| Local.from_local_datetime(&t).earliest())
            .map(|t| t.with_timezone(&Utc))
    };
    let (Some(start), Some(end)) = (midnight(from), midnight(to)) else {
        return fail(step, "those dates could not be read");
    };
    let all = match listed_now(state) {
        Ok(all) => all,
        Err(why) => return fail(step, why),
    };
    let notes = match store_now(state, |store| Ok(store.notes_dir.clone())) {
        Ok(n) => n,
        Err(_) => return fail(step, "leo could not read the notes"),
    };
    let events = state.calendar.events(&notes, start, end);
    let task_line = |t: &Listed| {
        let from = t
            .note
            .as_ref()
            .map(|n| format!(" · from the note “{}”", n.title))
            .unwrap_or_default();
        let at = t
            .time
            .map(|tm| format!("{} ", tm.format("%H:%M")))
            .unwrap_or_default();
        format!(
            "- [task {}] {at}{}{} ({}){from}",
            t.id,
            t.title,
            if t.done { " · done" } else { "" },
            t.kind
        )
    };
    let mut lines = vec![format!(
        "Today is {}. Tasks and events from {} to {} (YYYY-MM-DD dates below):",
        today,
        from,
        to - Duration::days(1)
    )];
    let overdue: Vec<&Listed> = all
        .iter()
        .filter(|t| !t.done && t.date.is_some_and(|d| d < today))
        .collect();
    if !overdue.is_empty() {
        lines.push("Overdue:".into());
        lines.extend(overdue.iter().map(|t| {
            format!(
                "{} · was due {}",
                task_line(t),
                t.date.map(|d| d.to_string()).unwrap_or_default()
            )
        }));
    }
    let mut day = from;
    while day < to && lines.len() < MOST_LINES {
        let mut items: Vec<(String, String)> = Vec::new();
        for (_, calendar, e) in &events {
            let on = if e.all_day {
                e.date
                    .is_some_and(|d| d <= day && e.end_date.is_none_or(|end| day < end))
            } else {
                e.start.with_timezone(&Local).date_naive() == day
            };
            if !on {
                continue;
            }
            let when = if e.all_day {
                "all day".to_string()
            } else {
                format!(
                    "{}–{}",
                    e.start.with_timezone(&Local).format("%H:%M"),
                    e.end.with_timezone(&Local).format("%H:%M")
                )
            };
            let place = if e.location.is_empty() {
                String::new()
            } else {
                format!(" · {}", e.location)
            };
            items.push((
                if e.all_day {
                    "00:00".into()
                } else {
                    e.start.with_timezone(&Local).format("%H:%M").to_string()
                },
                format!("- {when} {}{place} (event in {calendar})", e.title),
            ));
        }
        for t in all.iter().filter(|t| t.date == Some(day)) {
            items.push((
                t.time
                    .map(|tm| tm.format("%H:%M").to_string())
                    .unwrap_or_else(|| "00:00".into()),
                task_line(t),
            ));
        }
        if !items.is_empty() {
            items.sort_by(|a, b| a.0.cmp(&b.0));
            lines.push(format!("{} {}:", day, day.format("%A")));
            lines.extend(items.into_iter().map(|(_, l)| l));
        }
        day += Duration::days(1);
    }
    let undated: Vec<&Listed> = all.iter().filter(|t| !t.done && t.date.is_none()).collect();
    if !undated.is_empty() {
        lines.push("No date:".into());
        lines.extend(undated.iter().take(60).map(|t| task_line(t)));
    }
    if lines.len() == 1 {
        lines.push("Nothing scheduled and no tasks.".into());
    }
    lines.truncate(MOST_LINES);
    Done {
        step: step.into(),
        result: lines.join("\n"),
        proposal: None,
        found: Vec::new(),
    }
}

async fn reply_of(response: Response) -> Result<serde_json::Value, String> {
    let ok = response.status().is_success();
    let bytes = axum::body::to_bytes(response.into_body(), 2_000_000)
        .await
        .map_err(|_| "leo could not read its own answer".to_string())?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    if ok {
        Ok(value)
    } else {
        Err(value["error"]
            .as_str()
            .unwrap_or("leo could not do that")
            .to_string())
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| "that change could not be read".to_string())
}

fn status_error(code: StatusCode) -> String {
    match code {
        StatusCode::NOT_FOUND => "it is not there any more".into(),
        StatusCode::CONFLICT => "something with that name is already there".into(),
        StatusCode::BAD_REQUEST => "that name cannot be used".into(),
        _ => "leo could not do that".into(),
    }
}

pub(crate) async fn act(
    state: &AppState,
    p: &Proposal,
) -> Result<(serde_json::Value, Proposal), String> {
    use crate::routes::{notes, tasks as task_routes, trash};
    match p {
        Proposal::AddTask {
            title,
            date,
            time,
            task_kind,
            ..
        } => {
            let made = reply_of(
                task_routes::create_task(
                    State(state.clone()),
                    Json(parse(serde_json::json!({ "title": title, "date": date, "time": time, "kind": task_kind.clone().unwrap_or_else(|| "other".into()) }))?),
                )
                .await,
            )
            .await?;
            let id = made["id"].as_str().unwrap_or_default().to_string();
            Ok((
                serde_json::json!({ "task": id }),
                Proposal::DeleteTask {
                    summary: format!("Remove the task “{title}”"),
                    id,
                },
            ))
        }
        Proposal::DeleteTask { id, .. } => {
            let gone =
                reply_of(task_routes::delete_task(State(state.clone()), Path(id.clone())).await)
                    .await?;
            let title = gone["title"].as_str().unwrap_or("the task").to_string();
            Ok((
                serde_json::Value::Null,
                Proposal::AddTask {
                    summary: format!("Add the task “{title}”"),
                    title,
                    date: gone["date"].as_str().map(str::to_string),
                    time: gone["time"].as_str().map(|t| t.chars().take(5).collect()),
                    task_kind: gone["kind"].as_str().map(str::to_string),
                },
            ))
        }
        Proposal::UpdateTask {
            id,
            done,
            date,
            clear_date,
            time,
            task_kind,
            ..
        } => {
            let all = listed_now(state)?;
            let before = all
                .iter()
                .find(|t| &t.id == id)
                .cloned()
                .ok_or("that task is not there any more")?;
            let mut change = serde_json::Map::new();
            if let Some(done) = done {
                change.insert("done".into(), serde_json::json!(done));
            }
            let new_date = if *clear_date {
                None
            } else {
                date.clone()
                    .or_else(|| time.as_ref().and(before.date.map(|d| d.to_string())))
            };
            if *clear_date || new_date.is_some() {
                change.insert("date".into(), serde_json::json!(new_date));
                let keep = time
                    .clone()
                    .or_else(|| before.time.map(|t| t.format("%H:%M").to_string()));
                change.insert(
                    "time".into(),
                    serde_json::json!(if *clear_date { None } else { keep }),
                );
                change.insert("confirm".into(), serde_json::json!(true));
            }
            if let Some(kind) = task_kind {
                change.insert("kind".into(), serde_json::json!(kind));
            }
            reply_of(
                task_routes::change_task(
                    State(state.clone()),
                    Path(id.clone()),
                    Json(parse(serde_json::Value::Object(change))?),
                )
                .await,
            )
            .await?;
            let dated = *clear_date || date.is_some() || time.is_some();
            Ok((
                serde_json::Value::Null,
                Proposal::UpdateTask {
                    summary: format!("Put “{}” back as it was", before.title),
                    id: id.clone(),
                    done: done.map(|_| before.done),
                    date: if dated {
                        before.date.map(|d| d.to_string())
                    } else {
                        None
                    },
                    clear_date: dated && before.date.is_none(),
                    time: if dated {
                        before.time.map(|t| t.format("%H:%M").to_string())
                    } else {
                        None
                    },
                    task_kind: task_kind.as_ref().map(|_| before.kind.clone()),
                },
            ))
        }
        Proposal::MoveNote {
            note, title, to, ..
        } => {
            let id = note.clone();
            let from = state
                .with_store(move |store| {
                    store
                        .find_note(&id)
                        .map(|n| n.directory.clone())
                        .ok_or(StatusCode::NOT_FOUND)
                })
                .await
                .map_err(status_error)?;
            let _ = notes::move_note(
                State(state.clone()),
                Path(note.clone()),
                Json(MoveBody {
                    directory: to.clone(),
                }),
            )
            .await
            .map_err(status_error)?;
            Ok((
                serde_json::Value::Null,
                Proposal::MoveNote {
                    summary: format!("Move “{title}” back to {}", folder_label(&from)),
                    note: note.clone(),
                    title: title.clone(),
                    to: from,
                },
            ))
        }
        Proposal::MakeFolder { path, .. } => {
            let _ = notes::create_dir(
                State(state.clone()),
                Json(CreateDirBody { path: path.clone() }),
            )
            .await
            .map_err(status_error)?;
            Ok((
                serde_json::Value::Null,
                Proposal::RemoveFolder {
                    summary: format!("Remove the empty folder {path}"),
                    path: path.clone(),
                },
            ))
        }
        Proposal::RemoveFolder { path, .. } => {
            let wanted = path.clone();
            state
                .with_store(move |store| {
                    if wanted.is_empty()
                        || store.validate_directory(&wanted).is_err()
                        || !store.dir_exists(&wanted)
                    {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    let inside = format!("{wanted}/");
                    if store
                        .notes
                        .iter()
                        .any(|n| n.directory == wanted || n.directory.starts_with(&inside))
                        || !store.subdirs(&wanted).is_empty()
                    {
                        return Err(StatusCode::CONFLICT);
                    }
                    store.delete_dir_recursive(&wanted);
                    save(store)
                })
                .await
                .map_err(|code| {
                    if code == StatusCode::CONFLICT {
                        "the folder has notes in it now, so leo left it".to_string()
                    } else {
                        status_error(code)
                    }
                })?;
            Ok((
                serde_json::Value::Null,
                Proposal::MakeFolder {
                    summary: format!("Make the folder {path}"),
                    path: path.clone(),
                },
            ))
        }
        Proposal::MoveFolder { from, into, .. } => {
            let parent = from
                .rsplit_once('/')
                .map_or(String::new(), |(p, _)| p.to_string());
            let moved = notes::move_dir(
                State(state.clone()),
                Json(MoveDirBody {
                    from: from.clone(),
                    into: into.clone(),
                }),
            )
            .await
            .map_err(status_error)?;
            let now = moved.0["path"].as_str().unwrap_or_default().to_string();
            Ok((
                serde_json::json!({ "path": now }),
                Proposal::MoveFolder {
                    summary: format!("Move the folder {now} back into {}", folder_label(&parent)),
                    from: now,
                    into: parent,
                },
            ))
        }
        Proposal::DeleteNote { note, title, .. } => {
            let code = notes::delete_note(State(state.clone()), Path(note.clone())).await;
            if !code.is_success() {
                return Err(status_error(code));
            }
            Ok((
                serde_json::Value::Null,
                Proposal::RestoreNote {
                    summary: format!("Bring “{title}” back from the trash"),
                    note: note.clone(),
                    title: title.clone(),
                },
            ))
        }
        Proposal::RestoreNote { note, title, .. } => {
            let _ = trash::restore_note(State(state.clone()), Path(note.clone()))
                .await
                .map_err(status_error)?;
            Ok((
                serde_json::Value::Null,
                Proposal::DeleteNote {
                    summary: format!("Move “{title}” to the trash"),
                    note: note.clone(),
                    title: title.clone(),
                },
            ))
        }
        Proposal::RenameNote {
            note, from, title, ..
        } => {
            let body: notes::UpdateBody = parse(serde_json::json!({ "title": title }))?;
            let _ = notes::update_note(State(state.clone()), Path(note.clone()), Json(body))
                .await
                .map_err(status_error)?;
            Ok((
                serde_json::Value::Null,
                Proposal::RenameNote {
                    summary: format!("Rename “{title}” back to “{from}”"),
                    note: note.clone(),
                    from: title.clone(),
                    title: from.clone(),
                },
            ))
        }
        Proposal::PinNote {
            note,
            title,
            pinned,
            ..
        } => {
            let body: notes::UpdateBody = parse(serde_json::json!({ "pinned": pinned }))?;
            let _ = notes::update_note(State(state.clone()), Path(note.clone()), Json(body))
                .await
                .map_err(status_error)?;
            Ok((
                serde_json::Value::Null,
                Proposal::PinNote {
                    summary: format!("{} “{title}”", if *pinned { "Unpin" } else { "Pin" }),
                    note: note.clone(),
                    title: title.clone(),
                    pinned: !pinned,
                },
            ))
        }
        Proposal::RenameFolder { from, name, .. } => {
            let old = from.rsplit('/').next().unwrap_or(from).to_string();
            let body: notes::RenameDirBody =
                parse(serde_json::json!({ "from": from, "name": name }))?;
            let renamed = notes::rename_dir(State(state.clone()), Json(body))
                .await
                .map_err(status_error)?;
            let now = renamed.0["path"].as_str().unwrap_or_default().to_string();
            Ok((
                serde_json::json!({ "path": now }),
                Proposal::RenameFolder {
                    summary: format!("Rename the folder {now} back to {old}"),
                    from: now,
                    name: old,
                },
            ))
        }
        Proposal::DeleteFolder { path, .. } => {
            let wanted = path.trim().trim_matches('/').to_string();
            if wanted.is_empty() {
                return Err("say which folder".into());
            }
            let body: trash::TrashMove =
                parse(serde_json::json!({ "notes": [], "dirs": [wanted] }))?;
            let gone = trash::move_to_trash(State(state.clone()), Json(body))
                .await
                .map_err(status_error)?;
            let ids: Vec<String> = gone.0["ids"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut dirs: Vec<String> = gone.0["dirs"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if !dirs.contains(&wanted) {
                dirs.insert(0, wanted.clone());
            }
            Ok((
                serde_json::Value::Null,
                Proposal::RestoreFolder {
                    summary: format!("Bring the folder {wanted} back from the trash"),
                    ids,
                    dirs,
                },
            ))
        }
        Proposal::RestoreFolder { ids, dirs, .. } => {
            let body: trash::TrashChoice = parse(serde_json::json!({ "ids": ids, "dirs": dirs }))?;
            let _ = trash::restore_many(State(state.clone()), Json(body))
                .await
                .map_err(status_error)?;
            let first = dirs.first().cloned().unwrap_or_default();
            Ok((
                serde_json::Value::Null,
                Proposal::DeleteFolder {
                    summary: format!("Move the folder {first} to the trash"),
                    path: first,
                },
            ))
        }
        Proposal::Edit { .. } | Proposal::Create { .. } => {
            Err("that change has its own Apply".into())
        }
    }
}

pub(crate) async fn act_route(State(state): State<AppState>, Json(p): Json<Proposal>) -> Response {
    match act(&state, &p).await {
        Ok((extra, undo)) => {
            let mut shown = serde_json::json!(p);
            if let (Some(shown), Some(extra)) = (shown.as_object_mut(), extra.as_object()) {
                shown.extend(extra.clone());
            }
            shown["state"] = serde_json::json!("applied");
            shown["undo"] = serde_json::json!(undo);
            Json(shown).into_response()
        }
        Err(why) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": why })),
        )
            .into_response(),
    }
}

pub(crate) fn act_now(state: &AppState, p: &Proposal) -> Result<serde_json::Value, String> {
    let handle = tokio::runtime::Handle::current();
    let (extra, undo) = handle.block_on(act(state, p))?;
    let mut shown = serde_json::json!(p);
    if let (Some(shown), Some(extra)) = (shown.as_object_mut(), extra.as_object()) {
        shown.extend(extra.clone());
    }
    shown["state"] = serde_json::json!("applied");
    shown["undo"] = serde_json::json!(undo);
    Ok(shown)
}
