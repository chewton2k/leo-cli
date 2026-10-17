use std::path::Path;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::interview::{self, Attempt, Error, Interview, Problem, Workspace};

const MOST_UNDO: usize = 20;
pub(crate) const UNDO_HOURS: i64 = 24;
const MOST_SNAPSHOT: usize = interview::BODY_BYTES * 2;

#[derive(Serialize, Deserialize)]
struct Saved {
    problem: Problem,
    attempts: Vec<Attempt>,
    index: usize,
    selected: Option<String>,
    #[serde(default)]
    key: Option<(String, String, String)>,
}

fn storage(_: impl std::fmt::Display) -> Error {
    Error::Storage
}

pub fn delete(
    path: &Path,
    id: &str,
    revision: u64,
    challenge: &str,
) -> Result<(Interview, String), Error> {
    let token = uuid::Uuid::new_v4().to_string();
    let item = interview::mutate_tx(path, id, Some(revision), |item, tx| {
        let index = item
            .problems
            .iter()
            .position(|p| p.challenge.id == challenge)
            .ok_or_else(|| {
                Error::Invalid(
                    "That problem is no longer here. Choose another saved problem.".into(),
                )
            })?;
        let selected = item.challenge.as_ref().map(|c| c.id.clone());
        let mut problem = item.problems[index].clone();
        if selected.as_deref() == Some(challenge) {
            problem.workspace = item.workspace.clone();
        }
        let key = tx.query_row(
            "SELECT status, updated, data FROM interview_keys WHERE interview = ?1 AND challenge = ?2",
            params![id, challenge],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        ).optional().map_err(storage)?;
        let saved = Saved {
            problem,
            attempts: item
                .attempts
                .iter()
                .filter(|a| a.challenge.id == challenge)
                .cloned()
                .collect(),
            index,
            selected,
            key: key.filter(|(status, _, _)| status != "preparing"),
        };
        let encoded = serde_json::to_string(&saved).map_err(storage)?;
        if encoded.len() > MOST_SNAPSHOT {
            return Err(Error::Invalid(
                "This problem is too large to keep for Undo. Remove it from Storage instead."
                    .into(),
            ));
        }
        let now = Utc::now();
        let cutoff = (now - chrono::Duration::hours(UNDO_HOURS)).to_rfc3339();
        tx.execute(
            "DELETE FROM interview_problem_undo WHERE updated < ?1",
            [cutoff],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM interview_problem_undo WHERE id IN (SELECT id FROM interview_problem_undo ORDER BY updated DESC, id DESC LIMIT -1 OFFSET ?1)", [(MOST_UNDO - 1) as i64]).map_err(storage)?;
        tx.execute("INSERT INTO interview_problem_undo (id, interview, updated, data) VALUES (?1, ?2, ?3, ?4)", params![token, id, now.to_rfc3339(), encoded]).map_err(storage)?;
        tx.execute(
            "DELETE FROM interview_keys WHERE interview = ?1 AND challenge = ?2",
            params![id, challenge],
        )
        .map_err(storage)?;
        item.problems.remove(index);
        item.attempts.retain(|a| a.challenge.id != challenge);
        if saved.selected.as_deref() == Some(challenge) {
            if let Some(next) = item
                .problems
                .get(index.min(item.problems.len().saturating_sub(1)))
            {
                item.challenge = Some(next.challenge.clone());
                item.workspace = next.workspace.clone();
            } else {
                item.challenge = None;
                item.workspace = Workspace::default();
            }
        }
        Ok(())
    })?;
    Ok((item, token))
}

pub fn restore(path: &Path, id: &str, token: &str) -> Result<(Interview, String), Error> {
    if uuid::Uuid::parse_str(token).is_err() {
        return Err(Error::Invalid(
            "That Undo is no longer available. Choose a recent problem deletion.".into(),
        ));
    }
    let mut restored = String::new();
    let item = interview::mutate_tx(path, id, None, |item, tx| {
        let cutoff = (Utc::now() - chrono::Duration::hours(UNDO_HOURS)).to_rfc3339();
        let raw: Option<String> = tx.query_row(
            "SELECT data FROM interview_problem_undo WHERE id = ?1 AND interview = ?2 AND updated >= ?3 AND length(CAST(data AS BLOB)) <= ?4",
            params![token, id, cutoff, MOST_SNAPSHOT as i64],
            |r| r.get(0),
        ).optional().map_err(storage)?;
        let raw = raw.ok_or_else(|| Error::Invalid("That Undo expired or was cleared in Storage. Your remaining problems are unchanged.".into()))?;
        let saved: Saved = serde_json::from_str(&raw).map_err(storage)?;
        let challenge = &saved.problem.challenge.id;
        if item.problems.iter().any(|p| p.challenge.id == *challenge)
            || item.problems.len() >= interview::MOST_PROBLEMS
            || item.attempts.len() + saved.attempts.len() > interview::MOST_ATTEMPTS
            || saved
                .attempts
                .iter()
                .any(|a| item.attempts.iter().any(|old| old.id == a.id))
        {
            return Err(Error::Conflict);
        }
        saved.problem.challenge.validate(&item.framework)?;
        saved.problem.workspace.validate()?;
        let active = item.challenge.as_ref().map(|c| c.id.clone());
        if let Some(active) = active {
            if let Some(problem) = item.problems.iter_mut().find(|p| p.challenge.id == active) {
                problem.workspace = item.workspace.clone();
            }
        }
        if saved.selected.as_deref() == Some(challenge) || item.challenge.is_none() {
            item.challenge = Some(saved.problem.challenge.clone());
            item.workspace = saved.problem.workspace.clone();
        }
        item.problems
            .insert(saved.index.min(item.problems.len()), saved.problem.clone());
        item.attempts.extend(saved.attempts);
        item.attempts
            .sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
        if let Some((status, updated, data)) = saved.key {
            tx.execute("INSERT OR REPLACE INTO interview_keys (interview, challenge, status, updated, data) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, challenge, status, updated, data]).map_err(storage)?;
        }
        tx.execute("DELETE FROM interview_problem_undo WHERE id = ?1", [token])
            .map_err(storage)?;
        restored = challenge.clone();
        Ok(())
    })?;
    Ok((item, restored))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, storage_saved};

    fn seed(path: &Path, id: &str) -> Interview {
        let framework = serde_json::from_value(serde_json::json!({"title":"Coding", "track":"coding", "company":"", "role":"", "language":"Python", "style":"coach", "date":"", "weekly_minutes":120, "skills":["Approach"], "wants":""})).unwrap();
        interview::configure(path, id, 0, framework).unwrap();
        for index in 0..3 {
            let problem = serde_json::from_value(serde_json::json!({"id":format!("problem-delete-{index}"), "title":"Same title", "question":"Find a pair.", "minutes":30, "skills":["Approach"], "topics":["Arrays"], "origin":"generated", "sources":[], "coding":{"language":"Python", "starter":"def solve():\n    pass", "difficulty":"easy", "examples":[{"input":"1","output":"1"}], "constraints":["Small"]}})).unwrap();
            let item = interview::set_challenge(path, id, problem).unwrap();
            interview::change(
                path,
                id,
                interview::Change::Submit {
                    revision: item.revision,
                    challenge: format!("problem-delete-{index}"),
                    attempt: format!("attempt-delete-{index}"),
                    workspace: Workspace {
                        answer: format!("implementation {index}"),
                        ..Workspace::default()
                    },
                    tests: vec![],
                },
            )
            .unwrap();
            db::for_chats(path)
                .with(|c| {
                    c.execute(
                        "INSERT INTO interview_keys VALUES (?1, ?2, 'ready', 'now', ?3)",
                        params![
                            id,
                            format!("problem-delete-{index}"),
                            format!("private key {index}")
                        ],
                    )
                })
                .unwrap();
        }
        interview::load(path, id).unwrap()
    }

    #[test]
    fn deleting_active_inactive_and_final_problems_preserves_remaining_work_and_restores_full_snapshots(
    ) {
        for index in [0, 2, 1] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path();
            let before = seed(path, "practice-delete-01");
            let challenge = format!("problem-delete-{index}");
            assert!(matches!(
                delete(path, &before.id, before.revision - 1, &challenge),
                Err(Error::Conflict)
            ));
            let (after, token) = delete(path, &before.id, before.revision, &challenge).unwrap();
            assert_eq!(after.problems.len(), 2);
            assert_eq!(after.attempts.len(), 2);
            assert!(after.problems.iter().all(|p| p.challenge.id != challenge));
            assert!(crate::interview_keys::status(path, &before.id, &challenge).is_none());
            assert_eq!(
                after.workspace.answer,
                if index == 2 {
                    "implementation 1"
                } else {
                    "implementation 2"
                }
            );
            let (restored, returned) = restore(path, &before.id, &token).unwrap();
            assert_eq!(returned, challenge);
            assert_eq!(restored.problems, before.problems);
            assert_eq!(restored.attempts, before.attempts);
            assert_eq!(restored.challenge, before.challenge);
            assert_eq!(restored.workspace, before.workspace);
            let private: String = db::for_chats(path)
                .with(|c| {
                    c.query_row(
                        "SELECT data FROM interview_keys WHERE interview = ?1 AND challenge = ?2",
                        params![before.id, challenge],
                        |r| r.get(0),
                    )
                })
                .unwrap();
            assert_eq!(private, format!("private key {index}"));
            assert!(restore(path, &before.id, &token).is_err());
            let mut item = restored;
            for problem in item.problems.clone() {
                item = delete(path, &item.id, item.revision, &problem.challenge.id)
                    .unwrap()
                    .0;
            }
            assert!(
                item.challenge.is_none() && item.problems.is_empty() && item.attempts.is_empty()
            );
            assert_eq!(
                interview::load(path, &item.id).unwrap().workspace,
                Workspace::default()
            );
        }
    }

    #[test]
    fn undo_is_private_scoped_bounded_expiring_and_clearable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let before = seed(path, "practice-delete-01");
        let other = seed(path, "practice-delete-02");
        let (mut item, token) =
            delete(path, &before.id, before.revision, "problem-delete-2").unwrap();
        assert!(restore(path, &other.id, &token).is_err());
        for invalid in ["", "..", "not-a-token"] {
            assert!(restore(path, &before.id, invalid).is_err());
        }
        let changed = Workspace {
            answer: "new work on the next problem".into(),
            ..Workspace::default()
        };
        item = interview::change(
            path,
            &item.id,
            interview::Change::Workspace {
                revision: item.revision,
                challenge: "problem-delete-1".into(),
                workspace: changed.clone(),
            },
        )
        .unwrap();
        let request = crate::storage::Request {
            area: "interview-checks".into(),
            action: "delete".into(),
            items: vec![before.id.clone()],
        };
        storage_saved::act_on(path, &request).unwrap().unwrap();
        item = restore(path, &item.id, &token).unwrap().0;
        assert_eq!(item.problems[1].workspace, changed);
        assert!(crate::interview_keys::status(path, &item.id, "problem-delete-2").is_none());
        let (mut item, _) = delete(path, &item.id, item.revision, "problem-delete-2").unwrap();
        for index in 0..MOST_UNDO + 2 {
            let challenge = item.problems[0].challenge.id.clone();
            let (_, token) = delete(path, &item.id, item.revision, &challenge).unwrap();
            item = restore(path, &item.id, &token).unwrap().0;
            db::for_chats(path)
                .with(|c| {
                    c.execute(
                        "INSERT INTO interview_problem_undo VALUES (?1, ?2, ?3, '{}')",
                        params![
                            uuid::Uuid::new_v4().to_string(),
                            item.id,
                            Utc::now().to_rfc3339()
                        ],
                    )
                })
                .unwrap();
            let count: usize = db::for_chats(path)
                .with(|c| {
                    c.query_row("SELECT count(*) FROM interview_problem_undo", [], |r| {
                        r.get(0)
                    })
                })
                .unwrap();
            assert!(count <= MOST_UNDO, "iteration {index}: {count}");
        }
        let (after, token) = delete(path, &item.id, item.revision, "problem-delete-0").unwrap();
        db::for_chats(path)
            .with(|c| {
                c.execute(
                    "UPDATE interview_problem_undo SET updated = ?1 WHERE id = ?2",
                    params![
                        (Utc::now() - chrono::Duration::hours(UNDO_HOURS + 1)).to_rfc3339(),
                        token
                    ],
                )
            })
            .unwrap();
        assert!(restore(path, &after.id, &token).is_err());
        let (after, token) = delete(path, &after.id, after.revision, "problem-delete-1").unwrap();
        storage_saved::act_on(
            path,
            &crate::storage::Request {
                area: "interview-problem-undo".into(),
                action: "delete".into(),
                items: vec![token.clone()],
            },
        )
        .unwrap()
        .unwrap();
        assert!(restore(path, &after.id, &token).is_err());
        interview::remove(path, &after.id).unwrap();
        let count: usize = db::for_chats(path)
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM interview_problem_undo WHERE interview = ?1",
                    [&after.id],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
