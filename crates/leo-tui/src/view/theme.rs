//! The palette the views draw with.
//!
//! Set once from config at startup, then read by every view, so changing a colour
//! is a config edit rather than a hunt through twelve files. Parsing lives in
//! [`leo_services::config::theme`]; this is only the handoff to ratatui.

use std::sync::OnceLock;

use ratatui::style::Color;

use leo_services::config::theme::{Palette, Rgb};

static PALETTE: OnceLock<Palette> = OnceLock::new();

/// Install the user's palette. Called once, before the first frame.
///
/// Later calls are ignored rather than panicking: a second call would mean two
/// interfaces in one process, which does not happen, and a panic in a paint path
/// is a poor way to find that out.
pub fn init(palette: Palette) {
    let _ = PALETTE.set(palette);
}

fn palette() -> &'static Palette {
    // Falling back to the default keeps tests and any pre-init paint working.
    PALETTE.get_or_init(Palette::default)
}

fn to_color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}

/// Borders with focus, titles, headings, the `:` prompt.
pub fn accent() -> Color {
    to_color(palette().accent)
}

/// Borders without focus: the accent's hue, darker, so the frame reads as one
/// palette rather than one lit pane and two grey ones.
pub fn accent_muted() -> Color {
    to_color(palette().accent_muted)
}

/// The status bar's background.
pub fn bar() -> Color {
    to_color(palette().bar)
}

/// Attention, but not failure.
pub fn warn() -> Color {
    to_color(palette().warn)
}

/// Confirmed, healthy, done.
pub fn good() -> Color {
    to_color(palette().good)
}

/// Failed.
pub fn bad() -> Color {
    to_color(palette().bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True colour, so nothing inherits the terminal's idea of a named colour.
    #[test]
    fn every_colour_is_true_colour() {
        for color in [accent(), accent_muted(), bar(), warn(), good(), bad()] {
            assert!(matches!(color, Color::Rgb(..)), "{color:?} is not true colour");
        }
    }

    #[test]
    fn the_default_accent_is_orange_and_the_frame_recedes_behind_it() {
        let p = Palette::default();
        assert!(p.accent.r > p.accent.g && p.accent.g > p.accent.b, "not orange");
        // Focused border brightest, unfocused dimmer, bar dimmest.
        assert!(p.accent.luminance() > p.accent_muted.luminance());
        assert!(p.accent_muted.luminance() > p.bar.luminance());
    }

    #[test]
    fn status_colours_stay_distinct_from_the_accent() {
        let p = Palette::default();
        assert_ne!(p.good, p.accent);
        assert_ne!(p.bad, p.accent);
        assert_ne!(p.warn, p.accent);
    }
}
