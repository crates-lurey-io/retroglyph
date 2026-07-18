//! [`Modal`]: a bordered, filled box centered on screen.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{Panel, Widget};
use crate::Theme;
use crate::layout::centered_rect;

/// A bordered, filled box centered in a screen [`Rect`].
///
/// Shorthand for a [`Panel`] sized `width` x `height` and centered via
/// [`centered_rect`]. `border_style`/`fill_style` default to
/// [`Style::new()`] and there is no title by default -- set whichever a
/// caller needs via [`Modal::border_style`]/[`Modal::fill_style`]/[`Modal::title`],
/// the same as [`Panel`].
///
/// [`Modal::render`] returns the inner content [`Rect`] (inside the
/// border, the same implicit one-cell inset [`Panel`] uses for its own
/// interior) ready to hand to another widget (e.g. [`super::Log`]).
///
/// Draws only the box itself; everything outside it is left untouched (no
/// dimming or backdrop fill -- that would need to read and blend existing
/// cells, a separate feature from this thin layout convenience). Not a
/// [`Widget`]: [`Widget::render`] can't return a value, and the inner
/// content rect is part of this type's contract.
#[derive(Clone, Copy, Debug)]
pub struct Modal<'a> {
    width: u16,
    height: u16,
    title: Option<&'a str>,
    border_style: Style,
    fill_style: Style,
}

impl<'a> Modal<'a> {
    /// A `width` x `height` modal in the default style, with no title.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            title: None,
            border_style: Style::new(),
            fill_style: Style::new(),
        }
    }

    /// Set the modal's title.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
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

    /// Applies `theme`'s named roles to this modal's border and fill, the same mapping as
    /// [`Panel::theme`] (a [`Modal`] is just a centered [`Panel`]): `border_style` becomes
    /// `theme.border` on `theme.title_bg`, and `fill_style` becomes `theme.panel_bg`.
    ///
    /// Call before any manual [`Modal::border_style`]/[`Modal::fill_style`] override you want to
    /// keep -- whichever call comes last wins.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.border_style = Style::new().fg(theme.border).bg(theme.title_bg);
        self.fill_style = Style::new().bg(theme.panel_bg);
        self
    }

    /// Draw the modal centered in `screen`, returning its inner content
    /// [`Rect`].
    pub fn render<B: Backend>(self, screen: Rect, term: &mut Terminal<B>) -> Rect {
        let rect = centered_rect(screen, self.width, self.height);
        let mut panel = Panel::new()
            .border_style(self.border_style)
            .fill_style(self.fill_style);
        if let Some(title) = self.title {
            panel = panel.title(title);
        }
        panel.render(rect, term);
        Rect::new(
            rect.left() + 1,
            rect.top() + 1,
            rect.width().saturating_sub(2),
            rect.height().saturating_sub(2),
        )
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn centers_the_box_and_returns_the_inner_content_rect() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut term = Terminal::new(Headless::new(20, 10));
        let inner = Modal::new(10, 4).render(screen, &mut term);

        // Box is centered_rect(screen, 10, 4) = Rect::new(5, 3, 10, 4);
        // the inner content rect is inset by the one-cell border.
        assert_eq!(inner, Rect::new(6, 4, 8, 2));
        // The border was actually drawn at the box's corners.
        assert_eq!(term.grid().get(5, 3).glyph(), '┌');
        assert_eq!(term.grid().get(14, 3).glyph(), '┐');
    }

    #[test]
    fn draws_only_the_box_leaving_the_rest_of_the_screen_untouched() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut term = Terminal::new(Headless::new(20, 10));
        Modal::new(10, 4).render(screen, &mut term);

        // A corner of the screen far from the centered box is untouched.
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_border_and_fill() {
        let screen = Rect::new(0, 0, 20, 10);
        let mut term = Terminal::new(Headless::new(20, 10));
        Modal::new(10, 4)
            .theme(Theme::DARK)
            .render(screen, &mut term);

        // Box is centered_rect(screen, 10, 4) = Rect::new(5, 3, 10, 4).
        assert_eq!(
            term.grid().get(5, 3).style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(
            term.grid().get(5, 3).style().background(),
            Theme::DARK.title_bg
        );
        assert_eq!(
            term.grid().get(6, 4).style().background(),
            Theme::DARK.panel_bg
        );
    }
}
