//! Select Graphic Rendition (SGR) ANSI encoding, shared by every text-terminal encoder in this
//! workspace.
//!
//! [`Headless::format_styled`](crate::backend::Headless::format_styled) (styled snapshots) and
//! `retroglyph-recorder`'s `write_cast` (asciicast export) both need "the SGR codes for this
//! [`Style`]," and neither should carry its own copy.
//!
//! Codes follow ECMA-48 5th ed. section 8.3.117: 30-37/40-47 for the standard foreground/
//! background colors, 90-97/100-107 for the bright variants, `38;5;n`/`48;5;n` for 256-color
//! indices, and `38;2;r;g;b`/`48;2;r;g;b` for 24-bit truecolor.
//! (<https://www.ecma-international.org/publications-and-standards/standards/ecma-48/>)

use super::{Color, Style};
use alloc::string::String;
use core::fmt::Write as _;

/// Appends the SGR codes for `style`'s non-default foreground/background to `out`, as a
/// single `\x1b[...m` sequence.
///
/// A `Color::Default` channel is left unset, relying on the caller's preceding `\x1b[0m`
/// reset rather than emitting an explicit `39`/`49` reset code. Emits nothing at all when
/// both channels are `Color::Default`.
pub fn push_sgr(out: &mut String, style: Style) {
    let mut params = String::new();
    if let Some(code) = sgr_color(style.foreground(), false) {
        let _ = write!(params, "{code}");
    }
    if let Some(code) = sgr_color(style.background(), true) {
        if !params.is_empty() {
            params.push(';');
        }
        let _ = write!(params, "{code}");
    }
    if !params.is_empty() {
        let _ = write!(out, "\x1b[{params}m");
    }
}

/// The SGR parameter string for `color` in the foreground (`bg: false`) or background
/// (`bg: true`) slot, or `None` for `Color::Default` (nothing to emit).
///
/// Codes follow ECMA-48 SGR (see this module's file-level docs): 30/40 base for standard
/// colors, 90/100 for bright, offset by the color index within its group of 8.
#[must_use]
pub fn sgr_color(color: Color, bg: bool) -> Option<String> {
    match color {
        Color::Default => None,
        Color::Ansi(ansi) => {
            let index = ansi.to_index();
            let base = match (index < 8, bg) {
                (true, false) => 30,
                (true, true) => 40,
                (false, false) => 90,
                (false, true) => 100,
            };
            Some(alloc::format!("{}", base + index % 8))
        }
        Color::Indexed(index) => Some(alloc::format!("{};5;{index}", if bg { 48 } else { 38 })),
        Color::Rgb { r, g, b } => {
            Some(alloc::format!("{};2;{r};{g};{b}", if bg { 48 } else { 38 }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::AnsiColor;
    use alloc::borrow::ToOwned as _;

    #[test]
    fn sgr_color_is_none_for_default() {
        assert_eq!(sgr_color(Color::Default, false), None);
        assert_eq!(sgr_color(Color::Default, true), None);
    }

    #[test]
    fn sgr_color_standard_ansi_uses_30_40_base() {
        assert_eq!(
            sgr_color(Color::Ansi(AnsiColor::Red), false),
            Some("31".to_owned())
        );
        assert_eq!(
            sgr_color(Color::Ansi(AnsiColor::Red), true),
            Some("41".to_owned())
        );
    }

    #[test]
    fn sgr_color_bright_ansi_uses_90_100_base() {
        assert_eq!(
            sgr_color(Color::Ansi(AnsiColor::BrightRed), false),
            Some("91".to_owned())
        );
        assert_eq!(
            sgr_color(Color::Ansi(AnsiColor::BrightRed), true),
            Some("101".to_owned())
        );
    }

    #[test]
    fn sgr_color_indexed_and_rgb() {
        assert_eq!(
            sgr_color(Color::Indexed(200), false),
            Some("38;5;200".to_owned())
        );
        assert_eq!(
            sgr_color(Color::Indexed(200), true),
            Some("48;5;200".to_owned())
        );
        assert_eq!(
            sgr_color(Color::rgb(1, 2, 3), false),
            Some("38;2;1;2;3".to_owned())
        );
        assert_eq!(
            sgr_color(Color::rgb(1, 2, 3), true),
            Some("48;2;1;2;3".to_owned())
        );
    }

    #[test]
    fn push_sgr_emits_nothing_for_a_fully_default_style() {
        let mut out = String::new();
        push_sgr(&mut out, Style::default());
        assert_eq!(out, "");
    }

    #[test]
    fn push_sgr_combines_fg_and_bg_into_one_escape() {
        let mut out = String::new();
        push_sgr(
            &mut out,
            Style::new()
                .fg(Color::Ansi(AnsiColor::Red))
                .bg(Color::Ansi(AnsiColor::Blue)),
        );
        assert_eq!(out, "\x1b[31;44m");
    }

    #[test]
    fn push_sgr_emits_only_the_non_default_channel() {
        let mut out = String::new();
        push_sgr(&mut out, Style::new().bg(Color::Ansi(AnsiColor::Blue)));
        assert_eq!(out, "\x1b[44m");
    }
}
