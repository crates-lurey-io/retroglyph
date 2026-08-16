//! [`Modal`]: a bordered, filled box centered on screen.
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::Rect;

use super::panel::PanelChrome;
use super::{BorderType, Panel, PanelTitle, Widget};
use crate::Surface;
use crate::align::Align;
use crate::layout::centered_rect;
use crate::style::Sides;
use crate::theme::Theme;

/// A bordered, filled box centered in a screen [`Rect`].
///
/// Shorthand for a [`Panel`] sized `width` x `height` and centered via
/// [`centered_rect`]. `border_style`/`fill_style` default to
/// [`Theme::DARK`] (as if [`Modal::theme`] had been called) and there is no title by default: set
/// whichever a caller needs via [`Modal::border_style`]/[`Modal::fill_style`]/[`Modal::title`],
/// the same as [`Panel`].
///
/// [`Modal::render`] returns the inner content [`Rect`] (via [`Panel::inner`], the border inset
/// plus this modal's [`Modal::padding`]) ready to hand to another widget (e.g. [`super::Log`]).
///
/// Draws only the box itself; everything outside it is left untouched (no
/// dimming or backdrop fill, that would need to read and blend existing
/// cells, a separate feature from this thin layout convenience). Not a
/// [`Widget`]: [`Widget::render`] can't return a value, and the inner
/// content rect is part of this type's contract.
///
/// [`Modal::render`] draws through whatever [`Surface`] it's given, on whatever layer that
/// surface is already scoped to: it has no layer of its own to default. A modal painted over
/// an active screen should be given a surface on [`Layer::Overlay`](retroglyph_core::surface::Layer::Overlay)
/// (`surface.on_tier(Layer::Overlay)`), so it paints on top regardless of whether the screen or
/// the modal renders first this frame; see [`Layer`](retroglyph_core::surface::Layer)'s docs for why that
/// beats ordering the two draw calls.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_core::surface::Layer;
/// use retroglyph_ui::widget::Modal;
/// use retroglyph_ui::Surface;
///
/// let screen = Rect::new(0, 0, 20, 10);
/// let mut grid = Grid::new(20, 10);
/// let mut surface = Surface::new(&mut grid, screen, Layer::World.as_u8());
/// let inner = Modal::new(10, 4)
///     .title("Confirm")
///     .render(screen, &mut surface.on_tier(Layer::Overlay));
/// // `inner` is ready to hand to another widget, e.g. a `Log` or `Text`.
/// assert_eq!(inner.width(), 8);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Modal<'a> {
    width: u16,
    height: u16,
    chrome: PanelChrome<'a>,
    titles: &'a [PanelTitle<'a>],
}

impl<'a> Modal<'a> {
    /// A `width` x `height` modal with no title, styled from [`Theme::DARK`] (as if
    /// [`Modal::theme`] had been called).
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            chrome: PanelChrome::default().title_align(Align::Center),
            titles: &[],
        }
        .theme(Theme::DARK)
    }

    /// Set the modal's title.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.chrome = self.chrome.title(title);
        self
    }

    /// Set how the title is aligned along the top border. Defaults to
    /// [`Align::Center`], the same as [`Panel::title_align`].
    #[must_use]
    pub const fn title_align(mut self, align: Align) -> Self {
        self.chrome = self.chrome.title_align(align);
        self
    }

    /// Add titles beyond [`Modal::title`]'s single top one -- a bottom title, more than one on
    /// an edge, or one aligned other than via [`Modal::title_align`] -- the [`Modal`] equivalent
    /// of [`Panel::add_title`]. Additive: never disturbs `title`/`title_align`.
    ///
    /// Takes the whole set at once as a borrowed slice, built from [`PanelTitle::new`], rather
    /// than accumulating one at a time like [`Panel::add_title`]: an owned `Vec` growing across
    /// repeated calls is exactly what would cost [`Modal`] its [`Copy`] (see this struct's own
    /// doc comment), so a later call here replaces the slice instead of appending to it. Drawn in
    /// the same order and with the same per-edge truncation rules as [`Panel::add_title`].
    #[must_use]
    pub const fn titles(mut self, titles: &'a [PanelTitle<'a>]) -> Self {
        self.titles = titles;
        self
    }

    /// Set the box outline and title's style.
    #[must_use]
    pub const fn border_style(mut self, style: Style) -> Self {
        self.chrome = self.chrome.border_style(style);
        self
    }

    /// Set the interior background's style.
    #[must_use]
    pub const fn fill_style(mut self, style: Style) -> Self {
        self.chrome = self.chrome.fill_style(style);
        self
    }

    /// Set which box-drawing glyphs the border is drawn with. Defaults to
    /// [`BorderType::Plain`], the same as [`Panel::border_type`].
    #[must_use]
    pub const fn border_type(mut self, border_type: BorderType) -> Self {
        self.chrome = self.chrome.border_type(border_type);
        self
    }

    /// Reserve `padding` between the border and the rect [`Modal::render`] returns, the same as
    /// [`Panel::padding`] (a [`Modal`] is just a centered [`Panel`]). Defaults to [`Sides::ZERO`].
    #[must_use]
    pub const fn padding(mut self, padding: Sides) -> Self {
        self.chrome = self.chrome.padding(padding);
        self
    }

    /// Applies `theme`'s named roles to this modal's border and fill, the same mapping as
    /// [`Panel::theme`] (a [`Modal`] is just a centered [`Panel`]): `border_style` becomes
    /// `theme.border` on `theme.title_bg`, and `fill_style` becomes `theme.panel_bg`.
    ///
    /// Call before any manual [`Modal::border_style`]/[`Modal::fill_style`] override you want to
    /// keep: whichever call comes last wins.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.chrome = self.chrome.theme(theme);
        self
    }

    /// Same as [`Modal::theme`], but `fill_style` is drawn on `bg` instead of `theme.panel_bg` --
    /// the same [`Panel::theme_on`] escape hatch, for a modal whose interior should read as a
    /// different surface than `theme.panel_bg` (`border_style` still uses `theme.title_bg`,
    /// unaffected by `bg`). [`Modal::theme`] is exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.chrome = self.chrome.theme_on(theme, bg);
        self
    }

    /// Draw the modal centered in `screen`, returning its inner content
    /// [`Rect`].
    pub fn render(self, screen: Rect, surface: &mut Surface<'_>) -> Rect {
        let rect = centered_rect(screen, self.width, self.height);
        let mut panel = Panel::from_chrome(self.chrome);
        for title in self.titles {
            let (text, position, align) = title.parts();
            panel = panel.add_title(text, position, align);
        }
        panel.render(&mut surface.scope(rect));
        panel.inner(rect)
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use retroglyph_core::grid::{Grid, Pos};

    use super::*;
    use crate::widget::TitlePosition;

    /// PanelChrome-sharing with `Panel` (retroglyph#1385) must not cost `Modal` its `Copy`: it's
    /// the reason `Modal` embeds `PanelChrome` by value instead of a `Panel` (not `Copy`, owns a
    /// `Vec` of titles).
    #[test]
    fn modal_is_copy() {
        const fn assert_copy<T: Copy>() {}
        assert_copy::<Modal<'_>>();
    }

    #[test]
    fn centers_the_box_and_returns_the_inner_content_rect() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut grid = Grid::new(20, 10);
        let inner = Modal::new(10, 4).render(screen, &mut Surface::new(&mut grid, screen, 0));

        // Box is centered_rect(screen, 10, 4) = Rect::new(5, 3, 10, 4);
        // the inner content rect is inset by the one-cell border.
        assert_eq!(inner, Rect::new(6, 4, 8, 2));
        // The border was actually drawn at the box's corners.
        assert_eq!(grid[Pos::new(5, 3)].glyph(), '┌');
        assert_eq!(grid[Pos::new(14, 3)].glyph(), '┐');
    }

    #[test]
    fn padding_shrinks_the_returned_inner_rect() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut grid = Grid::new(20, 10);
        let inner = Modal::new(10, 4)
            .padding(Sides::symmetric(0, 1))
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        // Box is centered_rect(screen, 10, 4) = Rect::new(5, 3, 10, 4); the border inset is
        // Rect::new(6, 4, 8, 2), and symmetric(0, 1) padding trims 1 more cell off each side.
        assert_eq!(inner, Rect::new(7, 4, 6, 2));
    }

    #[test]
    fn draws_only_the_box_leaving_the_rest_of_the_screen_untouched() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut grid = Grid::new(20, 10);
        Modal::new(10, 4).render(screen, &mut Surface::new(&mut grid, screen, 0));

        // A corner of the screen far from the centered box is untouched.
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_border_and_fill() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut grid = Grid::new(20, 10);
        Modal::new(10, 4)
            .theme(Theme::DARK)
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        // Box is centered_rect(screen, 10, 4) = Rect::new(5, 3, 10, 4).
        assert_eq!(
            grid[Pos::new(5, 3)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(
            grid[Pos::new(5, 3)].style().background(),
            Theme::DARK.title_bg
        );
        assert_eq!(
            grid[Pos::new(6, 4)].style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut grid = Grid::new(20, 10);
        Modal::new(10, 4)
            .theme_on(Theme::DARK, Color::Default)
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        assert_eq!(
            grid[Pos::new(5, 3)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(
            grid[Pos::new(5, 3)].style().background(),
            Theme::DARK.title_bg
        );
        assert_eq!(grid[Pos::new(6, 4)].style().background(), Color::Default);
    }

    #[test]
    fn titles_draws_into_the_bottom_border_row() {
        let screen = Rect::new(0, 0, 12, 8);
        let mut grid = Grid::new(12, 8);
        Modal::new(12, 4)
            .titles(&[PanelTitle::new(
                "hint",
                TitlePosition::Bottom,
                Align::Center,
            )])
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        // Box is centered_rect(screen, 12, 4) = Rect::new(0, 2, 12, 4); bottom border row is y=5.
        let bottom_row: String = (0..12).map(|x| grid[Pos::new(x, 5)].glyph()).collect();
        assert!(bottom_row.contains("hint"));
    }

    #[test]
    fn title_and_titles_coexist_on_different_edges() {
        let screen = Rect::new(0, 0, 20, 8);
        let mut grid = Grid::new(20, 8);
        Modal::new(20, 4)
            .title("Confirm")
            .titles(&[PanelTitle::new(
                "3 / 10",
                TitlePosition::Bottom,
                Align::Right,
            )])
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        // Box is centered_rect(screen, 20, 4) = Rect::new(0, 2, 20, 4); rows 2 (top) and 5 (bottom).
        let top_row: String = (0..20).map(|x| grid[Pos::new(x, 2)].glyph()).collect();
        let bottom_row: String = (0..20).map(|x| grid[Pos::new(x, 5)].glyph()).collect();
        assert!(top_row.contains("Confirm"));
        assert!(bottom_row.contains("3 / 10"));
    }

    /// A later `.titles(...)` call replaces the slice rather than appending to it (see the
    /// method's own doc comment): this is what keeps [`Modal`] [`Copy`] instead of growing a
    /// `Vec` like [`Panel::add_title`].
    #[test]
    fn a_later_titles_call_replaces_the_earlier_one() {
        let screen = Rect::new(0, 0, 12, 8);
        let mut grid = Grid::new(12, 8);
        Modal::new(12, 4)
            .titles(&[PanelTitle::new(
                "first",
                TitlePosition::Bottom,
                Align::Center,
            )])
            .titles(&[PanelTitle::new(
                "second",
                TitlePosition::Bottom,
                Align::Center,
            )])
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        let bottom_row: String = (0..12).map(|x| grid[Pos::new(x, 5)].glyph()).collect();
        assert!(bottom_row.contains("second"));
        assert!(!bottom_row.contains("first"));
    }

    #[test]
    fn border_type_selects_the_glyph_set() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut grid = Grid::new(20, 10);
        Modal::new(10, 4)
            .border_type(BorderType::Thick)
            .render(screen, &mut Surface::new(&mut grid, screen, 0));

        // Box is centered_rect(screen, 10, 4) = Rect::new(5, 3, 10, 4).
        assert_eq!(grid[Pos::new(5, 3)].glyph(), '┏');
        assert_eq!(grid[Pos::new(14, 3)].glyph(), '┓');
    }
}
