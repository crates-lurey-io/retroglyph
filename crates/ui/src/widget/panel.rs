//! [`Panel`]: a bordered, titled panel.
use alloc::vec::Vec;

use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::Rect;
use retroglyph_core::text::truncate_measured;

use super::{BorderType, BoxBorder, Measure, Widget};
use crate::Surface;
use crate::draw::fill_rect;
use crate::style::Sides;
use crate::text::draw_clipped;
use crate::{Align, Theme};

/// Which border edge a [`PanelTitle`] is drawn into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TitlePosition {
    /// The top border row, the same edge [`Panel::title`]'s sugar title is drawn into.
    #[default]
    Top,
    /// The bottom border row.
    Bottom,
}

/// One title added via [`Panel::add_title`]: its text, which edge it's drawn on, and its
/// alignment.
///
/// Not constructed directly; built up through [`Panel::add_title`]'s arguments instead, the same
/// as [`Panel::title`]/[`Panel::title_align`] build the single implicit top title.
#[derive(Clone, Copy, Debug)]
pub struct PanelTitle<'a> {
    text: &'a str,
    position: TitlePosition,
    align: Align,
}

/// A bordered panel: a filled background with a box border and, optionally, one or more titles
/// along the top and/or bottom edge.
///
/// `border_style` (the box outline and titles) and `fill_style` (the
/// interior background) both default to [`Theme::DARK`] (as if [`Panel::theme`] had been called);
/// there is no title by default, and the title set by [`Panel::title`] (if any) defaults to
/// [`Align::Center`]. Set whichever of these a caller needs via
/// [`Panel::border_style`]/[`Panel::fill_style`]/[`Panel::title`]/[`Panel::title_align`].
///
/// [`Panel::title`] is sugar for the common case: one top, centered (by default) title. For
/// anything past that (a bottom title, more than one title on an edge, or a title aligned other
/// than via `title_align`), use [`Panel::add_title`], which is fully additive: it never disturbs
/// `title`/`title_align`, and multiple `add_title` calls stack rather than overwrite each other.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::{Align, Panel, Surface, TitlePosition, Widget};
///
/// let area = Rect::new(0, 0, 20, 5);
/// let mut grid = Grid::new(20, 5);
/// Panel::new()
///     .title("Status")
///     .add_title("3 / 10", TitlePosition::Bottom, Align::Right)
///     .render(&mut Surface::new(&mut grid, area, 0));
/// ```
///
/// Not [`Copy`] (unlike most other widgets here): [`Panel::add_title`] stores its titles in a
/// `Vec`, so an unbounded number of them is exactly as cheap, and as fallible in the same ways
/// (only an allocation failure, never a silently dropped title), as pushing onto any other `Vec`.
#[derive(Clone, Debug, Default)]
pub struct Panel<'a> {
    title: Option<&'a str>,
    title_align: Align,
    titles: Vec<PanelTitle<'a>>,
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

    /// Add a title to `position`'s edge, aligned per `align`. Additive and independent of
    /// [`Panel::title`] and of every other `add_title` call: nothing here overwrites another
    /// title, so a top-left title plus a top-right status, or a top title plus a bottom hint
    /// bar, is two calls (or three, alongside `.title(...)`) rather than two overlapping
    /// `Panel`s.
    ///
    /// Multiple titles are allowed on the same edge. Each is drawn in declaration order:
    /// `.title(...)`'s implicit top title first (if set), then `add_title` calls in the order
    /// they were made, truncated to whatever room is left on that title's edge after the
    /// titles declared before it on the same edge have claimed theirs, the same way a single
    /// title already truncates to fit the whole edge. A title that has no room left once earlier
    /// titles on its edge are placed is clipped down to nothing rather than overdrawing them or
    /// panicking. Because a [`Align::Center`] title claims its edge's entire remaining span, a
    /// title declared after a centered one on the same edge always has no room left; put
    /// non-centered titles first if a centered one needs to share an edge.
    ///
    /// Unbounded: every call is kept (this is what makes [`Panel`] not [`Copy`]; see its own doc
    /// comment), there is no cap to silently drop past.
    #[must_use]
    pub fn add_title(mut self, title: &'a str, position: TitlePosition, align: Align) -> Self {
        self.titles.push(PanelTitle {
            text: title,
            position,
            align,
        });
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
    /// use retroglyph_core::grid::Rect;
    /// use retroglyph_ui::{Panel, Sides};
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
    /// The 1-cell border on each edge plus this panel's [`Panel::padding`] (the height a
    /// zero-height inner content area would need), matching [`Panel::inner`]'s own vertical inset.
    /// Every title (the one set by [`Panel::title`], and any added by [`Panel::add_title`],
    /// whichever edge it's on) is drawn into its border row rather than adding one of its own, so
    /// none of them add to this count. `width` is unused: `Panel` never wraps content of its own,
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

        // Top edge: `.title(...)`'s implicit title (if any) claims space first, then any
        // `add_title(..., TitlePosition::Top, ...)` titles in declaration order.
        let mut top = TitleCursor::new(width);
        if let Some(t) = self.title {
            top.draw(surface, 0, t, self.title_align, self.border_style);
        }
        for title in &self.titles {
            if title.position == TitlePosition::Top {
                top.draw(surface, 0, title.text, title.align, self.border_style);
            }
        }

        // Bottom edge: `add_title(..., TitlePosition::Bottom, ...)` titles in declaration order.
        // `height >= 2` (checked above) means `height - 1 >= 1`, always a different row than the
        // top edge's row `0`.
        let mut bottom = TitleCursor::new(width);
        for title in &self.titles {
            if title.position == TitlePosition::Bottom {
                bottom.draw(
                    surface,
                    height - 1,
                    title.text,
                    title.align,
                    self.border_style,
                );
            }
        }
    }
}

/// Tracks how much of one border edge's span (the columns strictly between its two corners) is
/// still free while [`Panel::render`] draws that edge's titles in declaration order, so a title
/// that would overlap an earlier one on the same edge is truncated down to whatever room is left
/// instead of overdrawing it.
struct TitleCursor {
    /// Leftmost free column (inclusive).
    lo: u16,
    /// Rightmost free column (exclusive).
    hi: u16,
}

impl TitleCursor {
    /// A cursor over the whole span between `width`'s two corners: columns `1..width - 1`.
    const fn new(width: u16) -> Self {
        Self {
            lo: 1,
            hi: width.saturating_sub(1),
        }
    }

    /// Draw one title into whatever of this cursor's span is still free, then shrink the span so
    /// a later call on the same edge doesn't overdraw it. Mirrors the truncate-then-pad sequence
    /// a single top title always used: [`truncate_measured`] up front (the padding spaces flank
    /// the title, so their position depends on the truncated title's own width, not the other
    /// way around) then a leading/trailing space either side of it.
    ///
    /// A title with no room left (`lo >= hi`, or fewer than 2 free columns, not even enough for
    /// the two padding spaces) is dropped entirely, drawing nothing, rather than panicking.
    /// [`Align::Center`] claims this cursor's whole remaining span regardless of how much of it
    /// the title itself actually used, so a title declared after a centered one on the same edge
    /// always finds `lo >= hi` and is dropped.
    fn draw(&mut self, surface: &mut Surface<'_>, y: u16, text: &str, align: Align, style: Style) {
        if self.lo >= self.hi {
            return;
        }
        let avail = self.hi - self.lo;
        let Some(max_title_w) = avail.checked_sub(2) else {
            return;
        };
        let (t, t_w) = truncate_measured(text, max_title_w);
        let padded = t_w + 2;
        let title_x = self.lo + align.offset(avail, padded);
        surface.put((title_x, y), ' ', style);
        let _ = draw_clipped(surface, (title_x + 1, y), t_w, t, Align::Left, style);
        surface.put((title_x + 1 + t_w, y), ' ', style);

        match align {
            Align::Left => self.lo = title_x + padded,
            Align::Right => self.hi = title_x,
            Align::Center => self.hi = self.lo,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use retroglyph_core::color::Color;
    use retroglyph_core::grid::{Grid, Pos};

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

        // Column 5 is where a wide char's spacer column sits, reserved on every feature
        // combination: `Grid::put_tile` (which `put` uses without `egc`, and `write_grapheme`
        // uses with it) always writes a real spacer there (retroglyph#869), so it reads back as
        // the trailing pad space this widget also explicitly writes at column 6.
        assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(6, 0)].glyph(), ' ');
    }

    #[test]
    fn add_title_draws_into_the_bottom_border_row() {
        let area = Rect::new(0, 0, 12, 4);
        let mut grid = Grid::new(12, 4);
        Panel::new()
            .add_title("hint", TitlePosition::Bottom, Align::Center)
            .render(&mut Surface::new(&mut grid, area, 0));

        let bottom_row: String = (0..12).map(|x| grid[Pos::new(x, 3)].glyph()).collect();
        assert!(bottom_row.contains("hint"));
        // Top row is untouched: no title was set for it.
        let top_row: String = (0..12).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(!top_row.contains("hint"));
    }

    #[test]
    fn title_and_add_title_coexist_on_different_edges() {
        let area = Rect::new(0, 0, 20, 4);
        let mut grid = Grid::new(20, 4);
        Panel::new()
            .title("Inventory")
            .add_title("3 / 10 items", TitlePosition::Bottom, Align::Right)
            .render(&mut Surface::new(&mut grid, area, 0));

        let top_row: String = (0..20).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(top_row.contains("Inventory"));
        let bottom_row: String = (0..20).map(|x| grid[Pos::new(x, 3)].glyph()).collect();
        assert!(bottom_row.contains("3 / 10 items"));
    }

    #[test]
    fn two_titles_on_the_same_edge_are_left_and_right_aligned() {
        let area = Rect::new(0, 0, 20, 3);
        let mut grid = Grid::new(20, 3);
        Panel::new()
            .add_title("left", TitlePosition::Top, Align::Left)
            .add_title("right", TitlePosition::Top, Align::Right)
            .render(&mut Surface::new(&mut grid, area, 0));

        let top_row: String = (0..20).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(top_row.contains("left"));
        assert!(top_row.contains("right"));
        assert!(top_row.find("left").unwrap() < top_row.find("right").unwrap());
    }

    #[test]
    fn a_later_title_overlapping_an_earlier_one_is_clipped_instead_of_overdrawing_it() {
        // Only 10 columns wide: an 8-column left title (" long L ") leaves no room for a
        // right title declared after it, so the right title must be dropped rather than
        // overdrawing the left one or panicking.
        let area = Rect::new(0, 0, 10, 3);
        let mut grid = Grid::new(10, 3);
        Panel::new()
            .add_title("long L", TitlePosition::Top, Align::Left)
            .add_title("R", TitlePosition::Top, Align::Right)
            .render(&mut Surface::new(&mut grid, area, 0));

        let top_row: String = (0..10).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(top_row.contains("long L"));
        assert!(!top_row.contains('R'));
    }

    #[test]
    fn a_title_after_a_centered_title_on_the_same_edge_finds_no_room() {
        let area = Rect::new(0, 0, 20, 3);
        let mut grid = Grid::new(20, 3);
        Panel::new()
            .add_title("centered", TitlePosition::Top, Align::Center)
            .add_title("dropped", TitlePosition::Top, Align::Right)
            .render(&mut Surface::new(&mut grid, area, 0));

        let top_row: String = (0..20).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(top_row.contains("centered"));
        assert!(!top_row.contains("dropped"));
    }

    #[test]
    fn a_fifth_add_title_call_still_renders() {
        // `Panel::add_title` has no cap (unlike an earlier fixed-slot design): a fifth call
        // (across both edges) must still draw, not silently drop.
        let area = Rect::new(0, 0, 40, 3);
        let mut grid = Grid::new(40, 3);
        Panel::new()
            .add_title("a", TitlePosition::Top, Align::Left)
            .add_title("b", TitlePosition::Bottom, Align::Left)
            .add_title("c", TitlePosition::Top, Align::Right)
            .add_title("d", TitlePosition::Bottom, Align::Right)
            .add_title("e", TitlePosition::Top, Align::Center)
            .render(&mut Surface::new(&mut grid, area, 0));

        let top_row: String = (0..40).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(top_row.contains('a'));
        assert!(top_row.contains('c'));
        assert!(top_row.contains('e'));
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
