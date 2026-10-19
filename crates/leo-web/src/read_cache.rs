use std::collections::VecDeque;

const MOST_READS: usize = 16;
const MOST_BYTES: usize = 256_000;
const LEAST_BYTES: usize = 512;

#[derive(Clone, Default)]
pub struct Reads(VecDeque<(String, String)>);

impl Reads {
    pub fn compact(&mut self, name: &str, key: &str, result: &str) -> Option<String> {
        if !matches!(
            name,
            "open_note"
                | "read_document"
                | "interview_code"
                | "interview_progress"
                | "interview_tests"
                | "interview_attempt"
                | "interview_workspace"
        ) || !(LEAST_BYTES..=MOST_BYTES / 2).contains(&result.len())
            || result.starts_with("That did not work:")
        {
            return None;
        }
        let key = format!("{name}:{key}");
        let previous = self
            .0
            .iter()
            .position(|(known, _)| known == &key)
            .and_then(|at| self.0.remove(at));
        let unchanged = previous.as_ref().is_some_and(|(_, body)| body == result);
        self.0.push_back((key, result.into()));
        while self.0.len() > MOST_READS
            || self
                .0
                .iter()
                .map(|(key, body)| key.len() + body.len())
                .sum::<usize>()
                > MOST_BYTES
        {
            self.0.pop_front();
        }
        unchanged.then(|| format!("Unchanged: the saved content was rechecked and exactly matches the earlier {name} result for these arguments. Reuse that complete result already in this conversation; no data was changed."))
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_identical_successful_reads_are_reused_and_changes_and_undo_stay_fresh() {
        let mut cache = Reads::default();
        let a = "答え".repeat(200);
        let b = "code".repeat(200);
        for name in [
            "open_note",
            "read_document",
            "interview_code",
            "interview_progress",
            "interview_tests",
            "interview_attempt",
            "interview_workspace",
        ] {
            assert!(cache.compact(name, "first", &a).is_none());
            assert!(cache
                .compact(name, "first", &a)
                .unwrap()
                .contains("Unchanged"));
            assert!(cache.compact(name, "first", &b).is_none());
            assert!(cache.compact(name, "first", &a).is_none());
            assert!(cache.compact(name, "second", &a).is_none());
        }
        assert!(cache.compact("edit_note", "first", &a).is_none());
        let error = format!("That did not work: {}", a);
        assert!(cache.compact("open_note", "failed", &error).is_none());
        assert!(cache.compact("open_note", "failed", &error).is_none());
        cache.clear();
        assert!(cache.compact("open_note", "first", &a).is_none());
    }

    #[test]
    fn read_cache_has_both_count_and_memory_limits() {
        let mut cache = Reads::default();
        for n in 0..100 {
            cache.compact("open_note", &n.to_string(), &"x".repeat(40_000));
        }
        assert!(cache.0.len() <= MOST_READS);
        assert!(
            cache
                .0
                .iter()
                .map(|(key, body)| key.len() + body.len())
                .sum::<usize>()
                <= MOST_BYTES
        );
        assert!(cache
            .compact("open_note", "huge", &"x".repeat(MOST_BYTES))
            .is_none());
    }
}
