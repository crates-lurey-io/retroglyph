//! [`BoxBorder`]: a single-line box border.
use retroglyph_core::color::{Color, Style};

use super::{BorderType, Widget};
use crate::Surface;
use crate::Theme;

/// A single-line box border drawn around a [`Rect`](retroglyph_core::grid::Rect).
///
/// The interior of the rectangle is not touched. `area` must be at least
/// 2×2, or [`Widget::render`] is a no-op. `style` defaults to [`Theme::DARK`] (as if
/// [`BoxBorder::theme`] had been called); set it with [`BoxBorder::style`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::{BoxBorder, Surface, Widget};
///
/// let area = Rect::new(0, 0, 10, 4);
/// let mut grid = Grid::new(10, 4);
/// BoxBorder::new().render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct BoxBorder {
    style: Style,
    border_type: BorderType,
}

impl BoxBorder {
    /// A box border styled from [`Theme::DARK`] (as if [`BoxBorder::theme`] had been called); see
    /// [`BoxBorder::style`] to override it.
    #[must_use]
    pub fn new() -> Self {
        Self::default().theme(Theme::DARK)
    }

    /// Set the border's style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set which box-drawing glyphs the border is drawn with. Defaults to
    /// [`BorderType::Plain`].
    #[must_use]
    pub const fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Sets `style` to `theme.border` on `theme.panel_bg`.
    ///
    /// The background is set explicitly for the same reason, and with the same caveat, as
    /// [`super::Gauge::theme`]; see its doc comment for the full explanation. Unlike
    /// [`super::Panel`], which also owns and fills its own interior, a standalone `BoxBorder`
    /// genuinely doesn't know what it's drawn over: `theme.panel_bg` is the closest default,
    /// matching what a themed [`super::Panel`]/[`super::Modal`] around it would use. Drawing this
    /// border directly on the raw screen background instead needs a manual [`BoxBorder::style`]
    /// override afterwards.
    ///
    /// Call before any manual [`BoxBorder::style`] override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`BoxBorder::theme`], but `style` is drawn on `bg` instead of `theme.panel_bg` --
    /// for a border drawn directly on a backdrop other than a themed [`super::Panel`]/
    /// [`super::Modal`]'s fill. [`BoxBorder::theme`] is exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.style = Style::new().fg(theme.border).bg(bg);
        self
    }
}

impl Widget for BoxBorder {
    fn render(&self, surface: &mut Surface<'_>) {
        let (w, h) = (surface.width(), surface.height());
        if w < 2 || h < 2 {
            return;
        }

        let x1 = w - 1;
        let y1 = h - 1;
        let glyphs = self.border_type.glyphs();
        let mut surface = surface.with_style(self.style);

        // Corners
        surface.put((0, 0), glyphs.top_left);
        surface.put((x1, 0), glyphs.top_right);
        surface.put((0, y1), glyphs.bottom_left);
        surface.put((x1, y1), glyphs.bottom_right);

        // Horizontal edges
        for x in 1..x1 {
            surface.put((x, 0), glyphs.horizontal);
            surface.put((x, y1), glyphs.horizontal);
        }

        // Vertical edges
        for y in 1..y1 {
            surface.put((0, y), glyphs.vertical);
            surface.put((x1, y), glyphs.vertical);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::color::Color;
    use retroglyph_core::grid::{Grid, Pos, Rect};
    use retroglyph_core::symbols::border::PLAIN;

    use super::*;

    #[test]
    fn draws_corners_and_edges() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .style(Style::new().fg(Color::WHITE))
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), PLAIN.top_left);
        assert_eq!(grid[Pos::new(4, 0)].glyph(), PLAIN.top_right);
        assert_eq!(grid[Pos::new(0, 2)].glyph(), PLAIN.bottom_left);
        assert_eq!(grid[Pos::new(4, 2)].glyph(), PLAIN.bottom_right);
        assert_eq!(grid[Pos::new(2, 0)].glyph(), PLAIN.horizontal);
        assert_eq!(grid[Pos::new(0, 1)].glyph(), PLAIN.vertical);
        // Interior untouched.
        assert_eq!(grid[Pos::new(2, 1)].glyph(), ' ');
    }

    #[test]
    fn border_type_defaults_to_plain() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new().render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), PLAIN.top_left);
    }

    #[test]
    fn border_type_selects_the_glyph_set() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .border_type(BorderType::Rounded)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '╭');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), '╮');
        assert_eq!(grid[Pos::new(0, 2)].glyph(), '╰');
        assert_eq!(grid[Pos::new(4, 2)].glyph(), '╯');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), '─');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), '│');
    }

    #[test]
    fn border_type_double() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .border_type(BorderType::Double)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '╔');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), '╗');
        assert_eq!(grid[Pos::new(0, 2)].glyph(), '╚');
        assert_eq!(grid[Pos::new(4, 2)].glyph(), '╝');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), '═');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), '║');
    }

    #[test]
    fn border_type_thick() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .border_type(BorderType::Thick)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '┏');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), '┓');
        assert_eq!(grid[Pos::new(0, 2)].glyph(), '┗');
        assert_eq!(grid[Pos::new(4, 2)].glyph(), '┛');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), '━');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), '┃');
    }

    #[test]
    fn too_small_is_a_no_op() {
        let area = Rect::new(0, 0, 1, 1);
        let mut grid = Grid::new(1, 1);
        BoxBorder::new().render(&mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_border_role_onto_style() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .theme(Theme::DARK)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .theme_on(Theme::DARK, Color::Default)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::Default);
    }
}
