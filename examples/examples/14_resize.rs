//! 14: Resize
//!
//! [`Terminal::resize`] plus [`Event::Resize`] -- both real, working plumbing (the software
//! backend's window layer already computes new cell dimensions from an OS window resize and
//! pushes a real `Event::Resize`; crossterm maps the terminal's own resize the same way) that no
//! earlier example ever calls. Worse: every earlier example draws at hardcoded coordinates up to
//! `(49, 24)`, silently assuming the terminal is at least 50x25 -- `Terminal::new` actually sizes
//! the grid from `backend.size()`, which on crossterm is whatever the real terminal reports, not
//! a fixed 50x25. This example is the first to read `term.area()` fresh every frame instead of
//! assuming a size, and the first to react to `Event::Resize` by calling `term.resize()` and
//! redrawing to fit.
//!
//! `Terminal::present` is a cell-*diff* renderer: it only sends a backend the cells that
//! actually changed since last frame. `Terminal::resize` preserves content in the overlapping
//! region and only clears its own bookkeeping of what was previously sent (see both methods'
//! doc comments) -- so a cell this example never explicitly draws to is simply not part of the
//! diff, and a backend that itself preserves old content across its own resize (the in-memory
//! `Headless` backend does; a real terminal or window typically doesn't, since resizing clears
//! or replaces the physical surface) can keep showing whatever was there before, indefinitely.
//! Concretely: shrink this example, then grow it back past the original size, and a naive
//! hollow-border-only redraw leaves the *old*, smaller border's glyphs sitting in what is now
//! the middle of the frame, because nothing ever explicitly writes over them again. The fix
//! isn't a `Terminal` method -- it's that this example fills its *entire* current area every
//! frame (background first, then border and label on top) rather than only touching the
//! outline, so every cell's on-screen content is always explained by this frame's own draw call,
//! with no assumption about what a diff-based backend does or doesn't clear on its own.
//!
//! ```sh
//! cargo run --example 14_resize --features crossterm
//! cargo run --example 14_resize --features software
//! cargo run --example 14_resize  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Resize the terminal (crossterm) or window (software) to see it adapt live. `q`/`Escape` quits.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{AnsiColor, Backend, Color, Frame, Style, Terminal};
use retroglyph_examples::Example;

/// State for the resize example (none needed: every frame is drawn fresh from `term.area()`).
#[derive(Default)]
pub struct Resize;

impl Resize {
    /// Drains pending input. `Event::Resize` is captured rather than acted on immediately: it
    /// arrives mixed in with other events in the same drain, and `term.resize()` needs `&mut
    /// term` while [`Terminal::drain_events`]'s iterator still holds one -- so the requested
    /// size is recorded here and applied once the loop (and the borrow) ends.
    fn handle_events<B: Backend>(term: &mut Terminal<B>) -> bool {
        let mut requested_size = None;
        let mut quit = false;
        for event in term.drain_events() {
            match event {
                Event::Key(key) if key.is_down() => {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) {
                        quit = true;
                    }
                }
                Event::Close => quit = true,
                Event::Resize(width, height) => requested_size = Some((width, height)),
                _ => {}
            }
        }
        if let Some((width, height)) = requested_size {
            term.resize(width, height);
        }
        !quit
    }

    /// Draws a border and a centered size readout over the terminal's *current* area --
    /// `term.area()`, not a remembered or hardcoded one -- so the whole frame is always correct
    /// for whatever size just got applied above.
    fn draw<B: Backend>(term: &mut Terminal<B>) {
        let area = term.area();
        if area.width() == 0 || area.height() == 0 {
            term.present().ok();
            return;
        }

        // Fill every cell in the current area, not just the border outline -- see the module
        // doc comment for why a hollow-border-only redraw can leave stale glyphs behind after a
        // shrink-then-grow sequence.
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                term.put_styled(x, y, ' ', Style::default());
            }
        }

        let border = Style::new().fg(Color::Ansi(AnsiColor::BrightBlack));
        for x in area.left()..area.right() {
            term.put_styled(x, area.top(), '#', border);
            term.put_styled(x, area.bottom() - 1, '#', border);
        }
        for y in area.top()..area.bottom() {
            term.put_styled(area.left(), y, '#', border);
            term.put_styled(area.right() - 1, y, '#', border);
        }

        let label = format!("{}x{} cells -- resize me", area.width(), area.height());
        #[allow(clippy::cast_possible_truncation)]
        let label_width = label.chars().count() as u16;
        if label_width < area.width() {
            let x = area.left() + (area.width() - label_width) / 2;
            let y = area.top() + area.height() / 2;
            term.reset_style();
            term.print(x, y, &label);
        }

        term.present().ok();
    }
}

impl Example for Resize {
    const NAME: &'static str = "14_resize";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        if !Self::handle_events(term) {
            return false;
        }
        Self::draw(term);
        true
    }
}

retroglyph_examples::example_main!(Resize);
