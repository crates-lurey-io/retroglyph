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
