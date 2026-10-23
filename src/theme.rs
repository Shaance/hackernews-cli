//! Startup colour detection and immutable semantic styles.

use ratatui::style::{Color, Modifier, Style};
use std::time::Duration;
use terminal_colorsaurus::{QueryOptions, ThemeMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub motion: bool,
    pub brand: Style,
    pub fg: Style,
    pub dim: Style,
    pub scrim: Style,
    pub line: Style,
    pub accent: Style,
    pub sel: Style,
    pub link: Style,
    pub error: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self::ANSI
    }
}

impl Theme {
    pub const TRUE_DARK: Self = Self {
        motion: true,
        brand: Style::new()
            .fg(Color::Rgb(0xff, 0xff, 0xff))
            .bg(Color::Rgb(0xff, 0x66, 0x00))
            .add_modifier(Modifier::BOLD),
        fg: Style::new().fg(Color::Rgb(0xe1, 0xe5, 0xe5)),
        dim: Style::new().fg(Color::Rgb(0xa1, 0xac, 0xb0)),
        scrim: Style::new().fg(Color::Rgb(0x3a, 0x42, 0x45)),
        line: Style::new().fg(Color::Rgb(0x35, 0x3d, 0x40)),
        accent: Style::new().fg(Color::Rgb(0xf3, 0xb7, 0x7b)),
        sel: Style::new().bg(Color::Rgb(0x25, 0x32, 0x2f)),
        link: Style::new().fg(Color::Rgb(0x80, 0xc9, 0xd5)),
        error: Style::new().fg(Color::Rgb(0xee, 0x8a, 0x8a)),
    };
    pub const TRUE_LIGHT: Self = Self {
        motion: true,
        brand: Self::TRUE_DARK.brand,
        fg: Style::new().fg(Color::Rgb(0x24, 0x29, 0x2c)),
        dim: Style::new().fg(Color::Rgb(0x61, 0x6b, 0x70)),
        scrim: Style::new().fg(Color::Rgb(0xcd, 0xd2, 0xd1)),
        line: Style::new().fg(Color::Rgb(0xd4, 0xd9, 0xd8)),
        accent: Style::new().fg(Color::Rgb(0x99, 0x50, 0x15)),
        sel: Style::new().bg(Color::Rgb(0xe9, 0xee, 0xec)),
        link: Style::new().fg(Color::Rgb(0x00, 0x6b, 0x79)),
        error: Style::new().fg(Color::Rgb(0xa3, 0x3a, 0x3a)),
    };
    pub const ANSI: Self = Self {
        motion: false,
        brand: Style::new()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        fg: Style::new().fg(Color::Reset),
        dim: Style::new().add_modifier(Modifier::DIM),
        scrim: Style::new().fg(Color::DarkGray),
        line: Style::new().fg(Color::DarkGray),
        accent: Style::new().fg(Color::Yellow),
        sel: Style::new().bg(Color::DarkGray),
        link: Style::new().fg(Color::Cyan),
        error: Style::new().fg(Color::Red),
    };
    pub const MONO: Self = Self {
        motion: false,
        brand: Style::new().add_modifier(Modifier::REVERSED.union(Modifier::BOLD)),
        fg: Style::new(),
        dim: Style::new().add_modifier(Modifier::DIM),
        scrim: Style::new().add_modifier(Modifier::DIM),
        line: Style::new().add_modifier(Modifier::DIM),
        accent: Style::new().add_modifier(Modifier::BOLD),
        sel: Style::new().add_modifier(Modifier::REVERSED),
        link: Style::new().add_modifier(Modifier::UNDERLINED),
        error: Style::new().add_modifier(Modifier::BOLD),
    };

    /// Call once, before entering raw mode or the alternate screen.
    pub fn detect() -> Self {
        let override_name = std::env::var_os("HN_THEME");
        let overridden = override_name
            .as_deref()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                ["light", "dark", "ansi"]
                    .iter()
                    .any(|known| name.eq_ignore_ascii_case(known))
            });
        let truecolor =
            supports_color::on_cached(supports_color::Stream::Stdout).map(|level| level.has_16m);
        let background = if !overridden && truecolor == Some(true) {
            let mut options = QueryOptions::default();
            options.timeout = Duration::from_millis(250);
            terminal_colorsaurus::theme_mode(options).ok()
        } else {
            None
        };
        Self::resolve(
            override_name.as_deref().and_then(|name| name.to_str()),
            truecolor,
            background,
            std::env::var_os("HN_REDUCED_MOTION").is_some_and(|v| !v.is_empty()),
        )
    }

    // Project colour support to None (unsupported) or Some(has_16m).
    // A failed or skipped background query is None; never assume dark.
    fn resolve(
        override_name: Option<&str>,
        truecolor: Option<bool>,
        background: Option<ThemeMode>,
        reduced_motion: bool,
    ) -> Self {
        let mut theme = match override_name.map(str::to_ascii_lowercase).as_deref() {
            Some("light") => Self::TRUE_LIGHT,
            Some("dark") => Self::TRUE_DARK,
            Some("ansi") => Self::ANSI,
            _ => match (truecolor, background) {
                (None, _) => Self::MONO,
                (Some(true), Some(ThemeMode::Dark)) => Self::TRUE_DARK,
                (Some(true), Some(ThemeMode::Light)) => Self::TRUE_LIGHT,
                _ => Self::ANSI,
            },
        };
        theme.motion &= !reduced_motion;
        theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_resolution_table() {
        use ThemeMode::{Dark, Light};
        let dark = Theme::TRUE_DARK;
        let light = Theme::TRUE_LIGHT;
        let ansi = Theme::ANSI;
        let mono = Theme::MONO;
        let cases = [
            // Overrides win over both colour support and detected background.
            (Some("LiGhT"), None, None, light),
            (Some("DARK"), None, None, dark),
            (Some("AnSi"), None, None, ansi),
            (Some("light"), Some(true), Some(Dark), light),
            (Some("dark"), Some(true), Some(Light), dark),
            (Some("ansi"), Some(true), Some(Dark), ansi),
            // Other values are ignored.
            (Some("unknown"), Some(true), Some(Light), light),
            (Some(""), Some(false), None, ansi),
            (Some("unknown"), None, None, mono),
            // No colour, including NO_COLOR, wins over automatic detection.
            (None, None, None, mono),
            (None, None, Some(Dark), mono),
            (None, Some(true), Some(Dark), dark),
            (None, Some(true), Some(Light), light),
            // Failed/unknown background must never guess dark.
            (None, Some(true), None, ansi),
            // 256-colour support cannot select a truecolour palette.
            (None, Some(false), Some(Dark), ansi),
            (None, Some(false), Some(Light), ansi),
            (None, Some(false), None, ansi),
        ];
        for (override_name, truecolor, background, expected) in cases {
            for reduced_motion in [false, true] {
                let expected = Theme {
                    motion: expected.motion && !reduced_motion,
                    ..expected
                };
                assert_eq!(
                Theme::resolve(override_name, truecolor, background, reduced_motion),
                expected,
                "override={override_name:?}, truecolour={truecolor:?}, background={background:?}, reduced_motion={reduced_motion} (None means unknown)"
            );
            }
        }
    }
}
