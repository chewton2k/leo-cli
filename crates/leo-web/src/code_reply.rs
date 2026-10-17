const MOST_PREFIX: usize = 512;

pub fn missing_tools(text: &str) -> bool {
    let text = text
        .chars()
        .take(MOST_PREFIX)
        .collect::<String>()
        .to_lowercase()
        .replace('’', "'");
    let text = text.trim_start().trim_start_matches('*');
    let text = text.strip_prefix("sorry,").unwrap_or(text).trim_start();
    let first = text
        .split(['.', '!', '?', '\n', ';'])
        .next()
        .unwrap_or(text);
    let personal = [
        "i can't ",
        "i cannot ",
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
        "i cannot run ",
        "i can't execute ",
        "i cannot execute ",
    ]
    .iter()
    .any(|s| first.starts_with(s));
    if execution || (!personal && !named) {
        return false;
    }
    let refuses = [
        "can't",
        "cannot",
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
    refuses && editor
}

pub struct Head {
    held: String,
    released: bool,
    blocked: bool,
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
            "The interview_code_edit tool is not available.",
        ] {
            assert!(missing_tools(text), "{text}");
        }
        for text in [
            "I cannot run code. You can run it in the browser.",
            "I can edit your code; I cannot execute it with my tools.",
            "Read only is active. Switch to Ask or Auto to change the editor.",
            "You can accept the code editing tool's suggestion.",
            "The previous claim that the code editing tools aren't available was wrong.",
            "\"I cannot edit your code\" was the incorrect response.",
            "I cannot run code, but the code editing tools can prepare a change.",
            "The code editing tools are available; the old reply said they were unavailable.",
            "I cannot be certain about every edge case; the editor tools can help us review it.",
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
}
