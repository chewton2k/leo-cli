use std::path::Path;

use chrono::{NaiveDate, NaiveTime, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::{chats, db};

pub const MOST_PLANS: usize = 100;
pub const MOST_ATTEMPTS: usize = 100;
pub const MOST_PROBLEMS: usize = 128;
pub const BODY_BYTES: usize = 1_000_000;
pub const MOST_ANSWER: usize = 32_000;
pub const BOARD_LIMIT: f64 = 1_000_000.0;

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Conflict,
    Missing,
    Storage,
}

impl Error {
    pub fn message(&self) -> String {
        match self {
            Self::Invalid(why) => why.clone(),
            Self::Conflict => "This practice changed in another window. Reload it before saving; your draft is kept on this device.".into(),
            Self::Missing => "That interview practice is no longer here. Open Interview practice to start another.".into(),
            Self::Storage => "Leo could not save your interview practice. Check the space on your computer and try again.".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Framework {
    pub title: String,
    pub track: String,
    pub company: String,
    pub role: String,
    pub language: String,
    pub style: String,
    pub date: String,
    pub weekly_minutes: u32,
    pub skills: Vec<String>,
    pub wants: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub title: String,
    pub url: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub id: String,
    pub title: String,
    pub question: String,
    pub minutes: u32,
    pub skills: Vec<String>,
    pub origin: String,
    pub sources: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding: Option<Coding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Coding {
    pub language: String,
    pub starter: String,
    pub difficulty: String,
    pub examples: Vec<Example>,
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub input: String,
    pub output: String,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub kind: String,
    pub points: Vec<[f64; 2]>,
    pub text: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub answer: String,
    #[serde(default)]
    pub reasoning: String,
    pub board: Vec<Shape>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Score {
    pub skill: String,
    pub value: u32,
    pub evidence: String,
    pub next: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub attempt: String,
    pub summary: String,
    pub scores: Vec<Score>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attempt {
    pub id: String,
    pub challenge: Challenge,
    pub workspace: Workspace,
    pub at: String,
    pub feedback: Option<Feedback>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub id: String,
    pub date: String,
    pub time: String,
    pub title: String,
    pub minutes: u32,
    pub skill: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Problem {
    pub challenge: Challenge,
    pub workspace: Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Interview {
    pub id: String,
    pub revision: u64,
    pub updated: String,
    pub framework: Framework,
    pub challenge: Option<Challenge>,
    pub workspace: Workspace,
    pub attempts: Vec<Attempt>,
    pub schedule: Vec<Slot>,
    #[serde(default)]
    pub problems: Vec<Problem>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Framework {
        revision: u64,
        framework: Framework,
    },
    Workspace {
        revision: u64,
        challenge: String,
        workspace: Workspace,
    },
    Submit {
        revision: u64,
        challenge: String,
        attempt: String,
        workspace: Workspace,
    },
    Select {
        revision: u64,
        challenge: String,
    },
    Schedule {
        revision: u64,
        schedule: Vec<Slot>,
    },
}

fn require(ok: bool, why: &str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(why.into()))
    }
}

fn text(value: &str, most: usize, required: bool) -> bool {
    value.chars().count() <= most && (!required || !value.trim().is_empty())
}

fn date(value: &str, optional: bool) -> bool {
    (optional && value.is_empty())
        || (value.len() == 10 && NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok())
}

fn skills(values: &[String]) -> bool {
    !values.is_empty()
        && values.len() <= 12
        && values.iter().all(|s| text(s, 80, true))
        && values
            .iter()
            .enumerate()
            .all(|(i, s)| !values[..i].iter().any(|p| p.eq_ignore_ascii_case(s)))
}

impl Framework {
    pub fn validate(&self) -> Result<(), Error> {
        require(text(&self.title, 120, true) && text(&self.company, 120, false)
            && text(&self.role, 120, false) && text(&self.language, 80, false)
            && text(&self.wants, 2_000, false), "Keep the practice title, company and role under 120 characters, and your preferences under 2,000.")?;
        require(
            [
                "coding",
                "systems",
                "design",
                "marketing",
                "business",
                "quant",
                "custom",
            ]
            .contains(&self.track.as_str()),
            "Choose one of the interview tracks, or Custom.",
        )?;
        require(
            ["coach", "mock"].contains(&self.style.as_str()),
            "Choose Coach or Mock interview.",
        )?;
        require(
            date(&self.date, true) && (15..=2_400).contains(&self.weekly_minutes),
            "Choose a valid interview date and 15–2,400 practice minutes per week.",
        )?;
        require(
            skills(&self.skills),
            "Use 1–12 distinct skill criteria, each under 80 characters.",
        )
    }
}

impl Workspace {
    pub fn validate(&self) -> Result<(), Error> {
        require(
            text(&self.answer, MOST_ANSWER, false)
                && text(&self.reasoning, 8_000, false)
                && self.board.len() <= 100,
            "Keep your answer under 32,000 characters, reasoning under 8,000, and your board under 100 shapes.",
        )?;
        let mut points = 0;
        let mut labels = 0;
        for shape in &self.board {
            points += shape.points.len();
            labels += shape.text.chars().count();
            require(
                ["pen", "box", "arrow", "text"].contains(&shape.kind.as_str())
                    && !shape.points.is_empty()
                    && shape.points.len() <= 128
                    && text(&shape.text, 300, false)
                    && shape
                        .points
                        .iter()
                        .flatten()
                        .all(|p| p.is_finite() && p.abs() <= BOARD_LIMIT),
                "Use bounded board shapes and labels of at most 300 characters.",
            )?;
        }
        require(
            points <= 4_000 && labels <= 4_000,
            "Your board is full. Undo or clear some strokes before drawing more.",
        )
    }

    fn empty(&self) -> bool {
        self.answer.trim().is_empty() && self.reasoning.trim().is_empty() && self.board.is_empty()
    }

    fn untouched(&self, challenge: Option<&Challenge>) -> bool {
        self.empty()
            || (self.reasoning.trim().is_empty()
                && self.board.is_empty()
                && challenge
                    .and_then(|c| c.coding.as_ref())
                    .is_some_and(|c| self.answer.trim() == c.starter.trim()))
    }
}

impl Challenge {
    fn validate(&self, framework: &Framework) -> Result<(), Error> {
        require(chats::valid_id(&self.id) && text(&self.title, 160, true)
            && text(&self.question, 12_000, true) && (5..=180).contains(&self.minutes)
            && skills(&self.skills) && self.skills.iter().all(|s| framework.skills.contains(s)), "Give the challenge a stable 8–64 character ID, title, question, 5–180 minutes and skills from this framework.")?;
        require(["generated", "official", "reported"].contains(&self.origin.as_str())
            && self.sources.len() <= 6 && (self.origin == "generated" || !self.sources.is_empty()), "Label a challenge generated, official or reported; researched challenges need source links.")?;
        for source in &self.sources {
            require(
                text(&source.title, 200, true)
                    && text(&source.url, 2_000, true)
                    && source.url.starts_with("https://")
                    && !source.url.chars().any(char::is_whitespace)
                    && ["official", "reported"].contains(&source.kind.as_str()),
                "Use HTTPS source links and label each official or reported.",
            )?;
        }
        if let Some(coding) = &self.coding {
            require(framework.track == "coding"
                && text(&coding.language, 80, true) && text(&coding.starter, 8_000, true)
                && ["easy", "medium", "hard"].contains(&coding.difficulty.as_str())
                && !coding.examples.is_empty() && coding.examples.len() <= 8
                && coding.examples.iter().all(|e| text(&e.input, 2_000, true) && text(&e.output, 2_000, true) && text(&e.explanation, 1_000, false))
                && !coding.constraints.is_empty() && coding.constraints.len() <= 12
                && coding.constraints.iter().all(|c| text(c, 400, true)),
                "For coding, include a language, an unsolved starter under 8,000 characters, easy/medium/hard difficulty, 1–8 input/output examples and 1–12 constraints.")?;
        }
        require(
            self.origin != "official" || self.sources.iter().any(|s| s.kind == "official"),
            "An official challenge needs an official source.",
        )
    }
}

pub(crate) fn validate_schedule(slots: &[Slot], framework: &Framework) -> Result<(), Error> {
    require(
        slots.len() <= 60,
        "Keep a practice schedule to at most 60 sessions.",
    )?;
    let mut weeks = std::collections::BTreeMap::<String, u32>::new();
    for (i, slot) in slots.iter().enumerate() {
        require(chats::valid_id(&slot.id) && !slots[..i].iter().any(|s| s.id == slot.id)
            && date(&slot.date, false) && (slot.time.is_empty() || (slot.time.len() == 5 && NaiveTime::parse_from_str(&slot.time, "%H:%M").is_ok()))
            && text(&slot.title, 160, true) && (5..=180).contains(&slot.minutes)
            && framework.skills.contains(&slot.skill), "Give each session a unique ID, valid date/time, title, 5–180 minutes and a framework skill.")?;
        require(
            framework.date.is_empty() || slot.date <= framework.date,
            "Practice sessions must be on or before your interview date.",
        )?;
        let day = NaiveDate::parse_from_str(&slot.date, "%Y-%m-%d")
            .map_err(|_| Error::Invalid("Choose a valid session date.".into()))?;
        let minutes = weeks.entry(day.format("%G-%V").to_string()).or_default();
        *minutes += slot.minutes;
        require(*minutes <= framework.weekly_minutes, "This schedule exceeds your weekly practice budget. Shorten it or update the budget first.")?;
    }
    Ok(())
}

fn remember(item: &mut Interview) -> Result<(), Error> {
    if let Some(challenge) = &item.challenge {
        let problem = Problem {
            challenge: challenge.clone(),
            workspace: item.workspace.clone(),
        };
        if let Some(old) = item
            .problems
            .iter_mut()
            .find(|p| p.challenge.id == challenge.id)
        {
            *old = problem;
        } else {
            require(
                item.problems.len() < MOST_PROBLEMS,
                "This practice is full. Start another practice to add more.",
            )?;
            item.problems.push(problem);
        }
    }
    Ok(())
}

fn legacy_problems(item: &mut Interview) -> Result<(), Error> {
    if !item.problems.is_empty() {
        return Ok(());
    }
    for attempt in &item.attempts {
        let problem = Problem {
            challenge: attempt.challenge.clone(),
            workspace: attempt.workspace.clone(),
        };
        if let Some(old) = item
            .problems
            .iter_mut()
            .find(|p| p.challenge.id == attempt.challenge.id)
        {
            *old = problem;
        } else {
            require(
                item.problems.len() < MOST_PROBLEMS,
                "This practice is full. Start another practice to add more.",
            )?;
            item.problems.push(problem);
        }
    }
    if item.workspace.empty() {
        if let Some(previous) = item
            .problems
            .iter()
            .find(|p| Some(&p.challenge) == item.challenge.as_ref())
        {
            item.workspace = previous.workspace.clone();
        }
    }
    remember(item)
}

fn select(item: &mut Interview, challenge: &str) -> Result<(), Error> {
    let problem = item
        .problems
        .iter()
        .find(|p| p.challenge.id == challenge)
        .ok_or_else(|| Error::Invalid("Choose a saved problem from this practice.".into()))?;
    item.challenge = Some(problem.challenge.clone());
    item.workspace = problem.workspace.clone();
    Ok(())
}

pub fn load(path: &Path, id: &str) -> Result<Interview, Error> {
    require(chats::valid_id(id), "Choose a valid interview practice.")?;
    let data: Option<String> = db::for_chats(path)
        .with(|c| {
            c.query_row("SELECT data FROM interviews WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
        })
        .map_err(|_| Error::Storage)?;
    let mut item: Interview =
        serde_json::from_str(&data.ok_or(Error::Missing)?).map_err(|_| Error::Storage)?;
    legacy_problems(&mut item)?;
    Ok(item)
}

pub fn list(path: &Path) -> Result<Vec<serde_json::Value>, Error> {
    db::for_chats(path).with(|c| {
        let mut query = c.prepare("SELECT id, json_extract(data, '$.framework'), updated, json_array_length(data, '$.attempts'), length(CAST(data AS BLOB)) FROM interviews ORDER BY updated DESC LIMIT ?1")?;
        let rows = query.query_map([MOST_PLANS as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, u32>(3)?, r.get::<_, i64>(4)?)))?;
        let mut found = Vec::new();
        for row in rows {
            let (id, framework, updated, attempts, bytes) = row?;
            if let Ok(framework) = serde_json::from_str::<Framework>(&framework) {
                found.push(serde_json::json!({"id":id,"framework":framework,"updated":updated,"attempts":attempts,"bytes":bytes}));
            }
        }
        Ok(found)
    }).map_err(|_| Error::Storage)
}

pub fn remove(path: &Path, id: &str) -> Result<(), Error> {
    require(chats::valid_id(id), "Choose a valid interview practice.")?;
    db::for_chats(path)
        .with(|c| c.execute("DELETE FROM interviews WHERE id = ?1", [id]))
        .map_err(|_| Error::Storage)?;
    Ok(())
}

pub fn mutate(
    path: &Path,
    id: &str,
    revision: Option<u64>,
    work: impl FnOnce(&mut Interview) -> Result<(), Error>,
) -> Result<Interview, Error> {
    require(chats::valid_id(id), "Choose a valid interview practice.")?;
    let db = db::for_chats(path);
    let mut changed = None;
    let mut problem = None;
    db.with(|c| {
        let tx = c.transaction()?;
        let data: Option<String> = tx
            .query_row("SELECT data FROM interviews WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(data) = data else {
            problem = Some(Error::Missing);
            return Ok(());
        };
        let Ok(mut item) = serde_json::from_str::<Interview>(&data) else {
            problem = Some(Error::Storage);
            return Ok(());
        };
        if revision.is_some_and(|r| r != item.revision) {
            problem = Some(Error::Conflict);
            return Ok(());
        }
        if let Err(why) = legacy_problems(&mut item)
            .and_then(|()| work(&mut item))
            .and_then(|()| remember(&mut item))
        {
            problem = Some(why);
            return Ok(());
        }
        item.revision += 1;
        item.updated = Utc::now().to_rfc3339();
        let Ok(encoded) = serde_json::to_string(&item) else {
            problem = Some(Error::Storage);
            return Ok(());
        };
        if encoded.len() > BODY_BYTES {
            problem = Some(Error::Invalid(
                "This practice is full. Start a new plan to keep practising.".into(),
            ));
            return Ok(());
        }
        tx.execute(
            "UPDATE interviews SET data = ?1, updated = ?2 WHERE id = ?3",
            rusqlite::params![encoded, item.updated, id],
        )?;
        tx.commit()?;
        changed = Some(item);
        Ok(())
    })
    .map_err(|_| Error::Storage)?;
    changed.ok_or_else(|| problem.unwrap_or(Error::Storage))
}

pub fn configure(
    path: &Path,
    id: &str,
    revision: u64,
    framework: Framework,
) -> Result<Interview, Error> {
    framework.validate()?;
    require(chats::valid_id(id), "Choose a valid interview practice.")?;
    if revision != 0 {
        return mutate(path, id, Some(revision), |item| {
            validate_schedule(&item.schedule, &framework)?;
            item.framework = framework;
            Ok(())
        });
    }
    let item = Interview {
        id: id.into(),
        revision: 1,
        updated: Utc::now().to_rfc3339(),
        framework,
        challenge: None,
        workspace: Workspace::default(),
        attempts: Vec::new(),
        schedule: Vec::new(),
        problems: Vec::new(),
    };
    let data = serde_json::to_string(&item).map_err(|_| Error::Storage)?;
    let mut conflict = false;
    let mut full = false;
    db::for_chats(path)
        .with(|c| {
            let tx = c.transaction()?;
            conflict = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM interviews WHERE id = ?1)",
                [id],
                |r| r.get(0),
            )?;
            let count: i64 = tx.query_row("SELECT count(*) FROM interviews", [], |r| r.get(0))?;
            full = count >= MOST_PLANS as i64;
            if !conflict && !full {
                tx.execute(
                    "INSERT INTO interviews (id, updated, data) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, item.updated, data],
                )?;
            }
            tx.commit()
        })
        .map_err(|_| Error::Storage)?;
    if conflict {
        return Err(Error::Conflict);
    }
    require(
        !full,
        "You have 100 interview plans. Remove an old one before making another.",
    )?;
    Ok(item)
}

pub fn change(path: &Path, id: &str, change: Change) -> Result<Interview, Error> {
    match change {
        Change::Framework {
            revision,
            framework,
        } => configure(path, id, revision, framework),
        Change::Workspace {
            revision,
            challenge,
            workspace,
        } => {
            workspace.validate()?;
            mutate(path, id, Some(revision), |item| {
                if item.challenge.as_ref().is_some_and(|c| c.id == challenge) {
                    item.workspace = workspace;
                } else {
                    let problem = item
                        .problems
                        .iter_mut()
                        .find(|p| p.challenge.id == challenge)
                        .ok_or_else(|| {
                            Error::Invalid(
                                "The problem is no longer here. Your draft stays on this device."
                                    .into(),
                            )
                        })?;
                    problem.workspace = workspace;
                }
                Ok(())
            })
        }
        Change::Submit {
            revision,
            challenge,
            attempt,
            workspace,
        } => {
            workspace.validate()?;
            require(
                chats::valid_id(&attempt) && !workspace.empty(),
                "Write an answer or draw on the board before submitting.",
            )?;
            if let Ok(item) = load(path, id) {
                if item.attempts.iter().any(|a| {
                    a.id == attempt && a.challenge.id == challenge && a.workspace == workspace
                }) {
                    return Ok(item);
                }
            }
            mutate(path, id, Some(revision), |item| {
                require(item.attempts.len() < MOST_ATTEMPTS && !item.attempts.iter().any(|a| a.id == attempt), "This practice is full, or the attempt was already submitted. Start another practice or reload.")?;
                require(!workspace.untouched(item.challenge.as_ref()), "Add your implementation, reasoning or a drawing before submitting; the starter alone is not an answer.")?;
                let current = item
                    .challenge
                    .as_ref()
                    .filter(|c| c.id == challenge)
                    .ok_or_else(|| {
                        Error::Invalid(
                            "The challenge changed. Reload before submitting your answer.".into(),
                        )
                    })?;
                item.attempts.push(Attempt {
                    id: attempt,
                    challenge: current.clone(),
                    workspace: workspace.clone(),
                    at: Utc::now().to_rfc3339(),
                    feedback: None,
                });
                item.workspace = workspace;
                Ok(())
            })
        }
        Change::Select {
            revision,
            challenge,
        } => mutate(path, id, Some(revision), |item| select(item, &challenge)),
        Change::Schedule { revision, schedule } => mutate(path, id, Some(revision), |item| {
            validate_schedule(&schedule, &item.framework)?;
            item.schedule = schedule;
            Ok(())
        }),
    }
}

pub fn set_challenge(path: &Path, id: &str, challenge: Challenge) -> Result<Interview, Error> {
    mutate(path, id, None, |item| {
        challenge.validate(&item.framework)?;
        if item.challenge.as_ref() == Some(&challenge) {
            return Ok(());
        }
        if let Some(previous) = item
            .problems
            .iter()
            .find(|p| p.challenge.id == challenge.id)
        {
            require(
                previous.challenge == challenge,
                "Use a new ID for a new problem; saved problems cannot be overwritten.",
            )?;
            return select(item, &challenge.id);
        }
        require(
            item.problems.len() < MOST_PROBLEMS,
            "This practice is full. Start another practice to add more.",
        )?;
        item.workspace = Workspace {
            answer: challenge
                .coding
                .as_ref()
                .map(|c| c.starter.clone())
                .unwrap_or_default(),
            ..Workspace::default()
        };
        item.challenge = Some(challenge);
        Ok(())
    })
}

pub fn select_problem(path: &Path, id: &str, challenge: &str) -> Result<Interview, Error> {
    mutate(path, id, None, |item| select(item, challenge))
}

pub fn assess(path: &Path, id: &str, feedback: Feedback) -> Result<Interview, Error> {
    require(
        text(&feedback.summary, 4_000, true)
            && !feedback.scores.is_empty()
            && feedback.scores.len() <= 12,
        "Give a concise assessment and 1–12 skill scores.",
    )?;
    mutate(path, id, None, |item| {
        let attempt = item.attempts.iter_mut().find(|a| a.id == feedback.attempt).ok_or_else(|| Error::Invalid("That attempt was not submitted. Ask the user to press Review with Felix in their workspace; never invent an attempt.".into()))?;
        for (i, score) in feedback.scores.iter().enumerate() {
            require(attempt.challenge.skills.contains(&score.skill) && score.value <= 4
                && text(&score.evidence, 1_000, true) && text(&score.next, 500, true)
                && !feedback.scores[..i].iter().any(|s| s.skill == score.skill), "Score only this attempt's challenge skills from 0–4, with concrete evidence and a next step for each.")?;
        }
        attempt.feedback = Some(feedback);
        Ok(())
    })
}

pub fn prepare_schedule(item: &Interview, mut schedule: Vec<Slot>) -> Result<Vec<Slot>, Error> {
    validate_schedule(&schedule, &item.framework)?;
    for slot in &mut schedule {
        slot.done = item
            .schedule
            .iter()
            .any(|old| old.id == slot.id && old.done);
    }
    Ok(schedule)
}

pub fn apply_schedule(
    path: &Path,
    id: &str,
    before: &[Slot],
    schedule: Vec<Slot>,
) -> Result<Interview, Error> {
    mutate(path, id, None, |item| {
        if item.schedule.as_slice() != before {
            return Err(Error::Conflict);
        }
        validate_schedule(&schedule, &item.framework)?;
        item.schedule = schedule;
        Ok(())
    })
}

fn board_material(workspace: &Workspace) -> Vec<serde_json::Value> {
    workspace
        .board
        .iter()
        .map(|shape| {
            let ends: Vec<_> = shape
                .points
                .first()
                .into_iter()
                .chain(shape.points.last())
                .collect();
            serde_json::json!({"kind":shape.kind,"text":shape.text,"ends":ends})
        })
        .collect()
}

pub fn attempt_material(attempt: &Attempt) -> String {
    serde_json::json!({"id":attempt.id,"challenge":attempt.challenge,"answer":attempt.workspace.answer,"reasoning":attempt.workspace.reasoning,"board":board_material(&attempt.workspace),"feedback":attempt.feedback}).to_string()
}

pub fn context_problem(item: &Interview, wanted: Option<&str>) -> String {
    let problem = wanted
        .filter(|id| item.challenge.as_ref().is_none_or(|c| c.id != *id))
        .and_then(|id| item.problems.iter().find(|p| p.challenge.id == id));
    let challenge = problem.map(|p| &p.challenge).or(item.challenge.as_ref());
    let workspace = problem.map(|p| &p.workspace).unwrap_or(&item.workspace);
    let attempts: Vec<_> = item.attempts.iter().rev().take(3).map(|a| {
        let feedback = a.feedback.as_ref().map(|f| serde_json::json!({"summary":crate::chat::clip(&f.summary, 500),"scores":f.scores.iter().map(|s| serde_json::json!({"skill":s.skill,"value":s.value,"evidence_excerpt":crate::chat::clip(&s.evidence, 150),"next":crate::chat::clip(&s.next, 150)})).collect::<Vec<_>>()}));
        serde_json::json!({"id":a.id,"title":a.challenge.title,"answer_excerpt":crate::chat::clip(&a.workspace.answer, 1000),"feedback":feedback})
    }).collect();
    let scores: Vec<_> = item.framework.skills.iter().map(|skill| {
        let values: Vec<_> = item.attempts.iter().filter_map(|a| a.feedback.as_ref()).flat_map(|f| &f.scores).filter(|s| &s.skill == skill).map(|s| s.value).collect();
        serde_json::json!({"skill":skill,"observations":values.len(),"latest_scores":values.iter().rev().take(5).collect::<Vec<_>>()})
    }).collect();
    let active_problem = challenge.map(|c| serde_json::json!({"id":c.id,"title":c.title,"number":item.problems.iter().position(|p|p.challenge.id==c.id).map_or(1,|i|i+1)}));
    let data = serde_json::json!({"active_problem":active_problem,"id":item.id,"revision":item.revision,"framework":item.framework,"challenge":challenge,"draft_answer":crate::chat::clip(&workspace.answer, 8000),"draft_reasoning":workspace.reasoning,"board":board_material(workspace),"latest_attempts":attempts,"assessed_scores":scores,"schedule":item.schedule,"problems":item.problems.iter().enumerate().map(|(i,p)| serde_json::json!({"number":i+1,"id":p.challenge.id,"title":p.challenge.title,"submitted":item.attempts.iter().any(|a|a.challenge.id==p.challenge.id)})).collect::<Vec<_>>()});
    format!("<interview_material>\n{}\n</interview_material>\nUse interpretable language. The active_problem is the problem open in the workspace. References like this problem or my code refer to it and its current draft unless the user names a different problem. For full submitted work use interview_attempt with its ID before assessing it.\n", data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_schedule(path: &Path, id: &str, schedule: Vec<Slot>) -> Result<Interview, Error> {
        let item = load(path, id)?;
        let schedule = prepare_schedule(&item, schedule)?;
        apply_schedule(path, id, &item.schedule, schedule)
    }

    fn framework() -> Framework {
        Framework {
            title: "練習".into(),
            track: "coding".into(),
            company: "Example".into(),
            role: "Engineer".into(),
            language: "Python".into(),
            style: "coach".into(),
            date: "2026-11-01".into(),
            weekly_minutes: 120,
            skills: vec!["Approach".into(), "Testing".into()],
            wants: "Hints first".into(),
        }
    }

    fn challenge(id: &str) -> Challenge {
        Challenge {
            id: id.into(),
            title: "Pairs".into(),
            question: "Find a pair; check duplicates.".into(),
            minutes: 30,
            skills: vec!["Approach".into(), "Testing".into()],
            origin: "generated".into(),
            sources: vec![],
            coding: None,
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            answer: "答え: use a hash map".into(),
            reasoning: String::new(),
            board: vec![Shape {
                kind: "text".into(),
                text: "Client".into(),
                points: vec![[10.0, 30.0]],
            }],
        }
    }

    #[test]
    fn revisions_protect_drafts_and_submissions_freeze_work_without_needing_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let id = "interview-test-01";
        let first = configure(path, id, 0, framework()).unwrap();
        assert_eq!(first.revision, 1);
        assert!(matches!(
            configure(path, id, 0, framework()),
            Err(Error::Conflict)
        ));
        let current = set_challenge(path, id, challenge("challenge-test-01")).unwrap();
        let saved = change(
            path,
            id,
            Change::Workspace {
                revision: current.revision,
                challenge: "challenge-test-01".into(),
                workspace: workspace(),
            },
        )
        .unwrap();
        assert!(matches!(
            change(
                path,
                id,
                Change::Workspace {
                    revision: current.revision,
                    challenge: "challenge-test-01".into(),
                    workspace: Workspace::default()
                }
            ),
            Err(Error::Conflict)
        ));
        assert!(set_challenge(path, id, challenge("challenge-test-02")).is_ok());
        let duplicate = set_challenge(path, id, challenge("challenge-test-01")).unwrap();
        assert_eq!(duplicate.workspace, saved.workspace);
        let submit = || Change::Submit {
            revision: duplicate.revision,
            challenge: "challenge-test-01".into(),
            attempt: "attempt-test-01".into(),
            workspace: workspace(),
        };
        let submitted = change(path, id, submit()).unwrap();
        assert_eq!(submitted.workspace, workspace());
        let retried = change(path, id, submit()).unwrap();
        assert_eq!(retried.attempts.len(), 1);
        assert_eq!(retried.attempts[0].workspace.answer, "答え: use a hash map");
        let next = set_challenge(path, id, challenge("challenge-test-02")).unwrap();
        assert_eq!(next.attempts[0].challenge.id, "challenge-test-01");
        assert_eq!(list(path).unwrap().len(), 1);
        assert_eq!(
            load(path, id).unwrap().challenge.unwrap().id,
            "challenge-test-02"
        );
        remove(path, id).unwrap();
        assert!(matches!(load(path, id), Err(Error::Missing)));
    }

    #[test]
    fn feedback_cannot_invent_attempts_criteria_or_execution_and_remains_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let id = "interview-test-02";
        configure(dir.path(), id, 0, framework()).unwrap();
        let item = set_challenge(dir.path(), id, challenge("challenge-test-01")).unwrap();
        let feedback = Feedback {
            attempt: "attempt-test-01".into(),
            summary: "Review only; no execution".into(),
            scores: vec![Score {
                skill: "Approach".into(),
                value: 3,
                evidence: "You chose a hash map".into(),
                next: "Trace duplicates".into(),
            }],
        };
        assert!(assess(dir.path(), id, feedback.clone()).is_err());
        change(
            dir.path(),
            id,
            Change::Submit {
                revision: item.revision,
                challenge: "challenge-test-01".into(),
                attempt: "attempt-test-01".into(),
                workspace: workspace(),
            },
        )
        .unwrap();
        let mut bad = feedback.clone();
        bad.scores[0].value = 5;
        assert!(assess(dir.path(), id, bad).is_err());
        let mut bad = feedback.clone();
        bad.scores[0].skill = "Hiring probability".into();
        assert!(assess(dir.path(), id, bad).is_err());
        let mut bad = feedback.clone();
        bad.scores.push(bad.scores[0].clone());
        assert!(assess(dir.path(), id, bad).is_err());
        assess(dir.path(), id, feedback.clone()).unwrap();
        let item = assess(dir.path(), id, feedback).unwrap();
        assert_eq!(item.attempts.len(), 1);
        assert_eq!(item.attempts[0].feedback.as_ref().unwrap().scores.len(), 1);
        assert!(context_problem(&item, None).contains("interview_attempt"));
        assert!(attempt_material(&item.attempts[0]).contains("答え"));
    }

    #[test]
    fn schedules_validate_dates_budget_and_criteria_and_models_cannot_complete_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let id = "interview-test-03";
        configure(dir.path(), id, 0, framework()).unwrap();
        let slot = Slot {
            id: "schedule-test-01".into(),
            date: "2026-10-13".into(),
            time: "09:00".into(),
            title: "Testing".into(),
            minutes: 30,
            skill: "Testing".into(),
            done: true,
        };
        let item = set_schedule(dir.path(), id, vec![slot.clone()]).unwrap();
        assert!(!item.schedule[0].done);
        let item = change(
            dir.path(),
            id,
            Change::Schedule {
                revision: item.revision,
                schedule: vec![slot.clone()],
            },
        )
        .unwrap();
        assert!(item.schedule[0].done);
        assert!(
            set_schedule(dir.path(), id, vec![slot.clone()])
                .unwrap()
                .schedule[0]
                .done
        );
        for (day, time, minutes, skill) in [
            ("2026-10-32", "", 30, "Testing"),
            ("2026-11-02", "", 30, "Testing"),
            ("2026-10-13", "25:00", 30, "Testing"),
            ("2026-10-13", "", 121, "Testing"),
            ("2026-10-13", "", 30, "Missing"),
        ] {
            let mut bad = slot.clone();
            bad.date = day.into();
            bad.time = time.into();
            bad.minutes = minutes;
            bad.skill = skill.into();
            assert!(set_schedule(dir.path(), id, vec![bad]).is_err());
        }
        assert!(set_schedule(dir.path(), id, vec![slot.clone(), slot]).is_err());
    }

    #[test]
    fn schedule_approval_and_undo_check_the_plan_without_blocking_draft_edits() {
        let dir = tempfile::tempdir().unwrap();
        let id = "schedule-approval-01";
        configure(dir.path(), id, 0, framework()).unwrap();
        let item = set_challenge(dir.path(), id, coding_challenge("schedule-problem-01")).unwrap();
        let slot = Slot {
            id: "schedule-session-01".into(),
            date: "2026-10-13".into(),
            time: "09:00".into(),
            title: "Testing".into(),
            minutes: 30,
            skill: "Testing".into(),
            done: false,
        };
        change(
            dir.path(),
            id,
            Change::Workspace {
                revision: item.revision,
                challenge: "schedule-problem-01".into(),
                workspace: workspace(),
            },
        )
        .unwrap();
        let applied = apply_schedule(dir.path(), id, &[], vec![slot.clone()]).unwrap();
        assert_eq!(applied.workspace, workspace());
        assert!(matches!(
            apply_schedule(dir.path(), id, &[], vec![]),
            Err(Error::Conflict)
        ));
        let undone = apply_schedule(dir.path(), id, &[slot.clone()], vec![]).unwrap();
        assert!(undone.schedule.is_empty());
        let applied = apply_schedule(dir.path(), id, &[], vec![slot.clone()]).unwrap();
        let mut completed = slot.clone();
        completed.done = true;
        change(
            dir.path(),
            id,
            Change::Schedule {
                revision: applied.revision,
                schedule: vec![completed.clone()],
            },
        )
        .unwrap();
        assert!(matches!(
            apply_schedule(dir.path(), id, &[slot], vec![]),
            Err(Error::Conflict)
        ));
        assert_eq!(load(dir.path(), id).unwrap().schedule, vec![completed]);
    }

    #[test]
    fn large_progress_is_complete_json_and_full_submissions_stay_available() {
        let dir = tempfile::tempdir().unwrap();
        let id = "interview-test-04";
        let mut f = framework();
        f.skills = (0..12).map(|n| format!("Skill {n}")).collect();
        let mut item = configure(dir.path(), id, 0, f).unwrap();
        let mut c = challenge("challenge-large-01");
        c.skills = item.framework.skills.clone();
        c.question = "界".repeat(12_000);
        let work = Workspace {
            answer: "答".repeat(MOST_ANSWER),
            reasoning: String::new(),
            board: vec![],
        };
        let feedback = Feedback {
            attempt: "attempt-large-01".into(),
            summary: "評".repeat(4_000),
            scores: item
                .framework
                .skills
                .iter()
                .map(|skill| Score {
                    skill: skill.clone(),
                    value: 2,
                    evidence: "根".repeat(1_000),
                    next: "次".repeat(500),
                })
                .collect(),
        };
        item.challenge = Some(c.clone());
        item.workspace = work.clone();
        item.attempts = (0..3)
            .map(|n| Attempt {
                id: format!("attempt-large-{n:02}"),
                challenge: c.clone(),
                workspace: work.clone(),
                at: Utc::now().to_rfc3339(),
                feedback: Some(feedback.clone()),
            })
            .collect();
        let material = context_problem(&item, None);
        let body = material
            .split_once("<interview_material>\n")
            .unwrap()
            .1
            .split_once("\n</interview_material>")
            .unwrap()
            .0;
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["latest_attempts"].as_array().unwrap().len(), 3);
        assert_eq!(
            parsed["challenge"]["question"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            12_000
        );
        let full: serde_json::Value =
            serde_json::from_str(&attempt_material(&item.attempts[0])).unwrap();
        assert_eq!(
            full["answer"].as_str().unwrap().chars().count(),
            MOST_ANSWER
        );
    }

    #[test]
    fn frameworks_challenges_and_boards_reject_the_entire_class_of_unbounded_or_malformed_input() {
        for track in [
            "coding",
            "systems",
            "design",
            "marketing",
            "business",
            "quant",
            "custom",
        ] {
            let mut f = framework();
            f.track = track.into();
            assert!(f.validate().is_ok());
        }
        for title in ["".into(), "界".repeat(121)] {
            let mut f = framework();
            f.title = title;
            assert!(f.validate().is_err());
        }
        let mut f = framework();
        f.skills.push("approach".into());
        assert!(f.validate().is_err());
        let mut c = challenge("challenge-test-01");
        c.origin = "official".into();
        assert!(c.validate(&framework()).is_err());
        c.sources.push(Reference {
            title: "Official".into(),
            url: "javascript:alert(1)".into(),
            kind: "official".into(),
        });
        assert!(c.validate(&framework()).is_err());
        c.sources[0].url = "https://example.com/preparation".into();
        assert!(c.validate(&framework()).is_ok());
        for point in [
            [f64::NAN, 1.0],
            [BOARD_LIMIT + 1.0, 1.0],
            [-BOARD_LIMIT - 1.0, 1.0],
        ] {
            let mut w = workspace();
            w.board[0].points = vec![point];
            assert!(w.validate().is_err());
        }
        let mut extended = workspace();
        extended.board[0].points = vec![[-2000.0, 3000.0]];
        assert!(extended.validate().is_ok());
        let mut w = workspace();
        w.board[0].points = vec![[1.0, 2.0]; 129];
        assert!(w.validate().is_err());
        let mut w = workspace();
        w.answer = "界".repeat(MOST_ANSWER + 1);
        assert!(w.validate().is_err());
        let mut w = workspace();
        w.board = vec![w.board[0].clone(); 101];
        assert!(w.validate().is_err());
    }

    fn coding_challenge(id: &str) -> Challenge {
        let mut c = challenge(id);
        c.coding = Some(Coding {
            language: "Python".into(),
            starter: "def pair_sum(nums, target):\n    pass".into(),
            difficulty: "easy".into(),
            examples: vec![Example {
                input: "nums=[2,3], target=5".into(),
                output: "[0,1]".into(),
                explanation: String::new(),
            }],
            constraints: vec!["Two or more values; exactly one pair.".into()],
        });
        c
    }

    #[test]
    fn coding_starters_are_seeded_once_and_never_count_as_submitted_work() {
        let dir = tempfile::tempdir().unwrap();
        let id = "interview-starter-01";
        configure(dir.path(), id, 0, framework()).unwrap();
        let c = coding_challenge("challenge-starter-01");
        let seeded = set_challenge(dir.path(), id, c.clone()).unwrap();
        assert_eq!(seeded.workspace.answer, c.coding.as_ref().unwrap().starter);
        assert!(change(
            dir.path(),
            id,
            Change::Submit {
                revision: seeded.revision,
                challenge: c.id.clone(),
                attempt: "attempt-starter-01".into(),
                workspace: seeded.workspace.clone()
            }
        )
        .is_err());
        let next = set_challenge(dir.path(), id, coding_challenge("challenge-starter-02")).unwrap();
        let mut work = next.workspace.clone();
        work.reasoning = "A hash map handles duplicates.".into();
        let saved = change(
            dir.path(),
            id,
            Change::Workspace {
                revision: next.revision,
                challenge: "challenge-starter-02".into(),
                workspace: work.clone(),
            },
        )
        .unwrap();
        assert_eq!(saved.workspace, work);
        let repeat =
            set_challenge(dir.path(), id, coding_challenge("challenge-starter-02")).unwrap();
        assert_eq!(repeat.workspace, work);
        assert!(set_challenge(dir.path(), id, coding_challenge("challenge-starter-03")).is_ok());
        select_problem(dir.path(), id, "challenge-starter-02").unwrap();
        let submitted = change(
            dir.path(),
            id,
            Change::Submit {
                revision: load(dir.path(), id).unwrap().revision,
                challenge: "challenge-starter-02".into(),
                attempt: "attempt-starter-02".into(),
                workspace: work,
            },
        )
        .unwrap();
        assert!(attempt_material(&submitted.attempts[0]).contains("A hash map handles duplicates."));
        assert_eq!(
            load(dir.path(), id).unwrap().attempts[0].challenge.coding,
            next.challenge.unwrap().coding
        );
    }

    #[test]
    fn legacy_challenges_and_workspaces_remain_readable() {
        let mut data = serde_json::to_value(challenge("challenge-legacy-01")).unwrap();
        data.as_object_mut().unwrap().remove("coding");
        assert!(serde_json::from_value::<Challenge>(data)
            .unwrap()
            .coding
            .is_none());
        let legacy: Workspace =
            serde_json::from_value(serde_json::json!({"answer":"old draft","board":[]})).unwrap();
        assert_eq!(legacy.reasoning, "");
        assert_eq!(legacy.answer, "old draft");
    }

    #[test]
    fn new_problems_preserve_unsubmitted_code_reasoning_and_boards() {
        let dir = tempfile::tempdir().unwrap();
        let id = "interview-problems-01";
        configure(dir.path(), id, 0, framework()).unwrap();
        let first =
            set_challenge(dir.path(), id, coding_challenge("challenge-problems-01")).unwrap();
        let work = workspace();
        change(
            dir.path(),
            id,
            Change::Workspace {
                revision: first.revision,
                challenge: "challenge-problems-01".into(),
                workspace: work.clone(),
            },
        )
        .unwrap();
        let next =
            set_challenge(dir.path(), id, coding_challenge("challenge-problems-02")).unwrap();
        let data = serde_json::to_value(load(dir.path(), id).unwrap()).unwrap();
        assert_eq!(data["problems"].as_array().unwrap().len(), 2);
        assert_eq!(
            data["problems"][0]["workspace"],
            serde_json::to_value(work).unwrap()
        );
        let material: serde_json::Value =
            serde_json::from_str(context_problem(&next, None).lines().nth(1).unwrap()).unwrap();
        assert_eq!(material["active_problem"]["number"], 2);
        assert_eq!(material["active_problem"]["id"], "challenge-problems-02");
        let previous: serde_json::Value = serde_json::from_str(
            context_problem(&next, Some("challenge-problems-01"))
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(previous["active_problem"]["number"], 1);
        assert_eq!(previous["draft_answer"], workspace().answer);
        assert_eq!(previous["draft_reasoning"], workspace().reasoning);
        assert_eq!(next.challenge.unwrap().id, "challenge-problems-02");
        let restored = select_problem(dir.path(), id, "challenge-problems-01").unwrap();
        assert_eq!(restored.workspace, workspace());
        assert_eq!(restored.problems.len(), 2);
        let old_revision = restored.revision;
        let duplicate =
            set_challenge(dir.path(), id, coding_challenge("challenge-problems-01")).unwrap();
        assert_eq!(duplicate.workspace, workspace());
        assert_eq!(duplicate.problems.len(), 2);
        assert!(matches!(
            change(
                dir.path(),
                id,
                Change::Select {
                    revision: old_revision,
                    challenge: "challenge-problems-02".into()
                }
            ),
            Err(Error::Conflict)
        ));
        assert!(select_problem(dir.path(), id, "challenge-missing-01").is_err());
        let mut collision = coding_challenge("challenge-problems-01");
        collision.title = "Replacement".into();
        assert!(set_challenge(dir.path(), id, collision).is_err());
        let item = load(dir.path(), id).unwrap();
        assert_eq!(item.workspace, workspace());
        assert_eq!(item.problems.len(), 2);
    }

    #[test]
    fn problem_history_is_bounded_and_legacy_attempts_remain_reopenable() {
        let dir = tempfile::tempdir().unwrap();
        let id = "interview-history-01";
        configure(dir.path(), id, 0, framework()).unwrap();
        for n in 0..MOST_PROBLEMS {
            set_challenge(
                dir.path(),
                id,
                coding_challenge(&format!("challenge-history-{n:03}")),
            )
            .unwrap();
        }
        assert!(set_challenge(
            dir.path(),
            id,
            coding_challenge("challenge-history-overflow")
        )
        .is_err());
        assert_eq!(load(dir.path(), id).unwrap().problems.len(), MOST_PROBLEMS);
        select_problem(dir.path(), id, "challenge-history-000").unwrap();
        let mut item = load(dir.path(), id).unwrap();
        item.problems.clear();
        item.attempts = vec![Attempt {
            id: "attempt-history-01".into(),
            challenge: coding_challenge("challenge-history-001"),
            workspace: workspace(),
            at: Utc::now().to_rfc3339(),
            feedback: None,
        }];
        item.workspace = Workspace::default();
        item.challenge = Some(item.attempts[0].challenge.clone());
        let mut data = serde_json::to_value(item).unwrap();
        data.as_object_mut().unwrap().remove("problems");
        db::for_chats(dir.path())
            .with(|c| {
                c.execute(
                    "UPDATE interviews SET data = ?1 WHERE id = ?2",
                    rusqlite::params![data.to_string(), id],
                )
            })
            .unwrap();
        let restored = load(dir.path(), id).unwrap();
        assert_eq!(restored.problems.len(), 1);
        assert_eq!(restored.workspace, workspace());
        let current =
            set_challenge(dir.path(), id, coding_challenge("challenge-history-002")).unwrap();
        assert_eq!(current.problems.len(), 2);
        let previous = select_problem(dir.path(), id, "challenge-history-001").unwrap();
        assert_eq!(previous.workspace, workspace());
        let saved = change(
            dir.path(),
            id,
            Change::Workspace {
                revision: previous.revision,
                challenge: "challenge-history-002".into(),
                workspace: workspace(),
            },
        )
        .unwrap();
        assert_eq!(
            saved.challenge.as_ref().unwrap().id,
            "challenge-history-001"
        );
        assert_eq!(saved.problems[1].workspace, workspace());
    }

    #[test]
    fn coding_payloads_bound_every_field_and_reject_wrong_tracks() {
        let c = coding_challenge("challenge-bounds-01");
        assert!(c.validate(&framework()).is_ok());
        for n in 0..8 {
            let mut bad = c.clone();
            let code = bad.coding.as_mut().unwrap();
            match n {
                0 => code.starter = "x".repeat(8001),
                1 => code.language.clear(),
                2 => code.difficulty = "unknown".into(),
                3 => code.examples.clear(),
                4 => code.examples = vec![code.examples[0].clone(); 9],
                5 => code.examples[0].input = "x".repeat(2001),
                6 => code.constraints = vec!["x".into(); 13],
                _ => code.constraints[0] = "x".repeat(401),
            }
            assert!(bad.validate(&framework()).is_err());
        }
        let mut other = framework();
        other.track = "systems".into();
        assert!(c.validate(&other).is_err());
        let work = Workspace {
            reasoning: "答".repeat(8001),
            ..Workspace::default()
        };
        assert!(work.validate().is_err());
    }
}
