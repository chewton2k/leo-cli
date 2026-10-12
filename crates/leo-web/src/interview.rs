use std::path::Path;

use chrono::{NaiveDate, NaiveTime, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::{chats, db};

pub const MOST_PLANS: usize = 100;
pub const MOST_ATTEMPTS: usize = 100;
pub const BODY_BYTES: usize = 1_000_000;
pub const MOST_ANSWER: usize = 32_000;

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
pub struct Interview {
    pub id: String,
    pub revision: u64,
    pub updated: String,
    pub framework: Framework,
    pub challenge: Option<Challenge>,
    pub workspace: Workspace,
    pub attempts: Vec<Attempt>,
    pub schedule: Vec<Slot>,
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
            text(&self.answer, MOST_ANSWER, false) && self.board.len() <= 100,
            "Keep your answer under 32,000 characters and your board under 100 shapes.",
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
                        .all(|p| p.is_finite() && (0.0..=1_000.0).contains(p)),
                "Use bounded board shapes and labels of at most 300 characters.",
            )?;
        }
        require(
            points <= 4_000 && labels <= 4_000,
            "Your board is full. Undo or clear some strokes before drawing more.",
        )
    }

    fn empty(&self) -> bool {
        self.answer.trim().is_empty() && self.board.is_empty()
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
        require(
            self.origin != "official" || self.sources.iter().any(|s| s.kind == "official"),
            "An official challenge needs an official source.",
        )
    }
}

fn validate_schedule(slots: &[Slot], framework: &Framework) -> Result<(), Error> {
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
    serde_json::from_str(&data.ok_or(Error::Missing)?).map_err(|_| Error::Storage)
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
        if let Err(why) = work(&mut item) {
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
                require(
                    item.challenge.as_ref().is_some_and(|c| c.id == challenge),
                    "The challenge changed. Reload before saving your answer.",
                )?;
                item.workspace = workspace;
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
                    workspace,
                    at: Utc::now().to_rfc3339(),
                    feedback: None,
                });
                item.workspace = Workspace::default();
                Ok(())
            })
        }
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
        require(
            !item.attempts.iter().any(|a| a.challenge.id == challenge.id)
                && item.challenge.as_ref().is_none_or(|c| c.id != challenge.id),
            "Use a new ID for a new challenge.",
        )?;
        require(item.workspace.empty(), "There is an unfinished answer. Ask the user to submit or clear their workspace before replacing the challenge.")?;
        item.challenge = Some(challenge);
        Ok(())
    })
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

pub fn set_schedule(path: &Path, id: &str, mut schedule: Vec<Slot>) -> Result<Interview, Error> {
    mutate(path, id, None, |item| {
        validate_schedule(&schedule, &item.framework)?;
        for slot in &mut schedule {
            slot.done = item
                .schedule
                .iter()
                .any(|old| old.id == slot.id && old.done);
        }
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
    serde_json::json!({"id":attempt.id,"challenge":attempt.challenge,"answer":attempt.workspace.answer,"board":board_material(&attempt.workspace),"feedback":attempt.feedback}).to_string()
}

pub fn context(item: &Interview) -> String {
    let attempts: Vec<_> = item.attempts.iter().rev().take(3).map(|a| {
        let feedback = a.feedback.as_ref().map(|f| serde_json::json!({"summary":crate::chat::clip(&f.summary, 500),"scores":f.scores.iter().map(|s| serde_json::json!({"skill":s.skill,"value":s.value,"evidence_excerpt":crate::chat::clip(&s.evidence, 150),"next":crate::chat::clip(&s.next, 150)})).collect::<Vec<_>>()}));
        serde_json::json!({"id":a.id,"title":a.challenge.title,"answer_excerpt":crate::chat::clip(&a.workspace.answer, 1000),"feedback":feedback})
    }).collect();
    let scores: Vec<_> = item.framework.skills.iter().map(|skill| {
        let values: Vec<_> = item.attempts.iter().filter_map(|a| a.feedback.as_ref()).flat_map(|f| &f.scores).filter(|s| &s.skill == skill).map(|s| s.value).collect();
        serde_json::json!({"skill":skill,"observations":values.len(),"latest_scores":values.iter().rev().take(5).collect::<Vec<_>>()})
    }).collect();
    let data = serde_json::json!({"id":item.id,"revision":item.revision,"framework":item.framework,"challenge":item.challenge,"draft_answer":crate::chat::clip(&item.workspace.answer, 8000),"board":board_material(&item.workspace),"latest_attempts":attempts,"assessed_scores":scores,"schedule":item.schedule});
    format!("<interview_material>\n{}\n</interview_material>\nFor full submitted work use interview_attempt with its ID before assessing it.\n", data)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            answer: "答え: use a hash map".into(),
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
        assert!(set_challenge(path, id, challenge("challenge-test-02")).is_err());
        let duplicate = set_challenge(path, id, challenge("challenge-test-01")).unwrap();
        assert_eq!(duplicate.workspace, saved.workspace);
        let submit = || Change::Submit {
            revision: duplicate.revision,
            challenge: "challenge-test-01".into(),
            attempt: "attempt-test-01".into(),
            workspace: workspace(),
        };
        let submitted = change(path, id, submit()).unwrap();
        assert!(submitted.workspace.empty());
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
        assert!(context(&item).contains("interview_attempt"));
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
        let material = context(&item);
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
        for point in [[f64::NAN, 1.0], [1001.0, 1.0], [-1.0, 1.0]] {
            let mut w = workspace();
            w.board[0].points = vec![point];
            assert!(w.validate().is_err());
        }
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
}
