//! The setup screen's state and keys. Shown on the first run, and by `/setup`.

use super::*;

/// The steps as last checked, which one is selected, and the result of the
/// last thing Enter did.
pub(super) struct WelcomeScreen {
    pub(super) steps: Vec<leo_services::health::Check>,
    pub(super) selected: usize,
    pub(super) status: Option<String>,
}

impl App {
    pub(super) fn open_welcome(&mut self, status: Option<String>) {
        let config = leo_services::config::Config::load();
        let steps = leo_services::health::setup_steps(
            &config,
            leo_services::config::secret::default_store().as_ref(),
            &self.store.notes_dir,
        );
        let selected = self.welcome.as_ref().map_or(0, |w| w.selected);
        self.welcome = Some(WelcomeScreen {
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
                self.say(Kind::Dim, "/setup brings the setup screen back any time.");
            }
            event::KeyCode::Char('j') | event::KeyCode::Down => {
                screen.selected = (screen.selected + 1).min(last);
            }
            event::KeyCode::Char('k') | event::KeyCode::Up => {
                screen.selected = screen.selected.saturating_sub(1);
            }
            event::KeyCode::Enter => match screen.selected {
                // Both AI steps are settled on the provider screen.
                0 | 1 => self.open_settings(None),
                2 => {
                    let result = if leo_services::health::on_path("rec") {
                        let check = leo_services::health::microphone();
                        match &check.state {
                            leo_services::health::State::Ready => {
                                "The microphone is heard. Recording is ready: press R in the notes.".to_string()
                            }
                            leo_services::health::State::Missing { fix } => format!(
                                "Nothing was heard. {} (On a MacBook, the built-in mic is off with the lid closed.)",
                                fix.lines().next().unwrap_or("")
                            ),
                            leo_services::health::State::Warn { note } => format!("The microphone {note}."),
                        }
                    } else {
                        "Install SoX first: brew install sox".to_string()
                    };
                    self.open_welcome(Some(result));
                }
                _ => {
                    self.welcome = None;
                    self.cmd.open("sync connect ");
                    self.mode = Mode::Command;
                    self.say(
                        Kind::Dim,
                        "Make an empty private repository on GitHub, paste its URL, then Enter.",
                    );
                }
            },
            _ => {}
        }
        Ok(())
    }
}
