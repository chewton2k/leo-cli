use std::time::{Duration, Instant};

const AGAIN: Duration = Duration::from_secs(3);
const QUIT: &str = "leo-quit";

pub fn menu(app: &tauri::AppHandle) -> anyhow::Result<()> {
    use anyhow::Context;
    use tauri::menu::{Menu, MenuItem};

    let menu = Menu::default(app)?;
    let app_menu = menu
        .items()?
        .into_iter()
        .next()
        .and_then(|item| item.as_submenu().cloned())
        .context("Leo could not prepare its app menu. Restart Leo and try again.")?;
    let quit = app_menu
        .items()?
        .pop()
        .context("Leo could not prepare Quit. Restart Leo and try again.")?;
    app_menu.remove(&quit)?;
    app_menu.append(&MenuItem::with_id(
        app,
        QUIT,
        "Quit Leo",
        true,
        Some("CmdOrCtrl+Q"),
    )?)?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id.as_ref() == QUIT {
            app.exit(0);
        }
    });
    Ok(())
}

#[derive(Default)]
pub struct Confirm {
    asked: Option<Instant>,
}

impl Confirm {
    pub fn allows(&mut self, code: Option<i32>, now: Instant) -> bool {
        if code != Some(0) {
            return true;
        }
        if self
            .asked
            .take()
            .is_some_and(|asked| now.saturating_duration_since(asked) <= AGAIN)
        {
            return true;
        }
        self.asked = Some(now);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_quits_need_a_second_attempt_and_confirmation_is_consumed() {
        let now = Instant::now();
        let mut confirm = Confirm::default();
        assert!(!confirm.allows(Some(0), now));
        assert!(confirm.allows(Some(0), now + AGAIN));
        assert!(!confirm.allows(Some(0), now + AGAIN));
    }

    #[test]
    fn an_expired_attempt_starts_a_new_confirmation_window() {
        let now = Instant::now();
        let mut confirm = Confirm::default();
        assert!(!confirm.allows(Some(0), now));
        assert!(!confirm.allows(Some(0), now + AGAIN + Duration::from_nanos(1)));
        assert!(confirm.allows(Some(0), now + AGAIN + Duration::from_secs(1)));
    }

    #[test]
    fn update_restarts_and_error_exits_need_no_confirmation() {
        for code in [None, Some(tauri::RESTART_EXIT_CODE), Some(1), Some(-1)] {
            for armed in [false, true] {
                let now = Instant::now();
                let mut confirm = Confirm::default();
                if armed {
                    assert!(!confirm.allows(Some(0), now));
                }
                assert!(confirm.allows(code, now));
            }
        }
    }
}
