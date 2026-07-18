//! 18: Weighted fill
//!
//! Proves `Constraint::Fill(weight)`: `split_h`'s remainder now divides in proportion to each
//! pane's weight instead of always splitting equally. Four static rows, stacked with
//! `split_v`, each showing a different `split_h` call and labeling every pane with the
//! constraint that produced it and its resulting width:
//!
//! - Row 1: `Fill(1)` three times -- the pre-weight behavior, unchanged: an equal three-way
//!   split.
//! - Row 2: `Fill(1)`, `Fill(2)`, `Fill(3)` -- a 1:2:3 ratio of the same 48 columns.
//! - Row 3: `Fixed(10)` then `Fill(1)`/`Fill(3)` -- weighted fill only applies to the
//!   remainder *after* the fixed pane is reserved.
//! - Row 4: `Min(6)`, `Fill(2)`, `Max(12)` -- weighted `Fill` mixed with `Min`/`Max`, which
//!   always weigh 1 regardless of their floor/cap value.
//!
//! ```sh
//! cargo run --example 18_weighted_fill --features crossterm
//! cargo run --example 18_weighted_fill --features software
//! cargo run --example 18_weighted_fill  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Press `q` or `Escape` to quit on the interactive backends, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Rect, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{Constraint, split_h, split_v};

/// State for the weighted-fill example (none needed: the pane layout never changes).
#[derive(Default)]
pub struct WeightedFill;

impl WeightedFill {
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

    /// Draws a box-drawing border around `rect` and, if it's tall enough for both, a `label`
    /// on the first interior row and a `width` readout on the second.
    fn draw_pane<B: Backend>(term: &mut Terminal<B>, rect: Rect, label: &str) {
        if rect.width() == 0 || rect.height() == 0 {
            return;
        }
        let (l, t, r, b) = (rect.left(), rect.top(), rect.right() - 1, rect.bottom() - 1);

        term.put(l, t, '┌');
        term.put(r, t, '┐');
        term.put(l, b, '└');
        term.put(r, b, '┘');
        for x in (l + 1)..r {
            term.put(x, t, '─');
            term.put(x, b, '─');
        }
        for y in (t + 1)..b {
            term.put(l, y, '│');
            term.put(r, y, '│');
        }

        let interior_width = rect.width().saturating_sub(2);
        let center = |text: &str| -> u16 {
            let len = u16::try_from(text.len()).unwrap_or(interior_width);
            l + 1 + interior_width.saturating_sub(len) / 2
        };
        let width_readout = format!("w={}", rect.width());
        if rect.height() >= 4 {
            term.print(center(label), t + 1, label);
            term.print(center(&width_readout), t + 2, &width_readout);
        } else if rect.height() >= 2 {
            term.print(center(label), t + rect.height() / 2, label);
        }
    }

    /// Draws one demo row: a caption above a `split_h` of `row` using `constraints`, with each
    /// resulting pane labeled by the entry in `labels` at the same index.
    fn draw_row<B: Backend>(
        term: &mut Terminal<B>,
        row: Rect,
        caption: &str,
        constraints: &[Constraint],
        labels: &[&str],
    ) {
        term.reset_style();
        term.print(row.left(), row.top(), caption);
        let body = Rect::new(row.left(), row.top() + 1, row.width(), row.height() - 1);
        let panes = split_h(body, constraints);
        for (pane, label) in panes.iter().zip(labels) {
            Self::draw_pane(term, *pane, label);
        }
    }

    /// Draws this frame and presents it.
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let full = Rect::new(0, 0, 50, 25);
        term.print(1, 0, "Constraint::Fill(weight): proportional splits");

        let rows = split_v(
            Rect::new(full.left(), full.top() + 1, full.width(), full.height() - 1),
            &[Constraint::Fill(1); 4],
        );

        Self::draw_row(
            term,
            rows[0],
            "Row 1: Fill(1) x3 -- equal thirds, unchanged",
            &[
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ],
            &["Fill(1)", "Fill(1)", "Fill(1)"],
        );
        Self::draw_row(
            term,
            rows[1],
            "Row 2: Fill(1):Fill(2):Fill(3) -- 1:2:3 ratio",
            &[
                Constraint::Fill(1),
                Constraint::Fill(2),
                Constraint::Fill(3),
            ],
            &["Fill(1)", "Fill(2)", "Fill(3)"],
        );
        Self::draw_row(
            term,
            rows[2],
            "Row 3: Fixed(10) + Fill(1)/Fill(3) remainder",
            &[
                Constraint::Fixed(10),
                Constraint::Fill(1),
                Constraint::Fill(3),
            ],
            &["Fixed(10)", "Fill(1)", "Fill(3)"],
        );
        Self::draw_row(
            term,
            rows[3],
            "Row 4: Min(6), Fill(2), Max(12) -- Min/Max=1",
            &[Constraint::Min(6), Constraint::Fill(2), Constraint::Max(12)],
            &["Min(6)", "Fill(2)", "Max(12)"],
        );

        term.present().ok();
    }
}

impl Example for WeightedFill {
    const NAME: &'static str = "18_weighted_fill";

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

retroglyph_examples::example_main!(WeightedFill);
