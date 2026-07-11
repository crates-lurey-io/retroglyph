//! 02: Colors
//!
//! retroglyph's whole styling vocabulary: [`Style`] has exactly two knobs, `fg`
//! and `bg`, both [`Color`]. This example lays out every `Color` variant --
//! `Ansi` (16), `Indexed` (a sampled strip of the 256-value palette), `Rgb`
//! (24-bit gradient), and `Default` -- plus inverse video (swapping fg/bg on
//! the same two colors), which is the only "styled text" effect retroglyph
//! has (no bold/italic/underline -- see [`Style`]'s doc comment for why).
//!
//! ```sh
//! cargo run --example 02_colors --features crossterm
//! cargo run --example 02_colors --features software
//! cargo run --example 02_colors  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Press `q` or `Escape` to quit on the interactive backends, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::text::{Line, Span};
use retroglyph_core::{AnsiColor, Backend, Color, Style, Terminal};
use retroglyph_examples::Example;

/// State for the colors example (none needed: the palette layout never changes).
#[derive(Default)]
pub struct Colors;

impl Colors {
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

    /// Draws one row of `count` one-cell swatches starting at `(x, y)`, each colored by `color_at`.
    ///
    /// `color_at` takes the swatch index as a `u8` (never more than 32 swatches wide on a 50-column
    /// grid), so every color computation below stays in `u8` without a truncating cast.
    fn swatch_row<B: Backend>(
        term: &mut Terminal<B>,
        x: u16,
        y: u16,
        count: u8,
        color_at: impl Fn(u8) -> Color,
    ) {
        for i in 0..count {
            let style = Style::new().bg(color_at(i));
            term.put_styled(x + u16::from(i), y, ' ', style);
        }
    }

    /// Draws this frame and presents it.
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(1, 1, "Ansi (16 standard colors):");
        Self::swatch_row(term, 1, 2, 16, |i| {
            Color::Ansi(AnsiColor::try_from(i).expect("0..16 is a valid AnsiColor index"))
        });

        term.print(1, 4, "Indexed (sampled across 0..256):");
        // 32 swatches sampled evenly across the 256-value palette (every 8th index):
        // the full palette doesn't fit a 50-column grid, and a representative strip
        // is enough to prove the Indexed(u8) mapping is stable per backend.
        Self::swatch_row(term, 1, 5, 32, |i| Color::Indexed(i * 8));

        term.print(1, 7, "Rgb (24-bit gradient, red channel 0..255):");
        Self::swatch_row(term, 1, 8, 32, |i| Color::Rgb {
            // `u32` intermediate (`i * 255` up to 31 * 255 = 7905 doesn't fit `u8`), then
            // `try_from` back down: the `/ 31` bounds the result to 0..=255, so this never fails.
            r: u8::try_from(u32::from(i) * 255 / 31).expect("0..=31 * 255 / 31 fits in u8"),
            g: 64,
            b: 192,
        });

        term.print(1, 10, "Default (backend's configured fg/bg):");
        Self::swatch_row(term, 1, 11, 1, |_| Color::Default);

        term.print(
            1,
            13,
            "Inverse video (fg/bg swap is the only \"styled text\" retroglyph has):",
        );
        let fg = Color::Ansi(AnsiColor::BrightYellow);
        let bg = Color::Ansi(AnsiColor::Blue);
        term.print_styled(
            1,
            14,
            &Line::from(Span::styled(
                "normal: yellow on blue",
                Style::new().fg(fg).bg(bg),
            )),
        );
        term.print_styled(
            1,
            15,
            &Line::from(Span::styled(
                "inverse: blue on yellow",
                Style::new().fg(bg).bg(fg),
            )),
        );

        term.present().ok();
    }
}

impl Example for Colors {
    const NAME: &'static str = "02_colors";

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

retroglyph_examples::example_main!(Colors);
