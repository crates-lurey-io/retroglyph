//! [`Theme`]: named color roles for a light/dark-aware app.

use retroglyph_core::{Color, Style};

use crate::Response;

/// A palette of named color roles, rather than a CSS-style cascade: draw
/// code picks the role it means (`theme.accent`, `theme.border`) and the
/// active [`Theme`] decides what color that resolves to.
///
/// This crate has no opinion on *how* an app picks between
/// [`DARK`](Self::DARK) and [`LIGHT`](Self::LIGHT) (a manual toggle key, a
/// [`SystemTheme`](retroglyph_core::SystemTheme) from
/// [`Event::ThemeChanged`](retroglyph_core::Event::ThemeChanged), or just
/// always the same one): it only owns the two palettes themselves, so an
/// app doesn't have to invent one from scratch.
///
/// # Examples
///
/// ```
/// use retroglyph_widgets::Theme;
///
/// let theme = Theme::DARK;
/// assert_eq!(theme.fg, Theme::DARK.fg);
/// assert_ne!(theme.bg, Theme::LIGHT.bg);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Contrast is higher than a typical OS light theme:
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

    /// The background for an interactive widget in `response`'s current state, over `base`
    /// when idle. Precedence: [`pressed`](Response::pressed), then
    /// [`hovered`](Response::hovered), then `base`.
    ///
    /// `base` is caller-supplied rather than defaulted to [`panel_bg`](Self::panel_bg) so this
    /// composes with widgets that already take a backdrop, e.g. a bar drawn over
    /// [`title_bg`](Self::title_bg) instead of a panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_widgets::Theme;
    /// use retroglyph_widgets::Interaction;
    /// use retroglyph_core::{Pos, Rect};
    ///
    /// let theme = Theme::DARK;
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.panel_bg);
    /// ```
    #[must_use]
    pub const fn bg_for(&self, response: &Response, base: Color) -> Color {
        if response.pressed() {
            self.press_bg
        } else if response.hovered() {
            self.hover_bg
        } else {
            base
        }
    }

    /// The foreground for an interactive widget in `response`'s current state. Precedence:
    /// [`pressed`](Response::pressed) or [`focused`](Response::focused), then
    /// [`hovered`](Response::hovered), then [`dim`](Self::dim).
    ///
    /// Idle interactive text reads as [`dim`](Self::dim) rather than [`fg`](Self::fg): an
    /// interactive widget that looks identical to static text at rest gives no visual hint
    /// that it's interactive at all, so the state that actually needs [`fg`](Self::fg)'s full
    /// contrast is hover, not idle.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_widgets::Theme;
    /// use retroglyph_widgets::Interaction;
    /// use retroglyph_core::{Pos, Rect};
    ///
    /// let theme = Theme::DARK;
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// assert_eq!(theme.fg_for(&response), theme.dim);
    /// ```
    #[must_use]
    pub const fn fg_for(&self, response: &Response) -> Color {
        if response.pressed() || response.focused() {
            self.accent
        } else if response.hovered() {
            self.fg
        } else {
            self.dim
        }
    }

    /// The resolved [`Style`] for an interactive widget in `response`'s current state, over
    /// `base` when idle. Precedence: [`disabled`](Response::disabled), then
    /// [`pressed`](Response::pressed), then [`focused`](Response::focused), then
    /// [`hovered`](Response::hovered), then idle.
    ///
    /// Unlike [`bg_for`](Self::bg_for)/[`fg_for`](Self::fg_for), which resolve each channel
    /// independently, `style_for` resolves both at once against a single, shared precedence
    /// order: that's the only place `disabled` can cleanly take priority over everything else,
    /// and the only place a press (`accent` on `press_bg`) and a focus ring (`accent` on `base`,
    /// no background change) can be told apart instead of both collapsing into the same `accent`
    /// foreground.
    ///
    /// `base` is caller-supplied for the same reason as [`bg_for`](Self::bg_for): so this
    /// composes with widgets that already have their own backdrop.
    ///
    /// This uses [`dim`](Self::dim) for the disabled foreground, same as idle; retroglyph has
    /// no separate disabled color role yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_widgets::Theme;
    /// use retroglyph_widgets::Interaction;
    /// use retroglyph_core::{Pos, Rect};
    ///
    /// let theme = Theme::DARK;
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// let style = theme.style_for(&response, theme.panel_bg);
    /// assert_eq!(style.foreground(), theme.dim);
    /// assert_eq!(style.background(), theme.panel_bg);
    /// ```
    #[must_use]
    pub fn style_for(&self, response: &Response, base: Color) -> Style {
        if response.disabled() {
            return Style::new().fg(self.dim).bg(base);
        }
        if response.pressed() {
            return Style::new().fg(self.accent).bg(self.press_bg);
        }
        if response.focused() {
            return Style::new().fg(self.accent).bg(base);
        }
        if response.hovered() {
            return Style::new().fg(self.fg).bg(self.hover_bg);
        }
        Style::new().fg(self.dim).bg(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_are_distinct() {
        assert_ne!(Theme::DARK, Theme::LIGHT);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serializes_and_deserializes() {
        let json = serde_json::to_string(&Theme::DARK).expect("serialize");
        let round_tripped: Theme = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_tripped, Theme::DARK);
    }

    #[test]
    fn bg_for_is_base_when_idle() {
        let theme = Theme::DARK;
        let response = Response::default();
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.panel_bg);
    }

    #[test]
    fn bg_for_is_hover_bg_when_hovered() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            ..Response::default()
        };
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.hover_bg);
    }

    #[test]
    fn bg_for_prefers_press_bg_over_hover_bg() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.press_bg);
    }

    #[test]
    fn fg_for_is_dim_when_idle() {
        let theme = Theme::DARK;
        let response = Response::default();
        assert_eq!(theme.fg_for(&response), theme.dim);
    }

    #[test]
    fn fg_for_is_fg_when_hovered() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.fg);
    }

    #[test]
    fn fg_for_prefers_accent_over_hover_when_focused() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            focused: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.accent);
    }

    #[test]
    fn fg_for_prefers_accent_over_hover_when_pressed() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.accent);
    }

    #[test]
    fn style_for_is_dim_on_base_when_idle() {
        let theme = Theme::DARK;
        let response = Response::default();
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.dim);
        assert_eq!(style.background(), theme.panel_bg);
    }

    #[test]
    fn style_for_is_fg_on_hover_bg_when_hovered() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.fg);
        assert_eq!(style.background(), theme.hover_bg);
    }

    #[test]
    fn style_for_is_accent_on_base_when_focused_and_not_hovered() {
        let theme = Theme::DARK;
        let response = Response {
            focused: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.accent);
        assert_eq!(style.background(), theme.panel_bg);
    }

    #[test]
    fn style_for_prefers_focused_over_hovered() {
        let theme = Theme::DARK;
        let response = Response {
            hovered: true,
            focused: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.accent);
        assert_eq!(style.background(), theme.panel_bg);
    }

    #[test]
    fn style_for_is_accent_on_press_bg_when_pressed() {
        let theme = Theme::DARK;
        let response = Response {
            pressed: true,
            focused: true,
            hovered: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.accent);
        assert_eq!(style.background(), theme.press_bg);
    }

    #[test]
    fn style_for_prefers_disabled_over_everything_else() {
        let theme = Theme::DARK;
        let response = Response {
            disabled: true,
            pressed: true,
            focused: true,
            hovered: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.dim);
        assert_eq!(style.background(), theme.panel_bg);
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
