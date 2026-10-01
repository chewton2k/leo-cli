use anyhow::Result;

use super::chat::{self, clock, points_as_markdown, Jotted, Prompt};
use crate::session::Part;

pub const WORDS_PER_PART: usize = 4000;
const SUMMARY_INPUT_CHARS: usize = 24_000;

pub const PARALLEL_PARTS: usize = 3;

type Written = (String, Option<String>);

pub type Chat<'a> = &'a (dyn Fn(Prompt, u32) -> Result<String> + Sync);
pub type Progress<'a> = &'a (dyn Fn(usize, usize) + Sync);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structured {
    pub title: String,
    pub body: String,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub start_secs: u64,
    pub end_secs: u64,
    pub text: String,
}

pub fn groups(parts: &[Part], words_per_group: usize) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    let mut words = 0;
    for part in parts {
        let count = part.text.split_whitespace().count();
        let start_new = match out.last() {
            None => true,
            Some(_) => words > 0 && words + count > words_per_group,
        };
        if start_new {
            out.push(Group {
                start_secs: part.start_secs,
                end_secs: part.end_secs,
                text: String::new(),
            });
            words = 0;
        }
        let group = out.last_mut().expect("pushed above");
        group.text = super::live::stitch(&group.text, &part.text);
        group.end_secs = part.end_secs;
        words += count;
    }
    out.retain(|g| !g.text.trim().is_empty());
    out
}

fn span(g: &Group) -> String {
    format!("{}–{}", clock(g.start_secs), clock(g.end_secs))
}

fn raw(points: &[Jotted], heading: &str, text: &str) -> String {
    let mut body = String::new();
    if !points.is_empty() {
        body.push_str(&points_as_markdown(points));
        body.push('\n');
    }
    body.push_str(&format!("## {heading}\n\n{}\n", text.trim()));
    body
}

fn points_for(points: &[Jotted], groups: &[Group], index: usize) -> Vec<Jotted> {
    let starts_after = |at: u64| {
        groups
            .get(index + 1)
            .is_none_or(|next| at < next.start_secs)
    };
    let starts_from = |at: u64| index == 0 || at >= groups[index].start_secs;
    points
        .iter()
        .filter(|p| starts_from(p.at_secs) && starts_after(p.at_secs))
        .cloned()
        .collect()
}

pub fn structure_recording(
    parts: &[Part],
    points: &[Jotted],
    existing: Option<&str>,
    fallback_title: &str,
    chat_fn: Chat<'_>,
    progress: Progress<'_>,
) -> Structured {
    let groups = groups(parts, WORDS_PER_PART);
    let mut problems = Vec::new();

    if groups.len() <= 1 {
        let text = groups.first().map(|g| g.text.clone()).unwrap_or_default();
        progress(0, 1);
        let result = match existing {
            Some(body) => chat_fn(
                chat::build_append_prompt_with(&text, body, points),
                super::STRUCTURE_MAX_TOKENS,
            )
            .map(|reply| (fallback_title.to_string(), chat::clean_reply(&reply))),
            None => chat_fn(
                chat::build_structure_prompt_with(&text, points),
                super::STRUCTURE_MAX_TOKENS,
            )
            .map(|reply| chat::split_title_body(&reply)),
        };
        progress(1, 1);
        return match result {
            Ok((title, body)) if !body.trim().is_empty() => Structured {
                title: if title.trim().is_empty() {
                    fallback_title.to_string()
                } else {
                    title
                },
                body,
                problems,
            },
            Ok(_) => Structured {
                title: fallback_title.to_string(),
                body: raw(points, "Transcript", &text),
                problems: vec![
                    "the AI returned an empty note, so the transcript was saved as it is".into(),
                ],
            },
            Err(e) => Structured {
                title: fallback_title.to_string(),
                body: raw(points, "Transcript", &text),
                problems: vec![format!(
                    "the notes could not be written ({e}), so the transcript was saved as it is"
                )],
            },
        };
    }

    let total = groups.len() + usize::from(existing.is_none());
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Written>>> =
        groups.iter().map(|_| std::sync::Mutex::new(None)).collect();
    progress(0, total);
    std::thread::scope(|scope| {
        for _ in 0..PARALLEL_PARTS.min(groups.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(g) = groups.get(i) else {
                    return;
                };
                let own = points_for(points, &groups, i);
                let prompt = chat::build_part_prompt(
                    &g.text,
                    &own,
                    i + 1,
                    groups.len(),
                    &span(g),
                );
                let outcome = match chat_fn(prompt, super::STRUCTURE_MAX_TOKENS) {
                    Ok(reply) if !chat::clean_reply(&reply).trim().is_empty() => {
                        (chat::clean_reply(&reply), None)
                    }
                    Ok(_) => (
                        raw(&own, "Transcript", &g.text),
                        Some(format!("part {} came back empty; its transcript is included instead", i + 1)),
                    ),
                    Err(e) => (
                        raw(&own, "Transcript", &g.text),
                        Some(format!("part {} could not be written ({e}); its transcript is included instead", i + 1)),
                    ),
                };
                if let Ok(mut slot) = results[i].lock() {
                    *slot = Some(outcome);
                }
                let done = finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                progress(done, total);
            });
        }
    });
    let mut sections = Vec::with_capacity(groups.len());
    for (i, (g, slot)) in groups.iter().zip(results).enumerate() {
        let (notes, problem) = slot.into_inner().ok().flatten().unwrap_or_else(|| {
            (
                raw(&points_for(points, &groups, i), "Transcript", &g.text),
                None,
            )
        });
        if let Some(p) = problem {
            problems.push(p);
        }
        sections.push(format!("*{}*\n\n{}", span(g), notes.trim()));
    }

    let joined = sections.join("\n\n---\n\n");
    if let Some(_existing) = existing {
        progress(total, total);
        let mut body = format!("## Recording ({})\n\n", fallback_title);
        body.push_str(&joined);
        return Structured {
            title: fallback_title.to_string(),
            body,
            problems,
        };
    }

    progress(groups.len(), total);
    let digest: String = joined.chars().take(SUMMARY_INPUT_CHARS).collect();
    let (title, summary) = match chat_fn(chat::build_summary_prompt(&digest), 600) {
        Ok(reply) => {
            let (t, s) = chat::split_title_body(&reply);
            (
                if t.trim().is_empty() {
                    fallback_title.to_string()
                } else {
                    t
                },
                s,
            )
        }
        Err(e) => {
            problems.push(format!("the title and summary could not be written ({e})"));
            (fallback_title.to_string(), String::new())
        }
    };
    progress(total, total);

    let mut body = String::new();
    if !summary.trim().is_empty() {
        body.push_str(summary.trim());
        body.push_str("\n\n");
    }
    body.push_str(&joined);
    Structured {
        title,
        body,
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn part(index: u32, words: usize, tag: &str) -> Part {
        Part {
            index,
            start_secs: index as u64 * 300,
            end_secs: (index as u64 + 1) * 300,
            text: (0..words)
                .map(|i| format!("{tag}{i}"))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    #[test]
    fn short_recordings_are_one_group_and_long_ones_split_by_words() {
        let parts: Vec<Part> = (0..10).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let g = groups(&parts, 3500);
        assert_eq!(g.len(), 4);
        assert_eq!((g[0].start_secs, g[0].end_secs), (0, 900));
        assert_eq!((g[3].start_secs, g[3].end_secs), (2700, 3000));
        let all: usize = g.iter().map(|g| g.text.split_whitespace().count()).sum();
        assert_eq!(all, 10_000, "a word was lost or doubled");
        assert_eq!(groups(&parts[..2], 3500).len(), 1);
    }

    #[test]
    fn silent_parts_add_no_empty_groups() {
        let parts = vec![part(0, 0, "x"), part(1, 5, "y"), part(2, 0, "z")];
        let g = groups(&parts, 3);
        assert_eq!(g.len(), 1);
        assert!(g[0].text.starts_with("y0"));
    }

    #[test]
    fn a_short_recording_is_one_request() {
        let calls = AtomicUsize::new(0);
        let chat = |_: Prompt, _: u32| -> Result<String> {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok("Graphs\n\nAbout graphs.\n\n## BFS\n- queue".to_string())
        };
        let s = structure_recording(
            &[part(0, 50, "w")],
            &[],
            None,
            "Recording",
            &chat,
            &|_, _| {},
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(s.title, "Graphs");
        assert!(s.body.contains("## BFS"));
        assert!(s.problems.is_empty());
    }

    #[test]
    fn when_the_ai_fails_the_transcript_is_still_the_note() {
        let chat =
            |_: Prompt, _: u32| -> Result<String> { anyhow::bail!("context length exceeded") };
        let points = vec![Jotted {
            at_secs: 5,
            text: "exam".into(),
        }];
        let s = structure_recording(
            &[part(0, 50, "w")],
            &points,
            None,
            "Recording, Sep 29",
            &chat,
            &|_, _| {},
        );
        assert_eq!(s.title, "Recording, Sep 29");
        assert!(s.body.contains("w0 w1"), "{}", s.body);
        assert!(s.body.contains("- exam\n"), "{}", s.body);
        assert_eq!(s.problems.len(), 1);
    }

    #[test]
    fn a_long_recording_is_written_part_by_part_then_summarized() {
        let parts: Vec<Part> = (0..40).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let prompts = std::sync::Mutex::new(Vec::new());
        let asked = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            prompts.lock().unwrap().push(p.user.clone());
            if p.system.contains("name and summarize") {
                return Ok("A long day\n\nEverything covered.".to_string());
            }
            let n = asked.fetch_add(1, Ordering::SeqCst);
            if n == 2 {
                anyhow::bail!("503")
            } else {
                Ok(format!("## Section {n}\n- point"))
            }
        };
        let points = vec![Jotted {
            at_secs: 7000,
            text: "midterm".into(),
        }];
        let steps = std::sync::Mutex::new(Vec::new());
        let s = structure_recording(&parts, &points, None, "Recording", &chat, &|d, t| {
            steps.lock().unwrap().push((d, t))
        });
        let asked = prompts.lock().unwrap();
        let group_count = groups(&parts, WORDS_PER_PART).len();
        assert_eq!(asked.len(), group_count + 1);
        assert!(
            asked.iter().all(|u| u.chars().count() < 60_000),
            "a request grew with the recording"
        );
        assert_eq!(s.title, "A long day");
        assert!(
            s.body.starts_with("Everything covered."),
            "{}",
            &s.body[..80]
        );
        assert!(
            asked
                .iter()
                .any(|u| u.contains("<typed_points>\n- midterm\n")),
            "the point never reached the part it was typed during"
        );
        assert!(!s.body.starts_with("## Key points"));
        assert!(s.body.contains("## Section 0"));
        assert!(
            s.body.contains("## Transcript"),
            "the failed part fell back to its transcript"
        );
        assert_eq!(s.problems.len(), 1);
        let last = *steps.lock().unwrap().last().unwrap();
        assert_eq!(last.0, last.1);
        for w in ["p0w0", "p39w999"] {
            assert!(
                s.body.contains(w) || asked.iter().any(|u| u.contains(w)),
                "{w} never reached the AI or the note"
            );
        }
    }

    #[test]
    fn every_typed_point_goes_to_exactly_one_part() {
        let group = |start_secs, end_secs| Group {
            start_secs,
            end_secs,
            text: "x".into(),
        };
        let groups = [group(10, 100), group(100, 200), group(200, 300)];
        let points: Vec<Jotted> = [0, 50, 100, 250, 900]
            .into_iter()
            .map(|at_secs| Jotted {
                at_secs,
                text: format!("at {at_secs}"),
            })
            .collect();
        let texts = |i| -> Vec<String> {
            points_for(&points, &groups, i)
                .into_iter()
                .map(|p| p.text)
                .collect()
        };
        assert_eq!(texts(0), ["at 0", "at 50"]);
        assert_eq!(texts(1), ["at 100"]);
        assert_eq!(texts(2), ["at 250", "at 900"]);
    }

    #[test]
    fn parts_are_written_at_the_same_time_and_kept_in_order() {
        let parts: Vec<Part> = (0..30).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            if p.system.contains("name and summarize") {
                return Ok("T\n\nS".to_string());
            }
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(40));
            running.fetch_sub(1, Ordering::SeqCst);
            let first = p
                .user
                .split_whitespace()
                .find(|w| w.starts_with('p'))
                .unwrap_or("")
                .to_string();
            Ok(format!("## From {first}\n- x"))
        };
        let s = structure_recording(&parts, &[], None, "R", &chat, &|_, _| {});
        assert!(
            most.load(Ordering::SeqCst) >= 2,
            "parts were not written in parallel"
        );
        assert!(most.load(Ordering::SeqCst) <= PARALLEL_PARTS);
        let firsts: Vec<usize> = groups(&parts, WORDS_PER_PART)
            .iter()
            .map(|g| {
                s.body
                    .find(&format!(
                        "## From {}",
                        g.text.split_whitespace().next().unwrap()
                    ))
                    .unwrap()
            })
            .collect();
        assert!(
            firsts.windows(2).all(|w| w[0] < w[1]),
            "parts came back out of order"
        );
    }

    #[test]
    fn a_long_addition_to_a_note_has_no_title_request() {
        let parts: Vec<Part> = (0..20).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let calls = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            calls.fetch_add(1, Ordering::Relaxed);
            assert!(!p.system.contains("name and summarize"));
            Ok("## More\n- x".to_string())
        };
        let s = structure_recording(&parts, &[], Some("old"), "Sep 29", &chat, &|_, _| {});
        assert_eq!(
            calls.load(Ordering::Relaxed),
            groups(&parts, WORDS_PER_PART).len()
        );
        assert!(s.body.starts_with("## Recording (Sep 29)"));
    }
}
