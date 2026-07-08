//! 03: Text & lines -- `Line`/`Span` composition and `print_styled`
//!
//! Shows off [`Line`]/[`Span`] composition and `Terminal::print_styled`. 02 taught per-cell
//! `put_styled`; 03's new concept is composing a `Line` out of independently-styled `Span`s (a
//! mix of `Span::raw` and `Span::styled`) and drawing the whole thing in one call, instead of
//! styling cell-by-cell.
//!
//! ```sh
//! cargo run --example 03_text_and_lines                          # Headless (prints a few frames)
//! cargo run --example 03_text_and_lines --features crossterm     # Terminal
//! cargo run --example 03_text_and_lines --features default-font  # Desktop window
//! cargo run --example 03_text_and_lines --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press any key (Terminal/Desktop) to quit.

use retroglyph_core::text::{Line, Span};
use retroglyph_core::{App, Backend, Color, Flow, Frame, Style, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};

struct TextAndLines;

impl<B: Backend> App<B> for TextAndLines {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.print(0, 0, "03: Text & Lines");

        // `term.print` -- one uniform style for the whole string, same as 01/02's title above.
        term.print(0, 2, "print(): one uniform style");

        // A `Line` mixing `Span::raw` (unstyled labels) with `Span::styled` (colored values) --
        // drawn in a single `print_styled` call instead of one `put_styled` per cell.
        let hud = Line::from(vec![
            Span::raw("HP: "),
            Span::styled("100", Style::new().fg(Color::GREEN)),
            Span::raw("   MP: "),
            Span::styled("50", Style::new().fg(Color::BLUE)),
        ]);
        term.print_styled(0, 3, &hud);

        // `Line::width()` counts display columns, not `char`s -- wide glyphs (CJK, emoji) occupy
        // two columns each, so `width()` and a plain character count diverge.
        term.print(0, 5, "Line::width() counts columns:");
        let wide = Line::raw("幅広");
        term.print_styled(0, 6, &wide);
        term.print(
            5,
            6,
            &format!(
                "width()={} chars()={}",
                wide.width(),
                "幅広".chars().count()
            ),
        );

        term.present().expect("present failed");

        if any_key_pressed_or_window_closed(term) {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

rg_gallery_run!(TextAndLines, "03: Text & Lines", 40, 8);
