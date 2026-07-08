//! [`BoxBorder`]: a single-line box border.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::Widget;
use crate::draw::{BL, BR, H, TL, TR, V};

/// A single-line box border drawn around a [`Rect`].
///
/// The interior of the rectangle is not touched. `area` must be at least
/// 2×2, or [`Widget::render`] is a no-op. `style` defaults to
/// [`Style::new()`]; set it with [`BoxBorder::style`].
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
}

impl<B: Backend> Widget<B> for BoxBorder {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() < 2 || area.height() < 2 {
            return;
        }

        let x0 = area.left();
        let y0 = area.top();
        let x1 = area.right().saturating_sub(1);
        let y1 = area.bottom().saturating_sub(1);

        term.reset_style()
            .fg(self.style.foreground())
            .bg(self.style.background())
            .modifier(self.style.modifiers());

        // Corners
        term.put(x0, y0, TL);
        term.put(x1, y0, TR);
        term.put(x0, y1, BL);
        term.put(x1, y1, BR);

        // Horizontal edges
        for x in (x0 + 1)..x1 {
            term.put(x, y0, H);
            term.put(x, y1, H);
        }

        // Vertical edges
        for y in (y0 + 1)..y1 {
            term.put(x0, y, V);
            term.put(x1, y, V);
        }

        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Headless};

    use super::*;

    #[test]
    fn draws_corners_and_edges() {
        let area = Rect::new(0, 0, 5, 3);
        let mut term = Terminal::new(Headless::new(5, 3));
        BoxBorder::new()
            .style(Style::new().fg(Color::WHITE))
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), TL);
        assert_eq!(term.grid().get(4, 0).glyph(), TR);
        assert_eq!(term.grid().get(0, 2).glyph(), BL);
        assert_eq!(term.grid().get(4, 2).glyph(), BR);
        assert_eq!(term.grid().get(2, 0).glyph(), H);
        assert_eq!(term.grid().get(0, 1).glyph(), V);
        // Interior untouched.
        assert_eq!(term.grid().get(2, 1).glyph(), ' ');
    }

    #[test]
    fn too_small_is_a_no_op() {
        let area = Rect::new(0, 0, 1, 1);
        let mut term = Terminal::new(Headless::new(1, 1));
        BoxBorder::new().render(area, &mut term);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }
}
