use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Need {
    Recording,
    Writing,
}

pub(super) struct WelcomeScreen {
    pub(super) need: Need,
    pub(super) steps: Vec<leo_services::health::Check>,
    pub(super) selected: usize,
    pub(super) status: Option<String>,
}

pub(super) fn real_setup_steps(notes_dir: &std::path::Path) -> Vec<leo_services::health::Check> {
    leo_services::health::setup_steps(
        &leo_services::config::Config::load(),
        leo_services::config::secret::default_store().as_ref(),
        notes_dir,
    )
}

fn blocking(need: Need, what: &str) -> bool {
    match need {
        Need::Recording => matches!(what, "AI for speech" | "Recording"),
        Need::Writing => what == "AI for writing",
    }
}

fn wanted(need: Need, what: &str) -> bool {
    match need {
        Need::Recording => matches!(what, "AI for writing" | "AI for speech" | "Recording"),
        Need::Writing => what == "AI for writing",
    }
}

impl App {
    pub(super) fn missing_for(&self, need: Need) -> Vec<leo_services::health::Check> {
        (self.setup_steps)(&self.store.notes_dir)
            .into_iter()
            .filter(|step| blocking(need, &step.what) && !step.state.is_ready())
            .collect()
    }

    pub(super) fn set_up_first(&mut self, need: Need) -> bool {
        if self.missing_for(need).is_empty() {
            return false;
        }
        self.open_welcome(need, None);
        true
    }

    pub(super) fn open_welcome(&mut self, need: Need, status: Option<String>) {
        let steps: Vec<_> = (self.setup_steps)(&self.store.notes_dir)
            .into_iter()
            .filter(|step| wanted(need, &step.what))
            .collect();
        let selected = steps
            .iter()
            .position(|step| !step.state.is_ready())
            .unwrap_or(0);
        self.welcome = Some(WelcomeScreen {
            need,
            steps,
            selected,
            status,
        });
        self.mode = Mode::Welcome;
    }

    pub(super) fn on_welcome_key(&mut self, key: event::KeyEvent) -> Result<()> {
        let Some(screen) = self.welcome.as_mut() else {
            self.mode = Mode::Normal;
            return Ok(());
        };
        let last = screen.steps.len().saturating_sub(1);
        match key.code {
            event::KeyCode::Esc | event::KeyCode::Char('q') => {
                self.welcome = None;
                self.mode = Mode::Normal;
                self.say(Kind::Dim, "/doctor checks everything any time.");
            }
            event::KeyCode::Char('j') | event::KeyCode::Down => {
                screen.selected = (screen.selected + 1).min(last);
            }
            event::KeyCode::Char('k') | event::KeyCode::Up => {
                screen.selected = screen.selected.saturating_sub(1);
            }
            event::KeyCode::Enter => {
                let need = screen.need;
                let what = screen
                    .steps
                    .get(screen.selected)
                    .map(|s| s.what.clone())
                    .unwrap_or_default();
                match what.as_str() {
                    "Recording" => {
                        let result = if leo_services::session::mic::microphone_name().is_some() {
                            let check = leo_services::health::microphone();
                            match &check.state {
                                leo_services::health::State::Ready => {
                                    "The microphone is heard. Recording is ready: press R.".to_string()
                                }
                                leo_services::health::State::Missing { fix } => format!(
                                    "Nothing was heard. {} (On a MacBook, the built-in mic is off with the lid closed.)",
                                    fix.lines().next().unwrap_or("")
                                ),
                                leo_services::health::State::Warn { note } => {
                                    format!("The microphone {note}.")
                                }
                            }
                        } else {
                            "No microphone was found. Plug one in, or pick an input in your sound settings.".to_string()
                        };
                        self.open_welcome(need, Some(result));
                    }
                    _ => {
                        self.welcome = None;
                        self.open_settings(None);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
