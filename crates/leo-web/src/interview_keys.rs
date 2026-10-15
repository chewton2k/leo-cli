use std::path::Path;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graph::Writer;
use crate::interview::{Challenge, Framework, Interview, Workspace};
use crate::{chats, db};

pub const MOST_TESTS: usize = 20;
pub const MOST_VISIBLE: usize = 6;
pub const MOST_ASKS: usize = 8;
pub const MOST_SECTIONS: usize = 8;
pub const MOST_CRITERIA: usize = 10;
pub const MOST_REFERENCE: usize = 6_000;
pub const MOST_SOLUTION: usize = 12_000;
const MOST_VALUE_BYTES: usize = 4_000;
const KEY_TOKENS: u32 = 6_000;
const GRADE_TOKENS: u32 = 1_500;
const GRADE_ANSWER: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Test {
    pub id: String,
    pub args: Vec<Value>,
    pub expected: Value,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub unordered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ask {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default, deserialize_with = "strings")]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer: Value,
    #[serde(default, deserialize_with = "number")]
    pub tolerance: f64,
    #[serde(default, deserialize_with = "number")]
    pub factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "strings")]
    pub look_for: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Criterion {
    pub id: String,
    pub text: String,
    #[serde(default = "one", deserialize_with = "whole")]
    pub weight: u32,
    #[serde(default)]
    pub good: String,
    #[serde(default, deserialize_with = "strings")]
    pub misses: Vec<String>,
}

fn one() -> u32 {
    1
}

fn strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                Value::Null => None,
                other => Some(other.to_string()),
            })
            .collect(),
        Value::String(s) if s.trim().is_empty() => Vec::new(),
        Value::String(s) => vec![s],
        _ => Vec::new(),
    })
}

fn number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    })
}

fn whole<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let n = number(d)?;
    Ok(if n.is_finite() && n >= 0.0 {
        n.round().min(u32::MAX as f64) as u32
    } else {
        0
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Key {
    #[serde(default)]
    pub entry: String,
    #[serde(default)]
    pub tests: Vec<Test>,
    #[serde(default)]
    pub asks: Vec<Ask>,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub criteria: Vec<Criterion>,
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub solution: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub doubtful: bool,
    #[serde(default)]
    pub dropped: Vec<Test>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<LastRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LastRun {
    pub at: String,
    pub reports: Vec<TestReport>,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TestReport {
    pub id: String,
    pub pass: bool,
    #[serde(default)]
    pub got: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub pass: bool,
    #[serde(default)]
    pub given: String,
    #[serde(default)]
    pub expected: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Verdict {
    pub id: String,
    pub text: String,
    pub verdict: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Grade {
    pub criteria: Vec<Verdict>,
    pub overall: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Covered {
    pub id: String,
    pub text: String,
    pub covered: String,
}

fn short(value: &str, most: usize, required: bool) -> bool {
    value.chars().count() <= most && (!required || !value.trim().is_empty())
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn distinct<'a>(ids: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    ids.into_iter().all(|id| tag(id) && seen.insert(id))
}

fn small(value: &Value) -> bool {
    serde_json::to_string(value).is_ok_and(|s| s.len() <= MOST_VALUE_BYTES)
}

impl Key {
    pub fn validate(&mut self, challenge: &Challenge) -> Result<(), String> {
        self.tests.truncate(MOST_TESTS);
        self.asks.truncate(MOST_ASKS);
        self.sections.truncate(MOST_SECTIONS);
        self.criteria.truncate(MOST_CRITERIA);
        self.reference = self.reference.chars().take(MOST_REFERENCE).collect();
        if self.solution.chars().count() > MOST_SOLUTION || self.tests.is_empty() {
            self.solution.clear();
        }
        self.last_run = None;
        self.dropped.clear();
        if let Some(coding) = &challenge.coding {
            let sql = is_sql(&coding.language);
            if sql {
                self.entry = "sql".into();
            }
            if !self.tests.is_empty() {
                if sql
                    && !self.tests.iter().all(|t| {
                        t.args.len() == 1 && t.args[0].is_string() && t.expected.is_array()
                    })
                {
                    return Err(
                        "SQL tests need one setup string and an array of result rows".into(),
                    );
                }
                if !sql
                    && (!identifier(&self.entry) || !coding.starter.contains(self.entry.as_str()))
                {
                    return Err("entry must be the starter's function name".into());
                }
                if !self
                    .tests
                    .iter()
                    .all(|t| t.args.len() <= 8 && t.args.iter().all(small) && small(&t.expected))
                {
                    return Err("tests need small JSON arguments and results".into());
                }
                let visible = self.tests.iter().filter(|t| !t.hidden).count();
                if visible == 0 || visible > MOST_VISIBLE {
                    return Err("give 1-6 visible tests".into());
                }
            }
        } else {
            self.tests.clear();
            self.entry.clear();
            self.solution.clear();
        }
        self.verified = self.solution.trim().is_empty();
        self.doubtful = false;
        if !distinct(self.tests.iter().map(|t| t.id.as_str())) {
            return Err("test ids must be short and distinct".into());
        }
        for ask in &mut self.asks {
            let ok = short(&ask.label, 200, true)
                && short(&ask.unit, 20, false)
                && match ask.kind.as_str() {
                    "value" => {
                        ask.options.clear();
                        ask.answer.as_f64().is_some_and(f64::is_finite)
                            && ask.tolerance.is_finite()
                            && (0.0..=1.0).contains(&ask.tolerance)
                            && ask.factor.is_finite()
                            && (ask.factor == 0.0 || (1.0..=10.0).contains(&ask.factor))
                    }
                    "choice" => {
                        (2..=8).contains(&ask.options.len())
                            && ask.options.iter().all(|o| short(o, 200, true))
                            && ask
                                .answer
                                .as_str()
                                .is_some_and(|a| ask.options.iter().any(|o| o == a))
                    }
                    _ => false,
                };
            if !ok {
                return Err(format!("ask {} is not a valid value or choice", ask.id));
            }
        }
        if !distinct(self.asks.iter().map(|a| a.id.as_str())) {
            return Err("ask ids must be short and distinct".into());
        }
        if !self.sections.iter().all(|s| {
            short(&s.name, 80, true)
                && s.look_for.len() <= 8
                && s.look_for.iter().all(|w| short(w, 60, true))
        }) || !distinct(self.sections.iter().map(|s| s.id.as_str()))
        {
            return Err("sections need distinct ids and short names".into());
        }
        if self.criteria.is_empty()
            || !self.criteria.iter().all(|c| {
                short(&c.text, 300, true)
                    && (1..=5).contains(&c.weight)
                    && short(&c.good, 600, false)
                    && c.misses.len() <= 6
                    && c.misses.iter().all(|m| short(m, 300, true))
            })
            || !distinct(self.criteria.iter().map(|c| c.id.as_str()))
        {
            return Err("give 1-10 criteria with distinct ids".into());
        }
        Ok(())
    }

    pub fn needs_check(&self) -> bool {
        !self.verified && !self.solution.trim().is_empty() && !self.tests.is_empty()
    }

    pub fn stamp(&self) -> String {
        format!(
            "{:016x}",
            crate::warm::hash_of(&(
                serde_json::to_string(&self.tests).unwrap_or_default(),
                &self.solution,
                &self.entry
            ))
        )
    }

    pub fn public(&self, revealed: bool) -> Value {
        serde_json::json!({
            "entry": self.entry,
            "tests": self.tests.iter().filter(|t| revealed || !t.hidden).collect::<Vec<_>>(),
            "hidden_tests": self.tests.iter().filter(|t| t.hidden).count(),
            "revealed": revealed,
            "verify": self.needs_check(),
            "asks": self.asks.iter().map(|a| serde_json::json!({"id": a.id, "label": a.label, "kind": a.kind, "unit": a.unit, "options": a.options})).collect::<Vec<_>>(),
            "sections": self.sections.iter().map(|s| serde_json::json!({"id": s.id, "name": s.name})).collect::<Vec<_>>(),
            "criteria": self.criteria.len(),
        })
    }
}

pub fn status(path: &Path, interview: &str, challenge: &str) -> Option<(String, Option<Key>)> {
    db::for_chats(path)
        .with(|c| {
            c.query_row(
                "SELECT status, data FROM interview_keys WHERE interview = ?1 AND challenge = ?2",
                [interview, challenge],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
        })
        .ok()
        .flatten()
        .map(|(status, data)| (status, serde_json::from_str(&data).ok()))
}

pub fn ready(path: &Path, interview: &str, challenge: &str) -> Option<Key> {
    match status(path, interview, challenge) {
        Some((status, key)) if status == "ready" => key,
        _ => None,
    }
}

fn update<T>(
    path: &Path,
    interview: &str,
    challenge: &str,
    change: impl FnOnce(&mut Key) -> Result<T, String>,
) -> Result<T, String> {
    let mut change = Some(change);
    let mut out = None;
    let mut refused = None;
    db::for_chats(path)
        .with(|c| {
            let tx = c.transaction()?;
            let row: Option<(String, String)> = tx
                .query_row(
                    "SELECT status, data FROM interview_keys WHERE interview = ?1 AND challenge = ?2",
                    [interview, challenge],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((_, data)) = row.filter(|(status, _)| status == "ready") else {
                refused = Some("This problem's test cases are not ready yet.".to_string());
                return Ok(());
            };
            let Ok(mut key) = serde_json::from_str::<Key>(&data) else {
                refused = Some("This problem's test cases could not be read.".to_string());
                return Ok(());
            };
            match (change.take().expect("one change"))(&mut key) {
                Ok(value) => {
                    let next = serde_json::to_string(&key).unwrap_or(data);
                    tx.execute(
                        "UPDATE interview_keys SET data = ?3, updated = ?4 WHERE interview = ?1 AND challenge = ?2",
                        rusqlite::params![interview, challenge, next, Utc::now().to_rfc3339()],
                    )?;
                    out = Some(value);
                }
                Err(why) => refused = Some(why),
            }
            tx.commit()
        })
        .map_err(|_| "leo could not save the test cases.".to_string())?;
    match (out, refused) {
        (Some(value), _) => Ok(value),
        (None, Some(why)) => Err(why),
        (None, None) => Err("leo could not save the test cases.".into()),
    }
}

pub fn verify_job(path: &Path, interview: &str, challenge: &str) -> Option<Value> {
    let key = ready(path, interview, challenge)?;
    key.needs_check().then(|| {
        serde_json::json!({
            "entry": key.entry,
            "language": if key.entry == "sql" { "sql" } else { "javascript" },
            "solution": key.solution,
            "stamp": key.stamp(),
            "tests": key.tests.iter().map(|t| serde_json::json!({"id": t.id, "args": t.args, "expected": t.expected, "unordered": t.unordered})).collect::<Vec<_>>(),
        })
    })
}

pub fn verified(
    path: &Path,
    interview: &str,
    challenge: &str,
    stamp: &str,
    reports: &[TestReport],
    error: &str,
) -> Result<usize, String> {
    update(path, interview, challenge, |key| {
        if !key.needs_check() || key.stamp() != stamp {
            return Ok(0);
        }
        let failed: Vec<String> = key
            .tests
            .iter()
            .filter(|t| !reports.iter().any(|r| r.id == t.id && r.pass))
            .map(|t| t.id.clone())
            .collect();
        key.verified = true;
        if !error.trim().is_empty() || failed.len() * 2 > key.tests.len() {
            key.doubtful = true;
            return Ok(0);
        }
        let (dropped, kept): (Vec<Test>, Vec<Test>) = std::mem::take(&mut key.tests)
            .into_iter()
            .partition(|t| failed.contains(&t.id));
        key.tests = kept;
        if !key.tests.is_empty() && key.tests.iter().all(|t| t.hidden) {
            key.tests[0].hidden = false;
        }
        key.dropped = dropped;
        Ok(key.dropped.len())
    })
}

pub fn save_run(
    path: &Path,
    interview: &str,
    challenge: &str,
    reports: Vec<TestReport>,
    error: &str,
) -> Result<(), String> {
    update(path, interview, challenge, |key| {
        key.last_run = Some(LastRun {
            at: Utc::now().to_rfc3339(),
            reports: reports
                .into_iter()
                .filter(|r| key.tests.iter().any(|t| t.id == r.id))
                .map(|r| TestReport {
                    got: r.got.chars().take(500).collect(),
                    ..r
                })
                .collect(),
            error: error.chars().take(500).collect(),
        });
        Ok(())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TestChange {
    pub action: String,
    pub test: Test,
}

pub fn check_test(key: &Key, change: &TestChange) -> Result<Key, String> {
    if key.entry.is_empty() {
        return Err(
            "This problem has no test cases: it is not a coding problem with a function to call."
                .into(),
        );
    }
    let test = &change.test;
    if !tag(&test.id) {
        return Err("Give the test a short id of letters, digits, - or _, like t7.".into());
    }
    let mut next = key.clone();
    let at = next.tests.iter().position(|t| t.id == test.id);
    match (change.action.as_str(), at) {
        ("add", None) => next.tests.push(test.clone()),
        ("add", Some(_)) => {
            return Err(format!(
                "Test {} already exists; use edit to change it.",
                test.id
            ))
        }
        ("edit", Some(i)) => next.tests[i] = test.clone(),
        ("delete", Some(i)) => {
            next.tests.remove(i);
        }
        ("edit" | "delete", None) => {
            return Err(format!(
                "There is no test {}. Read the test cases for their ids.",
                test.id
            ))
        }
        _ => return Err("Use action add, edit or delete.".into()),
    }
    if next.tests.len() > MOST_TESTS {
        return Err(format!("A problem keeps at most {MOST_TESTS} test cases."));
    }
    if test.args.len() > 8 || !test.args.iter().all(small) || !small(&test.expected) {
        return Err("Tests need at most 8 small JSON arguments and a small JSON result.".into());
    }
    if next.tests.iter().all(|t| t.hidden) {
        return Err("Keep at least one visible test so the user has an example to run.".into());
    }
    Ok(next)
}

pub fn change_test(
    path: &Path,
    interview: &str,
    challenge: &str,
    change: &TestChange,
) -> Result<TestChange, String> {
    update(path, interview, challenge, |key| {
        let before = key.tests.iter().find(|t| t.id == change.test.id).cloned();
        let next = check_test(key, change)?;
        *key = next;
        key.last_run = None;
        Ok(match (change.action.as_str(), before) {
            ("add", _) => TestChange {
                action: "delete".into(),
                test: change.test.clone(),
            },
            ("edit", Some(old)) => TestChange {
                action: "edit".into(),
                test: old,
            },
            (_, Some(old)) => TestChange {
                action: "add".into(),
                test: old,
            },
            _ => TestChange {
                action: "delete".into(),
                test: change.test.clone(),
            },
        })
    })
}

pub fn tests_material(
    path: &Path,
    interview: &str,
    challenge: &str,
    submitted: Option<&[CheckResult]>,
) -> String {
    let body = match status(path, interview, challenge) {
        None => return String::new(),
        Some((status, _)) if status == "preparing" => {
            "The test cases and answer key are still being written.".to_string()
        }
        Some((status, _)) if status != "ready" => {
            "The answer key could not be written, so this problem has no test cases.".to_string()
        }
        Some((_, None)) => return String::new(),
        Some((_, Some(key))) if key.entry.is_empty() => return String::new(),
        Some((_, Some(key))) => {
            let ran = key.last_run.as_ref().map(|run| serde_json::json!({
                "at": run.at,
                "error": run.error,
                "results": run.reports.iter().map(|r| serde_json::json!({"id": r.id, "pass": r.pass, "got": r.got})).collect::<Vec<_>>(),
            }));
            let checked = if key.needs_check() {
                "not yet checked against the reference solution"
            } else if key.doubtful {
                "the reference solution disagreed with most tests, so they were kept as written: double-check any that look wrong"
            } else if key.solution.is_empty() {
                "written without a reference solution: double-check any that look wrong"
            } else {
                "every test matched the reference solution in the browser"
            };
            serde_json::json!({
                "problem": challenge,
                "entry": key.entry,
                "checked": checked,
                "tests": key.tests.iter().map(|t| serde_json::json!({"id": t.id, "call": call_of(&key.entry, t), "args": t.args, "expected": t.expected, "hidden": t.hidden, "unordered": t.unordered})).collect::<Vec<_>>(),
                "dropped_as_wrong": key.dropped.iter().map(|t| serde_json::json!({"id": t.id, "call": call_of(&key.entry, t), "expected": t.expected})).collect::<Vec<_>>(),
                "last_run": ran,
                "latest_submission_checks": submitted.map(|checks| checks.iter().map(|c| serde_json::json!({"id": c.id, "kind": c.kind, "label": c.label, "pass": c.pass, "given": c.given, "expected": c.expected})).collect::<Vec<_>>()),
            })
            .to_string()
            .replace('<', "\\u003c")
        }
    };
    format!("<test_cases>\n{body}\n</test_cases>\nThese are this problem's test cases, the user's latest Run in their browser and their latest submission's checks. Use them directly; never ask the user to paste a test. Hidden tests stay unmentioned until the user has submitted or asks. When a test is wrong, fix it with interview_test instead of asking the user to ignore it.\n")
}

fn finish(
    path: &Path,
    interview: &str,
    challenge: &str,
    generation: &str,
    made: Result<Key, String>,
) {
    let (status, data) = match made {
        Ok(key) => (
            "ready",
            serde_json::to_string(&key).unwrap_or_else(|_| "{}".into()),
        ),
        Err(why) => (
            "failed",
            serde_json::json!({"error": why.chars().take(300).collect::<String>()}).to_string(),
        ),
    };
    let _ = db::for_chats(path).with(|c| {
        c.execute(
            "UPDATE interview_keys SET status = ?3, updated = ?4, data = ?5 WHERE interview = ?1 AND challenge = ?2 AND status = 'preparing' AND data = ?6 AND EXISTS (SELECT 1 FROM interviews WHERE id = ?1)",
            rusqlite::params![interview, challenge, status, Utc::now().to_rfc3339(), data, generation],
        )
    });
}

fn claim(path: &Path, interview: &str, challenge: &str, again: bool) -> Option<String> {
    let generation =
        serde_json::json!({"generation": uuid::Uuid::new_v4().to_string()}).to_string();
    db::for_chats(path)
        .with(|c| {
            let now = Utc::now().to_rfc3339();
            let changed = if again {
                c.execute(
                    "UPDATE interview_keys SET status = 'preparing', updated = ?3, data = ?4 WHERE interview = ?1 AND challenge = ?2 AND status = 'failed' AND EXISTS (SELECT 1 FROM interviews WHERE id = ?1)",
                    rusqlite::params![interview, challenge, now, generation],
                )?
            } else {
                c.execute(
                    "INSERT OR IGNORE INTO interview_keys (interview, challenge, status, updated, data) SELECT ?1, ?2, 'preparing', ?3, ?4 WHERE EXISTS (SELECT 1 FROM interviews WHERE id = ?1)",
                    rusqlite::params![interview, challenge, now, generation],
                )?
            };
            Ok(changed == 1)
        })
        .ok().filter(|done| *done).map(|_| generation)
}

pub const KEY_RULES: &str = "You write the hidden answer key for one practice interview exercise, so the user's answer can be checked the moment they submit. Reply with one JSON object only, no prose, with these fields:
- criteria (always, 2-8): {id: short tag like c1, text: what a strong answer does, in one sentence, weight: 1-5, good: what meeting it looks like for this exercise, misses: common ways people fall short}. Use the exercise's skills.
- reference (always): an outline of a strong answer in at most 2,500 characters, with the key numbers, steps or trade-offs. The user never sees it before submitting.
- For coding exercises also: entry: the name of the function the starter defines; tests: 6-14 {id: t1..., args: [the arguments as JSON values, in order], expected: the exact JSON result, hidden: false for 2-4 tests that match the examples and true for the rest, unordered: true only when any order of a list result is right}. Cover edge cases (empty input, duplicates, limits, negative numbers) in hidden tests. Work every expected value out carefully by tracing the solution below; a wrong expected value is worse than none. solution: a correct, plain JavaScript function named exactly like entry that takes the test arguments in order and returns the expected JSON value, with no imports, input or printing; leo runs it in the user's browser sandbox before showing the tests and drops any test it disagrees with. The user never sees it. Use only JSON values (numbers, strings, booleans, null, arrays, objects); represent linked lists and trees as arrays. For typed languages (C++, Go, Java, TypeScript) give each argument exactly the starter's parameter type: whole numbers for int and long, lists for vectors, slices and arrays, one-character strings for char. In Go, Java and Rust, linked lists and binary trees use the LeetCode ListNode and TreeNode types (Rust: Option<Box<ListNode>> and Option<Rc<RefCell<TreeNode>>>) and are written as arrays, trees in level order with null for gaps; Rust functions sit in impl Solution like LeetCode's.
- For SQL exercises instead: entry: \"sql\"; each test has args: [one string of SQLite statements that create and fill the tables for that case], expected: the result rows as an array of row arrays in column order (numbers as numbers, NULL as null), unordered: true unless the question asks for a specific order; solution: the SQLite query that answers the question. Write SQLite-compatible SQL.
- For quantitative exercises and estimates also: asks: 1-6 {id: a1..., label: the quantity or question the user should state, kind: value, unit, answer: the number, tolerance: relative error allowed (0.01 for exact maths, 0 means exact), factor: for rough estimates the accepted ratio either way (like 3 for within 3x), else 0} or {id, label, kind: choice, options: 2-6 strings, answer: one of the options}. Only ask for quantities the exercise asks for or that are a natural step towards them.
- For open-ended exercises (system design, product, design, marketing, business, cases) also: sections: 3-7 {id: s1..., name: a part a strong answer covers, like Requirements or Success metrics, look_for: 1-5 words that show it is there}. System design may add value asks for capacity estimates with a factor.
Never include anything except the JSON object. Use interpretable language.";

fn key_prompt(framework: &Framework, challenge: &Challenge) -> String {
    format!(
        "<exercise>\n{}\n</exercise>\n<framework track=\"{}\" language=\"{}\" role=\"{}\"/>\nWrite the answer key.",
        serde_json::to_string(challenge).unwrap_or_default(),
        framework.track,
        framework.language,
        framework.role
    )
}

pub fn write_key(
    writer: &Writer,
    framework: &Framework,
    challenge: &Challenge,
) -> Result<Key, String> {
    let reply = writer(KEY_RULES, &key_prompt(framework, challenge), KEY_TOKENS)
        .map_err(|e| e.to_string())?;
    let value = crate::graph::json_in(&reply).ok_or("the key was not JSON")?;
    let mut key: Key = serde_json::from_value(value).map_err(|e| e.to_string())?;
    key.validate(challenge)?;
    Ok(key)
}

pub fn prepare(path: &Path, writer: Option<Writer>, item: &Interview, again: bool) -> bool {
    let (Some(writer), Some(challenge)) = (writer, item.challenge.clone()) else {
        return false;
    };
    let Some(generation) = claim(path, &item.id, &challenge.id, again) else {
        return false;
    };
    let path = path.to_path_buf();
    let framework = item.framework.clone();
    let interview = item.id.clone();
    std::thread::Builder::new()
        .name("leo-interview-key".into())
        .spawn(move || {
            let made = write_key(&writer, &framework, &challenge)
                .or_else(|_| write_key(&writer, &framework, &challenge));
            finish(&path, &interview, &challenge.id, &generation, made);
        })
        .is_ok()
}

pub fn number_in(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | '$' | '€' | '£' | '_' | ' ' | '%' | '~' | '≈'))
        .collect();
    let lower = cleaned.to_ascii_lowercase();
    let (digits, scale) = [
        ("trillion", 1e12),
        ("billion", 1e9),
        ("million", 1e6),
        ("thousand", 1e3),
        ("t", 1e12),
        ("b", 1e9),
        ("bn", 1e9),
        ("m", 1e6),
        ("k", 1e3),
    ]
    .iter()
    .find_map(|(end, scale)| lower.strip_suffix(end).map(|d| (d.to_string(), *scale)))
    .unwrap_or((lower, 1.0));
    let value = if let Some((a, b)) = digits.split_once('/') {
        a.parse::<f64>().ok()? / b.parse::<f64>().ok()?
    } else {
        digits.parse::<f64>().ok()?
    };
    let value = value * scale;
    value.is_finite().then_some(value)
}

fn value_passes(ask: &Ask, given: f64) -> bool {
    let Some(answer) = ask.answer.as_f64() else {
        return false;
    };
    if ask.factor >= 1.0 {
        if answer == 0.0 || given == 0.0 || answer.signum() != given.signum() {
            return answer == given;
        }
        let ratio = given / answer;
        return ratio <= ask.factor && ratio >= 1.0 / ask.factor;
    }
    let allowed = (answer.abs() * ask.tolerance).max(1e-9);
    (given - answer).abs() <= allowed
}

fn amount(ask: &Ask) -> String {
    let text = match ask.answer.as_f64() {
        Some(n) if ask.kind == "value" => {
            let rounded = format!("{:.4}", n);
            rounded
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
        _ => show(&ask.answer),
    };
    match ask.unit.as_str() {
        "" => text,
        "$" | "€" | "£" | "¥" => format!("{}{text}", ask.unit),
        unit => format!("{text} {unit}"),
    }
}

fn show(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn plain(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|w| match w.strip_suffix('s') {
            Some(stem) if stem.chars().count() >= 3 && !stem.ends_with('s') => stem,
            _ => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn covers(text: &str, section: &Section) -> bool {
    let text = format!(" {} ", plain(text));
    std::iter::once(&section.name)
        .chain(section.look_for.iter())
        .map(|w| plain(w))
        .filter(|w| !w.is_empty())
        .any(|w| text.contains(&format!(" {w} ")))
}

fn written(workspace: &Workspace) -> String {
    let labels: Vec<&str> = workspace.board.iter().map(|s| s.text.as_str()).collect();
    format!(
        "{}\n{}\n{}",
        workspace.answer,
        workspace.reasoning,
        labels.join("\n")
    )
}

pub fn instant(key: &Key, workspace: &Workspace, reports: &[TestReport]) -> Vec<CheckResult> {
    let mut out = Vec::new();
    for test in &key.tests {
        if let Some(report) = reports.iter().find(|r| r.id == test.id) {
            out.push(CheckResult {
                id: test.id.clone(),
                kind: "test".into(),
                label: call_of(&key.entry, test),
                pass: report.pass,
                given: report.got.chars().take(500).collect(),
                expected: show(&test.expected),
                hidden: test.hidden,
            });
        }
    }
    for ask in &key.asks {
        let given = workspace.values.get(&ask.id).cloned().unwrap_or_default();
        let pass = match ask.kind.as_str() {
            "value" => number_in(&given).is_some_and(|g| value_passes(ask, g)),
            _ => ask
                .answer
                .as_str()
                .is_some_and(|a| a.trim().eq_ignore_ascii_case(given.trim())),
        };
        out.push(CheckResult {
            id: ask.id.clone(),
            kind: ask.kind.clone(),
            label: ask.label.clone(),
            pass,
            given,
            expected: amount(ask),
            hidden: false,
        });
    }
    let text = written(workspace);
    for section in &key.sections {
        out.push(CheckResult {
            id: section.id.clone(),
            kind: "section".into(),
            label: section.name.clone(),
            pass: covers(&text, section),
            given: String::new(),
            expected: String::new(),
            hidden: false,
        });
    }
    out
}

const STARTER_RULES: &str = "You rewrite the starting code of a coding interview problem in another programming language. Reply with only the new starting code in one fenced code block. Keep the function or method name exactly as given, the same parameters in the same order and the same kind of result, with the new language's usual types for the example arguments shown (whole numbers as int or long, lists as the language's usual list or array type). Use the layout LeetCode uses for that language: a Solution class in Python, Java and C++, impl Solution in Rust, a plain function in Go, JavaScript and TypeScript. When the problem uses linked lists or binary trees, use LeetCode's ListNode and TreeNode and include their definitions as a comment, as LeetCode does. Leave the body unsolved: a placeholder return or a TODO, never the solution. No explanation outside the code block.";
const STARTER_TOKENS: u32 = 2_000;

fn code_in(reply: &str) -> String {
    let Some(start) = reply.find("```") else {
        return reply.trim().to_string();
    };
    let after = &reply[start + 3..];
    let body = after.split_once('\n').map_or("", |(_, rest)| rest);
    body.split("```")
        .next()
        .unwrap_or("")
        .trim_end()
        .to_string()
}

pub fn translate_starter(
    writer: &Writer,
    challenge: &Challenge,
    language: &str,
    key: Option<&Key>,
) -> Result<String, String> {
    let coding = challenge.coding.as_ref().ok_or("not a coding problem")?;
    let entry = key.map(|k| k.entry.as_str()).filter(|e| !e.is_empty());
    let calls: Vec<String> = key
        .map(|k| {
            k.tests
                .iter()
                .filter(|t| !t.hidden)
                .take(3)
                .map(|t| call_of(&k.entry, t))
                .collect()
        })
        .unwrap_or_default();
    let question: String = challenge.question.chars().take(4_000).collect();
    let prompt = format!(
        "<question>\n{question}\n</question>\n<starter language=\"{}\">\n{}\n</starter>\n{}{}Rewrite this starter in {language}.",
        coding.language,
        coding.starter,
        entry.map_or(String::new(), |e| format!("<function_name>{e}</function_name>\n")),
        if calls.is_empty() { String::new() } else { format!("<example_calls>\n{}\n</example_calls>\n", calls.join("\n")) },
    );
    let mut last = String::from("the new starter was empty");
    for _ in 0..2 {
        let reply = match writer(STARTER_RULES, &prompt, STARTER_TOKENS) {
            Ok(reply) => reply,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let code = code_in(&reply);
        if code.trim().is_empty() || code.chars().count() > 8_000 {
            last = "the new starter was empty or too long".into();
        } else if entry.is_some_and(|e| !code.contains(e)) {
            last = "the new starter renamed the function".into();
        } else {
            return Ok(code);
        }
    }
    Err(last)
}

pub fn is_sql(language: &str) -> bool {
    let l = language.trim().to_ascii_lowercase();
    l.contains("sql") || l == "postgres" || l == "sqlite"
}

fn call_of(entry: &str, test: &Test) -> String {
    if entry == "sql" {
        return test
            .args
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    format!(
        "{entry}({})",
        test.args
            .iter()
            .map(|a| serde_json::to_string(a).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub const GRADE_RULES: &str = "You mark a practice interview answer against a rubric written before the user answered. Be strict and specific: met means the answer clearly does it, partly means it is attempted with a gap, missed means it is absent or wrong. A right result with wrong or missing reasoning is at most partly. Use the reference only to judge; never quote it or give the full solution. Reply with one JSON object only: {\"criteria\": [{\"id\": the criterion id, \"verdict\": \"met\" | \"partly\" | \"missed\", \"why\": one sentence that points at the user's answer}], \"overall\": two sentences: the biggest strength and the most important fix}. Use interpretable language.";

pub const COVERAGE_RULES: &str = "You check which rubric items a draft interview answer already addresses, so the user can see what is missing before submitting. Do not judge quality, give hints or reveal answers. yes means the draft clearly addresses the item, partly means it touches on it, no means it does not. Reply with one JSON object only: {\"items\": [{\"id\": the criterion id, \"covered\": \"yes\" | \"partly\" | \"no\"}]}. Use interpretable language.";

fn rubric(key: &Key, with_answers: bool) -> String {
    key.criteria
        .iter()
        .map(|c| {
            if with_answers {
                format!(
                    "- {} (weight {}): {}\n  Looks like: {}\n  Common misses: {}",
                    c.id,
                    c.weight,
                    c.text,
                    c.good,
                    c.misses.join("; ")
                )
            } else {
                format!("- {}: {}", c.id, c.text)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn material(challenge: &Challenge, workspace: &Workspace) -> String {
    format!(
        "<exercise>\n{}\n\n{}\n</exercise>\n<answer>\n{}\n</answer>\n<approach>\n{}\n</approach>\n<board_labels>\n{}\n</board_labels>\n<values>\n{}\n</values>",
        challenge.title,
        challenge.question,
        crate::chat::clip(&workspace.answer, GRADE_ANSWER),
        crate::chat::clip(&workspace.reasoning, 4_000),
        workspace
            .board
            .iter()
            .map(|s| s.text.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        workspace
            .values
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

pub fn grade(
    quick: &Writer,
    key: &Key,
    challenge: &Challenge,
    workspace: &Workspace,
    checks: &[CheckResult],
) -> Result<Grade, String> {
    let instant = checks
        .iter()
        .map(|c| {
            format!(
                "- {} {}: {}",
                c.kind,
                c.label,
                if c.pass { "passed" } else { "failed" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let user = format!(
        "<rubric>\n{}\n</rubric>\n<reference>\n{}\n</reference>\n<automatic_checks>\n{}\n</automatic_checks>\n{}\nMark every criterion.",
        rubric(key, true),
        key.reference,
        if instant.is_empty() { "none" } else { &instant },
        material(challenge, workspace)
    );
    let reply = quick(GRADE_RULES, &user, GRADE_TOKENS).map_err(|e| e.to_string())?;
    let value = crate::graph::json_in(&reply).ok_or("the grade was not JSON")?;
    let criteria: Vec<Verdict> = key
        .criteria
        .iter()
        .filter_map(|c| {
            let found = value["criteria"]
                .as_array()?
                .iter()
                .find(|v| v["id"].as_str() == Some(c.id.as_str()))?;
            let verdict = found["verdict"].as_str()?;
            ["met", "partly", "missed"]
                .contains(&verdict)
                .then(|| Verdict {
                    id: c.id.clone(),
                    text: c.text.clone(),
                    verdict: verdict.into(),
                    why: found["why"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(400)
                        .collect(),
                })
        })
        .collect();
    if criteria.is_empty() {
        return Err("the grade named no criteria".into());
    }
    Ok(Grade {
        criteria,
        overall: value["overall"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(600)
            .collect(),
        at: Utc::now().to_rfc3339(),
    })
}

pub fn coverage(
    quick: &Writer,
    key: &Key,
    challenge: &Challenge,
    workspace: &Workspace,
) -> Result<Vec<Covered>, String> {
    let user = format!(
        "<rubric>\n{}\n</rubric>\n{}\nSay which items the draft addresses.",
        rubric(key, false),
        material(challenge, workspace)
    );
    let reply = quick(COVERAGE_RULES, &user, 600).map_err(|e| e.to_string())?;
    let value = crate::graph::json_in(&reply).ok_or("the check was not JSON")?;
    Ok(key
        .criteria
        .iter()
        .map(|c| Covered {
            id: c.id.clone(),
            text: c.text.clone(),
            covered: value["items"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|v| v["id"].as_str() == Some(c.id.as_str()))
                })
                .and_then(|v| v["covered"].as_str())
                .filter(|s| ["yes", "partly", "no"].contains(s))
                .unwrap_or("no")
                .to_string(),
        })
        .collect())
}

pub fn valid(interview: &str, challenge: &str) -> bool {
    chats::valid_id(interview) && chats::valid_id(challenge)
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_starter_is_rewritten_with_the_same_function_name_or_refused() {
        let mut challenge = coding_challenge();
        challenge.question = "Return the indices of two numbers that add up to target.".into();
        let key = Key {
            entry: "pair_sum".into(),
            tests: vec![Test {
                id: "t1".into(),
                args: vec![serde_json::json!([2, 3]), serde_json::json!(5)],
                expected: serde_json::json!([0, 1]),
                hidden: false,
                unordered: false,
            }],
            ..Key::default()
        };
        let asked = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let heard = std::sync::Arc::clone(&asked);
        let writer: Writer = std::sync::Arc::new(move |_: &str, user: &str, _| {
            *heard.lock().unwrap() = user.to_string();
            Ok("Here it is:\n```java\nclass Solution {\n    public int[] pair_sum(int[] nums, int target) {\n        return new int[0];\n    }\n}\n```\n".into())
        });
        let code = translate_starter(&writer, &challenge, "Java", Some(&key)).unwrap();
        assert!(
            code.starts_with("class Solution {") && code.ends_with('}'),
            "{code}"
        );
        let prompt = asked.lock().unwrap().clone();
        assert!(
            prompt.contains("pair_sum([2,3], 5)")
                && prompt.contains("def pair_sum")
                && prompt.contains("in Java"),
            "{prompt}"
        );
        let renamer: Writer = std::sync::Arc::new(|_: &str, _: &str, _| {
            Ok("```go\nfunc twoSum(nums []int) []int { return nil }\n```".into())
        });
        assert!(translate_starter(&renamer, &challenge, "Go", Some(&key)).is_err());
        assert_eq!(code_in("plain code"), "plain code");
    }

    use super::*;
    use crate::interview::{Coding, Example};

    #[test]
    fn cleared_keys_and_deleted_practices_cannot_be_recreated_by_old_workers() {
        for failure in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path();
            db::for_chats(path)
                .with(|c| {
                    c.execute(
                        "INSERT INTO interviews VALUES ('practice-one', 'now', '{}')",
                        [],
                    )
                })
                .unwrap();
            let old = claim(path, "practice-one", "problem-one", false).unwrap();
            assert!(claim(path, "practice-one", "problem-one", false).is_none());
            let clear = crate::storage::Request {
                area: "interview-checks".into(),
                action: "all".into(),
                items: vec![],
            };
            crate::storage_saved::act_on(path, &clear).unwrap().unwrap();
            let made = || {
                if failure {
                    Err("old failure".into())
                } else {
                    Ok(Key::default())
                }
            };
            finish(path, "practice-one", "problem-one", &old, made());
            assert!(status(path, "practice-one", "problem-one").is_none());
            let new = claim(path, "practice-one", "problem-one", false).unwrap();
            finish(path, "practice-one", "problem-one", &old, made());
            assert_eq!(
                status(path, "practice-one", "problem-one").unwrap().0,
                "preparing"
            );
            finish(
                path,
                "practice-one",
                "problem-one",
                &new,
                Ok(Key::default()),
            );
            assert!(ready(path, "practice-one", "problem-one").is_some());
            crate::interview::remove(path, "practice-one").unwrap();
            finish(path, "practice-one", "problem-one", &new, made());
            assert!(status(path, "practice-one", "problem-one").is_none());
            assert!(claim(path, "practice-one", "problem-one", false).is_none());
        }
    }

    fn coding_challenge() -> Challenge {
        Challenge {
            id: "challenge-pairs-01".into(),
            title: "Pair sum".into(),
            question: "Find two indices.".into(),
            minutes: 20,
            skills: vec!["Approach".into()],
            origin: "generated".into(),
            sources: vec![],
            coding: Some(Coding {
                language: "Python".into(),
                starter: "def pair_sum(nums, target):\n    pass".into(),
                difficulty: "easy".into(),
                examples: vec![Example {
                    input: "[2,3], 5".into(),
                    output: "[0,1]".into(),
                    explanation: String::new(),
                }],
                constraints: vec!["One pair".into()],
            }),
        }
    }

    fn criterion() -> Criterion {
        Criterion {
            id: "c1".into(),
            text: "Uses a hash map".into(),
            weight: 2,
            good: "One pass".into(),
            misses: vec!["Nested loops".into()],
        }
    }

    #[test]
    fn a_coding_key_needs_the_starters_function_and_visible_tests() {
        let challenge = coding_challenge();
        let test = |id: &str, hidden: bool| Test {
            id: id.into(),
            args: vec![serde_json::json!([2, 3]), serde_json::json!(5)],
            expected: serde_json::json!([0, 1]),
            hidden,
            unordered: false,
        };
        let mut key = Key {
            entry: "pair_sum".into(),
            tests: vec![test("t1", false), test("t2", true)],
            criteria: vec![criterion()],
            reference: "Hash map".into(),
            ..Key::default()
        };
        assert!(key.validate(&challenge).is_ok());
        let public = key.public(false);
        assert_eq!(public["tests"].as_array().unwrap().len(), 1);
        assert_eq!(public["hidden_tests"], 1);
        assert!(public.get("reference").is_none());
        let revealed = key.public(true);
        assert_eq!(
            revealed["tests"].as_array().unwrap().len(),
            2,
            "after a submission every test is shown"
        );
        assert_eq!(revealed["tests"][1]["hidden"], true);
        assert!(revealed.get("solution").is_none() && revealed.get("last_run").is_none());
        let mut wrong = key.clone();
        wrong.entry = "solve".into();
        assert!(wrong.validate(&challenge).is_err());
        let mut all_hidden = key.clone();
        all_hidden.tests = vec![test("t1", true)];
        assert!(all_hidden.validate(&challenge).is_err());
    }

    #[test]
    fn values_choices_and_sections_are_checked_without_any_ai() {
        let mut challenge = coding_challenge();
        challenge.coding = None;
        let mut key = Key {
            asks: vec![
                Ask {
                    id: "a1".into(),
                    label: "Expected value".into(),
                    kind: "value".into(),
                    unit: "$".into(),
                    options: vec![],
                    answer: serde_json::json!(3.5),
                    tolerance: 0.01,
                    factor: 0.0,
                },
                Ask {
                    id: "a2".into(),
                    label: "Peak requests per second".into(),
                    kind: "value".into(),
                    unit: "QPS".into(),
                    options: vec![],
                    answer: serde_json::json!(20_000),
                    tolerance: 0.0,
                    factor: 3.0,
                },
                Ask {
                    id: "a3".into(),
                    label: "Better bet".into(),
                    kind: "choice".into(),
                    unit: String::new(),
                    options: vec!["Take it".into(), "Pass".into()],
                    answer: serde_json::json!("Pass"),
                    tolerance: 0.0,
                    factor: 0.0,
                },
            ],
            sections: vec![Section {
                id: "s1".into(),
                name: "Requirements".into(),
                look_for: vec!["functional".into()],
            }],
            criteria: vec![criterion()],
            ..Key::default()
        };
        assert!(key.validate(&challenge).is_ok());
        assert!(key.public(false)["asks"][0].get("answer").is_none());
        let mut workspace = Workspace {
            answer: "## Requirement list\nUsers upload logs.".into(),
            ..Workspace::default()
        };
        workspace.values.insert("a1".into(), "$3.50".into());
        workspace.values.insert("a2".into(), "50k".into());
        workspace.values.insert("a3".into(), "take it".into());
        let checks = instant(&key, &workspace, &[]);
        let passed: Vec<(&str, bool)> = checks.iter().map(|c| (c.id.as_str(), c.pass)).collect();
        assert_eq!(
            passed,
            [("a1", true), ("a2", true), ("a3", false), ("s1", true)]
        );
        assert_eq!(checks[2].expected, "Pass");
        assert_eq!(checks[0].expected, "$3.5");
        assert_eq!(checks[1].expected, "20000 QPS");
        workspace.values.insert("a2".into(), "100,000".into());
        assert!(!instant(&key, &workspace, &[])[1].pass);
        assert_eq!(number_in("1.2M"), Some(1_200_000.0));
        assert_eq!(number_in("7/2"), Some(3.5));
        assert_eq!(number_in("abc"), None);
    }

    #[test]
    fn reported_tests_become_results_and_hidden_ones_stay_unnamed() {
        let challenge = coding_challenge();
        let mut key = Key {
            entry: "pair_sum".into(),
            tests: vec![
                Test {
                    id: "t1".into(),
                    args: vec![serde_json::json!([2, 3]), serde_json::json!(5)],
                    expected: serde_json::json!([0, 1]),
                    hidden: false,
                    unordered: false,
                },
                Test {
                    id: "t2".into(),
                    args: vec![serde_json::json!([]), serde_json::json!(1)],
                    expected: serde_json::json!([]),
                    hidden: true,
                    unordered: false,
                },
            ],
            criteria: vec![criterion()],
            ..Key::default()
        };
        key.validate(&challenge).unwrap();
        let checks = instant(
            &key,
            &Workspace::default(),
            &[
                TestReport {
                    id: "t1".into(),
                    pass: true,
                    got: "[0, 1]".into(),
                },
                TestReport {
                    id: "t2".into(),
                    pass: false,
                    got: "None".into(),
                },
                TestReport {
                    id: "t9".into(),
                    pass: true,
                    got: String::new(),
                },
            ],
        );
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].label, "pair_sum([2,3], 5)");
        assert_eq!(
            (checks[1].label.as_str(), checks[1].hidden),
            ("pair_sum([], 1)", true),
            "a submitted hidden test shows its call"
        );
        assert!(!checks[0].hidden && !checks[1].pass);
    }

    #[test]
    fn a_key_written_in_the_background_grades_a_submission_at_once() {
        use crate::interview::{self, Change};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let id = "interview-keys-01";
        let framework = Framework {
            title: "Quant".into(),
            track: "quant".into(),
            company: String::new(),
            role: String::new(),
            language: String::new(),
            style: "coach".into(),
            date: String::new(),
            weekly_minutes: 120,
            skills: vec!["Approach".into()],
            wants: String::new(),
        };
        interview::configure(path, id, 0, framework).unwrap();
        let mut challenge = coding_challenge();
        challenge.coding = None;
        challenge.id = "challenge-dice-01".into();
        let item = interview::set_challenge(path, id, challenge.clone()).unwrap();
        let writer: Writer = std::sync::Arc::new(|system: &str, user: &str, _| {
            assert!(system.contains("hidden answer key"));
            assert!(user.contains("challenge-dice-01"));
            Ok(r#"{"asks": [{"id": "a1", "label": "Expected value", "kind": "value", "unit": "", "answer": 3.5, "tolerance": 0.01, "factor": 0}], "criteria": [{"id": "c1", "text": "Uses linearity", "weight": 2, "good": "Sums faces", "misses": []}], "reference": "Average of 1..6 is 3.5."}"#.into())
        });
        assert!(prepare(path, Some(writer.clone()), &item, false));
        assert!(
            !prepare(path, Some(writer), &item, false),
            "one key per problem"
        );
        let started = std::time::Instant::now();
        while ready(path, id, &challenge.id).is_none() {
            assert!(started.elapsed().as_secs() < 5, "the key was never written");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let mut workspace = Workspace {
            answer: "Each face is equally likely.".into(),
            ..Workspace::default()
        };
        workspace.values.insert("a1".into(), "3.5".into());
        let current = interview::load(path, id).unwrap();
        let submitted = interview::change(
            path,
            id,
            Change::Submit {
                revision: current.revision,
                challenge: challenge.id.clone(),
                attempt: "attempt-dice-01".into(),
                workspace,
                tests: Vec::new(),
            },
        )
        .unwrap();
        let attempt = &submitted.attempts[0];
        assert_eq!(attempt.checks.len(), 1);
        assert!(attempt.checks[0].pass);
        let quick: Writer = std::sync::Arc::new(|_: &str, _: &str, _| {
            Ok(r#"{"criteria": [{"id": "c1", "verdict": "met", "why": "You averaged the faces."}], "overall": "Right."}"#.into())
        });
        let key = ready(path, id, &challenge.id).unwrap();
        let graded = grade(
            &quick,
            &key,
            &attempt.challenge,
            &attempt.workspace,
            &attempt.checks,
        )
        .unwrap();
        let saved = interview::graded(path, id, "attempt-dice-01", graded).unwrap();
        assert_eq!(
            saved.attempts[0].grade.as_ref().unwrap().criteria[0].verdict,
            "met"
        );
        let page = serde_json::to_string(&interview::load(path, id).unwrap()).unwrap();
        assert!(
            !page.contains("Average of 1..6"),
            "the reference never reaches the page"
        );
        interview::remove(path, id).unwrap();
        assert!(status(path, id, &challenge.id).is_none());
    }

    #[test]
    fn a_key_written_a_little_loosely_is_still_read() {
        let value: Value = serde_json::from_str(r#"{"criteria": [{"id": "c1", "text": "Merges overlaps", "weight": "5", "good": "Uses max", "misses": "Sets the cursor to end"}], "sections": [{"id": "s1", "name": "Scale", "look_for": "throughput"}], "asks": [{"id": "a1", "label": "QPS", "kind": "value", "answer": 2000, "tolerance": "0", "factor": "3"}], "reference": "r"}"#).unwrap();
        let mut key: Key = serde_json::from_value(value).unwrap();
        assert_eq!(key.criteria[0].misses, ["Sets the cursor to end"]);
        assert_eq!(key.criteria[0].weight, 5);
        assert_eq!(key.sections[0].look_for, ["throughput"]);
        assert_eq!(key.asks[0].factor, 3.0);
        let mut challenge = coding_challenge();
        challenge.coding = None;
        assert!(key.validate(&challenge).is_ok());
    }

    #[test]
    fn the_quick_grade_keeps_only_the_keys_criteria() {
        let mut challenge = coding_challenge();
        challenge.coding = None;
        let key = Key {
            criteria: vec![criterion()],
            reference: "Hash map".into(),
            ..Key::default()
        };
        let quick: Writer = std::sync::Arc::new(|system: &str, user: &str, _| {
            assert!(system.contains("rubric written before"));
            assert!(user.contains("Common misses: Nested loops"));
            Ok(r#"Here: {"criteria": [{"id": "c1", "verdict": "partly", "why": "You loop twice."}, {"id": "c9", "verdict": "met", "why": "x"}], "overall": "Good start."}"#.into())
        });
        let graded = grade(&quick, &key, &challenge, &Workspace::default(), &[]).unwrap();
        assert_eq!(graded.criteria.len(), 1);
        assert_eq!(graded.criteria[0].verdict, "partly");
        assert_eq!(graded.overall, "Good start.");
        let shy: Writer = std::sync::Arc::new(|system: &str, user: &str, _| {
            assert!(system.contains("Do not judge quality"));
            assert!(
                !user.contains("Nested loops"),
                "coverage never sees the answers"
            );
            Ok(r#"{"items": [{"id": "c1", "covered": "yes"}]}"#.into())
        });
        let covered = coverage(&shy, &key, &challenge, &Workspace::default()).unwrap();
        assert_eq!(covered[0].covered, "yes");
    }

    fn stored(path: &Path, key: &Key) {
        db::for_chats(path)
            .with(|c| {
                c.execute(
                    "INSERT INTO interview_keys VALUES ('practice-keys-01', 'problem-keys-01', 'ready', '2026-10-13T12:00:00Z', ?1)",
                    [serde_json::to_string(key).unwrap()],
                )
            })
            .unwrap();
    }

    fn sums(ids: &[(&str, bool, i64)]) -> Key {
        Key {
            entry: "add".into(),
            tests: ids
                .iter()
                .map(|(id, hidden, expected)| Test {
                    id: (*id).into(),
                    args: vec![serde_json::json!(1), serde_json::json!(2)],
                    expected: serde_json::json!(expected),
                    hidden: *hidden,
                    unordered: false,
                })
                .collect(),
            criteria: vec![criterion()],
            solution: "function add(a, b) { return a + b; }".into(),
            ..Key::default()
        }
    }

    fn report(id: &str, pass: bool) -> TestReport {
        TestReport {
            id: id.into(),
            pass,
            got: "3".into(),
        }
    }

    #[test]
    fn tests_the_reference_solution_disagrees_with_are_dropped_before_anyone_sees_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let (i, c) = ("practice-keys-01", "problem-keys-01");
        let key = sums(&[("t1", true, 4), ("t2", false, 3), ("t3", true, 3)]);
        assert!(key.needs_check() && key.public(false)["verify"] == true);
        stored(path, &key);
        let job = verify_job(path, i, c).unwrap();
        assert_eq!(job["tests"].as_array().unwrap().len(), 3);
        let stamp = job["stamp"].as_str().unwrap();
        assert_eq!(
            verified(path, i, c, "stale", &[report("t1", false)], "").unwrap(),
            0,
            "a check of older tests changes nothing"
        );
        assert!(ready(path, i, c).unwrap().needs_check());
        let reports = [report("t1", false), report("t2", true), report("t3", true)];
        assert_eq!(verified(path, i, c, stamp, &reports, "").unwrap(), 1);
        let now = ready(path, i, c).unwrap();
        assert!(!now.needs_check() && !now.doubtful);
        assert_eq!(
            now.tests.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t2", "t3"]
        );
        assert_eq!(now.dropped[0].id, "t1");
        assert!(verify_job(path, i, c).is_none());
        let text = tests_material(path, i, c, None);
        assert!(text.contains("matched the reference") && text.contains("dropped_as_wrong"));
    }

    #[test]
    fn a_reference_solution_that_fails_most_tests_is_doubted_instead_of_emptying_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let (i, c) = ("practice-keys-01", "problem-keys-01");
        stored(
            path,
            &sums(&[("t1", false, 3), ("t2", true, 3), ("t3", true, 3)]),
        );
        let stamp = verify_job(path, i, c).unwrap()["stamp"]
            .as_str()
            .unwrap()
            .to_string();
        let reports = [report("t1", false), report("t2", false), report("t3", true)];
        assert_eq!(verified(path, i, c, &stamp, &reports, "").unwrap(), 0);
        let now = ready(path, i, c).unwrap();
        assert!(now.verified && now.doubtful);
        assert_eq!(now.tests.len(), 3);
        assert!(tests_material(path, i, c, None).contains("double-check"));
    }

    #[test]
    fn felix_adds_corrects_and_deletes_tests_and_each_change_undoes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let (i, c) = ("practice-keys-01", "problem-keys-01");
        let mut key = sums(&[("t1", false, 4), ("t2", true, 3)]);
        key.verified = true;
        key.last_run = Some(LastRun {
            at: "2026-10-13T12:00:00Z".into(),
            reports: vec![report("t1", false)],
            error: String::new(),
        });
        stored(path, &key);
        let text = tests_material(path, i, c, None);
        assert!(text.contains("\"last_run\":{") && text.contains("add(1, 2)"));
        let mut fixed = key.tests[0].clone();
        fixed.expected = serde_json::json!(3);
        let undo = change_test(
            path,
            i,
            c,
            &TestChange {
                action: "edit".into(),
                test: fixed,
            },
        )
        .unwrap();
        assert_eq!(
            (undo.action.as_str(), &undo.test.expected),
            ("edit", &serde_json::json!(4))
        );
        let now = ready(path, i, c).unwrap();
        assert_eq!(now.tests[0].expected, serde_json::json!(3));
        assert!(
            now.last_run.is_none(),
            "an old run no longer matches the tests"
        );
        change_test(path, i, c, &undo).unwrap();
        assert_eq!(
            ready(path, i, c).unwrap().tests[0].expected,
            serde_json::json!(4)
        );
        let mut extra = key.tests[1].clone();
        extra.id = "t9".into();
        let undo = change_test(
            path,
            i,
            c,
            &TestChange {
                action: "add".into(),
                test: extra.clone(),
            },
        )
        .unwrap();
        assert_eq!(undo.action, "delete");
        assert_eq!(ready(path, i, c).unwrap().tests.len(), 3);
        assert!(change_test(
            path,
            i,
            c,
            &TestChange {
                action: "add".into(),
                test: extra
            }
        )
        .is_err());
        change_test(path, i, c, &undo).unwrap();
        let gone = change_test(
            path,
            i,
            c,
            &TestChange {
                action: "delete".into(),
                test: key.tests[1].clone(),
            },
        )
        .unwrap();
        assert_eq!((gone.action.as_str(), gone.test.hidden), ("add", true));
        assert!(
            change_test(
                path,
                i,
                c,
                &TestChange {
                    action: "delete".into(),
                    test: key.tests[0].clone()
                }
            )
            .is_err(),
            "the last visible test stays"
        );
        assert!(change_test(
            path,
            i,
            c,
            &TestChange {
                action: "edit".into(),
                test: extra_named("t42")
            }
        )
        .is_err());
    }

    fn extra_named(id: &str) -> Test {
        Test {
            id: id.into(),
            args: vec![],
            expected: serde_json::Value::Null,
            hidden: false,
            unordered: false,
        }
    }

    #[test]
    fn a_sql_key_runs_setup_statements_and_compares_result_rows() {
        let mut challenge = coding_challenge();
        if let Some(coding) = challenge.coding.as_mut() {
            coding.language = "SQL".into();
            coding.starter = "SELECT -- your query".into();
        }
        let setup = "CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1), (2);";
        let mut key = Key {
            entry: "anything".into(),
            tests: vec![Test {
                id: "t1".into(),
                args: vec![serde_json::json!(setup)],
                expected: serde_json::json!([[3]]),
                hidden: false,
                unordered: false,
            }],
            criteria: vec![criterion()],
            solution: "SELECT SUM(x) FROM t".into(),
            ..Key::default()
        };
        assert!(key.validate(&challenge).is_ok());
        assert_eq!(key.entry, "sql");
        assert_eq!(call_of(&key.entry, &key.tests[0]), setup);
        let mut wrong = key.clone();
        wrong.tests[0].args = vec![serde_json::json!(1), serde_json::json!(2)];
        assert!(wrong.validate(&challenge).is_err());
        assert!(is_sql("PostgreSQL") && is_sql("sqlite") && !is_sql("Python"));
    }
}
