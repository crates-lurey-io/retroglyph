//! [`Button`]: a clickable label, styled from an already-resolved [`Response`].
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::Size;
use retroglyph_core::text::width;

use super::{InteractiveWidget, MinSize, Widget};
use crate::Surface;
use crate::align::Align;
use crate::draw::fill_rect;
use crate::interact::Density;
use crate::interact::Response;
use crate::interact::Sense;
use crate::text::draw_clipped;
use crate::theme::Theme;

// This paragraph crossed the `too_long_first_doc_paragraph` threshold once its
// `Interaction::interact` link needed full qualification (retroglyph#1273); see the matching
// comment above `animate` in `lib.rs` for the general shape of this noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// A filled, centered `label`, styled by a [`Response`] the caller resolves via
/// [`Interaction::interact`](crate::interact::Interaction::interact) (or, through [`InteractiveWidget`],
/// has resolved automatically).
///
/// `Button` is pure presentation, not a new source of truth: it never calls `interact` itself and
/// has no `Id` type parameter, unlike `Interaction<Id>`. The app still owns the `Interaction<Id>`
/// context and decides the button's id: the same division of labor as every other widget here
/// (state lives outside; the widget only reads it). [`InteractiveWidget::sense`] fixes the
/// [`Sense`](crate::interact::Sense) this button needs ([`Sense::click`](crate::interact::Sense::click)), so a call
/// site can't mismatch it:
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::interact::Interaction;
/// use retroglyph_ui::widget::{Button, InteractiveWidget};
/// use retroglyph_ui::Surface;
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
/// let response = interaction.interact(area, Id::Save, InteractiveWidget::<Id>::sense(&button));
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
    const fn resolved_style<Id>(&self, response: &Response<Id>) -> Style {
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

impl MinSize for Button<'_> {
    /// [`label`](Button::new)'s display width plus 2 columns of padding on each side, one row
    /// tall, floored at `density`'s [`min_target_size`](Density::min_target_size) so a narrow
    /// label never claims less room than `density` calls for a clickable target.
    fn min_size(&self, density: Density) -> Size {
        let content = Size::new(width(self.label).saturating_add(4), 1);
        content.max(density.min_target_size())
    }
}

impl<Id> InteractiveWidget<Id> for Button<'_> {
    type State = ();

    fn sense(&self) -> Sense {
        Sense::click()
    }

    fn render(&self, surface: &mut Surface<'_>, (): &mut Self::State, response: Response<Id>) {
        let (width, height) = (surface.width(), surface.height());
        if width == 0 || height == 0 {
            return;
        }

        let style = self.resolved_style(&response);
        let local_area = surface.area().at_origin();
        fill_rect(surface, local_area, ' ', style);

        // Center row, biased toward the bottom for even heights (integer division floors).
        let y = height / 2;
        let _ = draw_clipped(surface, (0, y), width, self.label, Align::Center, style);
    }
}

impl Widget for Button<'_> {
    /// Draws this button in its idle style: the non-interactive counterpart to
    /// [`InteractiveWidget::render`], sharing the same drawing routine with
    /// [`Response::default`] standing in for "nothing happened".
    fn render(&self, surface: &mut Surface<'_>) {
        InteractiveWidget::<()>::render(self, surface, &mut (), Response::default());
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use retroglyph_core::grid::{Grid, HasSize as _, Pos, Rect};

    use super::*;
    use crate::interact::Interaction;

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
        let response: Response<()> = Response {
            hovered: true,
            pressed: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(&response).background(),
            button.pressed_style.background()
        );
    }

    #[test]
    fn hovered_takes_precedence_over_focused() {
        let response: Response<()> = Response {
            hovered: true,
            focused: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(&response).background(),
            button.hovered_style.background()
        );
    }

    #[test]
    fn focused_only_shows_when_not_pressed_or_hovered() {
        let response: Response<()> = Response {
            focused: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(
            button.resolved_style(&response).background(),
            button.focused_style.background()
        );
    }

    #[test]
    fn idle_by_default() {
        let button = Button::new("Go");
        assert_eq!(
            button
                .resolved_style(&Response::<()>::default())
                .background(),
            button.style.background()
        );
    }

    #[test]
    fn style_knobs_can_be_overridden() {
        let custom = Style::new().fg(Color::RED).bg(Color::GREEN);
        let response: Response<()> = Response {
            pressed: true,
            ..Response::default()
        };
        let button = Button::new("Go").pressed_style(custom);
        assert_eq!(button.resolved_style(&response).background(), Color::GREEN);
    }

    #[test]
    fn integrates_with_interaction_and_reflects_a_real_click() {
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(0, 0, 7, 1);
        let button = Button::new("Go");

        interaction.begin_frame();
        let _ = interaction.interact(area, Id::Save, InteractiveWidget::<Id>::sense(&button));
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            Pos::new(2, 0),
            KeyModifiers::NONE,
        )));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent::new(
            MouseEventKind::Up(MouseButton::Left),
            Pos::new(2, 0),
            KeyModifiers::NONE,
        )));

        interaction.begin_frame();
        let response =
            interaction.interact(area, Id::Save, InteractiveWidget::<Id>::sense(&button));
        interaction.end_frame();
        assert!(response.clicked());

        // The synthetic down+up pair above lands in one `handle_event` batch (see
        // `Interaction`'s doc comment on this exact edge case), so `pressed` is still `true` on
        // the same frame `clicked` resolves: `Button` renders with `pressed_style` here, not
        // idle. Confirms end-to-end wiring (a real click drives a real style pick), not just that
        // `resolved_style` matches its own precedence rules in isolation (the other tests above).
        assert_eq!(
            button.resolved_style(&response).background(),
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
        let response: Response<()> = Response {
            hovered: true,
            pressed: true,
            disabled: true,
            ..Response::default()
        };
        let button = Button::new("Go");
        assert_eq!(button.resolved_style(&response), button.disabled_style);
    }

    #[test]
    fn theme_on_maps_dim_onto_disabled_style() {
        use crate::theme::Theme;

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
        use crate::theme::Theme;

        let response: Response<()> = Response {
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
            button.resolved_style(&response).background(),
            Theme::DARK.hover_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        use crate::theme::Theme;

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

    #[test]
    fn min_size_pads_the_label_by_two_columns_each_side() {
        // "Go" is 2 cols wide; a mouse density is one row tall and doesn't hit the 6-wide
        // floor, so the label's own padding (2 cols + 2 cols) determines the width.
        let size = Button::new("Go").min_size(Density::Mouse);
        assert_eq!(size.width(), 6);
        assert_eq!(size.height(), 1);
    }

    #[test]
    fn min_size_floors_a_short_label_at_the_density_minimum() {
        // "OK" padded is 2 + 4 = 6 cols wide, matching `Density::min_target_size`'s own 6-cell
        // floor exactly, so this only proves the floor doesn't shrink it below that; the touch
        // row height (3, vs. this button's own unpadded 1) is what actually exercises the max.
        let size = Button::new("OK").min_size(Density::Touch);
        assert_eq!(size, Density::Touch.min_target_size());
    }

    #[test]
    fn min_size_grows_past_the_density_floor_for_a_wide_label() {
        // "Save Changes" is 12 cols; padded to 16, well past the 6-cell floor either density
        // sets, so the label's own width should win over `min_target_size`.
        let size = Button::new("Save Changes").min_size(Density::Mouse);
        assert_eq!(size.width(), 16);
        assert_eq!(size.height(), 1);
    }
}
