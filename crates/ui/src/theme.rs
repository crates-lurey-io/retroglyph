//! [`Theme`]: named color roles for a light/dark-aware app.

use retroglyph_core::color::{Color, Style};

use crate::interact::Response;

/// A palette of named color roles, rather than a CSS-style cascade: draw
/// code picks the role it means (`theme.accent`, `theme.border`) and the
/// active [`Theme`] decides what color that resolves to.
///
/// This crate has no opinion on *how* an app picks between
/// [`DARK`](Self::DARK) and [`LIGHT`](Self::LIGHT) (a manual toggle key, a
/// [`SystemTheme`](retroglyph_core::event::SystemTheme) from
/// [`Event::ThemeChanged`](retroglyph_core::event::Event::ThemeChanged), or just
/// always the same one): it only owns the two palettes themselves, so an
/// app doesn't have to invent one from scratch.
///
/// # Examples
///
/// ```
/// use retroglyph_ui::theme::Theme;
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
    /// De-emphasized text (hints, secondary labels).
    pub dim: Color,
    /// Foreground for a disabled interactive widget. Distinct from [`dim`](Self::dim): `dim`
    /// marks idle-but-usable text as secondary, while `disabled` marks a widget the user
    /// cannot interact with at all, and the two need to read differently at a glance.
    pub disabled: Color,
    /// Destructive/error emphasis: delete actions, validation failures, critical alerts.
    pub danger: Color,
    /// Positive/confirmation emphasis: completed states, successful actions.
    pub success: Color,
}

impl Theme {
    /// A dark palette: light text on a near-black background.
    pub const DARK: Self = Self {
        bg: Color::rgb(16, 16, 24),
        panel_bg: Color::rgb(22, 22, 32),
        border: Color::rgb(70, 74, 96),
        title_bg: Color::rgb(30, 32, 48),
        fg: Color::rgb(190, 192, 208),
        accent: Color::rgb(90, 170, 250),
        hover_bg: Color::rgb(40, 44, 64),
        press_bg: Color::rgb(60, 110, 170),
        dim: Color::rgb(110, 112, 130),
        disabled: Color::rgb(80, 82, 96),
        danger: Color::rgb(220, 90, 90),
        success: Color::rgb(80, 200, 120),
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
        bg: Color::rgb(240, 240, 246),
        panel_bg: Color::rgb(255, 255, 255),
        border: Color::rgb(160, 164, 180),
        title_bg: Color::rgb(224, 226, 240),
        fg: Color::rgb(20, 22, 32),
        accent: Color::rgb(20, 100, 210),
        hover_bg: Color::rgb(230, 236, 248),
        press_bg: Color::rgb(160, 194, 240),
        dim: Color::rgb(130, 132, 150),
        disabled: Color::rgb(190, 192, 202),
        danger: Color::rgb(200, 60, 60),
        success: Color::rgb(30, 140, 75),
    };

    /// The background for an interactive widget in `response`'s current state, over `base`
    /// when idle. Delegates to [`WidgetState::of`] for the precedence order; see there for why
    /// `disabled` now takes priority over everything else.
    ///
    /// `base` is caller-supplied rather than defaulted to [`panel_bg`](Self::panel_bg) so this
    /// composes with widgets that already take a backdrop, e.g. a bar drawn over
    /// [`title_bg`](Self::title_bg) instead of a panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_ui::theme::Theme;
    /// use retroglyph_ui::interact::Interaction;
    /// use retroglyph_core::grid::{Pos, Rect};
    ///
    /// let theme = Theme::DARK;
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.panel_bg);
    /// ```
    #[must_use]
    pub const fn bg_for<Id>(&self, response: &Response<Id>, base: Color) -> Color {
        match WidgetState::of(response) {
            WidgetState::Pressed => self.press_bg,
            WidgetState::Hovered => self.hover_bg,
            WidgetState::Disabled | WidgetState::Focused | WidgetState::Idle => base,
        }
    }

    /// The foreground for an interactive widget in `response`'s current state. Delegates to
    /// [`WidgetState::of`] for the precedence order.
    ///
    /// Idle interactive text reads as [`dim`](Self::dim) rather than [`fg`](Self::fg): an
    /// interactive widget that looks identical to static text at rest gives no visual hint
    /// that it's interactive at all, so the state that actually needs [`fg`](Self::fg)'s full
    /// contrast is hover, not idle.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_ui::theme::Theme;
    /// use retroglyph_ui::interact::Interaction;
    /// use retroglyph_core::grid::{Pos, Rect};
    ///
    /// let theme = Theme::DARK;
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// assert_eq!(theme.fg_for(&response), theme.dim);
    /// ```
    #[must_use]
    pub const fn fg_for<Id>(&self, response: &Response<Id>) -> Color {
        match WidgetState::of(response) {
            WidgetState::Disabled => self.disabled,
            WidgetState::Pressed | WidgetState::Focused => self.accent,
            WidgetState::Hovered => self.fg,
            WidgetState::Idle => self.dim,
        }
    }

    /// The resolved [`Style`] for an interactive widget in `response`'s current state, over
    /// `base` when idle. Delegates to [`WidgetState::of`] for the precedence order.
    ///
    /// Resolves both channels at once against a single, shared precedence order, rather than
    /// composing [`bg_for`](Self::bg_for) and [`fg_for`](Self::fg_for) independently: that's the
    /// only place a press (`accent` on `press_bg`) and a focus ring (`accent` on `base`, no
    /// background change) can be told apart instead of both collapsing into the same `accent`
    /// foreground.
    ///
    /// `base` is caller-supplied for the same reason as [`bg_for`](Self::bg_for): so this
    /// composes with widgets that already have their own backdrop.
    ///
    /// This uses [`disabled`](Self::disabled) for the disabled foreground, distinct from
    /// [`dim`](Self::dim)'s idle look.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_ui::theme::Theme;
    /// use retroglyph_ui::interact::Interaction;
    /// use retroglyph_core::grid::{Pos, Rect};
    ///
    /// let theme = Theme::DARK;
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// let style = theme.style_for(&response, theme.panel_bg);
    /// assert_eq!(style.foreground(), theme.dim);
    /// assert_eq!(style.background(), theme.panel_bg);
    /// ```
    #[must_use]
    pub fn style_for<Id>(&self, response: &Response<Id>, base: Color) -> Style {
        match WidgetState::of(response) {
            WidgetState::Disabled => Style::new().fg(self.disabled).bg(base),
            WidgetState::Pressed => Style::new().fg(self.accent).bg(self.press_bg),
            WidgetState::Focused => Style::new().fg(self.accent).bg(base),
            WidgetState::Hovered => Style::new().fg(self.fg).bg(self.hover_bg),
            WidgetState::Idle => Style::new().fg(self.dim).bg(base),
        }
    }
}

/// The single resolved state of an interactive widget, derived from a [`Response`]'s
/// independent flags into one precedence order.
///
/// Precedence: [`Disabled`](Self::Disabled), then [`Pressed`](Self::Pressed), then
/// [`Focused`](Self::Focused), then [`Hovered`](Self::Hovered), then [`Idle`](Self::Idle).
///
/// [`Theme`]'s `*_for` methods all derive this once via [`of`](Self::of) rather than each
/// re-deriving their own precedence chain from [`Response`]'s flags, which is how `bg_for`/
/// `fg_for` and `style_for` used to disagree on whether `disabled` mattered at all.
///
/// `#[non_exhaustive]`: widget state may grow further variants (e.g. a `Selected` state) without
/// that being a breaking change for callers who already match with a wildcard arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetState {
    /// [`Response::disabled`] is `true`, regardless of any other flag.
    Disabled,
    /// [`Response::pressed`] is `true` and the widget isn't disabled.
    Pressed,
    /// [`Response::focused`] is `true` and the widget is neither disabled nor pressed.
    Focused,
    /// [`Response::hovered`] is `true` and the widget is neither disabled, pressed, nor focused.
    Hovered,
    /// None of the above: the widget's resting state.
    Idle,
}

impl WidgetState {
    /// Derives the single resolved state from `response`'s independent flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_ui::theme::WidgetState;
    /// use retroglyph_ui::interact::Interaction;
    /// use retroglyph_core::grid::Rect;
    ///
    /// let mut interaction = Interaction::<u32>::default();
    /// let response = interaction.interact(Rect::new(0, 0, 1, 1), 1, Default::default());
    /// assert_eq!(WidgetState::of(&response), WidgetState::Idle);
    /// ```
    #[must_use]
    pub const fn of<Id>(response: &Response<Id>) -> Self {
        if response.disabled() {
            Self::Disabled
        } else if response.pressed() {
            Self::Pressed
        } else if response.focused() {
            Self::Focused
        } else if response.hovered() {
            Self::Hovered
        } else {
            Self::Idle
        }
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
        let response: Response<()> = Response::default();
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.panel_bg);
    }

    #[test]
    fn bg_for_is_hover_bg_when_hovered() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            hovered: true,
            ..Response::default()
        };
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.hover_bg);
    }

    #[test]
    fn bg_for_prefers_press_bg_over_hover_bg() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.press_bg);
    }

    #[test]
    fn bg_for_is_base_when_disabled_even_if_pressed() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            disabled: true,
            pressed: true,
            ..Response::default()
        };
        assert_eq!(theme.bg_for(&response, theme.panel_bg), theme.panel_bg);
    }

    #[test]
    fn fg_for_is_dim_when_idle() {
        let theme = Theme::DARK;
        let response: Response<()> = Response::default();
        assert_eq!(theme.fg_for(&response), theme.dim);
    }

    #[test]
    fn fg_for_is_fg_when_hovered() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            hovered: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.fg);
    }

    #[test]
    fn fg_for_prefers_accent_over_hover_when_focused() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            hovered: true,
            focused: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.accent);
    }

    #[test]
    fn fg_for_prefers_accent_over_hover_when_pressed() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.accent);
    }

    #[test]
    fn fg_for_is_disabled_even_if_pressed() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            disabled: true,
            pressed: true,
            ..Response::default()
        };
        assert_eq!(theme.fg_for(&response), theme.disabled);
    }

    #[test]
    fn widget_state_of_precedence_disabled_over_everything() {
        let response: Response<()> = Response {
            disabled: true,
            pressed: true,
            focused: true,
            hovered: true,
            ..Response::default()
        };
        assert_eq!(WidgetState::of(&response), WidgetState::Disabled);
    }

    #[test]
    fn widget_state_of_is_idle_by_default() {
        let response: Response<()> = Response::default();
        assert_eq!(WidgetState::of(&response), WidgetState::Idle);
    }

    #[test]
    fn style_for_is_dim_on_base_when_idle() {
        let theme = Theme::DARK;
        let response: Response<()> = Response::default();
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.dim);
        assert_eq!(style.background(), theme.panel_bg);
    }

    #[test]
    fn style_for_is_disabled_on_base_when_disabled() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
            disabled: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.disabled);
        assert_eq!(style.background(), theme.panel_bg);
    }

    #[test]
    fn style_for_is_fg_on_hover_bg_when_hovered() {
        let theme = Theme::DARK;
        let response: Response<()> = Response {
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
        let response: Response<()> = Response {
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
        let response: Response<()> = Response {
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
        let response: Response<()> = Response {
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
        let response: Response<()> = Response {
            disabled: true,
            pressed: true,
            focused: true,
            hovered: true,
            ..Response::default()
        };
        let style = theme.style_for(&response, theme.panel_bg);
        assert_eq!(style.foreground(), theme.disabled);
        assert_eq!(style.background(), theme.panel_bg);
    }

    #[test]
    fn disabled_is_distinct_from_dim() {
        assert_ne!(Theme::DARK.disabled, Theme::DARK.dim);
        assert_ne!(Theme::LIGHT.disabled, Theme::LIGHT.dim);
    }

    #[test]
    fn danger_and_success_are_distinct_per_theme() {
        assert_ne!(Theme::DARK.danger, Theme::DARK.success);
        assert_ne!(Theme::LIGHT.danger, Theme::LIGHT.success);
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
