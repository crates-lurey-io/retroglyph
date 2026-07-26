//! [`BoxBorder`]: a single-line box border.
use retroglyph_core::{Color, Rect, Style};

use super::Widget;
use crate::Surface;
use crate::Theme;
use crate::draw::{BL, BR, H, TL, TR, V};

/// A single-line box border drawn around a [`Rect`].
///
/// The interior of the rectangle is not touched. `area` must be at least
/// 2×2, or [`Widget::render`] is a no-op. `style` defaults to
/// [`Style::new()`]; set it with [`BoxBorder::style`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{BoxBorder, Surface, Widget};
///
/// let area = Rect::new(0, 0, 10, 4);
/// let mut grid = Grid::new(10, 4);
/// BoxBorder::new().render(area, &mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct BoxBorder {
    style: Style,
}

impl BoxBorder {
    /// A plain box border; see [`BoxBorder::style`] to color it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the border's style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets `style` to `theme.border` on `theme.panel_bg`.
    ///
    /// The background is set explicitly rather than left at [`Style::new()`]'s default: an unset
    /// background isn't "transparent" once a real backend draws it (a bare `Color::Default` cell
    /// paints as solid black behind the glyph, not whatever was there before -- see
    /// `retroglyph-software`'s `DEFAULT_BG`), which would leave a visible black grid of border
    /// cells on a light [`Theme`] rather than a border blending into its surroundings. That means
    /// this widget has to assume *something* about what it's drawn over, even though (unlike
    /// [`super::Panel`], which also owns and fills its own interior) a standalone `BoxBorder`
    /// genuinely doesn't know -- `theme.panel_bg` is the closest default, matching what a themed
    /// [`super::Panel`]/[`super::Modal`] around it would use. Drawing this border directly on the
    /// raw screen background instead needs a manual [`BoxBorder::style`] override afterwards.
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
    fn render(&self, area: Rect, surface: &mut Surface<'_>) {
        if area.width() < 2 || area.height() < 2 {
            return;
        }

        let x0 = area.left();
        let y0 = area.top();
        let x1 = area.right().saturating_sub(1);
        let y1 = area.bottom().saturating_sub(1);

        // Corners
        surface.put(x0, y0, TL, self.style);
        surface.put(x1, y0, TR, self.style);
        surface.put(x0, y1, BL, self.style);
        surface.put(x1, y1, BR, self.style);

        // Horizontal edges
        for x in (x0 + 1)..x1 {
            surface.put(x, y0, H, self.style);
            surface.put(x, y1, H, self.style);
        }

        // Vertical edges
        for y in (y0 + 1)..y1 {
            surface.put(x0, y, V, self.style);
            surface.put(x1, y, V, self.style);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Grid, Pos};

    use super::*;

    #[test]
    fn draws_corners_and_edges() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .style(Style::new().fg(Color::WHITE))
            .render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), TL);
        assert_eq!(grid[Pos::new(4, 0)].glyph(), TR);
        assert_eq!(grid[Pos::new(0, 2)].glyph(), BL);
        assert_eq!(grid[Pos::new(4, 2)].glyph(), BR);
        assert_eq!(grid[Pos::new(2, 0)].glyph(), H);
        assert_eq!(grid[Pos::new(0, 1)].glyph(), V);
        // Interior untouched.
        assert_eq!(grid[Pos::new(2, 1)].glyph(), ' ');
    }

    #[test]
    fn too_small_is_a_no_op() {
        let area = Rect::new(0, 0, 1, 1);
        let mut grid = Grid::new(1, 1);
        BoxBorder::new().render(area, &mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_border_role_onto_style() {
        let area = Rect::new(0, 0, 5, 3);
        let mut grid = Grid::new(5, 3);
        BoxBorder::new()
            .theme(Theme::DARK)
            .render(area, &mut Surface::new(&mut grid, area, 0));

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
            .render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::Default);
    }
}
