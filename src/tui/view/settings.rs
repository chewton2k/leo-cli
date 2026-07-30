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
}

impl Row {
    /// Headers are skipped when moving the selection: there is nothing to do to
    /// a header, so stopping on one would just cost the user a keypress.
    pub fn selectable(&self) -> bool {
        matches!(self, Row::Member { .. } | Row::Unused { .. })
    }

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
            Row::AvailableHeader => None,
        }
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
            Style::default().fg(theme::GOOD),
        ),
        Credential::Env { var, redacted } => Span::styled(
            format!("env {var} {redacted}"),
            Style::default().fg(theme::WARN),
        ),
        Credential::Missing => Span::styled(
            "no key — press l".to_string(),
            Style::default().fg(theme::BAD),
        ),
    }
}

fn item(row: &Row) -> ListItem<'static> {
    match row {
        Row::Header(task) => ListItem::new(TuiLine::from(Span::styled(
            format!(" {} chain", task.label()),
            Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        ))),

        Row::AvailableHeader => ListItem::new(TuiLine::from(Span::styled(
            " also configured".to_string(),
            Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
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

/// The action hints, which double as the only documentation this screen needs.
const HINTS: &str =
    "l login · x remove key · t test · J/K reorder · a add · d drop · e edit file · Esc close";

pub fn render(frame: &mut Frame, area: Rect, rows: &[Row], selected: usize, status: Option<&str>) {
    let box_area = centered(
        area,
        area.width.saturating_sub(4).max(40),
        area.height.saturating_sub(2).max(6),
    );

    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT))
            .title(" providers "),
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
                Style::default().fg(theme::WARN),
            )),
            status_area,
        );
    }

    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {HINTS}"),
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
