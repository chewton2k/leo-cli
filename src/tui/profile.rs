//! The provider and settings screen (Ctrl-S): opening it, its keys, and
//! applying what they change. The rows themselves are built in `settings.rs`.

use super::*;

impl App {
    /// Open or rebuild the provider screen. Rows come from the config file and
    /// the keychain every time, so an edit made here or in `$EDITOR` shows up
    /// immediately rather than going stale.
    pub(super) fn open_settings(&mut self, status: Option<String>) {
        let keep = self.settings.as_ref().map(|s| s.selected).unwrap_or(0);
        let cfg = crate::config::Config::load();
        let rows = settings::rows(
            &cfg,
            crate::config::secret::default_store().as_ref(),
            &self.store.notes_dir,
        );
        let selected = if keep == 0 || keep >= rows.len() {
            view::settings::first_selectable(&rows)
        } else {
            keep
        };
        self.settings = Some(SettingsScreen { rows, selected, status });
        self.mode = Mode::Settings;
    }

    pub(super) fn selected_provider(&self) -> Option<(String, Task, bool, settings::ProviderOp)> {
        let screen = self.settings.as_ref()?;
        let row = screen.rows.get(screen.selected)?;
        let name = row.provider_name()?.to_string();
        let task = row.task()?;
        let in_chain = matches!(row, SettingsRow::Member { .. });
        let primary = settings::primary_action(row.credential()?, in_chain);
        Some((name, task, in_chain, primary))
    }

    /// Perform a settings row's action.
    ///
    /// Each of these writes to the config or shells out to git, so they go
    /// through the same code the `:` line uses rather than a parallel path.
    pub(super) fn run_setting<B: TuiBackend>(
        &mut self,
        action: view::settings::SettingAction,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        use view::settings::SettingAction as A;
        match action {
            A::NextAutoPush => {
                let changed = settings::cycle_auto_push()?;
                self.after_settings_change(changed);
                Ok(())
            }

            A::NextTheme => {
                let changed = settings::cycle_theme()?;
                self.after_settings_change(changed);
                // A new palette only shows after a repaint with it installed;
                // the process-wide palette is set once, so say what happened
                // rather than pretending it took effect.
                Ok(())
            }
            A::EditConfig => {
                let out = self
                    .outside(terminal, || crate::run_config(action::ConfigAction::Edit))?;
                if let Err(e) = out {
                    self.say(Kind::Bad, e.to_string());
                }
                self.refresh_settings();
                Ok(())
            }
            A::SyncInit => {
                let dir = self.store.notes_dir.clone();
                match crate::sync::init(&dir) {
                    Ok(()) => {
                        self.say(Kind::Good, "Git backup started. Connect a remote next.");
                        self.refresh_settings();
                    }
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                Ok(())
            }
            A::SyncConnect { current } => {
                // The URL has to be typed, so hand over to the `:` line rather
                // than inventing a second text input on this screen. The existing
                // URL is prefilled so changing one character does not mean
                // retyping the whole thing.
                self.settings = None;
                self.mode = Mode::Command;
                match &current {
                    Some(url) => {
                        self.cmd.open(&format!("sync connect {url}"));
                        self.say(Kind::Dim, "Edit the URL, then Enter.");
                    }
                    None => {
                        self.cmd.open("sync connect ");
                        self.say(Kind::Dim, "Paste the repository URL, then Enter.");
                    }
                }
                Ok(())
            }
            A::SyncPush | A::SyncPull => {
                let notes_dir = self.store.notes_dir.clone();
                let push = matches!(action, A::SyncPush);
                let out = self.outside(terminal, || {
                    if push {
                        crate::sync::push(&notes_dir)
                    } else {
                        crate::sync::pull(&notes_dir)
                    }
                })?;
                match out {
                    Ok(()) => {
                        // A pull rewrites the notes on disk.
                        self.store = Store::load_from(&self.store.notes_dir.clone())?;
                        self.resync();
                        self.refresh_settings();
                    }
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                Ok(())
            }
        }
    }

    /// Rebuild the rows after something on the page changed.
    pub(super) fn refresh_settings(&mut self) {
        if self.settings.is_some() {
            let cfg = crate::config::Config::load();
            let rows = settings::rows(
                &cfg,
                crate::config::secret::default_store().as_ref(),
                &self.store.notes_dir,
            );
            if let Some(screen) = self.settings.as_mut() {
                screen.selected = screen.selected.min(rows.len().saturating_sub(1));
                screen.rows = rows;
            }
        }
    }

    pub(super) fn on_settings_key<B: TuiBackend>(
        &mut self,
        key: event::KeyEvent,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        // Esc and Ctrl-S both close, so the key that opened it also closes it.
        let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
        if key.code == event::KeyCode::Esc || (ctrl && key.code == event::KeyCode::Char('s')) {
            self.settings = None;
            self.mode = Mode::Normal;
            return Ok(());
        }

        // A settings row: appearance, backup, or where things live.
        let selected_action = self
            .settings
            .as_ref()
            .and_then(|s| s.rows.get(s.selected))
            .and_then(|row| row.action().cloned());
        if let Some(action) = selected_action {
            if let Some(screen) = self.settings.as_mut() {
                match key.code {
                    event::KeyCode::Char('j') | event::KeyCode::Down => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                        return Ok(());
                    }
                    event::KeyCode::Char('k') | event::KeyCode::Up => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if matches!(key.code, event::KeyCode::Enter) {
                return self.run_setting(action, terminal);
            }
            return Ok(());
        }

        let Some((name, task, in_chain, primary)) = self.selected_provider() else {
            // Nothing actionable is selected; only movement and closing apply.
            if let Some(screen) = self.settings.as_mut() {
                match key.code {
                    event::KeyCode::Char('j') | event::KeyCode::Down => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                    }
                    event::KeyCode::Char('k') | event::KeyCode::Up => {
                        screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                    }
                    _ => {}
                }
            }
            return Ok(());
        };

        let op = match key.code {
            event::KeyCode::Enter => Some(primary),
            event::KeyCode::Char('l') => Some(settings::ProviderOp::Login),
            event::KeyCode::Char('t') => Some(settings::ProviderOp::Test),
            event::KeyCode::Char('a') if !in_chain => Some(settings::ProviderOp::Add),
            _ => None,
        };
        if let Some(op) = op {
            return self.provider_op(op, &name, task, terminal);
        }

        match key.code {
            event::KeyCode::Char('j') | event::KeyCode::Down => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.selected = view::settings::step(&screen.rows, screen.selected, 1);
                }
            }
            event::KeyCode::Char('k') | event::KeyCode::Up => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.selected = view::settings::step(&screen.rows, screen.selected, -1);
                }
            }

            // Reorder. Capital J/K, so a mistyped movement key cannot silently
            // rewrite the user's config.
            event::KeyCode::Char('J') => {
                let changed = settings::reorder(task, &name, 1)?;
                self.after_settings_change(changed);
            }
            event::KeyCode::Char('K') => {
                let changed = settings::reorder(task, &name, -1)?;
                self.after_settings_change(changed);
            }

            event::KeyCode::Char('d') if in_chain => {
                let changed = settings::remove_from_chain(task, &name)?;
                self.after_settings_change(changed);
            }

            // Removing a key needs no prompt, so it happens in place.
            event::KeyCode::Char('x') => {
                let status = match crate::run_model(crate::action::ModelAction::Logout {
                    name: name.clone(),
                }) {
                    Ok(()) => format!("removed the key for {name}"),
                    Err(e) => e.to_string(),
                };
                self.open_settings(Some(status));
            }

            event::KeyCode::Char('e') => {
                let out = self.outside(terminal, || {
                    crate::run_config(crate::action::ConfigAction::Edit)
                })?;
                let status = match out {
                    Ok(()) => None,
                    Err(e) => Some(e.to_string()),
                };
                self.open_settings(status);
            }

            _ => {}
        }
        Ok(())
    }

    /// Log in to, add, or test the selected provider.
    fn provider_op<B: TuiBackend>(
        &mut self,
        op: settings::ProviderOp,
        name: &str,
        task: Task,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        match op {
            // Storing a key needs a prompt with echo disabled, which needs the
            // real terminal, so drop out of the TUI for it.
            settings::ProviderOp::Login => {
                let target = name.to_string();
                let out = self.outside(terminal, || {
                    crate::run_model(crate::action::ModelAction::Login { name: target })
                })?;
                let status = match out {
                    Ok(()) => format!("stored a key for {name}"),
                    Err(e) => e.to_string(),
                };
                self.open_settings(Some(status));
            }
            settings::ProviderOp::Add => {
                let changed = settings::add_to_chain(task, name)?;
                self.after_settings_change(changed);
            }
            // One small request. Blocking, so say what is happening first.
            settings::ProviderOp::Test => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.status = Some(format!("testing {name}..."));
                }
                terminal.draw(|frame| self.draw(frame))?;
                let status = match crate::test_provider(name) {
                    Ok(report) => report,
                    Err(e) => format!("{name}: {e}"),
                };
                self.open_settings(Some(status));
            }
        }
        Ok(())
    }

    /// Reload the screen after an edit, or report that nothing changed.
    pub(super) fn after_settings_change(&mut self, changed: settings::Changed) {
        match changed {
            settings::Changed::Yes(message) => self.open_settings(Some(message)),
            settings::Changed::No => {
                if let Some(screen) = self.settings.as_mut() {
                    screen.status = Some("nothing to change".to_string());
                }
            }
        }
    }
}
