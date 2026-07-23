//! [`Scrollbar`]: a vertical track+thumb indicator.
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::Widget;
use crate::Theme;
use crate::draw::thumb_geometry;

/// A vertical scrollbar (typically one cell wide) covering `total_len`
/// items in a `visible_len`-row viewport.
///
/// `offset` defaults to `0`; `track_style`/`thumb_style` default to
/// [`Style::new()`]. Set whichever a caller needs via
/// [`Scrollbar::offset`]/[`Scrollbar::track_style`]/[`Scrollbar::thumb_style`].
///
/// `track_style` fills the whole strip, then [`crate::draw::thumb_geometry`]'s
/// span (if any) is redrawn with `thumb_style` on top. Draws just the plain
/// track, with no thumb, if there's nothing to scroll -- see
/// [`crate::draw::thumb_geometry`].
///
/// Deliberately independent of [`crate::interact`] -- see
/// [`crate::draw::thumb_geometry`] and
/// [`crate::draw::offset_for_pos`]'s own doc comments for how to make this
/// draggable using [`Interaction`](crate::Interaction) instead.
#[derive(Clone, Copy, Debug)]
pub struct Scrollbar {
    total_len: usize,
    visible_len: usize,
    offset: usize,
    track_style: Style,
    thumb_style: Style,
}

impl Scrollbar {
    /// A scrollbar covering `total_len` items in a `visible_len`-row
    /// viewport, starting at offset `0` in the default style.
    #[must_use]
    pub fn new(total_len: usize, visible_len: usize) -> Self {
        Self {
            total_len,
            visible_len,
            offset: 0,
            track_style: Style::new(),
            thumb_style: Style::new(),
        }
    }

    /// Set the scroll offset the thumb is drawn at.
    #[must_use]
    pub const fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Set the track's style.
    #[must_use]
    pub const fn track_style(mut self, style: Style) -> Self {
        self.track_style = style;
        self
    }

    /// Set the thumb's style.
    #[must_use]
    pub const fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// Applies `theme`'s named roles to this scrollbar: `track_style` becomes `theme.panel_bg`
    /// (the same surface the scrolled content sits on), and `thumb_style` becomes `theme.border`
    /// -- a subtle divider-like color rather than `theme.accent`, so a themed scrollbar doesn't
    /// compete with an actually-selected/focused control for attention.
    ///
    /// Call before any manual [`Scrollbar::track_style`]/[`Scrollbar::thumb_style`] override you
    /// want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Scrollbar::theme`], but `track_style` is drawn on `bg` instead of
    /// `theme.panel_bg` -- for a scrollbar drawn directly on a backdrop other than a themed
    /// [`super::Panel`]/[`super::Modal`]'s fill. [`Scrollbar::theme`] is exactly
    /// `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.track_style = Style::new().bg(bg);
        self.thumb_style = Style::new().bg(theme.border);
        self
    }
}

impl<B: Backend> Widget<B> for Scrollbar {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                term.put_styled(x, y, ' ', self.track_style);
            }
        }

        let Some((start, len)) =
            thumb_geometry(area, self.total_len, self.visible_len, self.offset)
        else {
            return;
        };
        for y in (area.top() + start)..(area.top() + start + len) {
            for x in area.left()..area.right() {
                term.put_styled(x, y, ' ', self.thumb_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Headless};

    use super::*;

    #[test]
    fn draws_a_plain_track_with_no_thumb_when_nothing_to_scroll() {
        let area = Rect::new(0, 0, 1, 5);
        let mut term = Terminal::new(Headless::new(1, 5));
        let track = Style::new().bg(Color::Rgb { r: 1, g: 1, b: 1 });
        let thumb = Style::new().bg(Color::Rgb { r: 2, g: 2, b: 2 });
        Scrollbar::new(3, 5)
            .track_style(track)
            .thumb_style(thumb)
            .render(area, &mut term);
        for y in 0..5 {
            assert_eq!(
                term.grid().get(0, y).style().background(),
                track.background()
            );
        }
    }

    #[test]
    fn draws_the_thumb_over_the_track() {
        let area = Rect::new(0, 0, 1, 10);
        let mut term = Terminal::new(Headless::new(1, 10));
        let track = Style::new().bg(Color::Rgb { r: 1, g: 1, b: 1 });
        let thumb = Style::new().bg(Color::Rgb { r: 2, g: 2, b: 2 });
        Scrollbar::new(20, 5)
            .offset(0)
            .track_style(track)
            .thumb_style(thumb)
            .render(area, &mut term);

        let (start, len) = thumb_geometry(area, 20, 5, 0).unwrap();
        for y in 0..10 {
            let bg = term.grid().get(0, y).style().background();
            if y >= start && y < start + len {
                assert_eq!(bg, thumb.background());
            } else {
                assert_eq!(bg, track.background());
            }
        }
    }

    #[test]
    fn theme_maps_named_roles_onto_track_and_thumb() {
        let area = Rect::new(0, 0, 1, 10);
        let mut term = Terminal::new(Headless::new(1, 10));
        Scrollbar::new(20, 5)
            .theme(Theme::DARK)
            .render(area, &mut term);

        let (start, len) = thumb_geometry(area, 20, 5, 0).unwrap();
        for y in 0..10 {
            let bg = term.grid().get(0, y).style().background();
            if y >= start && y < start + len {
                assert_eq!(bg, Theme::DARK.border);
            } else {
                assert_eq!(bg, Theme::DARK.panel_bg);
            }
        }
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 1, 10);
        let mut term = Terminal::new(Headless::new(1, 10));
        Scrollbar::new(20, 5)
            .theme_on(Theme::DARK, Color::Default)
            .render(area, &mut term);

        let (start, len) = thumb_geometry(area, 20, 5, 0).unwrap();
        for y in 0..10 {
            let bg = term.grid().get(0, y).style().background();
            if y >= start && y < start + len {
                assert_eq!(bg, Theme::DARK.border);
            } else {
                assert_eq!(bg, Color::Default);
            }
        }
    }

    #[test]
    fn offset_defaults_to_zero() {
        let area = Rect::new(0, 0, 1, 10);
        let mut term = Terminal::new(Headless::new(1, 10));
        let track = Style::new().bg(Color::Rgb { r: 1, g: 1, b: 1 });
        let thumb = Style::new().bg(Color::Rgb { r: 2, g: 2, b: 2 });
        Scrollbar::new(20, 5)
            .track_style(track)
            .thumb_style(thumb)
            .render(area, &mut term);

        let (start, _) = thumb_geometry(area, 20, 5, 0).unwrap();
        assert_eq!(start, 0);
    }
}
