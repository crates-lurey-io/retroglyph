//! 08: Focus & checkboxes -- `FocusRing`, Tab-cycling, `is_focused`-guarded dispatch
//!
//! 07's `Table` was the only focusable thing on screen, so nothing ever had to answer "which
//! widget does input go to right now." This example's new concept is [`FocusRing`]: the primitive
//! that answers exactly that question when there's more than one focusable thing.
//!
//! Five independent checkboxes, each just a `bool` -- no [`ListState`](retroglyph_widgets::ListState),
//! no [`Table`](retroglyph_widgets::widget::Table), deliberately, so `FocusRing` is the only new
//! thing here rather than sharing the spotlight with something already covered in 07. `Tab`/
//! `Shift+Tab` cycle which checkbox is focused (via [`FocusRing::handle_event`]); `Space` toggles
//! whichever one currently is. Each frame:
//!
//! 1. `focus.begin_frame()` finalizes last frame's [`FocusRing::register`] calls into the order
//!    this frame's `Tab`/`Shift+Tab` will cycle through.
//! 2. Drained input is handled -- `FocusRing::handle_event` does `Tab`/`Shift+Tab` for you; `Space`
//!    is dispatched by hand, guarded by [`FocusRing::focused`].
//! 3. Drawing re-registers every checkbox as focusable for the *next* frame, and reads
//!    [`FocusRing::is_focused`] to decide each box's border color -- `FocusRing` doesn't care what
//!    you're drawing (there's no widget type behind a checkbox here at all, just two glyphs and a
//!    label), it only tracks *which id* currently has focus.
//!
//! `q` (or the window's close button) quits -- no example-specific meaning for `Esc` this time, so
//! it's left alone rather than overloaded with one.
//!
//! ```sh
//! cargo run --example 08_focus_and_checkboxes                          # Headless (prints a few frames)
//! cargo run --example 08_focus_and_checkboxes --features crossterm     # Terminal
//! cargo run --example 08_focus_and_checkboxes --features default-font  # Desktop window
//! cargo run --example 08_focus_and_checkboxes --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press Tab/Shift+Tab to move focus, Space to toggle, q (Terminal/Desktop) to quit.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Color, Flow, Frame, Style, Terminal};
use retroglyph_gallery::{pressed_key, rg_gallery_run};
use retroglyph_widgets::widget::{BoxBorder, Widget};
use retroglyph_widgets::{Constraint, FocusRing, split_v_spaced};

/// One checkbox: its label and current state.
struct Checkbox {
    label: &'static str,
    checked: bool,
}

impl Checkbox {
    const fn new(label: &'static str, checked: bool) -> Self {
        Self { label, checked }
    }
}

const SETTING_COUNT: usize = 5;

struct FocusAndCheckboxes {
    focus: FocusRing<usize>,
    settings: [Checkbox; SETTING_COUNT],
}

impl FocusAndCheckboxes {
    const fn new() -> Self {
        let mut focus = FocusRing::new();
        focus.request(0); // start with the first checkbox focused, not nothing
        Self {
            focus,
            settings: [
                Checkbox::new("Sound", true),
                Checkbox::new("Music", true),
                Checkbox::new("Fullscreen", false),
                Checkbox::new("Auto-save", true),
                Checkbox::new("Hardcore Mode", false),
            ],
        }
    }
}

impl<B: Backend> App<B> for FocusAndCheckboxes {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        // Finalize last frame's registrations into the order this frame's Tab/Shift+Tab walk --
        // see FocusRing's own doc comment for why this has to happen before handling input.
        self.focus.begin_frame();

        let mut exit = false;
        for event in term.drain_events() {
            self.focus.handle_event(&event); // Tab advances, Shift+Tab retreats
            if event == Event::Close {
                exit = true;
                continue;
            }
            match pressed_key(event) {
                Some(KeyCode::Char(' ')) => {
                    if let Some(i) = self.focus.focused() {
                        self.settings[i].checked = !self.settings[i].checked;
                    }
                }
                Some(KeyCode::Char('q')) => exit = true,
                _ => {}
            }
        }

        term.print(0, 0, "08: Focus & Checkboxes");
        term.print(
            0,
            2,
            "Tab/Shift+Tab: move focus    Space: toggle    q: quit",
        );

        // 5 boxes, 3 rows tall each, 1-cell gaps -- 5*3 + 4*1 = 19 rows total.
        let area = Rect::new(0, 4, 30, 19);
        let boxes = split_v_spaced(area, &[Constraint::Fixed(3); SETTING_COUNT], 1);
        for (i, rect) in boxes.into_iter().enumerate() {
            let focused = self.focus.is_focused(i);
            // Register for next frame's cycling regardless of whether this box is focused right
            // now -- FocusRing only learns a box exists by seeing it registered during draw.
            self.focus.register(i);

            let border = if focused {
                BoxBorder::new().style(Style::new().fg(Color::CYAN))
            } else {
                BoxBorder::new()
            };
            border.render(rect, term);

            let setting = &self.settings[i];
            let mark = if setting.checked { 'x' } else { ' ' };
            term.print(
                rect.left() + 1,
                rect.top() + 1,
                &format!("[{mark}] {}", setting.label),
            );
        }

        term.present().expect("present failed");

        if exit { Flow::Exit } else { Flow::Continue }
    }
}

rg_gallery_run!(FocusAndCheckboxes::new(), "08: Focus & Checkboxes", 60, 26);
