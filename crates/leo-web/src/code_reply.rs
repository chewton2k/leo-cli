const MOST_PREFIX: usize = 512;

pub fn missing_tools(text: &str) -> bool {
    let text = text
        .chars()
        .take(MOST_PREFIX)
        .collect::<String>()
        .to_lowercase()
        .replace('’', "'");
    let mut fence = None;
    let mut quoted = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let mark = ["```", "~~~"]
            .into_iter()
            .find(|mark| trimmed.starts_with(mark));
        if let Some(mark) = mark {
            if fence == Some(mark) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(mark);
            }
            continue;
        }
        if fence.is_some()
            || trimmed.starts_with('>')
            || line.starts_with("    ")
            || line.starts_with('\t')
        {
            continue;
        }
        if quoted && !trimmed.is_empty() {
            quoted = false;
            continue;
        }
        if trimmed.ends_with(':')
            && [
                "quote",
                "previous",
                "earlier",
                "old",
                "diagnostic",
                "example",
                "said",
                "response",
                "reply",
                "message",
            ]
            .iter()
            .any(|word| trimmed.contains(word))
        {
            quoted = true;
            continue;
        }
        if trimmed.split(['.', '!', '?', ';']).any(missing_claim) {
            return true;
        }
    }
    false
}

fn missing_claim(text: &str) -> bool {
    let text = text.trim_start().trim_start_matches('*');
    let first = text.strip_prefix("sorry,").unwrap_or(text).trim_start();
    let personal = [
        "i can't ",
        "i couldn't ",
        "i cannot ",
        "i could not ",
        "i can not ",
        "i'm unable to ",
        "i am unable to ",
        "i don't have ",
        "i do not have ",
    ]
    .iter()
    .any(|s| first.starts_with(s));
    let named = [
        "the interview_code",
        "interview_code",
        "the `interview_code",
        "`interview_code",
        "the code editing tool",
        "code editing tool",
        "the code-editing tool",
        "code-editing tool",
        "the editor tool",
        "editor tool",
    ]
    .iter()
    .any(|s| first.starts_with(s));
    let execution = [
        "i can't run ",
        "i couldn't run ",
        "i cannot run ",
        "i could not run ",
        "i can't execute ",
        "i couldn't execute ",
        "i cannot execute ",
        "i could not execute ",
    ]
    .iter()
    .any(|s| first.starts_with(s));
    if execution || (!personal && !named) {
        return false;
    }
    let refuses = [
        "can't",
        "couldn't",
        "cannot",
        "could not",
        "can not",
        "unable to",
        "don't have",
        "do not have",
        "no access",
        "not available",
        "unavailable",
        "isn't available",
        "aren't available",
    ]
    .iter()
    .any(|s| first.contains(s));
    let editor = [
        "interview_code",
        "code editing tool",
        "code-editing tool",
        "editor tool",
    ]
    .iter()
    .any(|s| first.contains(s))
        || (personal
            && ["code area", "code editor"]
                .iter()
                .any(|s| first.contains(s)))
        || (["can't edit", "cannot edit", "unable to edit"]
            .iter()
            .any(|s| first.contains(s))
            && (first.contains("code") || first.contains("editor")));
    let applying = [
        "can't apply",
        "cannot apply",
        "couldn't apply",
        "could not apply",
        "unable to apply",
    ]
    .iter()
    .any(|s| first.contains(s))
        && first.contains("editor");
    refuses && (editor || applying)
}

pub struct Head {
    held: String,
    released: bool,
    blocked: bool,
}

pub fn read(name: &str) -> bool {
    matches!(
        name,
        "interview_code"
            | "interview_workspace"
            | "interview_progress"
            | "interview_list"
            | "interview_attempt"
            | "interview_tests"
    )
}

impl Head {
    pub fn new(check: bool) -> Self {
        Self {
            held: String::new(),
            released: !check,
            blocked: false,
        }
    }

    pub fn pending(&self) -> bool {
        !self.released
    }

    pub fn refused(&self) -> bool {
        self.pending() && missing_tools(&self.held)
    }

    pub fn push(&mut self, piece: &str, show: &mut dyn FnMut(&str)) {
        if piece.is_empty() {
            return;
        }
        if self.released {
            show(piece);
            return;
        }
        if self.blocked {
            return;
        }
        let room = MOST_PREFIX.saturating_sub(self.held.chars().count());
        let end = piece
            .char_indices()
            .nth(room)
            .map_or(piece.len(), |(at, _)| at);
        self.held.push_str(piece.get(..end).unwrap_or_default());
        if self.held.chars().count() == MOST_PREFIX {
            if missing_tools(&self.held) {
                self.blocked = true;
                return;
            }
            self.release(show);
            if let Some(rest) = piece.get(end..).filter(|s| !s.is_empty()) {
                show(rest);
            }
        }
    }

    pub fn release(&mut self, show: &mut dyn FnMut(&str)) {
        if !self.blocked && !self.held.is_empty() {
            show(&self.held);
        }
        self.held.clear();
        self.released = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_refusals_are_distinct_from_execution_and_read_only_explanations() {
        for text in [
            "I can’t put it in the code area because the interview code editing tools aren’t available.",
            "I cannot edit your code from here.",
            "I couldn’t access Leo’s code editor tool, so I haven’t changed the code area.",
            "The interview_code_edit tool is not available.",
            "I can see the bug: head.prev is never set. I can’t apply the editor change from the tools available in this chat.",
            "The draft is empty. I cannot apply changes to the editor with these tools.",
            "The bug is in the eviction lookup: `head.prev` is `None`. The least recently used node is `head.next`, so change that line to:\n\n```python\nlru = self.head.next\n```\n\nI can’t apply it to your Leo editor from this chat.",
        ] {
            assert!(missing_tools(text), "{text}");
        }
        for text in [
            "I cannot run code. You can run it in the browser.",
            "I couldn’t run code, but the editor tools can prepare a change.",
            "I could not execute code, but I can use the code editor tools.",
            "I can edit your code; I cannot execute it with my tools.",
            "Read only is active. Switch to Ask or Auto to change the editor.",
            "You can accept the code editing tool's suggestion.",
            "The previous claim that the code editing tools aren't available was wrong.",
            "\"I cannot edit your code\" was the incorrect response.",
            "I cannot run code, but the code editing tools can prepare a change.",
            "The code editing tools are available; the old reply said they were unavailable.",
            "I cannot be certain about every edge case; the editor tools can help us review it.",
            "The bug is in eviction. I cannot run code, but I can prepare the editor change.",
            "The earlier answer said: ‘I cannot apply the editor change’. That claim was wrong.",
            "The old diagnostic was:\nI cannot access the code editor.\nThat was wrong.",
            "```text\nI cannot access the code editor.\n```\nThat was a quoted error.",
            "~~~text\nI cannot apply the editor change.\n~~~\nThat was a quoted error.",
            "> I cannot access the code editor.\nThat was the earlier answer.",
            "The example diagnostic:\n\nI cannot apply the editor change.\nThat is not true here.",
        ] {
            assert!(!missing_tools(text), "{text}");
        }
    }

    #[test]
    fn short_refusals_stay_unshown_and_normal_words_are_released_unchanged() {
        let mut head = Head::new(true);
        let mut shown = String::new();
        for piece in [
            "I can",
            "’t put it in the code area. The code editing tools aren’t available.",
        ] {
            head.push(piece, &mut |s| shown.push_str(s));
        }
        assert!(head.pending());
        assert!(shown.is_empty());
        for text in ["I’ll read your code first.", &"答😀".repeat(MOST_PREFIX)] {
            let mut head = Head::new(true);
            let mut shown = String::new();
            head.push(text, &mut |s| shown.push_str(s));
            head.release(&mut |s| shown.push_str(s));
            assert_eq!(shown, text);
        }
    }

    #[test]
    fn long_refusals_are_bounded_and_disabled_heads_stream_immediately() {
        let refusal = format!(
            "The interview_code_edit tool isn't available. {}",
            "答😀".repeat(MOST_PREFIX)
        );
        let mut head = Head::new(true);
        let mut shown = String::new();
        head.push(&refusal, &mut |s| shown.push_str(s));
        head.push(&refusal, &mut |s| shown.push_str(s));
        assert_eq!(head.held.chars().count(), MOST_PREFIX);
        assert!(head.pending());
        head.release(&mut |s| shown.push_str(s));
        assert!(shown.is_empty());
        let mut head = Head::new(false);
        head.push(&refusal, &mut |s| shown.push_str(s));
        assert_eq!(shown, refusal);
    }

    #[test]
    fn recovery_can_follow_reads_but_never_a_change_or_user_question() {
        for name in [
            "interview_code",
            "interview_progress",
            "interview_list",
            "interview_attempt",
            "interview_tests",
        ] {
            assert!(read(name));
        }
        for name in [
            "interview_code_edit",
            "interview_challenge",
            "ask_user",
            "quiz",
            "add_task",
            "edit_note",
            "unknown",
        ] {
            assert!(!read(name));
        }
        let mut head = Head::new(true);
        head.push("I couldn’t access Leo’s code editor tool.", &mut |_| {
            panic!("a refusal must stay held")
        });
        assert!(head.refused());
        let mut head = Head::new(true);
        head.push("The empty editor is ready for your solution.", &mut |_| {});
        assert!(!head.refused());
    }
}
