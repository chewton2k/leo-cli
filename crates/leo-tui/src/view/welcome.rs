//! The setup screen: the four steps to a working leo, each with whether it is
//! done and what Enter does about it.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use leo_services::health::{Check, State};

use super::theme;

/// What Enter does on each step, in order.
const ACTIONS: [&str; 4] = [
    "Enter: add a key or a local model",
    "Enter: add a key or a local model",
    "Enter: test the microphone",
    "Enter: connect a GitHub repository",
];

pub fn render(
    frame: &mut Frame,
    area: Rect,
    steps: &[Check],
    selected: usize,
    status: Option<&str>,
) {
    frame.render_widget(Clear, area);
    let accent = Style::default().fg(theme::accent());
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);

    let mut lines = vec![
        TuiLine::from(Span::styled(
            "Welcome to leo",
            accent.add_modifier(Modifier::BOLD),
        )),
        TuiLine::from(""),
        TuiLine::from(
            "Notes work already. These steps turn on the rest; do any of them now or later",
        ),
        TuiLine::from(Span::styled("with /setup. Esc starts using leo.", dim)),
        TuiLine::from(""),
    ];

    for (i, step) in steps.iter().enumerate() {
        let done = step.state.is_ready();
        let mark = if done { "●" } else { "○" };
        let picked = i == selected;
        let cursor = if picked { "›" } else { " " };
        let state = match &step.state {
            State::Ready => step.detail.clone().unwrap_or_else(|| "ready".to_string()),
            State::Warn { note } => note.clone(),
            State::Missing { fix } => fix.lines().next().unwrap_or("").to_string(),
        };
        let row_style = if picked {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(TuiLine::from(vec![
            Span::styled(format!(" {cursor} "), accent),
            Span::styled(
                format!("{mark} "),
                if done {
                    Style::default().fg(theme::good())
                } else {
                    dim
                },
            ),
            Span::styled(
                format!("{}. {:<18}", i + 1, step.what),
                bold.patch(row_style),
            ),
            Span::styled(state, if done { dim } else { Style::default() }),
        ]));
        if picked {
            lines.push(TuiLine::from(Span::styled(
                format!("        {}", ACTIONS.get(i).copied().unwrap_or("")),
                accent,
            )));
        }
    }

    lines.push(TuiLine::from(""));
    if let Some(status) = status {
        lines.push(TuiLine::from(Span::styled(status.to_string(), accent)));
        lines.push(TuiLine::from(""));
    }
    lines.push(TuiLine::from(Span::styled(
        "j/k move · Enter set up the selected step · Esc start using leo",
        dim,
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(accent)
                    .title(" setup "),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}
