//! Color and style types for character cells: [`Color`] (this module) and [`Style`], a `{fg,
//! bg}` pair of two `Color`s with no other relation to anything else in this crate.
//!
//! Split into private submodules by concern, `animate`/`backend`/`testing`-style: `ansi` is the
//! 16-color ANSI palette and the shared indexed/ANSI quantization machinery, `convert` is
//! `Color`'s inherent methods (constants, RGB resolution, `gem` color-space conversions),
//! `named` is `Color`'s string-name/hex constructors (`from_named`, `from_hex`), `palette_oklab`
//! is the generated Oklab table `ansi` quantizes against, `parse` is `Color`'s
//! `Display`/`FromStr`/serde impls, `style` is [`Style`] itself, and `tint` is [`Tint`], sprite
//! colour modulation.

mod ansi;
mod convert;
mod named;
#[cfg(feature = "indexed-quant")]
mod palette_oklab;
mod parse;
mod style;
mod tint;

pub use ansi::{AnsiColor, InvalidAnsiIndex};
pub use parse::ParseColorError;
pub use style::Style;
pub use tint::Tint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
/// Represents a color in the terminal grid.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Color;
///
/// let named = Color::GREEN;
/// let rgb = Color::Rgb { r: 255, g: 0, b: 0 };
/// let indexed = Color::Indexed(42);
/// assert_ne!(named, rgb);
/// assert_ne!(rgb, indexed);
/// ```
pub enum Color {
    #[default]
    /// Backend's default foreground/background color.
    ///
    /// This tells the rendering backend to use the terminal's configured
    /// default colors (e.g., the user's background color preference).
    Default,
    /// One of the 16 standard ANSI colors.
    ///
    /// Use these to respect the user's terminal theme.
    Ansi(AnsiColor),
    /// 256-color palette index.
    Indexed(u8),
    /// 24-bit RGB color.
    ///
    /// Use this for exact color matching regardless of terminal settings.
    Rgb {
        /// Red channel.
        r: u8,
        /// Green channel.
        g: u8,
        /// Blue channel.
        b: u8,
    },
}
