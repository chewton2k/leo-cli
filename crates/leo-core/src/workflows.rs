use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Profile {
    pub template: String,
    pub context: String,
    pub vocabulary: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workflows {
    pub profiles: BTreeMap<String, Profile>,
    pub templates: Vec<Prompt>,
    pub recipes: Vec<Prompt>,
}

pub fn templates() -> Vec<Prompt> {
    [
        ("lecture", "Lecture", "Keep the full study-note format, examples, worked explanations and Check yourself questions."),
        ("meeting", "Meeting", "Use a concise meeting format: Discussion, Decisions, Open questions, and Action items. Keep useful gap-filling explanations, but omit study exercises and programming walkthroughs unless asked. Include owners and deadlines only when mentioned."),
        ("interview", "Technical interview", "Organize by questions, candidate approaches, code and complexity, tradeoffs, and follow-up questions. Preserve useful explanations and worked examples. Do not invent hiring judgments."),
        ("design-review", "Design review", "Organize by problem, proposed design, alternatives, tradeoffs, decisions, blockers, and next steps. Keep useful explanations. Omit study exercises unless requested."),
    ].into_iter().map(|(id, name, prompt)| Prompt { id: id.into(), name: name.into(), prompt: prompt.into() }).collect()
}

pub fn recipes() -> Vec<Prompt> {
    [
        ("revision", "Make a revision sheet", "Make a revision sheet from these notes, covering key ideas, examples, common mistakes and questions to practice."),
        ("decisions", "Extract decisions", "List the decisions recorded in these notes, with their reasons and source citations. Say when something is still unresolved."),
        ("questions", "Find open questions", "List unresolved questions across these notes, grouped by topic, with source citations."),
        ("actions", "Extract action items", "Extract action items with their owners and deadlines where mentioned, and source citations. Do not invent assignments."),
    ].into_iter().map(|(id, name, prompt)| Prompt { id: id.into(), name: name.into(), prompt: prompt.into() }).collect()
}

impl Workflows {
    pub fn path(notes: &Path) -> std::path::PathBuf {
        notes.parent().unwrap_or(notes).join("workflows.json")
    }

    pub fn load(notes: &Path) -> Result<Self> {
        let path = Self::path(notes);
        let path = crate::paths::contained_path(
            path.parent().unwrap_or(notes),
            Path::new("workflows.json"),
        )?;
        if path.exists() {
            anyhow::ensure!(
                std::fs::metadata(&path)?.len() <= 8_000_000,
                "Workflows exceed the size limit"
            );
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                let workflows: Self = serde_json::from_slice(&bytes)?;
                workflows.validate()?;
                Ok(workflows)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, notes: &Path) -> Result<()> {
        self.validate()?;
        crate::recording::write_json(&Self::path(notes), self)
    }

    pub fn profile(&self, dir: &str) -> Profile {
        let mut current = dir;
        loop {
            if let Some(p) = self.profiles.get(current) {
                return p.clone();
            }
            if current.is_empty() {
                return Profile::default();
            }
            current = current.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    pub fn format(&self, id: &str) -> Option<Prompt> {
        self.templates
            .iter()
            .chain(templates().iter())
            .find(|t| t.id == id)
            .cloned()
    }

    pub fn validate(&self) -> Result<()> {
        if self.profiles.len() > 500 || self.templates.len() > 100 || self.recipes.len() > 100 {
            bail!("Too many workflows");
        }
        for p in self.profiles.values() {
            if p.context.len() > 8000
                || p.vocabulary.len() > 100
                || p.vocabulary
                    .iter()
                    .any(|v| v.trim().is_empty() || v.chars().count() > 100)
            {
                bail!("Use up to 100 short vocabulary terms and 8,000 characters of context");
            }
            if !p.template.is_empty() && self.format(&p.template).is_none() {
                bail!("Unknown note template");
            }
        }
        for list in [&self.templates, &self.recipes] {
            let mut ids = std::collections::HashSet::new();
            for p in list {
                if !crate::recording::valid_id(&p.id)
                    || !ids.insert(&p.id)
                    || p.name.trim().is_empty()
                    || p.name.len() > 100
                    || p.prompt.trim().is_empty()
                    || p.prompt.len() > 12000
                {
                    bail!("Each workflow needs a unique id, a name, and a prompt of up to 12,000 characters");
                }
                if templates().iter().any(|t| t.id == p.id)
                    || recipes().iter().any(|t| t.id == p.id)
                {
                    bail!("Choose a new id for a custom workflow");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_folders_inherit_and_can_override_formats() {
        let mut w = Workflows::default();
        w.profiles.insert(
            "cs130".into(),
            Profile {
                template: "lecture".into(),
                vocabulary: vec!["Dijkstra".into()],
                ..Default::default()
            },
        );
        assert_eq!(w.profile("cs130/week1").vocabulary, ["Dijkstra"]);
        w.profiles.insert(
            "cs130/week1".into(),
            Profile {
                template: "meeting".into(),
                ..Default::default()
            },
        );
        assert_eq!(w.profile("cs130/week1").template, "meeting");
        w.validate().unwrap();
    }
}
