//! Progress rendering for work the user is waiting on.
//!
//! Two shapes, because there are two kinds of waiting. Transcribing a long
//! recording has a known number of chunks, so it gets a real bar. Structuring
//! notes is one request of unknown length, so it gets a spinner and an elapsed
//! count — which is honest, where a bar creeping to 90% and stopping is not.

use std::time::Duration;

/// Frames of the indeterminate spinner, in order.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// How long each frame shows.
const FRAME: Duration = Duration::from_millis(120);

/// Width of the determinate bar, in cells.
const BAR_WIDTH: usize = 12;

/// What is being waited on.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub label: String,
    /// Known work: (done, total). `None` means an unknown duration.
    pub steps: Option<(usize, usize)>,
}

impl Progress {
    pub fn spinner(label: impl Into<String>) -> Progress {
        Progress {
            label: label.into(),
            steps: None,
        }
    }

    pub fn steps(label: impl Into<String>, done: usize, total: usize) -> Progress {
        Progress {
            label: label.into(),
            steps: Some((done, total)),
        }
    }
}

/// Which spinner frame belongs to `elapsed`.
pub fn spinner_frame(elapsed: Duration) -> &'static str {
    let ticks = (elapsed.as_millis() / FRAME.as_millis().max(1)) as usize;
    SPINNER[ticks % SPINNER.len()]
}

/// Draw a bar like `▕████▁▁▁▁▏`.
fn bar(done: usize, total: usize) -> String {
    let total = total.max(1);
    let filled = (done.min(total) * BAR_WIDTH) / total;
    format!("▕{}{}▏", "█".repeat(filled), "▁".repeat(BAR_WIDTH - filled))
}

/// Render progress as one line for the status bar.
///
/// Deliberately compact: this sits beside the current directory and whatever
/// message was last shown, on a single row.
pub fn render(progress: &Progress, elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let clock = format!("{:02}:{:02}", secs / 60, secs % 60);

    match progress.steps {
        Some((done, total)) => format!(
            "{} {} {}/{} · {clock}",
            progress.label,
            bar(done, total),
            done.min(total),
            total
        ),
        None => format!("{} {} {clock}", spinner_frame(elapsed), progress.label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_determinate_bar_fills_in_proportion() {
        assert_eq!(bar(0, 4), "▕▁▁▁▁▁▁▁▁▁▁▁▁▏");
        assert_eq!(bar(2, 4), "▕██████▁▁▁▁▁▁▏");
        assert_eq!(bar(4, 4), "▕████████████▏");
    }

    #[test]
    fn a_bar_cannot_overflow_or_divide_by_zero() {
        // More done than total is a caller bug, but it must not panic or draw
        // outside the bar.
        let b = bar(99, 4);
        assert_eq!(b.chars().filter(|c| *c == '█').count(), BAR_WIDTH);
        // A zero total is treated as one step.
        assert_eq!(bar(0, 0).chars().filter(|c| *c == '▁').count(), BAR_WIDTH);
    }

    #[test]
    fn the_spinner_advances_with_time_and_wraps() {
        let first = spinner_frame(Duration::ZERO);
        let second = spinner_frame(FRAME);
        assert_ne!(first, second, "the spinner did not advance");
        // A full cycle returns to the start.
        assert_eq!(spinner_frame(FRAME * SPINNER.len() as u32), first);
        // And it never panics on a long wait.
        assert!(!spinner_frame(Duration::from_secs(86_400)).is_empty());
    }

    #[test]
    fn determinate_progress_shows_the_count_and_the_clock() {
        let p = Progress::steps("Transcribing", 3, 8);
        let line = render(&p, Duration::from_secs(75));
        assert!(line.contains("Transcribing"), "{line}");
        assert!(line.contains("3/8"), "{line}");
        assert!(line.contains("01:15"), "{line}");
        assert!(line.contains('█'), "{line}");
    }

    /// An unknown duration must not pretend to know how far along it is.
    #[test]
    fn indeterminate_progress_shows_no_bar() {
        let p = Progress::spinner("Structuring notes");
        let line = render(&p, Duration::from_secs(9));
        assert!(line.contains("Structuring notes"), "{line}");
        assert!(line.contains("00:09"), "{line}");
        assert!(!line.contains('█'), "a spinner must not draw a bar: {line}");
        assert!(!line.contains('/'), "no fake step count: {line}");
    }

    #[test]
    fn a_step_count_past_the_total_is_clamped_in_the_text_too() {
        let line = render(&Progress::steps("x", 12, 8), Duration::ZERO);
        assert!(line.contains("8/8"), "{line}");
    }
}
