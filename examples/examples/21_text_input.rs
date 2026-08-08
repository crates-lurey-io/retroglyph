//! 21: Text input
//!
//! [`TextInputState`]/[`TextInput`] handle the field itself, drawing a cell-inverted caret (see
//! [`TextInput`]'s own docs on why it doesn't drive a real terminal cursor). This example is the
//! "app that wants a blinking, backend-native caret instead" that doc comment points at: it
//! positions and shapes the terminal's own cursor on top of the same field, from the exact
//! `state.cursor()`/`state.scroll()` the widget already reads, via the three passthroughs
//! `Terminal` gained in retroglyph#962 (`set_cursor_visible`/`set_cursor_position`/
//! `set_cursor_style`) -- `CursorStyle`'s first caller outside a test. Tab cycles through every
//! `CursorStyle` variant so all six actually run somewhere, not just the default.
//!
//! ```sh
//! cargo run --example 21_text_input --features crossterm
//! cargo run --example 21_text_input --features software
//! cargo run --example 21_text_input  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: type to edit, Left/Right/Home/End/Backspace/Delete to navigate. Tab cycles the cursor
//! style. Escape quits -- not `q`, which the field would just type. Or close the window.

use retroglyph_core::app::Frame;
use retroglyph_core::backend::{Backend, CursorStyle};
use retroglyph_core::color::{Color, Style};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::{Pos, Rect};
use retroglyph_core::terminal::Terminal;
use retroglyph_core::text::width_usize;
use retroglyph_examples::Example;
use retroglyph_ui::state::TextInputState;

use retroglyph_ui::widget::{Panel, StatefulWidget, TextInput, Widget as _};

use retroglyph_ui::Surface;

/// Every [`CursorStyle`] variant, in the order Tab cycles them (and DECSCUSR numbers them; see
/// [`Terminal::set_cursor_style`]'s own docs).
const CURSOR_STYLES: [(CursorStyle, &str); 6] = [
    (CursorStyle::BlinkingBlock, "BlinkingBlock"),
    (CursorStyle::SteadyBlock, "SteadyBlock"),
    (CursorStyle::BlinkingUnderline, "BlinkingUnderline"),
    (CursorStyle::SteadyUnderline, "SteadyUnderline"),
    (CursorStyle::BlinkingBar, "BlinkingBar"),
    (CursorStyle::SteadyBar, "SteadyBar"),
];

/// State for the text-input example: the field itself, and which [`CURSOR_STYLES`] entry Tab
/// has cycled to.
#[derive(Default)]
pub struct TextInputDemo {
    field: TextInputState,
    style: usize,
}

impl TextInputDemo {
    /// Routes every event to the field first: it consumes typed characters, Backspace/Delete,
    /// and the arrow/Home/End keys, and returns `false` for everything else (see
    /// [`TextInputState::handle_event`]'s own docs), so Tab and Escape always reach the `match`
    /// below instead.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            if self.field.handle_event(&event) {
                continue;
            }
            match event {
                Event::Key(key) if key.is_down() && matches!(key.code, KeyCode::Tab) => {
                    self.style = (self.style + 1) % CURSOR_STYLES.len();
                }
                Event::Key(key) if matches!(key.code, KeyCode::Escape) => return false,
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws the field inside a titled panel, then positions and shapes the real backend cursor
    /// on top of its cell-drawn caret.
    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let term_area = term.area();
        let text_style = Style::new().fg(Color::WHITE);

        // Kept under 48 columns (this gallery's narrowest grid, 50x25, minus the x=1 margin):
        // a longer string would wrap onto the next row -- see `Surface::print`'s own docs.
        Surface::new(term.grid_mut(), term_area, 0).print(
            (1, 1),
            "Type to edit. Arrows/Home/End/Backspace/Delete.",
            text_style,
        );
        Surface::new(term.grid_mut(), term_area, 0).print(
            (1, 2),
            "Tab: cycle cursor style. Escape: quit (not q).",
            text_style,
        );

        let field_area = Rect::new(1, 4, 40, 3);
        let panel = Panel::new().title("Text Input").border_style(text_style);
        panel.render(&mut Surface::new(term.grid_mut(), term_area, 0).scope(field_area));
        let inner = panel.inner(field_area);

        self.field.ensure_visible(inner.width());
        TextInput::new().placeholder("Type something...").render(
            &mut Surface::new(term.grid_mut(), term_area, 0).scope(inner),
            &mut self.field,
        );

        let (style, name) = CURSOR_STYLES[self.style];
        Surface::new(term.grid_mut(), term_area, 0).print(
            (1, inner.top() + 2),
            &format!("Cursor style: {name} (Tab to cycle)"),
            text_style,
        );

        // Mirrors `TextInput::render`'s own caret-column math
        // (crates/ui/src/widget/text_input.rs): `ensure_visible` above already keeps this
        // inside `[0, inner.width())`, so the real cursor always lands on the field's own
        // cell-drawn caret rather than drifting off it.
        let caret_col = width_usize(&self.field.value()[..self.field.cursor()])
            .saturating_sub(usize::from(self.field.scroll()));
        #[allow(clippy::cast_possible_truncation)] // caret_col < inner.width(), a u16
        let x = inner.left() + caret_col as u16;
        term.set_cursor_visible(true);
        term.set_cursor_position(Pos::new(x, inner.top()));
        term.set_cursor_style(style);
    }
}

impl Example for TextInputDemo {
    const NAME: &'static str = "21_text_input";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(TextInputDemo);
