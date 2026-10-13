use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chat::{Conversation, SourceRef, Turn};

pub const IDLE: Duration = Duration::from_secs(10 * 60);

pub struct Warm {
    pub key: u64,
    pub chat: String,
    pub seen: usize,
    pub history: u64,
    pub sources: Vec<SourceRef>,
    pub notes: BTreeMap<String, u64>,
    pub documents: BTreeSet<u64>,
    pub sent: usize,
    pub limit: usize,
    pub talk: Box<dyn Conversation>,
    pub used: Instant,
}

#[derive(Default)]
pub struct Slot(Mutex<Option<Warm>>);

pub fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

pub fn config_stamp() -> u64 {
    leo_core::paths::config_dir()
        .ok()
        .and_then(|dir| std::fs::metadata(dir.join("config.toml")).ok())
        .map(|m| hash_of(&(m.modified().ok(), m.len())))
        .unwrap_or(0)
}

pub fn history_of(messages: &[Turn]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for turn in messages {
        turn.role.hash(&mut hasher);
        turn.text.hash(&mut hasher);
    }
    hasher.finish()
}

impl Warm {
    pub fn continues(&self, key: u64, chat: &str, messages: &[Turn]) -> bool {
        self.key == key
            && self.chat == chat
            && self.sent < self.limit
            && self.used.elapsed() < IDLE
            && messages.len() == self.seen + 2
            && messages[self.seen].role == "assistant"
            && messages.last().is_some_and(|t| t.role == "user")
            && history_of(&messages[..self.seen]) == self.history
    }
}

impl Slot {
    pub fn take(&self, key: u64, chat: &str, messages: &[Turn]) -> Option<Warm> {
        let found = self.0.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        if found.continues(key, chat, messages) {
            Some(found)
        } else {
            drop(found);
            None
        }
    }

    pub fn keep(&self, warm: Warm) {
        let old = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(warm);
        drop(old);
    }

    pub fn clear(&self) {
        let old = self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(old);
    }

    pub fn tidy(&self) {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if held.as_ref().is_some_and(|w| w.used.elapsed() >= IDLE) {
            let old = held.take();
            drop(held);
            drop(old);
        }
    }

    #[cfg(test)]
    pub fn holding(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }
}

pub struct Renumbered {
    pub text: String,
    pub sources: Vec<SourceRef>,
    pub notes: BTreeMap<String, u64>,
}

fn block_of(text: &str, n: usize) -> Option<(usize, usize)> {
    let open = format!("<note id=\"n{n}\"");
    let start = text.find(&open)?;
    let end = text[start..].find("</note>\n")? + start + "</note>\n".len();
    let end = if text[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };
    Some((start, end))
}

pub fn renumber(
    text: &str,
    gathered: &[SourceRef],
    sources: &[SourceRef],
    notes: &BTreeMap<String, u64>,
) -> Renumbered {
    let mut all = sources.to_vec();
    let mut kept = notes.clone();
    let mut out = String::new();
    let mut next = all.iter().map(|s| s.n).max().unwrap_or(0);
    for source in gathered {
        let Some((start, end)) = block_of(text, source.n) else {
            continue;
        };
        let block = &text[start..end];
        let body = block.split_once('\n').map_or("", |(_, rest)| rest);
        let fingerprint = hash_of(body);
        if kept.get(&source.id) == Some(&fingerprint) {
            continue;
        }
        let n = match all.iter().find(|s| s.id == source.id) {
            Some(known) => known.n,
            None => {
                next += 1;
                all.push(SourceRef {
                    n: next,
                    ..source.clone()
                });
                next
            }
        };
        let changed = kept.contains_key(&source.id);
        kept.insert(source.id.clone(), fingerprint);
        let head = format!("<note id=\"n{}\"", source.n);
        let mut block = block.replacen(&head, &format!("<note id=\"n{n}\""), 1);
        if changed {
            if let Some((first, rest)) = block.split_once('\n') {
                if let Some(open) = first.strip_suffix('>') {
                    block = format!("{open} updated=\"since you last read it\">\n{rest}");
                }
            }
        }
        out.push_str(&block);
    }
    Renumbered {
        text: out,
        sources: all,
        notes: kept,
    }
}

pub fn fingerprints(text: &str, gathered: &[SourceRef]) -> BTreeMap<String, u64> {
    renumber(text, gathered, &[], &BTreeMap::new()).notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Exchange, Reply};

    fn source(n: usize, id: &str) -> SourceRef {
        SourceRef {
            n,
            id: id.into(),
            title: id.to_uppercase(),
            folder: String::new(),
            why: "matched".into(),
        }
    }

    fn block(n: usize, body: &str) -> String {
        format!("<note id=\"n{n}\" title=\"T\" class=\"c\" included=\"x\">\n{body}\n</note>\n\n")
    }

    fn turn(role: &str, text: &str) -> Turn {
        Turn {
            role: role.into(),
            text: text.into(),
        }
    }

    struct Quiet;
    impl Conversation for Quiet {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, _: Exchange<'_>) -> anyhow::Result<Reply> {
            unreachable!()
        }
    }

    fn warm(messages: &[Turn]) -> Warm {
        Warm {
            key: 7,
            chat: "chat-0001".into(),
            seen: messages.len(),
            history: history_of(messages),
            sources: vec![],
            notes: BTreeMap::new(),
            documents: BTreeSet::new(),
            sent: 0,
            limit: 1_000,
            talk: Box::new(Quiet),
            used: Instant::now(),
        }
    }

    #[test]
    fn notes_already_in_the_session_are_not_sent_again_and_keep_their_numbers() {
        let first = format!("{}{}", block(1, "1| heaps"), block(2, "1| graphs"));
        let gathered = vec![source(1, "heaps"), source(2, "graphs")];
        let started = renumber(&first, &gathered, &[], &BTreeMap::new());
        assert_eq!(started.text, first);
        assert_eq!(started.sources.len(), 2);
        let again = format!(
            "{}{}{}",
            block(1, "1| graphs"),
            block(2, "1| queues"),
            block(3, "1| heaps, now with heap sort")
        );
        let next = renumber(
            &again,
            &[source(1, "graphs"), source(2, "queues"), source(3, "heaps")],
            &started.sources,
            &started.notes,
        );
        assert!(!next.text.contains("graphs"), "unchanged notes stay out");
        assert!(
            next.text.contains("<note id=\"n3\" title=\"T\""),
            "a new note takes the next number"
        );
        assert!(next.text.contains("queues"));
        assert!(next.text.contains("<note id=\"n1\" title=\"T\" class=\"c\" included=\"x\" updated=\"since you last read it\">\n1| heaps, now with heap sort"));
        let numbers: Vec<(usize, &str)> =
            next.sources.iter().map(|s| (s.n, s.id.as_str())).collect();
        assert_eq!(numbers, [(1, "heaps"), (2, "graphs"), (3, "queues")]);
    }

    #[test]
    fn only_the_next_message_of_the_same_chat_continues_a_warm_session() {
        let earlier = vec![turn("user", "what is a heap?")];
        let slot = Slot::default();
        let next = vec![
            turn("user", "what is a heap?"),
            turn("assistant", "A tree…"),
            turn("user", "and heap sort?"),
        ];
        slot.keep(warm(&earlier));
        assert!(slot.take(7, "chat-0001", &next).is_some());
        assert!(!slot.holding(), "taking empties the slot");
        for (key, chat, messages) in [
            (8, "chat-0001", next.clone()),
            (7, "chat-0002", next.clone()),
            (
                7,
                "chat-0001",
                vec![
                    turn("user", "edited"),
                    turn("assistant", "A tree…"),
                    turn("user", "and heap sort?"),
                ],
            ),
            (7, "chat-0001", next[..2].to_vec()),
        ] {
            slot.keep(warm(&earlier));
            assert!(slot.take(key, chat, &messages).is_none());
            assert!(
                !slot.holding(),
                "a session that does not fit is stopped, never kept beside another"
            );
        }
        let mut full = warm(&earlier);
        full.sent = 1_000;
        slot.keep(full);
        assert!(
            slot.take(7, "chat-0001", &next).is_none(),
            "a long conversation starts fresh"
        );
        let mut old = warm(&earlier);
        old.used = Instant::now() - IDLE;
        slot.keep(old);
        slot.tidy();
        assert!(!slot.holding(), "an idle session is stopped");
        slot.keep(warm(&earlier));
        slot.keep(warm(&next));
        assert!(slot.holding());
        slot.clear();
        assert!(!slot.holding());
    }
}
