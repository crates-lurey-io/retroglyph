//! 18: Text alignment
//!
//! Proves the shared [`Align`] builder knob (`Left`/`Center`/`Right`) that
//! [`Text`], [`PrintLine`], [`Panel`] titles, and [`Modal`] titles all now
//! carry. [`Text`] and [`PrintLine`] default to [`Align::Left`] (their
//! long-standing behavior); [`Panel`]/[`Modal`] titles default to
//! [`Align::Center`] (theirs). Each of the three stacked panels below aligns
//! its own title one way and aligns a [`Text`] readout and a multi-span
//! [`PrintLine`] the same way inside it, so the knob's effect is visible on
//! all three widget types at once; the [`Modal`] at the bottom right-aligns
//! its title, the one alignment panel titles could never take before. The
//! bottom panel also shows the motivating case from the issue: a left-aligned
//! label with a right-aligned readout on the same interior row.
//!
//! ```sh
//! cargo run --example 18_text_align --features crossterm
//! cargo run --example 18_text_align --features software
//! cargo run --example 18_text_align  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Static display; press `q` or `Escape` to quit, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::text::{Line, Span};
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{Align, Modal, Panel, PrintLine, Text, Widget};

/// State for the text-alignment example (none needed: the layout never changes).
#[derive(Default)]
pub struct TextAlign;

impl TextAlign {
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

    /// Draws a titled panel whose title, a [`Text`] readout, and a two-span
    /// [`PrintLine`] are all aligned by `align`, labelled `name`.
    fn draw_panel<B: Backend>(term: &mut Terminal<B>, area: Rect, name: &str, align: Align) {
        let border = Style::new().fg(Color::WHITE);
        Panel::new()
            .title(name)
            .title_align(align)
            .border_style(border)
            .render(area, term);

        // Interior: one cell in from the border on every side.
        let inner = Rect::new(area.left() + 1, area.top() + 1, area.width() - 2, 1);
        Text::new("Text widget")
            .style(Style::new().fg(Color::CYAN))
            .align(align)
            .render(inner, term);

        let line = Line::from(vec![
            Span::styled("PrintLine ", Style::new().fg(Color::YELLOW)),
            Span::styled("spans", Style::new().fg(Color::GREEN)),
        ]);
        let below = Rect::new(inner.left(), inner.top() + 1, inner.width(), 1);
        PrintLine::new(&line).align(align).render(below, term);
    }

    /// Draws this frame and presents it. `&self` (unused) is the shape a real
    /// example's draw step needs; this one has no state to read.
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.reset_style().fg(Color::WHITE);
        term.print(1, 0, "Align: Left / Center / Right on titles and text");
        term.reset_style();

        Self::draw_panel(term, Rect::new(0, 1, 50, 5), "Left", Align::Left);
        Self::draw_panel(term, Rect::new(0, 6, 50, 5), "Center", Align::Center);
        Self::draw_panel(term, Rect::new(0, 11, 50, 5), "Right", Align::Right);

        // A right-aligned title -- the alignment panel titles could never take
        // before -- plus the motivating case: a left-aligned label alongside a
        // right-aligned readout on one interior row.
        let modal_region = Rect::new(0, 16, 50, 9);
        let inner = Modal::new(46, 7)
            .title("Modal: right title")
            .title_align(Align::Right)
            .border_style(Style::new().fg(Color::MAGENTA))
            .render(modal_region, term);

        let row = Rect::new(inner.left(), inner.top() + 1, inner.width(), 1);
        Text::new("Left-aligned label")
            .style(Style::new().fg(Color::WHITE))
            .align(Align::Left)
            .render(row, term);
        Text::new("42 / 100")
            .style(Style::new().fg(Color::GREEN))
            .align(Align::Right)
            .render(row, term);

        term.present().ok();
    }
}

impl Example for TextAlign {
    const NAME: &'static str = "18_text_align";

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

retroglyph_examples::example_main!(TextAlign);
