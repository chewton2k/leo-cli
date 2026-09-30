//! The provider screen: what leo will use, in what order, and what is missing.
//!
//! This is the answer to "where do I put my API key" — a list you can walk with
//! j/k rather than a config file you have to find and learn. Everything it shows
//! is derived state, passed in as plain rows, so it renders into a `TestBackend`
//! without a keychain or a network.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use leo_services::config::edit::Task;

use super::theme;

/// Where a provider's credential comes from, or why it has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// Nothing to authenticate — a local server or binary.
    NotNeeded,
    /// Stored in the OS keychain. Deliberately carries no preview of the value:
    /// showing even the last four characters would mean reading the secret, and
    /// on macOS every read of a keychain item can cost a permission dialog.
    Stored,
    /// Coming from an environment variable, which wins over the keychain.
    Env { var: String, redacted: String },
    /// Declared but absent.
    Missing,
}

/// One line of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A plain section heading, for the parts of the page that are not chains.
    Section(String),
    /// A fact with no action: a path, a count, a version.
    Fact { label: String, value: String },
    /// Something that can be changed from this screen.
    Setting {
        label: String,
        value: String,
        /// What pressing Enter does, named so the row can say so.
        action: SettingAction,
    },
}

/// What a settings row does when chosen.
///
/// Named rather than a closure so the rows stay comparable and testable, and so
/// the footer can describe the selected row's action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingAction {
    /// Cycle to the next theme preset.
    NextTheme,
    /// Cycle when leo pushes on its own.
    NextAutoPush,
    /// Start a git repo in the notes directory.
    SyncInit,
    /// Ask for a remote URL and connect it. Carries the current one, when there
    /// is one, so the prompt can prefill it for editing rather than making the
    /// user retype a URL to change one character of it.
    SyncConnect {
        current: Option<String>,
    },
    SyncPush,
    SyncPull,
    /// Open config.toml in `$EDITOR`.
    EditConfig,
    ChooseProvider(Task),
    ChooseModel(Task),
    GetLocalModel(Task),
    StoreKey {
        name: String,
    },
}

impl SettingAction {
    /// What the footer says the selected row will do.
    pub fn describe(&self) -> &'static str {
        match self {
            SettingAction::NextTheme => "Enter cycles the colour",
            SettingAction::NextAutoPush => "Enter changes when leo backs up",
            SettingAction::SyncInit => "Enter starts backing up to git",
            SettingAction::SyncConnect { current: None } => "Enter asks for a GitHub URL",
            SettingAction::SyncConnect { .. } => "Enter changes where notes are backed up",
            SettingAction::SyncPush => "Enter pushes now",
            SettingAction::SyncPull => "Enter pulls now",
            SettingAction::EditConfig => "Enter opens config.toml",
            SettingAction::ChooseProvider(_) => "Enter or → switches to the next · ← goes back",
            SettingAction::ChooseModel(_) => "Enter or → picks the next model · ← goes back",
            SettingAction::GetLocalModel(Task::Chat) => "Enter downloads it with ollama pull",
            SettingAction::GetLocalModel(Task::Transcribe) => "Enter downloads it (142 MB, once)",
            SettingAction::StoreKey { .. } => "Enter asks for the key (never shown) · x removes it",
        }
    }
}

impl Row {
    /// What choosing this row does, when it does anything.
    pub fn action(&self) -> Option<&SettingAction> {
        match self {
            Row::Setting { action, .. } => Some(action),
            _ => None,
        }
    }

    /// Whether the selection should be able to land here.
    ///
    /// Headings and facts are there to be read; stopping on them would cost the
    /// user a keypress for nothing.
    pub fn selectable(&self) -> bool {
        !matches!(self, Row::Section(_) | Row::Fact { .. })
    }
}

fn item(row: &Row) -> ListItem<'static> {
    match row {
        Row::Section(title) => ListItem::new(TuiLine::from(Span::styled(
            format!(" {title}"),
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ))),

        Row::Fact { label, value } => ListItem::new(TuiLine::from(vec![
            Span::raw(format!("     {label:<22}")),
            Span::styled(value.clone(), Style::default().add_modifier(Modifier::DIM)),
        ])),

        Row::Setting { label, value, .. } => ListItem::new(TuiLine::from(vec![
            Span::raw(format!("     {label:<22}")),
            Span::styled(value.clone(), Style::default().fg(theme::warn())),
        ])),
    }
}

/// What the footer says, which depends on what is selected: the provider keys
/// mean nothing on a theme row, and offering them there is how a screen starts
/// feeling like a list of everything rather than a place to do something.
fn hints_for(row: Option<&Row>) -> String {
    match row.and_then(Row::action) {
        Some(action) => format!("{} · Esc close", action.describe()),
        None => "↑↓ move · Esc close".to_string(),
    }
}

/// The box the page is drawn in, shared by the paint and by hit-testing.
///
/// The whole terminal. This is a page rather than a dialog: it holds two chains,
/// a colour, backup and four paths, and an inset box wasted rows on a margin
/// while pushing the parts below the fold.
fn page_area(area: Rect) -> Rect {
    area
}

/// Where the rows are drawn, for mapping a click back to one.
pub fn list_area(area: Rect) -> Rect {
    let box_area = page_area(area);
    Rect {
        x: box_area.x + 1,
        y: box_area.y + 1,
        width: box_area.width.saturating_sub(2),
        // Two rows at the bottom belong to the status and the hints.
        height: box_area.height.saturating_sub(4),
    }
}

/// Which row a click at `row` lands on. `None` outside the list.
pub fn row_at(list: Rect, row: u16, selected: usize, total: usize) -> Option<usize> {
    if row < list.y || row >= list.y + list.height {
        return None;
    }
    let offset = crate::view::notes::first_visible(selected, total, list.height);
    let index = offset + (row - list.y) as usize;
    (index < total).then_some(index)
}

pub fn render(frame: &mut Frame, area: Rect, rows: &[Row], selected: usize, status: Option<&str>) {
    let box_area = page_area(area);

    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::accent()))
            .title(" leo · settings "),
        box_area,
    );

    let inner = Rect {
        x: box_area.x + 1,
        y: box_area.y + 1,
        width: box_area.width.saturating_sub(2),
        height: box_area.height.saturating_sub(2),
    };
    let [list_area, status_area, hint_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let items: Vec<ListItem> = rows.iter().map(item).collect();
    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(selected.min(rows.len() - 1)));
        // The same offset `row_at` assumes, so a click on a scrolled page lands
        // on the row the user is looking at.
        *state.offset_mut() =
            crate::view::notes::first_visible(selected, rows.len(), list_area.height);
    }
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        list_area,
        &mut state,
    );

    if let Some(status) = status {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {status}"),
                Style::default().fg(theme::warn()),
            )),
            status_area,
        );
    }

    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {}", hints_for(rows.get(selected))),
            Style::default().add_modifier(Modifier::DIM),
        )),
        hint_area,
    );
}

/// Move the selection, skipping headers in whichever direction it is going.
pub fn step(rows: &[Row], from: usize, delta: isize) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let mut i = from as isize;
    loop {
        i += delta;
        if i < 0 || i as usize >= rows.len() {
            return from;
        }
        if rows[i as usize].selectable() {
            return i as usize;
        }
    }
}

/// The first selectable row, for opening the screen.
pub fn first_selectable(rows: &[Row]) -> usize {
    rows.iter().position(Row::selectable).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn rows() -> Vec<Row> {
        vec![
            Row::Section("AI".into()),
            Row::Setting {
                label: "writing".into(),
                value: "● Anthropic".into(),
                action: SettingAction::ChooseProvider(Task::Chat),
            },
            Row::Setting {
                label: "writing model".into(),
                value: "claude-sonnet-5-5".into(),
                action: SettingAction::ChooseModel(Task::Chat),
            },
            Row::Fact {
                label: "note".into(),
                value: "read only".into(),
            },
            Row::Setting {
                label: "Anthropic key".into(),
                value: "stored".into(),
                action: SettingAction::StoreKey {
                    name: "anthropic".into(),
                },
            },
            Row::Section("appearance".into()),
            Row::Setting {
                label: "colour".into(),
                value: "orange  #d97757".into(),
                action: SettingAction::NextTheme,
            },
        ]
    }

    #[test]
    fn headings_and_facts_are_skipped_and_settings_are_not() {
        let r = rows();
        assert!(!r[0].selectable());
        assert!(r[1].selectable());
        assert!(!r[3].selectable());
        assert_eq!(first_selectable(&r), 1);
        assert_eq!(first_selectable(&[]), 0);
        assert_eq!(step(&r, 2, 1), 4);
        assert_eq!(step(&r, 4, -1), 2);
        assert_eq!(step(&r, 4, 1), 6);
        assert_eq!(step(&r, 1, -1), 1);
        assert_eq!(step(&r, 6, 1), 6);
        assert_eq!(step(&[], 0, 1), 0);
    }

    #[test]
    fn the_footer_follows_the_selected_row() {
        let r = rows();
        let hints = hints_for(r.get(2));
        assert!(hints.contains("next model"), "{hints}");
        assert!(hints.contains("Esc"), "{hints}");
        assert!(hints_for(r.get(4)).contains("x removes it"));
        assert!(hints_for(r.first()).contains("Esc close"));
    }

    #[test]
    fn every_action_describes_itself() {
        for action in [
            SettingAction::NextTheme,
            SettingAction::SyncInit,
            SettingAction::SyncConnect { current: None },
            SettingAction::SyncConnect {
                current: Some("https://example.com/r.git".into()),
            },
            SettingAction::SyncPush,
            SettingAction::SyncPull,
            SettingAction::EditConfig,
            SettingAction::ChooseProvider(Task::Chat),
            SettingAction::ChooseModel(Task::Transcribe),
            SettingAction::GetLocalModel(Task::Chat),
            SettingAction::GetLocalModel(Task::Transcribe),
            SettingAction::StoreKey {
                name: "openai".into(),
            },
        ] {
            let text = action.describe();
            assert!(text.starts_with("Enter"), "{action:?}: {text}");
            assert!(text.len() > 10, "{action:?}: {text}");
        }
    }

    #[test]
    fn the_page_renders_every_row_and_the_hint() {
        let mut t = Terminal::new(TestBackend::new(90, 20)).unwrap();
        t.draw(|f| render(f, f.area(), &rows(), 1, Some("Writing now uses Anthropic.")))
            .unwrap();
        let out = t.backend().to_string();
        for expected in [
            "settings",
            "writing model",
            "claude-sonnet-5-5",
            "Anthropic key",
            "appearance",
            "orange",
            "Writing now uses Anthropic.",
            "switches to the next",
        ] {
            assert!(out.contains(expected), "missing {expected:?}:\n{out}");
        }
        assert!(!out.contains("chain"), "{out}");
    }

    #[test]
    fn rendering_in_a_small_terminal_does_not_panic() {
        for (w, h) in [(40, 8), (24, 6), (10, 4), (200, 60)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render(f, f.area(), &rows(), 3, Some("x")))
                .unwrap();
        }
    }

    #[test]
    fn an_out_of_range_selection_is_clamped() {
        let mut t = Terminal::new(TestBackend::new(80, 12)).unwrap();
        t.draw(|f| render(f, f.area(), &rows(), 999, None)).unwrap();
        t.draw(|f| render(f, f.area(), &[], 5, None)).unwrap();
    }
}
