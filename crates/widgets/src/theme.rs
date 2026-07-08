//! [`Theme`]: named color roles for a light/dark-aware app.

use retroglyph_core::Color;

/// A palette of named color roles, rather than a CSS-style cascade -- draw
/// code picks the role it means (`theme.accent`, `theme.border`) and the
/// active [`Theme`] decides what color that resolves to.
///
/// This crate has no opinion on *how* an app picks between
/// [`DARK`](Self::DARK) and [`LIGHT`](Self::LIGHT) (a manual toggle key, a
/// [`SystemTheme`](retroglyph_core::SystemTheme) from
/// [`Event::ThemeChanged`](retroglyph_core::Event::ThemeChanged), or just
/// always the same one) -- it only owns the two palettes themselves, so an
/// app doesn't have to invent one from scratch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The window/screen background, behind every panel.
    pub bg: Color,
    /// A panel's own background, layered over `bg`.
    pub panel_bg: Color,
    /// Panel borders and dividers.
    pub border: Color,
    /// A panel title bar's background.
    pub title_bg: Color,
    /// Default (non-emphasized) text.
    pub fg: Color,
    /// Emphasis: selection, focus rings, primary actions.
    pub accent: Color,
    /// An interactive widget's background while hovered.
    pub hover_bg: Color,
    /// An interactive widget's background while pressed.
    pub press_bg: Color,
    /// De-emphasized text (hints, secondary labels, disabled-looking text).
    pub dim: Color,
}

impl Theme {
    /// A dark palette: light text on a near-black background.
    pub const DARK: Self = Self {
        bg: Color::Rgb {
            r: 16,
            g: 16,
            b: 24,
        },
        panel_bg: Color::Rgb {
            r: 22,
            g: 22,
            b: 32,
        },
        border: Color::Rgb {
            r: 70,
            g: 74,
            b: 96,
        },
        title_bg: Color::Rgb {
            r: 30,
            g: 32,
            b: 48,
        },
        fg: Color::Rgb {
            r: 190,
            g: 192,
            b: 208,
        },
        accent: Color::Rgb {
            r: 90,
            g: 170,
            b: 250,
        },
        hover_bg: Color::Rgb {
            r: 40,
            g: 44,
            b: 64,
        },
        press_bg: Color::Rgb {
            r: 60,
            g: 110,
            b: 170,
        },
        dim: Color::Rgb {
            r: 110,
            g: 112,
            b: 130,
        },
    };

    /// A light palette: dark text on a near-white background. Same role
    /// relationships as [`DARK`](Self::DARK) (accent stays a legible blue,
    /// `hover_bg`/`press_bg` stay a step apart from `panel_bg`), inverted
    /// for contrast against a light background rather than just flipping
    /// each channel.
    ///
    /// Contrast is deliberately higher than a typical OS light theme:
    /// retroglyph's pseudo-graphics (gauges, progress bars, log lines)
    /// draw with a 2-color paletted look where every panel-bg/border/text
    /// pair has to be distinct at a glance with no sub-pixel anti-aliasing
    /// to soften the edges.
    pub const LIGHT: Self = Self {
        bg: Color::Rgb {
            r: 240,
            g: 240,
            b: 246,
        },
        panel_bg: Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        border: Color::Rgb {
            r: 160,
            g: 164,
            b: 180,
        },
        title_bg: Color::Rgb {
            r: 224,
            g: 226,
            b: 240,
        },
        fg: Color::Rgb {
            r: 20,
            g: 22,
            b: 32,
        },
        accent: Color::Rgb {
            r: 20,
            g: 100,
            b: 210,
        },
        hover_bg: Color::Rgb {
            r: 230,
            g: 236,
            b: 248,
        },
        press_bg: Color::Rgb {
            r: 160,
            g: 194,
            b: 240,
        },
        dim: Color::Rgb {
            r: 130,
            g: 132,
            b: 150,
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_are_distinct() {
        assert_ne!(Theme::DARK, Theme::LIGHT);
    }

    #[test]
    fn dark_background_is_darker_than_light_background() {
        let Color::Rgb {
            r: dr,
            g: dg,
            b: db,
        } = Theme::DARK.bg
        else {
            unreachable!()
        };
        let Color::Rgb {
            r: lr,
            g: lg,
            b: lb,
        } = Theme::LIGHT.bg
        else {
            unreachable!()
        };
        let dark_luma = u32::from(dr) + u32::from(dg) + u32::from(db);
        let light_luma = u32::from(lr) + u32::from(lg) + u32::from(lb);
        assert!(dark_luma < light_luma);
    }
}
