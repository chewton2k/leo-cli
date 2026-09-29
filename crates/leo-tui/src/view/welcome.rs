//! The setup screen: the four steps to a working leo, each with whether it is
//! done and what Enter does about it.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use leo_services::health::{Check, State};

use super::theme;

fn action(what: &str) -> &'static str {
    match what {
        "Recording" => "Enter: test the microphone",
        "Backup to GitHub" => "Enter: connect a GitHub repository",
        _ => "Enter: add a key or a local model",
    }
}

pub struct Heading<'a> {
    pub title: &'a str,
    pub why: &'a str,
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    heading: Heading<'_>,
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
            heading.title.to_string(),
            accent.add_modifier(Modifier::BOLD),
        )),
        TuiLine::from(""),
        TuiLine::from(heading.why.to_string()),
        TuiLine::from(Span::styled(
            "Everything else in leo works without these. Esc goes back.",
            dim,
        )),
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
                format!("        {}", action(&step.what)),
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
        "j/k move · Enter set up the selected step · Esc back",
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
