use crate::tools::Access;

const MOST_CHARS: usize = 120;
const MOST_TURNS: usize = 6;
const OPENINGS: [&str; 24] = [
    "when ",
    "where ",
    "which ",
    "find ",
    "search ",
    "show ",
    "list ",
    "open ",
    "do i ",
    "did i ",
    "have i ",
    "is there ",
    "are there ",
    "what time ",
    "what's my ",
    "whats my ",
    "what’s my ",
    "what is my ",
    "what are my ",
    "what's on ",
    "what’s on ",
    "what is on ",
    "how many ",
    "how much ",
];
const GREETINGS: [&str; 8] = [
    "hi",
    "hello",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "got it",
];
const DEEPER: [&str; 25] = [
    "why",
    "explain",
    "teach",
    "quiz",
    "practice",
    "practise",
    "compare",
    "prove",
    "solve",
    "derive",
    "calculate",
    "summar",
    "study",
    "write",
    "draft",
    "plan",
    "fix",
    "change",
    "edit",
    "move",
    "delete",
    "rename",
    "create",
    "make",
    "add",
];

pub struct Asked<'a> {
    pub message: &'a str,
    pub turns: usize,
    pub attached: usize,
    pub documents: usize,
    pub selected: bool,
    pub studying: bool,
    pub access: Access,
}

fn plain(message: &str) -> String {
    message
        .trim()
        .trim_end_matches(['?', '!', '.'])
        .to_lowercase()
}

pub fn light(asked: &Asked<'_>) -> bool {
    if asked.studying
        || asked.selected
        || asked.attached > 0
        || asked.documents > 0
        || asked.turns > MOST_TURNS
        || matches!(asked.access, Access::Auto)
    {
        return false;
    }
    let message = plain(asked.message);
    if message.chars().count() > MOST_CHARS || message.contains(['\n', '`', '$']) {
        return false;
    }
    if GREETINGS.contains(&message.as_str()) {
        return true;
    }
    OPENINGS.iter().any(|opening| message.starts_with(opening))
        && !message
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| DEEPER.iter().any(|deep| word.starts_with(deep)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plainly(message: &str) -> Asked<'_> {
        Asked {
            message,
            turns: 1,
            attached: 0,
            documents: 0,
            selected: false,
            studying: false,
            access: Access::Ask,
        }
    }

    #[test]
    fn short_lookups_and_greetings_go_to_the_cheap_model() {
        for message in [
            "when is my bio midterm?",
            "When is Lab 3 due",
            "where did I put my chem notes?",
            "which note has the Krebs cycle",
            "find my reading list",
            "show my tasks for friday",
            "do I have anything tomorrow?",
            "what's my next deadline?",
            "what’s my next deadline?",
            "how many notes mention enzymes?",
            "thanks!",
            "ok",
        ] {
            assert!(light(&plainly(message)), "{message}");
        }
    }

    #[test]
    fn anything_that_needs_thought_stays_on_the_chosen_model() {
        for message in [
            "what is the difference between glycolysis and the krebs cycle?",
            "explain the krebs cycle",
            "why is my code wrong?",
            "quiz me on chapter 7",
            "solve x^2 = 4",
            "which of these answers is right and why",
            "when should I study for bio?",
            "list the steps and explain each",
            "find my note and summarise it",
            "find my note and summarize it",
            "show me how to prove it",
            "where should I move this note?",
            "add a task for friday",
            "wann ist meine prüfung",
            "",
            "show $x^2$",
            "find this\nand that",
            "find ```code```",
        ] {
            assert!(!light(&plainly(message)), "{message}");
        }
        let long = format!("find {}", "notes ".repeat(30));
        assert!(!light(&plainly(&long)));
    }

    #[test]
    fn surrounding_work_keeps_the_chosen_model() {
        let message = "when is lab 3 due?";
        for asked in [
            Asked {
                attached: 1,
                ..plainly(message)
            },
            Asked {
                documents: 1,
                ..plainly(message)
            },
            Asked {
                selected: true,
                ..plainly(message)
            },
            Asked {
                studying: true,
                ..plainly(message)
            },
            Asked {
                turns: MOST_TURNS + 1,
                ..plainly(message)
            },
            Asked {
                access: Access::Auto,
                ..plainly(message)
            },
        ] {
            assert!(!light(&asked));
        }
        assert!(light(&Asked {
            turns: MOST_TURNS,
            access: Access::Read,
            ..plainly(message)
        }));
    }
}
