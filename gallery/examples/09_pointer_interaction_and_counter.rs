//! 09: Pointer interaction & a counter -- `Interaction`, `Sense`, `Response`
//!
//! 08's `FocusRing` answered "which widget has keyboard focus." This example's new concept is
//! [`Interaction`]: the piece that answers the mouse-side sibling questions too -- "is the
//! pointer over this widget," "did it just get clicked" -- and ties both together with one call
//! per widget instead of hand-rolling hit-testing.
//!
//! A `-`/`+` counter, floored at 0. Each button is registered once per frame via
//! [`Interaction::interact`] with [`Sense::click()`]:
//!
//! - [`Response::clicked`] fires from a full mouse press-and-release landing on the button --
//!   *and* from Enter/Space while the button is focused, for free. [`Sense::click()`] implies
//!   [`Sense::FOCUSABLE`], and [`Sense::FOCUSABLE`]'s doc comment spells out why: "terminals are
//!   frequently mouse-less." No separate keyboard code path was written for the buttons below --
//!   `Tab`/`Shift+Tab` to focus a button, then `Enter` or `Space` to press it, exercises the same
//!   `clicked()` a mouse click does.
//! - [`Response::hovered`]/[`Response::focused`] pick the border color, the same
//!   `is_focused`-style read 08 did, just sourced from `Interaction` instead of a bare
//!   `FocusRing`.
//!
//! ```sh
//! cargo run --example 09_pointer_interaction_and_counter                          # Headless (prints a few frames)
//! cargo run --example 09_pointer_interaction_and_counter --features crossterm     # Terminal
//! cargo run --example 09_pointer_interaction_and_counter --features default-font  # Desktop window
//! cargo run --example 09_pointer_interaction_and_counter --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Click, or Tab + Enter/Space, to adjust the counter. q (Terminal/Desktop) to quit.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Color, Flow, Frame, Style, Terminal};
use retroglyph_gallery::{pressed_key, rg_gallery_run};
use retroglyph_widgets::widget::{BoxBorder, Widget};
use retroglyph_widgets::{Interaction, Response, Sense};

/// Which of the two buttons an [`Interaction`] call is about -- the `Id` type parameter
/// `Interaction<Id>` asks every app to supply, per its own doc comment on why (a small `Copy`
/// enum instead of an opaque hash).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonId {
    Decrement,
    Increment,
}

struct PointerInteractionAndCounter {
    interaction: Interaction<ButtonId>,
    count: u32,
}

impl<B: Backend> App<B> for PointerInteractionAndCounter {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        // Resolve last frame's registrations/input, same begin_frame/handle_event/interact/
        // end_frame shape as FocusRing in 08 -- see Interaction's own doc comment for why each
        // step has to happen where it does.
        self.interaction.begin_frame();

        let mut exit = false;
        for event in term.drain_events() {
            self.interaction.handle_event(&event);
            if event == Event::Close {
                exit = true;
                continue;
            }
            if pressed_key(event) == Some(KeyCode::Char('q')) {
                exit = true;
            }
        }

        term.print(0, 0, "09: Pointer Interaction & Counter");
        term.print(0, 2, "Click, or Tab + Enter/Space, to adjust    q: quit");

        let dec_rect = Rect::new(0, 4, 7, 3);
        let count_rect = Rect::new(8, 4, 12, 3);
        let inc_rect = Rect::new(21, 4, 7, 3);

        let dec = self
            .interaction
            .interact(dec_rect, ButtonId::Decrement, Sense::click());
        draw_button(term, dec_rect, "-", dec);
        if dec.clicked() {
            self.count = self.count.saturating_sub(1); // floored at 0
        }

        let inc = self
            .interaction
            .interact(inc_rect, ButtonId::Increment, Sense::click());
        draw_button(term, inc_rect, "+", inc);
        if inc.clicked() {
            self.count += 1;
        }

        BoxBorder::new().render(count_rect, term);
        let label = self.count.to_string();
        let inner = count_rect.shrink(1, 1);
        // Left-aligned with a one-cell pad, not centered: centering would shift the digits
        // sideways every time the count grows or shrinks a digit (e.g. "9" -> "10"), which reads
        // as jitter rather than the number just changing.
        term.print(inner.left() + 1, inner.top(), &label);

        self.interaction.end_frame();

        term.present().expect("present failed");

        if exit { Flow::Exit } else { Flow::Continue }
    }
}

/// Draws one button: a border colored by hover/focus state, plus its centered label.
fn draw_button<B: Backend>(term: &mut Terminal<B>, rect: Rect, label: &str, response: Response) {
    let border = if response.focused() {
        // Cyan for focus, matching 08's convention.
        BoxBorder::new().style(Style::new().fg(Color::CYAN))
    } else if response.hovered() {
        BoxBorder::new().style(Style::new().fg(Color::YELLOW))
    } else {
        BoxBorder::new()
    };
    border.render(rect, term);

    let inner = rect.shrink(1, 1);
    term.print(
        inner.left() + center_offset(inner.width(), label),
        inner.top(),
        label,
    );
}

/// Horizontal offset (from `available`'s left edge) that centers `label` within it.
fn center_offset(available: u16, label: &str) -> u16 {
    let len = u16::try_from(label.len()).unwrap_or(u16::MAX);
    available.saturating_sub(len) / 2
}

rg_gallery_run!(
    PointerInteractionAndCounter {
        interaction: Interaction::new(),
        count: 0,
    },
    "09: Pointer Interaction & Counter",
    40,
    10
);
