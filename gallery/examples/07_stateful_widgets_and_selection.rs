//! 07: Stateful widgets & selection -- `StatefulWidget`, `ListState`, `Table`
//!
//! 06 taught [`Widget`](retroglyph_widgets::widget::Widget): a builder that draws itself and
//! retains nothing. This example's new concept is [`StatefulWidget`]: the same builder shape,
//! but `render` also takes `&mut State` -- state that outlives one render call and moves with
//! the widget across frames (a selection index, a scroll offset), instead of the widget
//! reconstructing everything from scratch every time.
//!
//! [`Table`] is the built-in [`StatefulWidget`] backing this, driven by [`ListState`]:
//!
//! - `state.selected()` / `state.select_next(len)` / `state.select_previous(len)` track which
//!   row is highlighted. `select_next`/`select_previous` clamp at the ends by default (pressing
//!   Down on the last row just stays there) -- see [`SelectionWrap`] if you want wraparound
//!   instead.
//! - `state.ensure_visible(visible_rows)` scrolls just enough to keep the selection on-screen.
//!   The table below has 20 rows in an 8-row-tall viewport specifically so scrolling is
//!   unavoidable, not an edge case you'd never hit.
//!
//! This is also the first example where a key does something other than quit: `Up`/`Down` move
//! the selection, so exiting needs its own key -- `q`, or the window's close button -- instead
//! of `any_key_pressed_or_window_closed`'s "any key quits". `Esc` is `ListState::select(None)`
//! instead: clears the selection (and its highlight) without closing the example, the same
//! "Esc cancels/deselects, it doesn't exit" convention tools like `fzf` use.
//!
//! ```sh
//! cargo run --example 07_stateful_widgets_and_selection                          # Headless (prints a few frames)
//! cargo run --example 07_stateful_widgets_and_selection --features crossterm     # Terminal
//! cargo run --example 07_stateful_widgets_and_selection --features default-font  # Desktop window
//! cargo run --example 07_stateful_widgets_and_selection --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press Up/Down to move the selection, Esc to clear it, q (Terminal/Desktop) to quit.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_gallery::{pressed_key, rg_gallery_run};
use retroglyph_widgets::ListState;
use retroglyph_widgets::widget::{StatefulWidget, Table};

/// One inventory row: item name, quantity carried, and weight in pounds.
struct Item {
    name: &'static str,
    qty: u32,
    weight: f32,
}

/// Fixed in-memory inventory, deliberately longer than the table's viewport so scrolling is
/// unavoidable rather than an edge case the example never actually exercises.
const ITEMS: &[Item] = &[
    Item {
        name: "Rusty Dagger",
        qty: 1,
        weight: 1.0,
    },
    Item {
        name: "Leather Boots",
        qty: 1,
        weight: 2.0,
    },
    Item {
        name: "Health Potion",
        qty: 5,
        weight: 0.5,
    },
    Item {
        name: "Iron Sword",
        qty: 1,
        weight: 4.0,
    },
    Item {
        name: "Wooden Shield",
        qty: 1,
        weight: 3.5,
    },
    Item {
        name: "Torch",
        qty: 3,
        weight: 1.0,
    },
    Item {
        name: "Rope (50ft)",
        qty: 1,
        weight: 5.0,
    },
    Item {
        name: "Lockpick Set",
        qty: 1,
        weight: 0.5,
    },
    Item {
        name: "Bread Loaf",
        qty: 4,
        weight: 0.5,
    },
    Item {
        name: "Waterskin",
        qty: 2,
        weight: 1.5,
    },
    Item {
        name: "Silver Ring",
        qty: 1,
        weight: 0.1,
    },
    Item {
        name: "Steel Helm",
        qty: 1,
        weight: 3.0,
    },
    Item {
        name: "Chain Mail",
        qty: 1,
        weight: 12.0,
    },
    Item {
        name: "Throwing Knife",
        qty: 6,
        weight: 0.3,
    },
    Item {
        name: "Antidote",
        qty: 2,
        weight: 0.3,
    },
    Item {
        name: "Spellbook",
        qty: 1,
        weight: 2.0,
    },
    Item {
        name: "Gold Coin",
        qty: 42,
        weight: 0.02,
    },
    Item {
        name: "Traveler's Cloak",
        qty: 1,
        weight: 2.5,
    },
    Item {
        name: "Flint & Steel",
        qty: 1,
        weight: 0.2,
    },
    Item {
        name: "Map Scroll",
        qty: 1,
        weight: 0.1,
    },
];

struct StatefulWidgetsAndSelection {
    table_state: ListState,
}

impl<B: Backend> App<B> for StatefulWidgetsAndSelection {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.print(0, 0, "07: Stateful Widgets & Selection");
        term.print(
            0,
            2,
            "Up/Down: move selection    Esc: clear selection    q: quit",
        );

        let mut exit = false;
        for event in term.drain_events() {
            if event == Event::Close {
                exit = true;
                continue;
            }
            match pressed_key(event) {
                Some(KeyCode::Up) => self.table_state.select_previous(ITEMS.len()),
                Some(KeyCode::Down) => self.table_state.select_next(ITEMS.len()),
                Some(KeyCode::Escape) => self.table_state.select(None),
                Some(KeyCode::Char('q')) => exit = true,
                _ => {}
            }
        }

        let table_area = Rect::new(0, 4, 40, 9); // 1 header row + 8 visible item rows
        // Keep the selection scrolled into view every frame -- cheap, idempotent, and correct
        // across terminal resizes without special-casing them (see ListState::ensure_visible's
        // own doc comment for why this belongs here rather than only after moving the selection).
        self.table_state
            .ensure_visible(table_area.height() as usize - 1);

        let rows: Vec<Vec<String>> = ITEMS
            .iter()
            .map(|item| {
                vec![
                    item.name.to_owned(),
                    item.qty.to_string(),
                    format!("{:.1}", item.weight),
                ]
            })
            .collect();
        Table::new(&["Name", "Qty", "Wt"], &[20, 5, 5], &rows).render(
            table_area,
            term,
            &mut self.table_state,
        );

        term.print(
            0,
            14,
            &format!(
                "selected: {:?}    offset: {}    (20 rows, 8-row viewport)",
                self.table_state.selected(),
                self.table_state.offset()
            ),
        );

        term.present().expect("present failed");

        if exit { Flow::Exit } else { Flow::Continue }
    }
}

rg_gallery_run!(
    StatefulWidgetsAndSelection {
        table_state: ListState::new(),
    },
    "07: Stateful Widgets & Selection",
    60,
    16
);
