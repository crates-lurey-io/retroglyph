//! `Color`'s string form: `Display`/`FromStr`, and (with the `serde` feature) the `Serialize`
//! round trip built on the same string form.

use alloc::string::String;

use super::Color;
use super::ansi::AnsiColor;

/// The name [`Color::fmt`](crate::color::Color::fmt) writes for an ANSI color, e.g. `"bright-red"`.
const fn ansi_name(color: AnsiColor) -> &'static str {
    match color {
        AnsiColor::Black => "black",
        AnsiColor::Red => "red",
        AnsiColor::Green => "green",
        AnsiColor::Yellow => "yellow",
        AnsiColor::Blue => "blue",
        AnsiColor::Magenta => "magenta",
        AnsiColor::Cyan => "cyan",
        AnsiColor::White => "white",
        AnsiColor::BrightBlack => "bright-black",
        AnsiColor::BrightRed => "bright-red",
        AnsiColor::BrightGreen => "bright-green",
        AnsiColor::BrightYellow => "bright-yellow",
        AnsiColor::BrightBlue => "bright-blue",
        AnsiColor::BrightMagenta => "bright-magenta",
        AnsiColor::BrightCyan => "bright-cyan",
        AnsiColor::BrightWhite => "bright-white",
    }
}

/// The inverse of [`ansi_name`]: matches a name with separators already stripped and lowercased
/// (e.g. `"brightred"`), as produced by [`Color::from_str`](crate::color::Color::from_str)'s normalization.
fn parse_ansi_name(name: &str) -> Option<AnsiColor> {
    Some(match name {
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Red,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Yellow,
        "blue" => AnsiColor::Blue,
        "magenta" => AnsiColor::Magenta,
        "cyan" => AnsiColor::Cyan,
        "white" => AnsiColor::White,
        "brightblack" => AnsiColor::BrightBlack,
        "brightred" => AnsiColor::BrightRed,
        "brightgreen" => AnsiColor::BrightGreen,
        "brightyellow" => AnsiColor::BrightYellow,
        "brightblue" => AnsiColor::BrightBlue,
        "brightmagenta" => AnsiColor::BrightMagenta,
        "brightcyan" => AnsiColor::BrightCyan,
        "brightwhite" => AnsiColor::BrightWhite,
        _ => return None,
    })
}

/// Parses `#rgb` or `#rrggbb` (case-insensitive) into an `(r, g, b)` triple.
///
/// Self-contained rather than routed through [`Color::from_hex`], which parses via
/// [`gem::space::Srgb`] and so round-trips each channel through `f32`. Parsing the digits
/// directly keeps [`Color`]'s [`FromStr`](core::str::FromStr) impl (and the `serde` feature built
/// on it) exact for every input.
fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let r = u8::try_from(chars.next()?.to_digit(16)?).ok()?;
            let g = u8::try_from(chars.next()?.to_digit(16)?).ok()?;
            let b = u8::try_from(chars.next()?.to_digit(16)?).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
            let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
            let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

impl core::fmt::Display for Color {
    /// Writes the string form [`FromStr`](core::str::FromStr) parses back and, with the `serde`
    /// feature, [`Serialize`](serde::Serialize) writes: `"default"`, an ANSI name like
    /// `"bright-red"`, a palette index like `"42"`, or `#rrggbb` hex.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::{AnsiColor, Color};
    ///
    /// assert_eq!(Color::Default.to_string(), "default");
    /// assert_eq!(Color::Ansi(AnsiColor::BrightRed).to_string(), "bright-red");
    /// assert_eq!(Color::Indexed(42).to_string(), "42");
    /// assert_eq!(Color::Rgb { r: 255, g: 128, b: 0 }.to_string(), "#ff8000");
    /// ```
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Default => write!(f, "default"),
            Self::Ansi(color) => write!(f, "{}", ansi_name(color)),
            Self::Indexed(i) => write!(f, "{i}"),
            Self::Rgb { r, g, b } => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// Error returned when parsing a [`Color`](crate::color::Color) from a string fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParseColorError;

impl core::fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid color string")
    }
}

impl core::error::Error for ParseColorError {}

impl core::str::FromStr for Color {
    type Err = ParseColorError;

    /// Parses the string form written by [`Display`](core::fmt::Display).
    ///
    /// Hyphens, underscores, and spaces are ignored and matching is case-insensitive, so
    /// `"BrightRed"`, `"bright red"`, and `"bright-red"` all parse the same as the canonical
    /// `"bright-red"` that [`Display`](core::fmt::Display) writes. `"reset"` is accepted as a
    /// synonym for `"default"`. Accepts `#rgb` in addition to the `#rrggbb` [`Display`](core::fmt::Display) writes.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::{AnsiColor, Color};
    ///
    /// assert_eq!("default".parse(), Ok(Color::Default));
    /// assert_eq!("reset".parse(), Ok(Color::Default));
    /// assert_eq!("bright-red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
    /// assert_eq!("Bright Red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
    /// assert_eq!("42".parse(), Ok(Color::Indexed(42)));
    /// assert_eq!("#ff8000".parse(), Ok(Color::Rgb { r: 255, g: 128, b: 0 }));
    /// assert_eq!("#f80".parse(), Ok(Color::Rgb { r: 255, g: 136, b: 0 }));
    /// assert!("not-a-color".parse::<Color>().is_err());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let normalized: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .map(|c| c.to_ascii_lowercase())
            .collect();

        if normalized == "default" || normalized == "reset" {
            return Ok(Self::Default);
        }
        if let Some(color) = parse_ansi_name(&normalized) {
            return Ok(Self::Ansi(color));
        }
        if normalized.bytes().all(|b| b.is_ascii_digit())
            && !normalized.is_empty()
            && let Ok(index) = normalized.parse::<u8>()
        {
            return Ok(Self::Indexed(index));
        }
        if let Some((r, g, b)) = parse_hex(trimmed) {
            return Ok(Self::Rgb { r, g, b });
        }
        Err(ParseColorError)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Color {
    /// Serializes through the [`Display`](core::fmt::Display) round trip, e.g. `"bright-red"` or
    /// `"#ff8000"`, rather than deriving a structural form: a hand-edited TOML/JSON theme
    /// file stays legible and isn't coupled to this (`#[non_exhaustive]`) enum's variant shape.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use alloc::string::ToString as _;

        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Color {
    /// Deserializes through [`FromStr`](core::str::FromStr), so every string
    /// [`Display`](core::fmt::Display) can produce, plus every alias [`FromStr`](core::str::FromStr)
    /// additionally accepts (e.g. `"BrightRed"`, `"#f80"`), round-trips.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ColorVisitor;

        impl serde::de::Visitor<'_> for ColorVisitor {
            type Value = Color;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(
                    "a color string: \"default\", an ANSI name, a palette index, or #rrggbb hex",
                )
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(ColorVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        use alloc::string::ToString as _;

        assert_eq!(Color::Default.to_string(), "default");
        assert_eq!(Color::Ansi(AnsiColor::Black).to_string(), "black");
        assert_eq!(
            Color::Ansi(AnsiColor::BrightYellow).to_string(),
            "bright-yellow"
        );
        assert_eq!(Color::Indexed(7).to_string(), "7");
        assert_eq!(
            Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            }
            .to_string(),
            "#ff8000"
        );
    }

    #[test]
    fn test_from_str_round_trips_display() {
        use alloc::string::ToString as _;

        let colors = [
            Color::Default,
            Color::Ansi(AnsiColor::Black),
            Color::Ansi(AnsiColor::BrightMagenta),
            Color::Indexed(200),
            Color::Rgb {
                r: 18,
                g: 52,
                b: 86,
            },
        ];
        for color in colors {
            let s = color.to_string();
            assert_eq!(s.parse(), Ok(color), "round trip of {s:?}");
        }
    }

    #[test]
    fn test_from_str_aliases_and_separators() {
        assert_eq!("reset".parse(), Ok(Color::Default));
        assert_eq!("Default".parse(), Ok(Color::Default));
        assert_eq!("BrightRed".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!("bright red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!("bright_red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!(
            "#F80".parse(),
            Ok(Color::Rgb {
                r: 255,
                g: 136,
                b: 0
            })
        );
        assert_eq!(
            " #ff8000 ".parse(),
            Ok(Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            })
        );
    }

    #[test]
    fn test_from_str_invalid() {
        assert!("not-a-color".parse::<Color>().is_err());
        assert!("#gg0000".parse::<Color>().is_err());
        assert!("#12345".parse::<Color>().is_err());
        assert!("256".parse::<Color>().is_err());
    }

    #[test]
    fn from_str_rejects_plus_signed_index_and_hex() {
        // `-` is a documented separator (stripped like `_`/` `), so `"-5"` legitimately
        // normalizes to `"5"`; only `+`, which isn't a separator, must be rejected.
        assert!("+5".parse::<Color>().is_err());
        assert!("#+f0000".parse::<Color>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialize_then_deserialize() -> Result<(), serde_json::Error> {
        let colors = [
            Color::Default,
            Color::Ansi(AnsiColor::BrightGreen),
            Color::Indexed(42),
            Color::Rgb {
                r: 255,
                g: 0,
                b: 255,
            },
        ];
        for color in colors {
            let json = serde_json::to_string(&color)?;
            assert_eq!(serde_json::from_str::<Color>(&json)?, color);
        }

        assert_eq!(
            serde_json::to_string(&Color::Rgb {
                r: 255,
                g: 0,
                b: 255
            })?,
            r##""#ff00ff""##
        );
        assert_eq!(
            serde_json::from_str::<Color>(r#""bright-white""#)?,
            Color::Ansi(AnsiColor::BrightWhite)
        );
        assert!(serde_json::from_str::<Color>(r#""not-a-color""#).is_err());

        Ok(())
    }
}
