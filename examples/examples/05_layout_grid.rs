//! 05: Layout grid
//!
//! Core [`Rect`] geometry: subdividing the 50x25 grid into panes by hand, with manual
//! arithmetic only. `retroglyph-widgets`'s `split_h`/`split_v`/`Constraint`/`Flex` would make
//! this shorter, and `09_widgets_dashboard` shows that path; this example is the plain `Rect`
//! math baseline instead. Each pane gets a box-drawn border and a centered label.
//!
//! ```sh
//! cargo run --example 05_layout_grid --features crossterm
//! cargo run --example 05_layout_grid --features software
//! cargo run --example 05_layout_grid  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `q` or `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Rect, Style, Surface, Terminal};
use retroglyph_examples::Example;

/// State for the layout example (none needed: the pane layout never changes).
#[derive(Default)]
pub struct LayoutGrid;

/// Splits `rect` into a left pane of `left_width` columns and a right pane filling the rest.
///
/// The manual arithmetic a real app writes before reaching for
/// `retroglyph-widgets`'s `split_h`: no library call, just `Rect::new` twice
/// with the second pane's `x`/`width` derived from the first.
const fn split_h(rect: Rect, left_width: u16) -> (Rect, Rect) {
    let left = Rect::new(rect.left(), rect.top(), left_width, rect.height());
    let right = Rect::new(
        rect.left() + left_width,
        rect.top(),
        rect.width() - left_width,
        rect.height(),
    );
    (left, right)
}

/// Splits `rect` into a top pane of `top_height` rows and a bottom pane filling the rest.
const fn split_v(rect: Rect, top_height: u16) -> (Rect, Rect) {
    let top = Rect::new(rect.left(), rect.top(), rect.width(), top_height);
    let bottom = Rect::new(
        rect.left(),
        rect.top() + top_height,
        rect.width(),
        rect.height() - top_height,
    );
    (top, bottom)
}

impl LayoutGrid {
    /// Drains pending input, returning `false` if the user asked to quit.
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) => {
                    return false;
                }
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws a box-drawing border around `rect`'s edges and a centered `label` inside it.
    fn draw_pane(surface: &mut Surface<'_>, rect: Rect, label: &str) {
        let (l, t, r, b) = (rect.left(), rect.top(), rect.right() - 1, rect.bottom() - 1);
        let style = Style::default();

        surface.put((l, t), '┌', style);
        surface.put((r, t), '┐', style);
        surface.put((l, b), '└', style);
        surface.put((r, b), '┘', style);
        for x in (l + 1)..r {
            surface.put((x, t), '─', style);
            surface.put((x, b), '─', style);
        }
        for y in (t + 1)..b {
            surface.put((l, y), '│', style);
            surface.put((r, y), '│', style);
        }

        let interior_width = rect.width().saturating_sub(2);
        let label_len = u16::try_from(label.len()).expect("pane labels are short ASCII strings");
        let label_x = l + 1 + interior_width.saturating_sub(label_len) / 2;
        let label_y = t + rect.height() / 2;
        surface.print((label_x, label_y), label, style);
    }

    /// Draws this frame (the driver presents).
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let mut surface = term.surface();
        let full = Rect::new(0, 0, 50, 25);

        // Top pane spans the full width; the remaining space splits into three
        // side-by-side panes below it -- an uneven layout (not just a 2x2 grid)
        // to show the split helpers composing.
        let (top, rest) = split_v(full, 8);
        let (left, rest) = split_h(rest, 16);
        let (middle, right) = split_h(rest, 17);

        Self::draw_pane(&mut surface, top, "Pane A");
        Self::draw_pane(&mut surface, left, "Pane B");
        Self::draw_pane(&mut surface, middle, "Pane C");
        Self::draw_pane(&mut surface, right, "Pane D");
    }
}

impl Example for LayoutGrid {
    const NAME: &'static str = "05_layout_grid";

    fn tick<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        _frame: &retroglyph_core::Frame,
    ) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(LayoutGrid);
