//! 06: Widgets & spaced layout -- `retroglyph-widgets`, the `Widget` trait, `BoxBorder`, `split_h_spaced`
//!
//! 01-05 stayed entirely on `retroglyph-core` -- every border in 04 was hand-drawn with `put`
//! calls, and every box's `Rect` was a hand-picked `Rect::new(x, y, w, h)` literal. This example
//! pulls in the first new crate, `retroglyph-widgets`, and two things from it:
//!
//! - [`Widget`]: the trait every widget in that crate implements. A `Widget` is a builder struct
//!   (here, [`BoxBorder`]) that owns no state of its own and knows how to draw itself once you
//!   give it a `Rect` and a `Terminal`: `widget.render(area, term)`. No retained tree, no
//!   diffing -- calling `render` again next frame just redraws it, the same immediate-mode shape
//!   every example so far has used for `term.put`.
//! - [`split_h_spaced`]: splits one `Rect` into N side-by-side panes by [`Constraint`], with a
//!   fixed gap between each. It's not `retroglyph_core::layout::TextLayout` from
//!   `04_text_layout_and_wrap.rs` -- that wraps/aligns *text inside* one `Rect`; this one carves
//!   *one `Rect` into several*. `retroglyph_widgets::layout` is a different layer entirely: three
//!   equal `Constraint::Fill` columns with 1-cell gaps replace three hand-picked `Rect::new`
//!   literals, so resizing the row to a different width doesn't mean re-deriving `x` for every
//!   box by hand.
//!
//! The three boxes below draw the exact same borders as `04_text_layout_and_wrap.rs`'s
//! `draw_border` helper, using [`BoxBorder`] instead -- compare the two files to see what a
//! widget buys you over hand-rolled `put` calls: no corner/edge math to get wrong, and a
//! `.style()` builder method instead of remembering to call `reset_style` yourself.
//!
//! ```sh
//! cargo run --example 06_widgets_and_spaced_layout                          # Headless (prints a few frames)
//! cargo run --example 06_widgets_and_spaced_layout --features crossterm     # Terminal
//! cargo run --example 06_widgets_and_spaced_layout --features default-font  # Desktop window
//! cargo run --example 06_widgets_and_spaced_layout --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press any key (Terminal/Desktop) to quit.

use retroglyph_core::grid::Rect;
use retroglyph_core::layout::{HAlign, TextLayout, VAlign};
use retroglyph_core::text::Line;
use retroglyph_core::{App, Backend, Color, Flow, Frame, Style, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};
use retroglyph_widgets::widget::{BoxBorder, Widget};
use retroglyph_widgets::{Constraint, split_h_spaced};

struct WidgetsAndSpacedLayout;

impl<B: Backend> App<B> for WidgetsAndSpacedLayout {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.print(0, 0, "06: Widgets & Spaced Layout");

        // Three equal columns with a 1-cell gap between each -- split_h_spaced resolves the
        // Constraint::Fill triplet against the row's actual width and hands back panes with the
        // gaps already carved out, instead of three hand-picked Rect::new(x, 2, 19, 6) literals.
        let row = Rect::new(0, 2, 59, 6);
        let cols = split_h_spaced(
            row,
            &[Constraint::Fill, Constraint::Fill, Constraint::Fill],
            1,
        );
        let borders = [
            BoxBorder::new(),
            BoxBorder::new().style(Style::new().fg(Color::CYAN)),
            BoxBorder::new().style(Style::new().fg(Color::YELLOW)),
        ];
        let labels = ["Plain", "Styled", "Also styled"];

        for ((rect, border), label) in cols.into_iter().zip(borders).zip(labels) {
            // `render` consumes `border` -- a Widget is a one-shot builder, not something you
            // hold onto and reuse across frames.
            border.render(rect, term);

            let line = Line::raw(label);
            TextLayout::new(&line)
                // Rect::shrink(1, 1) -- one cell in on every edge -- is Rect's own API for this.
                .rect(rect.shrink(1, 1))
                .h_align(HAlign::Center)
                .v_align(VAlign::Middle)
                .render(term);
        }

        term.present().expect("present failed");

        if any_key_pressed_or_window_closed(term) {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

rg_gallery_run!(WidgetsAndSpacedLayout, "06: Widgets & Spaced Layout", 60, 9);
