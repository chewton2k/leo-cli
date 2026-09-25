//! Pane rendering.
//!
//! Every view takes plain data rather than the `App`, so each can be rendered
//! into a `TestBackend` and asserted on without a store, a terminal, or any
//! network access.

pub mod dirs;
pub mod empty;
pub mod help;
pub mod hints;
pub mod line;
pub mod markdown;
pub mod menu;
pub mod notes;
pub mod preview;
pub mod progress;
pub mod settings;
pub mod status;
pub mod tabs;
pub mod theme;
pub mod welcome;

use ratatui::layout::{Constraint, Layout, Rect};

use super::keys::Pane;

/// Where each piece of the screen goes.
pub struct Frames {
    /// The strip of recent notes. Zero height when there are none, so an unused
    /// feature costs no space.
    pub tabs: Rect,
    pub dirs: Rect,
    pub notes: Rect,
    pub preview: Rect,
    /// The `:` line.
    pub command: Rect,
    /// One line of transient state below the command line.
    pub status: Rect,
}

impl Frames {
    /// Whether a pane is on screen. A hidden pane has no width, so focusing it
    /// or clicking where it used to be must do nothing.
    pub fn shows(&self, pane: Pane) -> bool {
        let rect = match pane {
            Pane::Dirs => self.dirs,
            Pane::Notes => self.notes,
            Pane::Preview => self.preview,
        };
        rect.width > 0
    }
}

/// Widths at which the layout changes shape.
///
/// Chosen from what the panes need rather than round numbers: the dirs column is
/// 18 wide and a note title needs roughly 24 to be worth reading, so below about
/// 90 there is not room for three panes without one of them being useless.
const THREE_PANE_MIN: u16 = 90;
/// Below this, one pane at a time: two panes of 30 columns each show almost
/// nothing but borders.
const TWO_PANE_MIN: u16 = 60;
/// Below this many rows the tab strip is dropped: with so little height, a row
/// of note titles costs more than it gives.
const TABS_MIN_HEIGHT: u16 = 12;

/// Fixed width of the directories column.
const DIRS_WIDTH: u16 = 18;

/// Width of the notes list.
///
/// Fixed rather than proportional, because a list of titles does not get more
/// useful with more room: it held a `Min` before and so absorbed every extra
/// column, ending up wider than the pane showing the note itself. Wide enough for
/// a three-digit number, a title of about thirty characters, and a tag.
const NOTES_WIDTH: u16 = 34;

/// Smallest useful preview. Below this a wrapped line is mostly hyphens.
const PREVIEW_MIN: u16 = 24;

/// How the panes are arranged at the current size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Directories, notes and the preview.
    Three,
    /// Notes and the preview; directories are reachable with `:cd`, and the
    /// current one is in the status bar.
    Two,
    /// Only the focused pane. Everything stays reachable with `h`/`l`, which is
    /// what makes this usable rather than merely small.
    One,
}

impl Shape {
    pub fn for_width(width: u16) -> Self {
        if width >= THREE_PANE_MIN {
            Shape::Three
        } else if width >= TWO_PANE_MIN {
            Shape::Two
        } else {
            Shape::One
        }
    }
}

/// Split the terminal into panes over a command line and a status line.
///
/// Responsive by necessity rather than fashion: at 120 columns three panes are
/// comfortable, at 70 the notes list is squeezed to nothing by a fixed dirs
/// column and a proportional preview, and at 40 nothing but borders fits. So the
/// shape changes with the width, and a pane that is not shown gets a zero-width
/// rect — which also means the mouse hit-test can never match it, for free.
///
/// `tabs` asks for the recent-notes strip; it is dropped anyway when there is not
/// enough height. `focus` decides which pane survives at the narrowest size.
///
/// The only way to compute the layout, deliberately: a variant that omitted the
/// tab row existed for one commit and put every click one line out.
pub fn layout_with_tabs(area: Rect, tabs: bool, focus: Pane) -> Frames {
    let tab_height = u16::from(tabs && area.height >= TABS_MIN_HEIGHT);

    // The command and status lines are the last things given up: without them
    // there is no way to type a command or read what happened.
    let (command_height, status_height) = match area.height {
        0 => (0, 0),
        1 => (0, 1),
        _ => (1, 1),
    };

    let [tabs_area, body, command, status] = Layout::vertical([
        Constraint::Length(tab_height),
        Constraint::Min(0),
        Constraint::Length(command_height),
        Constraint::Length(status_height),
    ])
    .areas(area);

    let empty = Rect::new(body.x, body.y, 0, body.height);
    let (dirs, notes, preview) = match Shape::for_width(area.width) {
        Shape::Three => {
            // The preview takes what is left, so extra width goes to the note
            // rather than to whitespace beside its title.
            let [dirs, notes, preview] = Layout::horizontal([
                Constraint::Length(DIRS_WIDTH),
                Constraint::Length(NOTES_WIDTH),
                Constraint::Min(PREVIEW_MIN),
            ])
            .areas(body);
            (dirs, notes, preview)
        }
        Shape::Two => {
            // Same reasoning without the dirs column: the list gets what it
            // needs and the note gets the rest.
            let [notes, preview] = Layout::horizontal([
                Constraint::Length(NOTES_WIDTH),
                Constraint::Min(PREVIEW_MIN),
            ])
            .areas(body);
            (empty, notes, preview)
        }
        Shape::One => match focus {
            Pane::Dirs => (body, empty, empty),
            Pane::Notes => (empty, body, empty),
            Pane::Preview => (empty, empty, body),
        },
    };

    Frames {
        tabs: tabs_area,
        dirs,
        notes,
        preview,
        command,
        status,
    }
}

#[cfg(test)]
mod tests {
    fn frames(width: u16, height: u16, tabs: bool) -> Frames {
        layout_with_tabs(Rect::new(0, 0, width, height), tabs, Pane::Notes)
    }

    /// The whole terminal is used: nothing is left over, and nothing overflows.
    #[test]
    fn the_layout_fills_the_terminal_exactly() {
        for (w, h) in [(120, 40), (100, 30), (80, 24), (60, 20), (40, 15), (30, 10)] {
            let f = frames(w, h, true);

            // Vertical: every row is accounted for, in order, with no gaps.
            let rows = f.tabs.height + f.notes.height + f.command.height + f.status.height;
            assert_eq!(rows, h, "{w}x{h}: rows do not sum to the height");
            assert_eq!(f.tabs.y, 0);
            assert_eq!(
                f.status.y + f.status.height,
                h,
                "{w}x{h}: bottom row unused"
            );

            // Horizontal: the visible panes span the full width.
            let used = f.dirs.width + f.notes.width + f.preview.width;
            assert_eq!(used, w, "{w}x{h}: panes do not span the width");
            assert!(f.notes.x + f.notes.width <= w);
            assert!(f.preview.x + f.preview.width <= w);
        }
    }

    /// Three shapes, chosen by width. Below 90 a fixed dirs column and a
    /// proportional preview squeeze the notes list to nothing.
    #[test]
    fn the_shape_changes_with_the_width() {
        assert_eq!(Shape::for_width(120), Shape::Three);
        assert_eq!(Shape::for_width(90), Shape::Three);
        assert_eq!(Shape::for_width(89), Shape::Two);
        assert_eq!(Shape::for_width(60), Shape::Two);
        assert_eq!(Shape::for_width(59), Shape::One);
        assert_eq!(Shape::for_width(20), Shape::One);
        assert_eq!(Shape::for_width(0), Shape::One);
    }

    #[test]
    fn a_wide_terminal_shows_all_three_panes() {
        let f = frames(120, 30, false);
        assert!(f.shows(Pane::Dirs));
        assert!(f.shows(Pane::Notes));
        assert!(f.shows(Pane::Preview));
        assert_eq!(f.dirs.width, DIRS_WIDTH);
        assert_eq!(f.notes.width, NOTES_WIDTH);
        // The note itself takes the remainder.
        assert_eq!(f.preview.width, 120 - DIRS_WIDTH - NOTES_WIDTH);
    }

    /// The dirs pane is the one to give up first: the current directory is in the
    /// status bar and `:cd` still works.
    #[test]
    fn a_medium_terminal_drops_the_directories_pane() {
        let f = frames(70, 24, false);
        assert!(!f.shows(Pane::Dirs));
        assert!(f.shows(Pane::Notes));
        assert!(f.shows(Pane::Preview));
        // The list keeps its fixed width and the note gets the rest.
        assert_eq!(f.notes.width + f.preview.width, 70);
        assert_eq!(f.notes.width, NOTES_WIDTH);
        assert!(
            f.preview.width > f.notes.width,
            "the note pane is the smaller one"
        );
    }

    /// The pane showing the note must get the extra room, not the list of
    /// titles. The list used to hold a `Min` and so absorbed every extra column,
    /// ending up wider than the note itself at every size.
    #[test]
    fn extra_width_goes_to_the_note_not_the_list() {
        let mut last_preview = 0;
        for width in [90, 110, 130, 160, 200] {
            let f = frames(width, 30, false);
            assert_eq!(f.notes.width, NOTES_WIDTH, "the list grew at {width}");
            assert!(
                f.preview.width > f.notes.width,
                "at {width} the list ({}) is wider than the note ({})",
                f.notes.width,
                f.preview.width
            );
            assert!(f.preview.width > last_preview, "the note pane did not grow");
            last_preview = f.preview.width;
        }
    }

    /// The list must still fit a number, a title and a tag.
    #[test]
    fn the_list_is_wide_enough_to_read() {
        let f = frames(120, 30, false);
        // Four columns for the number and a space, two for borders.
        assert!(
            f.notes.width >= 30,
            "only {} columns for titles",
            f.notes.width
        );
    }

    /// At the narrowest size the focused pane takes the screen, so everything
    /// stays reachable with h and l rather than becoming unusable.
    #[test]
    fn a_narrow_terminal_shows_only_the_focused_pane() {
        for (focus, other_a, other_b) in [
            (Pane::Notes, Pane::Dirs, Pane::Preview),
            (Pane::Dirs, Pane::Notes, Pane::Preview),
            (Pane::Preview, Pane::Dirs, Pane::Notes),
        ] {
            let f = layout_with_tabs(Rect::new(0, 0, 40, 20), false, focus);
            assert!(f.shows(focus), "{focus:?} is focused but not drawn");
            assert!(!f.shows(other_a), "{other_a:?} drawn at 40 columns");
            assert!(!f.shows(other_b), "{other_b:?} drawn at 40 columns");
            // The one pane gets everything.
            let rect = match focus {
                Pane::Dirs => f.dirs,
                Pane::Notes => f.notes,
                Pane::Preview => f.preview,
            };
            assert_eq!(rect.width, 40);
        }
    }

    /// A short terminal gives the row back to the panes.
    #[test]
    fn the_tab_strip_is_dropped_when_there_is_no_height_for_it() {
        assert_eq!(frames(100, 30, true).tabs.height, 1);
        assert_eq!(frames(100, TABS_MIN_HEIGHT, true).tabs.height, 1);
        assert_eq!(frames(100, TABS_MIN_HEIGHT - 1, true).tabs.height, 0);
        assert_eq!(frames(100, 6, true).tabs.height, 0);
        // And asking for no tabs never reserves the row.
        assert_eq!(frames(100, 40, false).tabs.height, 0);
    }

    /// The command and status lines are the last thing given up: without them
    /// there is no way to type a command or read what happened.
    #[test]
    fn the_command_and_status_lines_survive_until_there_is_no_room() {
        let f = frames(80, 24, false);
        assert_eq!(f.command.height, 1);
        assert_eq!(f.status.height, 1);

        let f = frames(80, 2, false);
        assert_eq!(f.command.height, 1);
        assert_eq!(f.status.height, 1);

        // One row left: the status line, which is where errors go.
        let f = frames(80, 1, false);
        assert_eq!(f.status.height, 1);
        assert_eq!(f.command.height, 0);
    }

    /// Degenerate sizes must not panic: terminals report 0x0 while resizing.
    #[test]
    fn absurd_sizes_do_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (0, 30), (30, 0), (1, 60), (400, 200)] {
            let f = frames(w, h, true);
            assert!(f.notes.width <= w);
            assert!(f.status.height <= h);
        }
    }

    /// A pane that is not drawn must not be clickable, which falls out of giving
    /// it no width.
    #[test]
    fn a_hidden_pane_has_no_area_to_click() {
        let f = frames(40, 20, false);
        assert_eq!(f.dirs.width, 0);
        assert_eq!(f.preview.width, 0);
    }

    use super::*;

    #[test]
    fn layout_reserves_one_line_each_for_the_command_and_status_lines() {
        let f = layout_with_tabs(Rect::new(0, 0, 100, 30), false, Pane::Notes);
        assert_eq!(f.command.height, 1);
        assert_eq!(f.status.height, 1);
        // The status line is the last row.
        assert_eq!(f.status.y, 29);
        assert_eq!(f.command.y, 28);
    }

    #[test]
    fn the_three_panes_tile_the_body_without_gaps() {
        let f = layout_with_tabs(Rect::new(0, 0, 100, 30), false, Pane::Notes);
        assert_eq!(f.dirs.x, 0);
        assert_eq!(f.notes.x, f.dirs.x + f.dirs.width);
        assert_eq!(f.preview.x, f.notes.x + f.notes.width);
        assert_eq!(f.preview.x + f.preview.width, 100);
        for pane in [f.dirs, f.notes, f.preview] {
            assert_eq!(pane.height, 28);
        }
    }

    /// A short or narrow terminal must still produce a valid layout rather
    /// than panicking on a negative remainder.
    #[test]
    fn a_tiny_terminal_still_lays_out() {
        for (w, h) in [(20, 5), (40, 3), (10, 4), (200, 60)] {
            let f = layout_with_tabs(Rect::new(0, 0, w, h), false, Pane::Notes);
            assert!(f.command.height <= 1);
            assert!(f.dirs.width <= w);
            assert!(f.preview.x + f.preview.width <= w);
        }
    }
}
