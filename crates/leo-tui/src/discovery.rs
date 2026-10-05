use super::*;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line as TuiLine;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub(super) const ACTIONS: &[(&str, &str)] = &[
    ("New note", "n"),
    ("Search notes", "/"),
    ("Record", "R"),
    ("Settings", ","),
    ("Quick tour", ":tutorial"),
];

pub(super) fn area(screen: Rect) -> Rect {
    view::help::centered(screen, 48, 9)
}

fn offset(area: Rect, selected: usize, count: usize) -> usize {
    let visible = area.height.saturating_sub(2) as usize;
    selected
        .saturating_sub(visible.saturating_sub(1))
        .min(count.saturating_sub(visible))
}

pub(super) fn action_at(screen: Rect, column: u16, row: u16, selected: usize) -> Option<usize> {
    let menu = area(screen);
    if column <= menu.x
        || column >= menu.right().saturating_sub(1)
        || row <= menu.y
        || row >= menu.bottom().saturating_sub(1)
    {
        return None;
    }
    let index = offset(menu, selected, ACTIONS.len()) + (row - menu.y - 1) as usize;
    (index < ACTIONS.len()).then_some(index)
}

pub(super) fn render(frame: &mut Frame, screen: Rect, selected: usize) {
    let area = area(screen);
    let label_width = area.width.saturating_sub(14).min(22) as usize;
    let lines: Vec<_> = ACTIONS
        .iter()
        .enumerate()
        .map(|(i, (label, key))| {
            let style = if i == selected {
                Style::default()
                    .fg(view::theme::accent())
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            TuiLine::styled(format!(" {label:<label_width$.label_width$} {key}"), style)
        })
        .chain([TuiLine::from(" ↑↓ choose · Enter open · Esc close")])
        .collect();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((offset(area, selected, ACTIONS.len()) as u16, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Actions · F2 / Ctrl-K "),
            ),
        area,
    );
}

impl App {
    pub(super) fn on_actions_key<B: TuiBackend>(
        &mut self,
        key: event::KeyEvent,
        mut selected: usize,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        match key.code {
            event::KeyCode::Down | event::KeyCode::Char('j') => {
                selected = (selected + 1).min(ACTIONS.len() - 1)
            }
            event::KeyCode::Up | event::KeyCode::Char('k') => selected = selected.saturating_sub(1),
            event::KeyCode::Enter => return self.choose_action(selected, terminal),
            event::KeyCode::Char('n') => return self.choose_action(0, terminal),
            event::KeyCode::Char('/') => return self.choose_action(1, terminal),
            event::KeyCode::Char('R') => return self.choose_action(2, terminal),
            event::KeyCode::Char(',') => return self.choose_action(3, terminal),
            event::KeyCode::Esc | event::KeyCode::F(2) => return Ok(()),
            _ => {}
        }
        self.mode = Mode::Actions { selected };
        Ok(())
    }

    pub(super) fn choose_action<B: TuiBackend>(
        &mut self,
        selected: usize,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        self.mode = Mode::Normal;
        let intent = match selected {
            0 => Intent::NewNote,
            1 => Intent::OpenFilter,
            2 => Intent::Record,
            3 => Intent::OpenSettings,
            _ => {
                self.open_tour();
                return Ok(());
            }
        };
        self.on_intent(intent, terminal)
    }
}

impl App {
    pub(super) fn render_sources(&self, frame: &mut Frame, screen: Rect, selected: usize) {
        let lines: Vec<_> = self
            .nav
            .answer_sources
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let text = self
                    .store
                    .find_note(id)
                    .map(|n| {
                        if n.directory.is_empty() {
                            n.title.clone()
                        } else {
                            format!("{}/{}", n.directory, n.title)
                        }
                    })
                    .unwrap_or_else(|| "Note no longer exists".to_string());
                let style = if i == selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                TuiLine::styled(format!(" {text}"), style)
            })
            .chain([TuiLine::from(" ↑↓ choose · Enter opens note · Esc back")])
            .collect();
        let area = view::help::centered(screen, 64, (lines.len() + 2) as u16);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((
                    offset(area, selected, self.nav.answer_sources.len()) as u16,
                    0,
                ))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Notes supplied for this answer "),
                ),
            area,
        );
    }

    pub(super) fn on_sources_key(
        &mut self,
        key: event::KeyEvent,
        mut selected: usize,
    ) -> Result<()> {
        match key.code {
            event::KeyCode::Down | event::KeyCode::Char('j') => {
                selected = (selected + 1).min(self.nav.answer_sources.len().saturating_sub(1))
            }
            event::KeyCode::Up | event::KeyCode::Char('k') => selected = selected.saturating_sub(1),
            event::KeyCode::Enter => {
                if let Some(id) = self.nav.answer_sources.get(selected).cloned() {
                    self.nav.filter = None;
                    self.resync();
                    self.jump_to(&id);
                }
                return Ok(());
            }
            event::KeyCode::Esc => return Ok(()),
            _ => {}
        }
        self.mode = Mode::Sources { selected };
        Ok(())
    }
}
