//! [`Button`]: a clickable label, styled from an already-resolved [`Response`].
use retroglyph_core::{Color, Rect, Style};

use super::{InteractiveWidget, Widget};
use crate::Response;
use crate::Sense;
use crate::Surface;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A filled, centered `label`, styled by a [`Response`] the caller resolves via
/// [`Interaction::interact`](crate::Interaction::interact) (or, through [`InteractiveWidget`],
/// has resolved automatically).
///
/// `Button` is pure presentation, not a new source of truth: it never calls `interact` itself and
/// has no `Id` type parameter, unlike `Interaction<Id>`. The app still owns the `Interaction<Id>`
/// context and decides the button's id: the same division of labor as every other widget here
/// (state lives outside; the widget only reads it). [`InteractiveWidget::sense`] fixes the
/// [`Sense`](crate::Sense) this button needs ([`Sense::click`](crate::Sense::click)), so a call
/// site can't mismatch it:
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Button, InteractiveWidget, Interaction, Surface};
///
/// #[derive(Clone, Copy, PartialEq, Eq)]
/// enum Id {
///     Save,
/// }
///
/// let mut grid = Grid::new(20, 10);
/// let mut interaction = Interaction::<Id>::new();
/// interaction.begin_frame();
/// let area = Rect::new(0, 0, 10, 1);
/// let button = Button::new("Save");
/// let response = interaction.interact(area, Id::Save, button.sense());
/// InteractiveWidget::render(&button, &mut Surface::new(&mut grid, area, 0), &mut (), response);
/// interaction.end_frame();
/// ```
///
/// Precedence when more than one [`Response`] flag is set at once:
/// [`disabled`](Response::disabled) > [`pressed`](Response::pressed) >
/// [`hovered`](Response::hovered) > [`focused`](Response::focused) > the default `style`:
/// matching the conventional `:disabled` > `:active` > `:hover` > `:focus` ordering, so a
/// disabled button always reads as muted regardless of a stale hover/press, a press always reads
/// as pressed even while still hovered, and a keyboard-focused-but-not-hovered button still shows
/// something distinct from idle.
///
/// `style`, `hovered_style`, `pressed_style`, `focused_style`, and `disabled_style` default to
/// [`Theme::DARK`], as if [`Button::theme`] had been called; set them with
/// [`Button::style`]/[`Button::hovered_style`]/[`Button::pressed_style`]/
/// [`Button::focused_style`]/[`Button::disabled_style`].
#[derive(Clone, Copy, Debug)]
pub struct Button<'a> {
    label: &'a str,
    style: Style,
    hovered_style: Style,
    pressed_style: Style,
    focused_style: Style,
    disabled_style: Style,
}

impl<'a> Button<'a> {
    /// A button labeled `label`, styled from [`Theme::DARK`] (as if [`Button::theme`] had been
    /// called); set [`Button::theme`]/[`Button::theme_on`] for a different [`Theme`] or one of the
    /// `_style` setters for a one-off override.
    #[must_use]
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            style: Style::new(),
            hovered_style: Style::new(),
            pressed_style: Style::new(),
            focused_style: Style::new(),
            disabled_style: Style::new(),
        }
        .theme(Theme::DARK)
    }

    /// Set the default (idle) style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style used while [`Response::hovered`] is `true`.
    #[must_use]
    pub const fn hovered_style(mut self, style: Style) -> Self {
        self.hovered_style = style;
        self
    }

    /// Set the style used while [`Response::pressed`] is `true`.
    #[must_use]
    pub const fn pressed_style(mut self, style: Style) -> Self {
        self.pressed_style = style;
        self
    }

    /// Set the style used while [`Response::focused`] is `true` (and neither pressed nor
    /// hovered).
    #[must_use]
    pub const fn focused_style(mut self, style: Style) -> Self {
        self.focused_style = style;
        self
    }

    /// Set the style used while [`Response::disabled`] is `true`, regardless of any other
    /// [`Response`] flag.
    #[must_use]
    pub const fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Applies `theme`'s named roles to all four of this button's states: idle becomes
    /// `theme.fg` on `theme.panel_bg`; hovered/pressed swap in `theme.hover_bg`/`theme.press_bg`
    /// for the background; focused becomes `theme.accent` on `theme.panel_bg`. The same mapping
    /// `09_widgets_dashboard`'s "Ping" button hand-threads today.
    ///
    /// Call before any manual `_style` override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    // `theme`/`theme_on` leave `disabled_style` untouched: `Theme` has one `dim`
    // role, already used for de-emphasized text elsewhere, and this button's default
    // `disabled_style` (set in `new`) already matches it. A themed button that wants a different
    // disabled treatment can still call `disabled_style` after `theme`/`theme_on`, same as any
    // other override.

    /// Same as [`Button::theme`], but the idle and focused states are drawn on `bg` instead of
    /// `theme.panel_bg` (`hovered_style`/`pressed_style` still use `theme.hover_bg`/
    /// `theme.press_bg`, unaffected by `bg`): for a button drawn directly on a backdrop other
    /// than a themed [`super::Panel`]/[`super::Modal`]'s fill. [`Button::theme`] is exactly
    /// `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.style = Style::new().fg(theme.fg).bg(bg);
        self.hovered_style = Style::new().fg(theme.fg).bg(theme.hover_bg);
        self.pressed_style = Style::new().fg(theme.fg).bg(theme.press_bg);
        self.focused_style = Style::new().fg(theme.accent).bg(bg);
        self.disabled_style = Style::new().fg(theme.dim).bg(bg);
        self
    }

    /// The style this button draws with this frame, per the disabled > pressed > hovered
    /// > focused > default precedence documented on [`Button`], given `response`.
    const fn resolved_style(&self, response: Response) -> Style {
        if response.disabled() {
            self.disabled_style
        } else if response.pressed() {
            self.pressed_style
        } else if response.hovered() {
            self.hovered_style
        } else if response.focused() {
            self.focused_style
        } else {
            self.style
        }
    }
}

impl InteractiveWidget for Button<'_> {
    type State = ();

    fn sense(&self) -> Sense {
        Sense::click()
    }

    fn render(&self, surface: &mut Surface<'_>, (): &mut Self::State, response: Response) {
        let (width, height) = (surface.width(), surface.height());
        if width == 0 || height == 0 {
            return;
        }

        let style = self.resolved_style(response);
        fill_rect(surface, Rect::new(0, 0, width, height), ' ', style);

        let text = truncate_to_cols(self.label, width);
        let text_width = retroglyph_core::text::width(text);
        let x = width.saturating_sub(text_width) / 2;
        let y = height / 2;

        surface.print((x, y), text, style);
    }
}

impl Widget for Button<'_> {
    /// Draws this button in its idle style: the non-interactive counterpart to
    /// [`InteractiveWidget::render`], sharing the same drawing routine with
    /// [`Response::default`] standing in for "nothing happened".
    fn render(&self, surface: &mut Surface<'_>) {
        InteractiveWidget::render(self, surface, &mut (), Response::default());
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{
        Event, Grid, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, Pos, Rect,
    };

    use super::*;
    use crate::Interaction;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Id {
        Save,
    }

    #[test]
    fn draws_the_label_centered_in_the_idle_style() {
        let area = Rect::new(0, 0, 7, 1);
        let mut grid = Grid::new(7, 1);
        Widget::render(&Button::new("Go"), &mut Surface::new(&mut grid, area, 0));

        // "Go" (2 cols) centered in width 7 starts at column (7-2)/2 = 2.
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'G');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'o');
    }

    #[test]
    fn fills_the_whole_area_with_the_background() {
        let area = Rect::new(0, 0, 7, 1);
        let mut grid = Grid::new(7, 1);
        Widget::render(&Button::new("Go"), &mut Surface::new(&mut grid, area, 0));

        let idle_bg = Theme::DARK.panel_bg;
        assert_eq!(grid[Pos::new(0, 0)].style().background(), idle_bg);
        assert_eq!(grid[Pos::new(6, 0)].style().background(), idle_bg);
    }

    #[test]
    fn pressed_takes_precedence_over_hovered() {
        let response = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(response).background(),
            button.pressed_style.background()
        );
    }

    #[test]
    fn hovered_takes_precedence_over_focused() {
        let response = Response {
            hovered: true,
            focused: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(response).background(),
            button.hovered_style.background()
        );
    }

    #[test]
    fn focused_only_shows_when_not_pressed_or_hovered() {
        let response = Response {
            focused: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(response).background(),
            button.focused_style.background()
        );
    }

    #[test]
    fn idle_by_default() {
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(Response::default()).background(),
            button.style.background()
        );
    }

    #[test]
    fn style_knobs_can_be_overridden() {
        let custom = Style::new().fg(Color::RED).bg(Color::GREEN);
        let response = Response {
            pressed: true,
            ..Response::default()
        };
        let button = Button::new("Go").pressed_style(custom);
        assert_eq!(button.resolved_style(response).background(), Color::GREEN);
    }

    #[test]
    fn integrates_with_interaction_and_reflects_a_real_click() {
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(0, 0, 7, 1);
        let button = Button::new("Go");

        interaction.begin_frame();
        let _ = interaction.interact(area, Id::Save, button.sense());
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        let response = interaction.interact(area, Id::Save, button.sense());
        interaction.end_frame();
        assert!(response.clicked());

        // The synthetic down+up pair above lands in one `handle_event` batch (see
        // `Interaction`'s doc comment on this exact edge case), so `pressed` is still `true` on
        // the same frame `clicked` resolves: `Button` renders with `pressed_style` here, not
        // idle. Confirms end-to-end wiring (a real click drives a real style pick), not just that
        // `resolved_style` matches its own precedence rules in isolation (the other tests above).
        assert_eq!(
            button.resolved_style(response).background(),
            button.pressed_style.background()
        );

        let mut grid = Grid::new(7, 1);
        InteractiveWidget::render(
            &button,
            &mut Surface::new(&mut grid, area, 0),
            &mut (),
            response,
        );
    }

    #[test]
    fn scoped_into_a_narrower_clip_still_centers_against_the_full_area() {
        let mut grid = Grid::new(10, 1);
        let full = Rect::new(0, 0, 10, 1);
        let mut surface = Surface::new(&mut grid, full, 0);
        // Clip to the right-hand half before scoping: mirrors a caller drawing this button
        // inside an already-clipped ancestor (e.g. a scrolled panel), then handing it a
        // sub-surface via `scope` for its own (unclipped-by-that-call) area.
        let mut clipped = surface.clip(Rect::new(6, 0, 4, 1));
        Widget::render(&Button::new("Save"), &mut clipped.scope(full));

        // "Save" (4 cols) centered in the full 10-col area starts at column 3, so only its
        // last column (6) falls inside the narrower clip. A widget that recentered itself
        // against the clip instead of `area` would draw the whole label starting at column
        // 6, showing 'S' there instead.
        assert_eq!(grid[Pos::new(6, 0)].glyph(), 'e');
        assert_eq!(grid[Pos::new(7, 0)].glyph(), ' ');
    }

    #[test]
    fn disabled_style_takes_precedence_over_pressed_and_hovered() {
        let response = Response {
            hovered: true,
            pressed: true,
            disabled: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(button.resolved_style(response), button.disabled_style);
    }

    #[test]
    fn theme_on_maps_dim_onto_disabled_style() {
        use crate::Theme;

        let button = Button::new("Go").theme_on(Theme::DARK, Color::Default);
        assert_eq!(button.disabled_style.foreground(), Theme::DARK.dim);
        assert_eq!(button.disabled_style.background(), Color::Default);
    }

    #[test]
    fn zero_size_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let mut grid = Grid::new(1, 1);
        Widget::render(&Button::new("Go"), &mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_every_state() {
        use crate::Theme;

        let response = Response {
            hovered: true,
            ..Response::default()
        };
        let button = Button::new("Go").theme(Theme::DARK);

        assert_eq!(button.style.foreground(), Theme::DARK.fg);
        assert_eq!(button.style.background(), Theme::DARK.panel_bg);
        assert_eq!(button.hovered_style.background(), Theme::DARK.hover_bg);
        assert_eq!(button.pressed_style.background(), Theme::DARK.press_bg);
        assert_eq!(button.focused_style.foreground(), Theme::DARK.accent);
        assert_eq!(
            button.resolved_style(response).background(),
            Theme::DARK.hover_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        use crate::Theme;

        let button = Button::new("Go").theme_on(Theme::DARK, Color::Default);

        assert_eq!(button.style.foreground(), Theme::DARK.fg);
        assert_eq!(button.style.background(), Color::Default);
        assert_eq!(button.focused_style.foreground(), Theme::DARK.accent);
        assert_eq!(button.focused_style.background(), Color::Default);
        // Unaffected by `bg`.
        assert_eq!(button.hovered_style.background(), Theme::DARK.hover_bg);
        assert_eq!(button.pressed_style.background(), Theme::DARK.press_bg);
    }

    #[test]
    fn button_wide_label_draws_outside_its_own_area() {
        let area = Rect::new(0, 0, 4, 1);
        let mut grid = Grid::new(5, 1);
        Widget::render(&Button::new("保存"), &mut Surface::new(&mut grid, area, 0));

        // "保存" is exactly 4 columns; the button's own area is the full width, so the label
        // starts at column 0 and its last continuation cell must land at column 3, not spill
        // into column 4 (outside the button, clobbering whatever's drawn next to it).
        assert_eq!(grid[Pos::new(0, 0)].glyph(), '保');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), '存');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), ' ');
    }

    #[test]
    fn button_centers_a_wide_label_by_display_width() {
        let area = Rect::new(0, 0, 8, 1);
        let mut grid = Grid::new(8, 1);
        Widget::render(&Button::new("保存"), &mut Surface::new(&mut grid, area, 0));

        // "保存" (4 cols) centered in width 8 starts at column (8-4)/2 = 2.
        assert_eq!(grid[Pos::new(2, 0)].glyph(), '保');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), '存');
    }
}
