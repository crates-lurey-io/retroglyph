//! 04: Text layout & wrap -- `TextLayout`, `HAlign`/`VAlign`, word wrap
//!
//! Shows off [`TextLayout`], which wraps and aligns a [`Line`] within a bounded [`Rect`]. 03
//! taught a single unbounded `Line`; 04's new concept is *bounding* one: word-wrapping to fit a
//! width, and aligning the result within a rectangle on both axes.
//!
//! ```sh
//! cargo run --example 04_text_layout_and_wrap                          # Headless (prints a few frames)
//! cargo run --example 04_text_layout_and_wrap --features crossterm     # Terminal
//! cargo run --example 04_text_layout_and_wrap --features default-font  # Desktop window
//! cargo run --example 04_text_layout_and_wrap --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press any key (Terminal/Desktop) to quit.

use retroglyph_core::grid::Rect;
use retroglyph_core::layout::{HAlign, TextLayout, VAlign};
use retroglyph_core::text::Line;
use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};

/// One alignment-demo box: a labeled `Rect` plus the `HAlign`/`VAlign` to render its label with.
struct AlignBox {
    rect: Rect,
    label: &'static str,
    h: HAlign,
    v: VAlign,
}

struct TextLayoutAndWrap;

impl<B: Backend> App<B> for TextLayoutAndWrap {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.print(0, 0, "04: Text Layout & Wrap");

        // Five representative HAlign/VAlign combinations -- the four corners plus dead center --
        // rather than the full 3x3 cartesian product, which is mostly repetition of the same
        // idea. Each box's border makes its Rect's bounds visible; alignment is meaningless to
        // look at without a boundary to align against.
        let boxes = [
            AlignBox {
                rect: Rect::new(0, 2, 19, 6),
                label: "Left+Top",
                h: HAlign::Left,
                v: VAlign::Top,
            },
            AlignBox {
                rect: Rect::new(20, 2, 19, 6),
                label: "Right+Top",
                h: HAlign::Right,
                v: VAlign::Top,
            },
            AlignBox {
                rect: Rect::new(40, 2, 19, 6),
                label: "Left+Bottom",
                h: HAlign::Left,
                v: VAlign::Bottom,
            },
            AlignBox {
                rect: Rect::new(0, 9, 19, 6),
                label: "Right+Bottom",
                h: HAlign::Right,
                v: VAlign::Bottom,
            },
            AlignBox {
                rect: Rect::new(20, 9, 19, 6),
                label: "Center+Middle",
                h: HAlign::Center,
                v: VAlign::Middle,
            },
        ];
        for AlignBox { rect, label, h, v } in boxes {
            draw_border(term, rect);
            let line = Line::raw(label);
            TextLayout::new(&line)
                .rect(rect.shrink(1, 1))
                .h_align(h)
                .v_align(v)
                .render(term);
        }

        // Word wrap: a sentence too long for the box's width, greedily broken on spaces. The
        // same `TextLayout` as above -- wrapping isn't a separate API, just what happens when
        // `measure`/`render` see a line wider than the rect.
        let wrap_rect = Rect::new(40, 9, 19, 6);
        draw_border(term, wrap_rect);
        let wrap_line = Line::raw("Word wrap breaks on spaces, and force-breaks overlong words.");
        let wrap_inner = wrap_rect.shrink(1, 1);
        TextLayout::new(&wrap_line)
            .rect(wrap_inner)
            .h_align(HAlign::Left)
            .v_align(VAlign::Top)
            .render(term);

        // `measure()` computes the wrapped size without touching the terminal at all -- useful
        // for e.g. sizing a container before deciding whether to render into it.
        let metrics = TextLayout::new(&wrap_line).rect(wrap_inner).measure();
        term.print(
            0,
            16,
            &format!(
                "measure(): {}x{} lines, no terminal touched",
                metrics.width, metrics.height
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

/// Hand-drawn box border, just so each `Rect`'s bounds are visible. Not the `crates/widgets`
/// `BoxBorder` widget -- the gallery deliberately stays on `retroglyph-core` alone this early in
/// the ladder.
fn draw_border<B: Backend>(term: &mut Terminal<B>, rect: Rect) {
    let (x0, y0) = (rect.left(), rect.top());
    let (x1, y1) = (rect.right() - 1, rect.bottom() - 1);

    term.put(x0, y0, '\u{250c}');
    term.put(x1, y0, '\u{2510}');
    term.put(x0, y1, '\u{2514}');
    term.put(x1, y1, '\u{2518}');
    for x in (x0 + 1)..x1 {
        term.put(x, y0, '\u{2500}');
        term.put(x, y1, '\u{2500}');
    }
    for y in (y0 + 1)..y1 {
        term.put(x0, y, '\u{2502}');
        term.put(x1, y, '\u{2502}');
    }
}

rg_gallery_run!(TextLayoutAndWrap, "04: Text Layout & Wrap", 60, 18);
