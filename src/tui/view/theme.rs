//! The palette.
//!
//! One place for the accent color so panes, overlays and the command line agree,
//! and so changing it is a one-line edit rather than a hunt through every view.

use ratatui::style::Color;

/// The accent: borders, titles, section headings, the `:` prompt.
///
/// A true-color orange rather than an ANSI slot, so it renders the same in every
/// terminal instead of picking up whatever the user's scheme assigns to cyan or
/// yellow.
pub const ACCENT: Color = Color::Rgb(217, 119, 87);

/// A dimmer accent, for the border of a pane that does not have focus.
pub const ACCENT_MUTED: Color = Color::Rgb(140, 82, 63);

/// Used where something needs attention but is not an error.
pub const WARN: Color = Color::Rgb(214, 161, 74);

/// Confirmed, healthy, done.
pub const GOOD: Color = Color::Rgb(122, 162, 108);

/// Failed.
pub const BAD: Color = Color::Rgb(199, 88, 78);

#[cfg(test)]
mod tests {
    use super::*;

    /// True color, so the accent does not inherit the terminal's idea of a
    /// named color.
    #[test]
    fn every_color_is_specified_as_rgb() {
        for color in [ACCENT, ACCENT_MUTED, WARN, GOOD, BAD] {
            assert!(
                matches!(color, Color::Rgb(..)),
                "{color:?} is not a true-color value"
            );
        }
    }

    #[test]
    fn the_accent_is_orange_and_its_muted_form_is_darker() {
        let (Color::Rgb(r, g, b), Color::Rgb(mr, mg, mb)) = (ACCENT, ACCENT_MUTED) else {
            panic!("expected rgb colors");
        };
        // Red dominant, green above blue: an orange rather than a pink or brown.
        assert!(r > g && g > b, "not orange: {r},{g},{b}");
        // The muted form is the same hue, darker.
        assert!(mr < r && mg < g && mb < b, "muted is not darker");
        assert!(mr > mg && mg > mb, "muted lost the hue");
    }

    #[test]
    fn the_status_colors_stay_distinguishable_from_the_accent() {
        assert_ne!(GOOD, ACCENT);
        assert_ne!(BAD, ACCENT);
        assert_ne!(WARN, ACCENT);
    }
}
