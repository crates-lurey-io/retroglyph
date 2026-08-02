//! [`Panel`]: a bordered, titled panel.
use retroglyph_core::{Color, Rect, Style};
use unicode_width::UnicodeWidthStr;

use super::{BorderType, BoxBorder, Widget};
use crate::Surface;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;
use crate::{Align, Theme};

/// A bordered panel: a filled background with a box border and an optional
/// title in the top edge.
///
/// `border_style` (the box outline and title) and `fill_style` (the
/// interior background) both default to [`Style::new()`]; there is no
/// title by default, and the title (if any) defaults to [`Align::Center`].
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
}

impl<'a> Panel<'a> {
    /// A plain, untitled panel in the default style.
    #[must_use]
    pub fn new() -> Self {
        Self {
            title_align: Align::Center,
            ..Self::default()
        }
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
            let max_title_w = width.saturating_sub(4) as usize; // 2 border + 2 spaces
            if max_title_w == 0 {
                return;
            }
            // Truncate to fit.
            let t = truncate_to_cols(t, max_title_w);
            // `truncate_to_cols` bounds `t` to `max_title_w` columns, itself derived from this
            // surface's own `u16` width, so narrowing the display width back is always exact.
            #[allow(clippy::cast_possible_truncation)]
            let t_w = t.width() as u16;
            // The padded title (a space either side of the text) is aligned
            // within the region between the two corners (`width - 2`).
            let padded = t_w + 2;
            let title_x = 1 + self.title_align.offset(width - 2, padded);
            surface.put((title_x, 0), ' ', self.border_style);
            surface.print((title_x + 1, 0), t, self.border_style);
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
        assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
    }
}
