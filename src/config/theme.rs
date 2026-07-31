//! The colours leo draws with, and how a user changes them.
//!
//! Parsing and validation live here, in the config layer, so the view layer
//! receives colours it can trust and a bad hex string is reported once at load
//! rather than swallowed at every call site.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A colour as three channels. Deliberately not `ratatui::style::Color`: the
/// config layer should not depend on the view layer, and true colour is the only
/// form leo stores — an ANSI slot would render differently in every terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `#rrggbb`, `rrggbb`, or `#rgb`.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        match hex.len() {
            6 => Some(Self::new(
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            )),
            // Shorthand: each digit doubled, so #f80 is #ff8800.
            3 => {
                let d = |i: usize| -> Option<u8> {
                    let v = u8::from_str_radix(&hex[i..i + 1], 16).ok()?;
                    Some(v * 17)
                };
                Some(Self::new(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Scale each channel, for deriving a dimmer form of the same hue.
    pub fn scaled(self, factor: f32) -> Self {
        let s = |c: u8| ((c as f32 * factor).round().clamp(0.0, 255.0)) as u8;
        Self::new(s(self.r), s(self.g), s(self.b))
    }

    /// Rough perceived brightness, 0.0–1.0. Used to keep a chosen accent legible.
    pub fn luminance(self) -> f32 {
        (0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32) / 255.0
    }
}

/// Every colour the interface uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Borders, titles, headings, the `:` prompt.
    pub accent: Rgb,
    /// The border of a pane without focus: the same hue, darker.
    pub accent_muted: Rgb,
    /// The status bar's background.
    pub bar: Rgb,
    pub warn: Rgb,
    pub good: Rgb,
    pub bad: Rgb,
}

/// How dark the unfocused border is, relative to the accent.
const MUTED_FACTOR: f32 = 0.64;
/// How dark the status bar is, relative to the accent.
const BAR_FACTOR: f32 = 0.28;

impl Palette {
    /// Derive a whole palette from one accent colour, so `accent = "#..."` is a
    /// complete theme and the rest is optional.
    pub fn from_accent(accent: Rgb) -> Self {
        Self {
            accent,
            accent_muted: accent.scaled(MUTED_FACTOR),
            bar: accent.scaled(BAR_FACTOR),
            warn: Rgb::new(214, 161, 74),
            good: Rgb::new(122, 162, 108),
            bad: Rgb::new(199, 88, 78),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::from_accent(PRESET_DEFAULT)
    }
}

/// leo's own accent.
const PRESET_DEFAULT: Rgb = Rgb::new(217, 119, 87);

/// Named palettes, so a user can change everything with one word.
pub fn presets() -> BTreeMap<&'static str, Rgb> {
    BTreeMap::from([
        ("orange", PRESET_DEFAULT),
        ("blue", Rgb::new(88, 141, 217)),
        ("green", Rgb::new(106, 168, 116)),
        ("purple", Rgb::new(153, 122, 211)),
        ("pink", Rgb::new(212, 108, 152)),
        ("mono", Rgb::new(168, 168, 168)),
    ])
}

/// The `[theme]` table in `config.toml`.
///
/// Every field optional: naming an accent is a complete theme, and naming
/// nothing keeps leo's own.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThemeConfig {
    /// A named palette from [`presets`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_muted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub good: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bad: Option<String>,
}

impl ThemeConfig {
    /// Build a palette, reporting anything unusable rather than failing.
    ///
    /// A typo in a colour should cost that one colour, not the whole interface:
    /// an unreadable config would otherwise leave the user with no way to see the
    /// message telling them about it.
    pub fn palette(&self) -> Palette {
        let mut palette = match &self.preset {
            Some(name) => match presets().get(name.trim().to_lowercase().as_str()) {
                Some(accent) => Palette::from_accent(*accent),
                None => {
                    crate::diag::warn(format!(
                        "theme: no preset named \"{name}\"; known presets are {}",
                        presets().keys().cloned().collect::<Vec<_>>().join(", ")
                    ));
                    Palette::default()
                }
            },
            None => Palette::default(),
        };

        // An explicit accent re-derives the whole palette, so the muted border
        // and the bar follow the hue the user chose rather than staying orange.
        if let Some(text) = &self.accent {
            match Rgb::parse(text) {
                Some(rgb) => palette = Palette::from_accent(rgb),
                None => Self::complain("accent", text),
            }
        }

        for (field, text, slot) in [
            ("accent_muted", &self.accent_muted, 0),
            ("bar", &self.bar, 1),
            ("warn", &self.warn, 2),
            ("good", &self.good, 3),
            ("bad", &self.bad, 4),
        ] {
            let Some(text) = text else { continue };
            match Rgb::parse(text) {
                Some(rgb) => match slot {
                    0 => palette.accent_muted = rgb,
                    1 => palette.bar = rgb,
                    2 => palette.warn = rgb,
                    3 => palette.good = rgb,
                    _ => palette.bad = rgb,
                },
                None => Self::complain(field, text),
            }
        }

        palette
    }

    fn complain(field: &str, value: &str) {
        crate::diag::warn(format!(
            "theme: {field} = \"{value}\" is not a colour; use #rrggbb"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_parsed_in_every_accepted_form() {
        assert_eq!(Rgb::parse("#d97757"), Some(Rgb::new(217, 119, 87)));
        assert_eq!(Rgb::parse("d97757"), Some(Rgb::new(217, 119, 87)));
        assert_eq!(Rgb::parse("  #D97757 "), Some(Rgb::new(217, 119, 87)));
        // Shorthand doubles each digit.
        assert_eq!(Rgb::parse("#f80"), Some(Rgb::new(255, 136, 0)));
        assert_eq!(Rgb::parse("#000"), Some(Rgb::new(0, 0, 0)));
        assert_eq!(Rgb::parse("#fff"), Some(Rgb::new(255, 255, 255)));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed_at() {
        for bad in ["", "#", "xyz", "#gg0011", "#12345", "rebeccapurple", "12345678"] {
            assert_eq!(Rgb::parse(bad), None, "accepted {bad:?}");
        }
    }

    #[test]
    fn hex_round_trips() {
        for text in ["#d97757", "#000000", "#ffffff", "#0a0b0c"] {
            assert_eq!(Rgb::parse(text).unwrap().to_hex(), text);
        }
    }

    /// One accent must be a complete theme, or every user has to pick five
    /// colours to change one.
    #[test]
    fn an_accent_alone_derives_a_whole_palette() {
        let theme = ThemeConfig {
            accent: Some("#588dd9".to_string()),
            ..Default::default()
        };
        let p = theme.palette();
        assert_eq!(p.accent, Rgb::new(88, 141, 217));
        // Muted and bar follow the chosen hue rather than staying orange.
        assert!(p.accent_muted.b > p.accent_muted.r, "muted lost the blue hue");
        assert!(p.bar.b > p.bar.r, "the bar lost the blue hue");
        assert!(p.accent_muted.luminance() < p.accent.luminance());
        assert!(p.bar.luminance() < p.accent_muted.luminance());
    }

    #[test]
    fn a_preset_sets_the_accent() {
        for name in presets().keys() {
            let theme = ThemeConfig {
                preset: Some(name.to_string()),
                ..Default::default()
            };
            assert_eq!(theme.palette().accent, presets()[*name], "preset {name}");
        }
    }

    #[test]
    fn preset_names_are_case_insensitive_and_trimmed() {
        let theme = ThemeConfig {
            preset: Some("  BLUE ".to_string()),
            ..Default::default()
        };
        assert_eq!(theme.palette().accent, presets()["blue"]);
    }

    /// A bad colour must cost that colour, not the interface. An unreadable
    /// screen cannot show the message explaining why it is unreadable.
    #[test]
    fn a_bad_colour_falls_back_instead_of_failing() {
        let theme = ThemeConfig {
            accent: Some("not a colour".to_string()),
            good: Some("#00ff00".to_string()),
            ..Default::default()
        };
        let p = theme.palette();
        assert_eq!(p.accent, Palette::default().accent, "accent was not restored");
        // And the fields that were fine still applied.
        assert_eq!(p.good, Rgb::new(0, 255, 0));
    }

    #[test]
    fn an_unknown_preset_falls_back_to_the_default() {
        let theme = ThemeConfig {
            preset: Some("chartreuse".to_string()),
            ..Default::default()
        };
        assert_eq!(theme.palette(), Palette::default());
    }

    /// An explicit field must win over the preset it sits beside.
    #[test]
    fn an_explicit_colour_overrides_the_preset() {
        let theme = ThemeConfig {
            preset: Some("blue".to_string()),
            bad: Some("#111111".to_string()),
            ..Default::default()
        };
        let p = theme.palette();
        assert_eq!(p.accent, presets()["blue"]);
        assert_eq!(p.bad, Rgb::new(17, 17, 17));
    }

    #[test]
    fn an_empty_theme_table_is_leos_own_palette() {
        assert_eq!(ThemeConfig::default().palette(), Palette::default());
    }

    /// The table has to survive a round trip through TOML, since the profile
    /// page writes it back.
    #[test]
    fn the_theme_table_round_trips_through_toml() {
        let theme = ThemeConfig {
            preset: Some("green".to_string()),
            accent: Some("#6aa874".to_string()),
            ..Default::default()
        };
        let text = toml::to_string(&theme).unwrap();
        assert_eq!(toml::from_str::<ThemeConfig>(&text).unwrap(), theme);
        // Unset fields must not be written, so the file stays minimal.
        assert!(!text.contains("warn"), "{text}");
    }

    #[test]
    fn scaling_darkens_without_wrapping() {
        assert_eq!(Rgb::new(255, 255, 255).scaled(0.5), Rgb::new(128, 128, 128));
        assert_eq!(Rgb::new(10, 10, 10).scaled(0.0), Rgb::new(0, 0, 0));
        // No overflow when a factor is above 1.
        assert_eq!(Rgb::new(200, 200, 200).scaled(4.0), Rgb::new(255, 255, 255));
    }
}
