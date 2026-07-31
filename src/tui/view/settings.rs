//! The provider screen: what leo will use, in what order, and what is missing.
//!
//! This is the answer to "where do I put my API key" — a list you can walk with
//! j/k rather than a config file you have to find and learn. Everything it shows
//! is derived state, passed in as plain rows, so it renders into a `TestBackend`
//! without a keychain or a network.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::help::centered;
use crate::config::edit::Task;

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
    /// A chain header, e.g. "chat chain".
    Header(Task),
    /// A provider inside a chain, at 1-based position `position`.
    Member {
        task: Task,
        position: usize,
        name: String,
        model: String,
        credential: Credential,
        /// Whether the chain runner would use it right now.
        ready: bool,
    },
    /// The heading above the unused providers.
    AvailableHeader,
    /// A provider defined in the config but not in either chain.
    Unused {
        name: String,
        model: String,
        credential: Credential,
        /// Which chain it would join, inferred from its kind.
        task: Task,
    },
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
    /// Start a git repo in the notes directory.
    SyncInit,
    /// Ask for a remote URL and connect it. Carries the current one, when there
    /// is one, so the prompt can prefill it for editing rather than making the
    /// user retype a URL to change one character of it.
    SyncConnect { current: Option<String> },
    SyncPush,
    SyncPull,
    /// Open config.toml in `$EDITOR`.
    EditConfig,
}

impl SettingAction {
    /// What the footer says the selected row will do.
    pub fn describe(&self) -> &'static str {
        match self {
            SettingAction::NextTheme => "Enter cycles the colour",
            SettingAction::SyncInit => "Enter starts backing up to git",
            SettingAction::SyncConnect { current: None } => "Enter asks for a GitHub URL",
            SettingAction::SyncConnect { .. } => "Enter changes where notes are backed up",
            SettingAction::SyncPush => "Enter pushes now",
            SettingAction::SyncPull => "Enter pulls now",
            SettingAction::EditConfig => "Enter opens config.toml",
        }
    }
}

impl Row {

    pub fn provider_name(&self) -> Option<&str> {
        match self {
            Row::Member { name, .. } | Row::Unused { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn task(&self) -> Option<Task> {
        match self {
            Row::Member { task, .. } | Row::Unused { task, .. } => Some(*task),
            Row::Header(task) => Some(*task),
            _ => None,
        }
    }

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
        !matches!(
            self,
            Row::Header(_) | Row::AvailableHeader | Row::Section(_) | Row::Fact { .. }
        )
    }
}

fn credential_span(credential: &Credential) -> Span<'static> {
    match credential {
        Credential::NotNeeded => Span::styled(
            "no key needed".to_string(),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Credential::Stored => Span::styled(
            "key stored".to_string(),
            Style::default().fg(theme::good()),
        ),
        Credential::Env { var, redacted } => Span::styled(
            format!("env {var} {redacted}"),
            Style::default().fg(theme::warn()),
        ),
        Credential::Missing => Span::styled(
            "no key — press l".to_string(),
            Style::default().fg(theme::bad()),
        ),
    }
}

fn item(row: &Row) -> ListItem<'static> {
    match row {
        Row::Header(task) => ListItem::new(TuiLine::from(Span::styled(
            format!(" {} chain", task.label()),
            Style::default().fg(theme::accent()).add_modifier(Modifier::BOLD),
        ))),

        Row::AvailableHeader => ListItem::new(TuiLine::from(Span::styled(
            " also configured".to_string(),
            Style::default().fg(theme::accent()).add_modifier(Modifier::BOLD),
        ))),

        Row::Member { position, name, model, credential, ready, .. } => {
            // A filled marker means the chain runner would use it now.
            let marker = if *ready { "●" } else { "○" };
            ListItem::new(TuiLine::from(vec![
                Span::styled(
                    format!("  {position}. "),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    marker.to_string(),
                    if *ready {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().add_modifier(Modifier::DIM)
                    },
                ),
                Span::raw(format!(" {name:<22}")),
                Span::styled(
                    format!("{model:<28}"),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                credential_span(credential),
            ]))
        }

        Row::Section(title) => ListItem::new(TuiLine::from(Span::styled(
            format!(" {title}"),
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ))),

        Row::Fact { label, value } => ListItem::new(TuiLine::from(vec![
            Span::raw(format!("     {label:<22}")),
            Span::styled(
                value.clone(),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ])),

        Row::Setting { label, value, .. } => ListItem::new(TuiLine::from(vec![
            Span::raw(format!("     {label:<22}")),
            Span::styled(value.clone(), Style::default().fg(theme::warn())),
        ])),

        Row::Unused { name, model, credential, .. } => ListItem::new(TuiLine::from(vec![
            Span::raw("     "),
            Span::raw(format!("{name:<22}")),
            Span::styled(
                format!("{model:<28}"),
                Style::default().add_modifier(Modifier::DIM),
            ),
            credential_span(credential),
        ])),
    }
}

/// The hints for a provider row, which double as the only documentation this
/// screen needs.
const PROVIDER_HINTS: &str =
    "l login · x remove key · t test · J/K reorder · a add · d drop · e edit file · Esc close";

/// What the footer says, which depends on what is selected: the provider keys
/// mean nothing on a theme row, and offering them there is how a screen starts
/// feeling like a list of everything rather than a place to do something.
fn hints_for(row: Option<&Row>) -> String {
    match row.and_then(Row::action) {
        Some(action) => format!("{} · Esc close", action.describe()),
        None => PROVIDER_HINTS.to_string(),
    }
}

/// The box the page is drawn in, shared by the paint and by hit-testing.
fn page_area(area: Rect) -> Rect {
    centered(
        area,
        area.width.saturating_sub(4).max(40),
        area.height.saturating_sub(2).max(6),
    )
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
    let offset = crate::tui::view::notes::first_visible(selected, total, list.height);
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
            .title(" leo · profile "),
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
            crate::tui::view::notes::first_visible(selected, rows.len(), list_area.height);
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
            Row::Header(Task::Chat),
            Row::Member {
                task: Task::Chat,
                position: 1,
                name: "ollama".to_string(),
                model: "qwen3:8b".to_string(),
                credential: Credential::NotNeeded,
                ready: false,
            },
            Row::Member {
                task: Task::Chat,
                position: 2,
                name: "openrouter".to_string(),
                model: "openrouter/free".to_string(),
                credential: Credential::Stored,
                ready: true,
            },
            Row::Header(Task::Transcribe),
            Row::Member {
                task: Task::Transcribe,
                position: 1,
                name: "groq".to_string(),
                model: "whisper-large-v3-turbo".to_string(),
                credential: Credential::Missing,
                ready: false,
            },
            Row::AvailableHeader,
            Row::Unused {
                name: "cerebras".to_string(),
                model: "llama-3.3-70b".to_string(),
                credential: Credential::Missing,
                task: Task::Chat,
            },
        ]
    }

    #[test]
    fn headers_are_not_selectable_but_providers_are() {
        let r = rows();
        assert!(!r[0].selectable());
        assert!(r[1].selectable());
        assert!(!r[5].selectable());
        assert!(r[6].selectable());
    }

    #[test]
    fn opening_selects_the_first_provider_not_the_header() {
        assert_eq!(first_selectable(&rows()), 1);
        assert_eq!(first_selectable(&[]), 0);
        assert_eq!(first_selectable(&[Row::Header(Task::Chat)]), 0);
    }

    /// Stopping on a header would waste a keypress, so movement skips them.
    #[test]
    fn moving_skips_headers_in_both_directions() {
        let r = rows();
        // 2 (openrouter) down past the transcribe header to 4 (groq).
        assert_eq!(step(&r, 2, 1), 4);
        // 4 back up past that header to 2.
        assert_eq!(step(&r, 4, -1), 2);
        // 4 down past the "also configured" header to 6.
        assert_eq!(step(&r, 4, 1), 6);
    }

    #[test]
    fn movement_saturates_at_both_ends() {
        let r = rows();
        assert_eq!(step(&r, 1, -1), 1, "nothing selectable above");
        assert_eq!(step(&r, 6, 1), 6, "nothing selectable below");
        assert_eq!(step(&[], 0, 1), 0);
    }

    #[test]
    fn the_screen_shows_each_provider_with_its_model_and_key_state() {
        let mut t = Terminal::new(TestBackend::new(110, 16)).unwrap();
        t.draw(|f| render(f, f.area(), &rows(), 1, None)).unwrap();
        let out = t.backend().to_string();

        assert!(out.contains("chat chain"), "{out}");
        assert!(out.contains("ollama"), "{out}");
        assert!(out.contains("qwen3:8b"), "{out}");
        assert!(out.contains("no key needed"), "{out}");
        assert!(out.contains("key stored"), "{out}");
        assert!(out.contains("no key"), "{out}");
        assert!(out.contains("transcribe chain"), "{out}");
        assert!(out.contains("also configured"), "{out}");
        assert!(out.contains("cerebras"), "{out}");
    }

    /// The footer must describe what the selected row does: the provider keys
    /// mean nothing on a theme row.
    #[test]
    fn the_footer_follows_the_selected_row() {
        let provider = Row::Unused {
            name: "groq".into(),
            model: "m".into(),
            credential: Credential::Missing,
            task: Task::Transcribe,
        };
        assert!(hints_for(Some(&provider)).contains("login"));

        let setting = Row::Setting {
            label: "colour".into(),
            value: "orange".into(),
            action: SettingAction::NextTheme,
        };
        let hints = hints_for(Some(&setting));
        assert!(hints.contains("cycles the colour"), "{hints}");
        assert!(!hints.contains("login"), "{hints}");
        // Every footer says how to leave.
        assert!(hints.contains("Esc"), "{hints}");
    }

    /// Every action has to describe itself, or a row can be chosen with no idea
    /// what it will do.
    #[test]
    fn every_action_describes_itself() {
        for action in [
            SettingAction::NextTheme,
            SettingAction::SyncInit,
            SettingAction::SyncConnect { current: None },
            SettingAction::SyncConnect { current: Some("https://example.com/r.git".into()) },
            SettingAction::SyncPush,
            SettingAction::SyncPull,
            SettingAction::EditConfig,
        ] {
            let text = action.describe();
            assert!(text.starts_with("Enter"), "{action:?}: {text}");
            assert!(text.len() > 10, "{action:?}: {text}");
        }
    }

    /// The whole page must render: providers, appearance, backup, storage.
    #[test]
    fn the_profile_page_renders_every_section() {
        let rows = vec![
            Row::Header(Task::Chat),
            Row::Member {
                task: Task::Chat,
                position: 1,
                name: "ollama".into(),
                model: "qwen3:8b".into(),
                credential: Credential::NotNeeded,
                ready: true,
            },
            Row::Section("appearance".into()),
            Row::Setting {
                label: "colour".into(),
                value: "orange  #d97757".into(),
                action: SettingAction::NextTheme,
            },
            Row::Section("backup to github".into()),
            Row::Setting {
                label: "git backup".into(),
                value: "not set up".into(),
                action: SettingAction::SyncInit,
            },
            Row::Section("where things live".into()),
            Row::Fact {
                label: "notes".into(),
                value: "/home/u/notes".into(),
            },
        ];

        let mut t = Terminal::new(TestBackend::new(90, 20)).unwrap();
        t.draw(|f| render(f, f.area(), &rows, 1, None)).unwrap();
        let out = t.backend().to_string();

        for expected in [
            "profile",
            "ollama",
            "appearance",
            "orange",
            "backup to github",
            "not set up",
            "where things live",
            "/home/u/notes",
        ] {
            assert!(out.contains(expected), "missing {expected:?}:\n{out}");
        }
    }

    /// The hints are the only instructions on the screen, so they must render.
    #[test]
    fn the_action_hints_are_always_visible() {
        let mut t = Terminal::new(TestBackend::new(110, 16)).unwrap();
        t.draw(|f| render(f, f.area(), &rows(), 1, None)).unwrap();
        let out = t.backend().to_string();
        for hint in ["l login", "x remove key", "t test", "reorder", "Esc close"] {
            assert!(out.contains(hint), "missing hint {hint:?}:\n{out}");
        }
    }

    #[test]
    fn a_status_message_is_shown_when_present() {
        let mut t = Terminal::new(TestBackend::new(110, 16)).unwrap();
        t.draw(|f| render(f, f.area(), &rows(), 1, Some("openrouter responded in 812ms")))
            .unwrap();
        assert!(t.backend().to_string().contains("responded in 812ms"));
    }

    /// A key value must never reach the screen.
    #[test]
    fn a_credential_renders_without_its_value() {
        let secret = "sk-or-v1-supersecretvalue";
        let rendered = credential_span(&Credential::Stored);
        assert!(!rendered.content.contains(secret));
        assert!(!rendered.content.contains("supersecret"));

        let env = credential_span(&Credential::Env {
            var: "OPENROUTER_API_KEY".to_string(),
            redacted: "…alue".to_string(),
        });
        assert!(env.content.contains("OPENROUTER_API_KEY"));
        assert!(!env.content.contains("supersecret"));
    }

    #[test]
    fn rendering_in_a_small_terminal_does_not_panic() {
        for (w, h) in [(40, 8), (24, 6), (10, 4), (200, 60)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render(f, f.area(), &rows(), 3, Some("x"))).unwrap();
        }
    }

    #[test]
    fn an_out_of_range_selection_is_clamped() {
        let mut t = Terminal::new(TestBackend::new(80, 12)).unwrap();
        t.draw(|f| render(f, f.area(), &rows(), 999, None)).unwrap();
        t.draw(|f| render(f, f.area(), &[], 5, None)).unwrap();
    }
}
