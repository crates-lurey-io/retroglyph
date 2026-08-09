//! 26: Menu Bar
//!
//! [`MenuBar`]/[`MenuBarState`]: the thin, app-specific layer over [`Menu`] (retroglyph#1290)
//! this example's own title asks for (retroglyph#1354) -- a "File Edit View" strip where a click
//! toggles a label's popup, hovering a *different* label while one is open switches to it, a
//! cold hover (no prior click) does nothing, and Left/Right step which label is open.
//!
//! ```sh
//! cargo run --example 26_menu_bar --features crossterm
//! cargo run --example 26_menu_bar --features software
//! cargo run --example 26_menu_bar  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: click a label (or Tab to the bar, Enter/Space) to open it. Left/Right switch which
//! label is open, Up/Down navigate its rows, Enter activates, Escape closes without firing.
//! Click outside the bar/popup to close it the same way. `q`/Escape quits while nothing is open,
//! or close the window.

use retroglyph::app::Frame;
use retroglyph::backend::Backend;
use retroglyph::color::{Color, Style};
use retroglyph::event::{Event, KeyCode};
use retroglyph::grid::{Rect, Size};
use retroglyph::terminal::Terminal;
use retroglyph::ui::interact::Interaction;
use retroglyph::ui::state::MenuBarState;
use retroglyph::ui::widget::{Marker, MenuBar, MenuBarIds, MenuRow};
use retroglyph_examples::Example;

/// Identifies the two hit-testable regions [`MenuBar::show`] needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Id {
    Bar,
    PopupContent,
    PopupBackdrop,
}

/// Row indices into `EDIT_ROWS`, named so `apply_row` doesn't hardcode magic numbers.
const WORD_WRAP: usize = 2;

const FILE_ROWS: [MenuRow<'static>; 4] = [
    MenuRow::Item {
        label: "New",
        marker: Marker::None,
        enabled: true,
    },
    MenuRow::Item {
        label: "Open",
        marker: Marker::None,
        enabled: true,
    },
    MenuRow::Separator,
    MenuRow::Item {
        label: "Save (disabled)",
        marker: Marker::None,
        enabled: false,
    },
];

/// Rebuilt each frame from `word_wrap` so its checkbox marker stays in sync with what it
/// represents, the same reason `25_menu`'s own `rows` is a function rather than a `const`.
const fn edit_rows(word_wrap: bool) -> [MenuRow<'static>; 3] {
    [
        MenuRow::Item {
            label: "Cut",
            marker: Marker::None,
            enabled: true,
        },
        MenuRow::Item {
            label: "Copy",
            marker: Marker::None,
            enabled: true,
        },
        MenuRow::Item {
            label: "Word Wrap",
            marker: Marker::Checkbox(word_wrap),
            enabled: true,
        },
    ]
}

const VIEW_ROWS: [MenuRow<'static>; 2] = [
    MenuRow::Item {
        label: "Zoom In",
        marker: Marker::None,
        enabled: true,
    },
    MenuRow::Item {
        label: "Zoom Out",
        marker: Marker::None,
        enabled: true,
    },
];

const TITLES: [&str; 3] = ["File", "Edit", "View"];

/// Apply whatever activating `(label, row)` means, returning a message for the status line.
const fn apply_row(label: usize, row: usize, word_wrap: &mut bool) -> &'static str {
    match (label, row) {
        (0, 0) => "New",
        (0, 1) => "Open",
        (1, 0) => "Cut",
        (1, 1) => "Copy",
        (1, WORD_WRAP) => {
            *word_wrap = !*word_wrap;
            if *word_wrap {
                "Word Wrap: on"
            } else {
                "Word Wrap: off"
            }
        }
        (2, 0) => "Zoom In",
        (2, 1) => "Zoom Out",
        _ => "?",
    }
}

/// State for the menu bar example: the shared interaction context, the bar's own open/selection
/// state, and the one app-level setting "Edit \u{2192} Word Wrap" toggles.
pub struct MenuBarDemo {
    interaction: Interaction<Id>,
    bar: MenuBarState,
    word_wrap: bool,
    last_action: &'static str,
}

impl Default for MenuBarDemo {
    fn default() -> Self {
        Self {
            interaction: Interaction::new(),
            bar: MenuBarState::new(),
            word_wrap: true,
            last_action: "(none yet)",
        }
    }
}

impl Example for MenuBarDemo {
    const NAME: &'static str = "26_menu_bar";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        let events: Vec<Event> = term.drain_events().collect();
        let screen = term.area();
        let mut surface = term.surface();
        let Self {
            interaction,
            bar,
            word_wrap,
            last_action,
        } = self;

        let bar_area = Rect::new(0, 0, screen.width(), 1);
        let popup_size = Size::new(22, 6);

        let mut quit = false;
        interaction.frame(&mut surface, |ui| {
            let edit_rows = edit_rows(*word_wrap);
            let rows: [&[MenuRow<'_>]; 3] = [&FILE_ROWS, &edit_rows, &VIEW_ROWS];
            let menu_bar = MenuBar::new(&TITLES, &rows);

            for event in &events {
                let _ = ui.interaction().handle_event(event);

                let was_open = bar.open_label().is_some();
                if let Some((label, row)) = menu_bar.handle_event(event, bar) {
                    *last_action = apply_row(label, row, word_wrap);
                }
                if was_open {
                    continue; // the bar owned this event: nav, activation, or an Escape close.
                }

                match event {
                    Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) => {
                        quit = true;
                    }
                    Event::Close => quit = true,
                    _ => {}
                }
            }
            if quit {
                return;
            }

            let style_white = Style::new().fg(Color::WHITE);
            ui.surface().print(
                (1, screen.height() - 3),
                "Click a label, or Tab+Enter, to open it.",
                style_white,
            );
            ui.surface().print(
                (1, screen.height() - 2),
                "Left/Right switch labels, hover switches too.",
                style_white,
            );
            ui.surface().print(
                (1, screen.height() - 1),
                &format!("Last action: {last_action}"),
                style_white,
            );

            if let Some((label, row)) = menu_bar.show(
                ui,
                screen,
                bar_area,
                popup_size,
                MenuBarIds {
                    label: Id::Bar,
                    popup_content: Id::PopupContent,
                    popup_backdrop: Id::PopupBackdrop,
                },
                bar,
            ) {
                *last_action = apply_row(label, row, word_wrap);
            }
        });
        !quit
    }
}

retroglyph_examples::example_main!(MenuBarDemo);
