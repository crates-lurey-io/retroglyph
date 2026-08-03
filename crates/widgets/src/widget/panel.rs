//! [`Panel`]: a bordered, titled panel.
use retroglyph_core::text::truncate_measured;
use retroglyph_core::{Color, Rect, Style};

use super::{BorderType, BoxBorder, Measure, Widget};
use crate::Surface;
use crate::draw::fill_rect;
use crate::style::Sides;
use crate::text::draw_clipped;
use crate::{Align, Theme};

/// A bordered panel: a filled background with a box border and an optional
/// title in the top edge.
///
/// `border_style` (the box outline and title) and `fill_style` (the
/// interior background) both default to [`Theme::DARK`] (as if [`Panel::theme`] had been called);
/// there is no title by default, and the title (if any) defaults to [`Align::Center`].
/// Set whichever of these a caller needs via
/// [`Panel::border_style`]/[`Panel::fill_style`]/[`Panel::title`]/[`Panel::title_align`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Panel, Surface, Widget};
///
/// let area = Rect::new(0, 0, 20, 5);
/// let mut grid = Grid::new(20, 5);
/// Panel::new()
///     .title("Status")
///     .render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct Panel<'a> {
    title: Option<&'a str>,
    title_align: Align,
    border_style: Style,
    fill_style: Style,
    border_type: BorderType,
    padding: Sides,
}

impl<'a> Panel<'a> {
    /// A plain, untitled panel, styled from [`Theme::DARK`] (as if [`Panel::theme`] had been
    /// called).
    #[must_use]
    pub fn new() -> Self {
        Self {
            title_align: Align::Center,
            ..Self::default()
        }
        .theme(Theme::DARK)
    }

    /// Set the panel's title.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Set how the title is aligned along the top border. Defaults to
    /// [`Align::Center`].
    #[must_use]
    pub const fn title_align(mut self, align: Align) -> Self {
        self.title_align = align;
        self
    }

    /// Set the box outline and title's style.
    #[must_use]
    pub const fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Set the interior background's style.
    #[must_use]
    pub const fn fill_style(mut self, style: Style) -> Self {
        self.fill_style = style;
        self
    }

    /// Set which box-drawing glyphs the border is drawn with. Defaults to
    /// [`BorderType::Plain`], the same as [`BoxBorder::border_type`].
    #[must_use]
    pub const fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Reserve `padding` between the border and the rect [`Panel::inner`] returns.
    ///
    /// Padding is not painted specially: [`Panel::render`] still fills the whole area inside the
    /// border with `fill_style` (padding cells included), the same as [`Panel::inner`]'s caller
    /// would see if they filled `area` themselves before drawing into the smaller inner rect.
    /// Defaults to [`Sides::ZERO`] (no padding beyond the 1-cell border).
    #[must_use]
    pub const fn padding(mut self, padding: Sides) -> Self {
        self.padding = padding;
        self
    }

    /// The content rect inside `area`'s border and padding, ready to hand to another widget.
    ///
    /// Derived from the same 1-cell border inset [`Panel::render`] uses plus this panel's
    /// [`Panel::padding`], so the two can't drift. Saturates to a zero-sized rect (at `area`'s
    /// origin) rather than underflowing when `area` is too small to hold the border and padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::Rect;
    /// use retroglyph_widgets::{Panel, Sides};
    ///
    /// let panel = Panel::new().padding(Sides::symmetric(0, 1));
    /// let area = Rect::new(0, 0, 20, 5);
    /// assert_eq!(panel.inner(area), Rect::new(2, 1, 16, 3));
    /// ```
    #[must_use]
    pub const fn inner(&self, area: Rect) -> Rect {
        let left = 1 + self.padding.left;
        let top = 1 + self.padding.top;
        let horizontal = 2 + self.padding.left + self.padding.right;
        let vertical = 2 + self.padding.top + self.padding.bottom;
        Rect::new(
            area.left().saturating_add(left),
            area.top().saturating_add(top),
            area.width().saturating_sub(horizontal),
            area.height().saturating_sub(vertical),
        )
    }

    /// Applies `theme`'s named roles to this panel's border and fill: `border_style` becomes
    /// `theme.border` on `theme.title_bg` (the same background the title, if any, is drawn on),
    /// and `fill_style` becomes `theme.panel_bg`.
    ///
    /// Like every other builder method here, whichever call comes last wins: call `.theme(...)`
    /// before any manual [`Panel::border_style`]/[`Panel::fill_style`] override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Panel::theme`], but `fill_style` is drawn on `bg` instead of `theme.panel_bg` --
    /// for a panel whose interior should read as a different surface than `theme.panel_bg`
    /// (`border_style` still uses `theme.title_bg`, unaffected by `bg`). [`Panel::theme`] is
    /// exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.border_style = Style::new().fg(theme.border).bg(theme.title_bg);
        self.fill_style = Style::new().bg(bg);
        self
    }
}

impl Measure for Panel<'_> {
    /// The 1-cell border on each edge plus this panel's [`Panel::padding`] -- the height a
    /// zero-height inner content area would need, matching [`Panel::inner`]'s own vertical inset.
    /// The title (if any) is drawn into the top border row rather than adding one of its own, so
    /// it does not add to this count. `width` is unused: `Panel` never wraps content of its own,
    /// only whatever a caller renders into [`Panel::inner`], so it has nothing to measure against
    /// `width` yet.
    fn height_for(&self, _width: u16) -> u16 {
        2u16.saturating_add(self.padding.top)
            .saturating_add(self.padding.bottom)
    }
}

impl Widget for Panel<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let (width, height) = (surface.width(), surface.height());
        if width < 2 || height < 2 {
            return;
        }

        // Fill interior (inside the border), in this surface's own local coordinates.
        let inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
        fill_rect(surface, inner, ' ', self.fill_style);

        BoxBorder::new()
            .style(self.border_style)
            .border_type(self.border_type)
            .render(surface);

        // Render the title into the top border if one was provided.
        if let Some(t) = self.title {
            let max_title_w = width.saturating_sub(4); // 2 border + 2 spaces
            if max_title_w == 0 {
                return;
            }
            // Truncate and measure up front: the padding spaces flank the title, so their
            // position depends on the truncated title's own width, not the other way around
            // (unlike the widgets that hand this whole sequence to `draw_clipped` in one call).
            let (t, t_w) = truncate_measured(t, max_title_w);
            // The padded title (a space either side of the text) is aligned
            // within the region between the two corners (`width - 2`).
            let padded = t_w + 2;
            let title_x = 1 + self.title_align.offset(width - 2, padded);
            surface.put((title_x, 0), ' ', self.border_style);
            let _ = draw_clipped(
                surface,
                (title_x + 1, 0),
                t_w,
                t,
                Align::Left,
                self.border_style,
            );
            surface.put((title_x + 1 + t_w, 0), ' ', self.border_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Grid, Pos};

    use super::*;

    #[test]
    fn draws_border_fill_and_title() {
        let area = Rect::new(0, 0, 10, 4);
        let border = Style::new().fg(Color::WHITE);
        let fill = Style::new();

        let mut grid = Grid::new(10, 4);
        Panel::new()
            .border_style(border)
            .fill_style(fill)
            .title("hi")
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '┌');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), ' '); // interior filled
        // Title centred in the top border somewhere.
        let top_row: String = (0..10).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(top_row.contains("hi"));
    }

    #[test]
    fn long_title_is_truncated_to_fit() {
        let area = Rect::new(0, 0, 8, 3); // max_title_w = 8 - 4 = 4
        let mut grid = Grid::new(8, 3);
        Panel::new()
            .title("a very long title")
            .render(&mut Surface::new(&mut grid, area, 0));

        let top_row: String = (0..8).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(!top_row.contains("a very long title"));
    }

    #[test]
    fn theme_maps_named_roles_onto_border_and_fill() {
        let area = Rect::new(0, 0, 10, 4);
        let mut grid = Grid::new(10, 4);
        Panel::new()
            .theme(Theme::DARK)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.title_bg
        );
        assert_eq!(
            grid[Pos::new(1, 1)].style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 10, 4);
        let mut grid = Grid::new(10, 4);
        Panel::new()
            .theme_on(Theme::DARK, Color::Default)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.title_bg
        );
        assert_eq!(grid[Pos::new(1, 1)].style().background(), Color::Default);
    }

    #[test]
    fn left_aligned_title_starts_after_the_corner() {
        let area = Rect::new(0, 0, 12, 3);
        let mut grid = Grid::new(12, 3);
        Panel::new()
            .title("hi")
            .title_align(Align::Left)
            .render(&mut Surface::new(&mut grid, area, 0));

        // Padded title " hi " starts at column 1 (just inside the corner):
        // space at 1, text at 2..4, trailing space at 4.
        assert_eq!(grid[Pos::new(1, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'i');
    }

    #[test]
    fn right_aligned_title_ends_before_the_corner() {
        let area = Rect::new(0, 0, 12, 3);
        let mut grid = Grid::new(12, 3);
        Panel::new()
            .title("hi")
            .title_align(Align::Right)
            .render(&mut Surface::new(&mut grid, area, 0));

        // Padded title " hi " (4 cols) ends against the right corner at
        // column 11: trailing space at 10, text at 8..10.
        assert_eq!(grid[Pos::new(8, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(9, 0)].glyph(), 'i');
        assert_eq!(grid[Pos::new(10, 0)].glyph(), ' ');
    }

    #[test]
    fn height_for_is_the_border_plus_padding() {
        assert_eq!(Panel::new().height_for(80), 2);
        let padded = Panel::new().padding(Sides::symmetric(1, 0));
        assert_eq!(padded.height_for(80), 4); // 2 border + 1 top + 1 bottom
    }

    #[test]
    fn inner_insets_by_the_one_cell_border() {
        let area = Rect::new(0, 0, 20, 5);
        assert_eq!(Panel::new().inner(area), Rect::new(1, 1, 18, 3));
    }

    #[test]
    fn inner_also_insets_by_padding() {
        let area = Rect::new(0, 0, 20, 5);
        let panel = Panel::new().padding(Sides::symmetric(0, 1));
        assert_eq!(panel.inner(area), Rect::new(2, 1, 16, 3));
    }

    #[test]
    fn inner_saturates_instead_of_underflowing_when_area_is_too_small() {
        let area = Rect::new(3, 4, 1, 1);
        let panel = Panel::new().padding(Sides::all(2));
        assert_eq!(panel.inner(area), Rect::new(6, 7, 0, 0));
    }

    #[test]
    fn too_small_is_a_no_op() {
        let area = Rect::new(0, 0, 1, 1);
        let mut grid = Grid::new(1, 1);
        Panel::new().render(&mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn border_type_selects_the_glyph_set() {
        let area = Rect::new(0, 0, 10, 4);
        let mut grid = Grid::new(10, 4);
        Panel::new()
            .border_type(BorderType::Double)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '╔');
        assert_eq!(grid[Pos::new(9, 0)].glyph(), '╗');
        assert_eq!(grid[Pos::new(0, 3)].glyph(), '╚');
        assert_eq!(grid[Pos::new(9, 3)].glyph(), '╝');
    }

    #[test]
    fn wide_char_title_is_centred_by_display_width_not_byte_length() {
        // "あ" is 1 char, 3 bytes (UTF-8), 2 display columns. A byte-length
        // title width (the pre-fix bug) would reserve 3 columns for it and
        // miscentre the title, and would place the trailing space one
        // column further right than it should be.
        let area = Rect::new(0, 0, 10, 3); // max_title_w = 10 - 4 = 6
        let mut grid = Grid::new(10, 3);
        Panel::new()
            .title("あ")
            .render(&mut Surface::new(&mut grid, area, 0));

        // title_x = 0 + (10 - 2 - 2) / 2 = 3; title glyph at 4, trailing
        // space at 5. With the pre-fix byte-length bug (width 3) this would
        // compute title_x = (10 - 3 - 2) / 2 = 2, off by one.
        assert_eq!(grid[Pos::new(3, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'あ');

        // Column 5 is where a wide char's spacer column would sit. Reserving it is an `egc`-only
        // guarantee (see README's "Extended grapheme cluster support" section): with `egc`,
        // `print` writes a real spacer there, so it reads back as the trailing pad space this
        // widget wrote. Without `egc`, `print` only ever touches one cell per `char` regardless of
        // its display width, so column 5 is left holding whatever the border already drew there,
        // and the pad space this widget writes lands one column further out instead.
        #[cfg(feature = "egc")]
        assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
        #[cfg(not(feature = "egc"))]
        {
            assert_eq!(grid[Pos::new(5, 0)].glyph(), '─');
            assert_eq!(grid[Pos::new(6, 0)].glyph(), ' ');
        }
    }

    #[test]
    fn renders_identically_at_the_origin_and_at_a_scoped_offset() {
        // #738: a widget correct only by not touching `surface.area()`'s absolute `left()`/
        // `top()` should draw the same relative to its own area regardless of where that area
        // sits on the grid. `Panel::render` is written that way (`surface.width()`/`height()`,
        // never `area().left()`/`top()`), so this must hold for it already.
        let (width, height) = (10, 4);
        let mut origin_grid = Grid::new(width, height);
        Panel::new().title("hi").render(&mut Surface::new(
            &mut origin_grid,
            Rect::new(0, 0, width, height),
            0,
        ));

        let (ox, oy) = (3, 2);
        let mut offset_grid = Grid::new(width + ox, height + oy);
        let mut root = Surface::new(
            &mut offset_grid,
            Rect::new(0, 0, width + ox, height + oy),
            0,
        );
        Panel::new()
            .title("hi")
            .render(&mut root.scope(Rect::new(ox, oy, width, height)));

        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    origin_grid[Pos::new(x, y)].glyph(),
                    offset_grid[Pos::new(x + ox, y + oy)].glyph(),
                    "mismatch at local ({x}, {y})"
                );
            }
        }
    }
}
