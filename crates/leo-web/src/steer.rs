use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const MOST_WAITING: usize = 8;
pub const MOST_CHARS: usize = 4000;

#[derive(Clone, Default)]
pub struct Steering(Arc<Mutex<HashMap<String, Vec<String>>>>);

pub struct Steer {
    id: String,
    all: Steering,
}

impl Steering {
    pub fn open(&self) -> Steer {
        let id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut all) = self.0.lock() {
            all.insert(id.clone(), Vec::new());
        }
        Steer {
            id,
            all: self.clone(),
        }
    }

    pub fn add(&self, id: &str, text: &str) -> Result<usize, &'static str> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty");
        }
        let Ok(mut all) = self.0.lock() else {
            return Err("gone");
        };
        let Some(waiting) = all.get_mut(id) else {
            return Err("gone");
        };
        if waiting.len() >= MOST_WAITING {
            return Err("full");
        }
        waiting.push(text.chars().take(MOST_CHARS).collect());
        Ok(waiting.len())
    }

    #[cfg(test)]
    pub fn open_ids(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|all| all.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.0.lock().is_ok_and(|all| all.contains_key(id))
    }
}

impl Steer {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn take(&self) -> Vec<String> {
        self.all
            .0
            .lock()
            .ok()
            .and_then(|mut all| all.get_mut(&self.id).map(std::mem::take))
            .unwrap_or_default()
    }
}

impl Drop for Steer {
    fn drop(&mut self) {
        if let Ok(mut all) = self.all.0.lock() {
            all.remove(&self.id);
        }
    }
}

pub fn added(texts: &[String]) -> String {
    if texts.is_empty() {
        return String::new();
    }
    let said: Vec<String> = texts.iter().map(|t| format!("User: {t}")).collect();
    format!(
        "\n\n<user_added_while_you_worked>\n{}\n</user_added_while_you_worked>\nThe user sent this while you were working. It may change what they want: take it into account from now on, and answer it too.",
        said.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_wait_for_the_answer_they_were_sent_to_and_go_with_it() {
        let steering = Steering::default();
        let steer = steering.open();
        let id = steer.id().to_string();
        assert_eq!(steering.add(&id, "  "), Err("empty"));
        assert_eq!(steering.add(&id, "only week 3"), Ok(1));
        assert_eq!(steering.add(&id, "and keep it short"), Ok(2));
        assert_eq!(steer.take(), ["only week 3", "and keep it short"]);
        assert!(steer.take().is_empty(), "each message is delivered once");
        for _ in 0..MOST_WAITING {
            steering.add(&id, "more").unwrap();
        }
        assert_eq!(steering.add(&id, "too many"), Err("full"));
        assert_eq!(steering.add("someone-else", "hi"), Err("gone"));
        drop(steer);
        assert!(!steering.is_open(&id), "a finished answer takes no more");
        assert_eq!(steering.add(&id, "late"), Err("gone"));
    }

    #[test]
    fn what_the_user_added_is_quoted_with_how_to_treat_it() {
        assert_eq!(added(&[]), "");
        let text = added(&["only week 3".into()]);
        assert!(text.contains("User: only week 3"));
        assert!(text.contains("take it into account"));
    }
}
