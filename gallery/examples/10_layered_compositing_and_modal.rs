//! 10: Layered compositing -- `Panel`, `Modal`, draw-order z-ordering
//!
//! Every example so far has drawn exactly one layer: whatever fit on screen this frame. This
//! example's new concept is that "layering" in retroglyph isn't a separate compositing API at
//! all -- it's just draw order. Draw the background, then draw a [`Modal`] on top of it: the
//! modal's cells simply overwrite whatever was already there, no different from two `term.print`
//! calls landing on the same cell. [`Modal`] (a centered, bordered [`Panel`]) is a layout
//! convenience over that -- it deliberately draws only its own box and leaves everything outside
//! it untouched (no dimming/backdrop fill), see its own doc comment for why.
//!
//! The background is a repeating filler pattern standing in for "the app," so the modal visibly
//! punches a hole in it rather than covering an already-blank screen. Pressing `q` -- or clicking
//! the window's close button -- doesn't quit immediately: both open a "Quit?" confirm [`Modal`]
//! instead, since a real close button shouldn't bypass a confirmation any more than a keyboard
//! shortcut should. While the modal is open, input goes *only* to it -- `Y`/`Enter` confirms,
//! `N`/`Esc` cancels -- and the background's own key handling (there isn't any here, but a real
//! app's would be) is skipped entirely, the way a real modal captures input until it's dismissed.
//!
//! ```sh
//! cargo run --example 10_layered_compositing_and_modal                          # Headless (prints a few frames)
//! cargo run --example 10_layered_compositing_and_modal --features crossterm     # Terminal
//! cargo run --example 10_layered_compositing_and_modal --features default-font  # Desktop window
//! cargo run --example 10_layered_compositing_and_modal --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! q, or the window's close button, opens the quit confirmation; Y/Enter confirms, N/Esc cancels.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Color, Flow, Frame, Style, Terminal};
use retroglyph_gallery::{pressed_key, rg_gallery_run};
use retroglyph_widgets::widget::Modal;

/// Which layer currently owns input: the background, or the confirm modal on top of it.
enum Screen {
    Background,
    ConfirmQuit,
}

struct LayeredCompositingAndModal {
    screen: Screen,
}

impl<B: Backend> App<B> for LayeredCompositingAndModal {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        let mut exit = false;
        for event in term.drain_events() {
            // The window's close button is just another way of asking to quit -- it goes
            // through the same confirm gate `q` does, not a bypass. Whichever screen is current
            // owns the whole input stream this frame; no event is ever handled by both. That's
            // what "the modal captures input" means in practice: there's no shared dispatch step
            // choosing who gets a look at each event first.
            let wants_to_quit =
                event == Event::Close || pressed_key(event) == Some(KeyCode::Char('q'));
            match self.screen {
                Screen::Background => {
                    if wants_to_quit {
                        self.screen = Screen::ConfirmQuit;
                    }
                }
                Screen::ConfirmQuit => match pressed_key(event) {
                    Some(KeyCode::Char('y' | 'Y') | KeyCode::Enter) => exit = true,
                    Some(KeyCode::Char('n' | 'N') | KeyCode::Escape) => {
                        self.screen = Screen::Background;
                    }
                    // A second Close/`q` while the confirmation is already up doesn't need
                    // special-casing: the dialog's already asking, so this is a no-op, the same
                    // as any other key the confirm screen doesn't recognize.
                    _ => {}
                },
            }
        }

        draw_background(term);
        if matches!(self.screen, Screen::ConfirmQuit) {
            draw_confirm_quit(term);
        }

        term.present().expect("present failed");

        if exit { Flow::Exit } else { Flow::Continue }
    }
}

/// A repeating filler pattern across the whole screen, standing in for "the app" -- content the
/// modal will visibly draw over, rather than a blank screen where covering it would go unnoticed.
fn draw_background<B: Backend>(term: &mut Terminal<B>) {
    term.print(0, 0, "10: Layered Compositing & Modal");
    term.print(0, 2, "q: quit");

    let (cols, rows) = (term.grid().width(), term.grid().height());
    for y in 4..rows {
        let mut line = String::new();
        for x in 0..cols {
            line.push(if (x + y) % 8 < 4 { '.' } else { ':' });
        }
        term.print(0, y, &line);
    }
}

/// The "Quit?" confirmation, drawn on top of whatever `draw_background` already put on screen.
fn draw_confirm_quit<B: Backend>(term: &mut Terminal<B>) {
    let (cols, rows) = (term.grid().width(), term.grid().height());
    let screen = Rect::new(0, 0, cols, rows);
    let inner = Modal::new(30, 5)
        .title("Quit?")
        .border_style(Style::new().fg(Color::YELLOW))
        .render(screen, term);

    // Modal(30, 5)'s inner content rect is 3 rows tall -- row 0 and row 2, with a blank row 1 as
    // a gap, not row 3, which would overflow onto the modal's own bottom border.
    term.print(inner.left() + 1, inner.top(), "Quit the example?");
    term.print(inner.left() + 1, inner.top() + 2, "[Y]es    [N]o");
}

rg_gallery_run!(
    LayeredCompositingAndModal {
        screen: Screen::Background,
    },
    "10: Layered Compositing & Modal",
    60,
    18
);
