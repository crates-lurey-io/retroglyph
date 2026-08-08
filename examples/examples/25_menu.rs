//! 25: Menu
//!
//! [`Menu`]/[`MenuState`]/[`MenuRow`]: the anchored-popup core a dropdown, a combobox's open
//! list, and a command palette all share (retroglyph#1290). A "File ▾" [`Button`] anchors an
//! 11-row menu below itself via [`anchored_rect`], clamped to an intentionally short 6-row
//! viewport so the `^`/`v` overflow indicators actually have something to prove. Two plain
//! items, a checkbox pair, a labeled section of mutually-exclusive choice rows, a disabled item,
//! and separators throughout exercise the row kinds `MenuRow` supports.
//!
//! [`Menu::show`] is what actually stresses the four primitives this widget exists to prove out:
//! it opens a [`Ui::modal`] scoped to the popup's own rect (so its rows always win a hit inside
//! that rect, per [`HitTester::push_barrier`]) while a full-screen backdrop registered *before*
//! the modal catches every click that lands outside it, closing the menu without firing --
//! [`Interaction::wants_pointer`] is this same "is the pointer over something registered" answer,
//! read the frame-stale way [`Response::hovered`] already is.
//!
//! ```sh
//! cargo run --example 25_menu --features crossterm
//! cargo run --example 25_menu --features software
//! cargo run --example 25_menu  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: click "File" (or Tab to it, Enter/Space) to open. Up/Down navigate, Enter activates,
//! Escape closes without firing. Click outside the menu to close it the same way. `q`/Escape
//! quits while the menu is closed, or close the window.

use retroglyph_core::app::Frame;
use retroglyph_core::backend::Backend;
use retroglyph_core::color::{Color, Style};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::{Rect, Size};
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;
use retroglyph_ui::interact::Interaction;
use retroglyph_ui::layout::{Side, anchored_rect};
use retroglyph_ui::state::MenuState;
use retroglyph_ui::theme::Theme;
use retroglyph_ui::widget::{Button, Marker, Menu, MenuRow};

/// Identifies the two hit-testable regions [`Menu::show`] needs, plus the "File" trigger
/// button's own id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Id {
    Trigger,
    MenuContent,
    MenuBackdrop,
}

/// Row indices into [`rows`], named so `apply_row` doesn't hardcode magic numbers.
const NEW: usize = 0;
const OPEN: usize = 1;
const WORD_WRAP: usize = 3;
const LINE_NUMBERS: usize = 4;
const LIGHT: usize = 7;
const DARK: usize = 8;

/// This menu's 11 rows, rebuilt each frame from current settings so the checkbox/choice markers
/// stay in sync with what they represent.
const fn rows(word_wrap: bool, line_numbers: bool, dark_theme: bool) -> [MenuRow<'static>; 11] {
    [
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
            label: "Word Wrap",
            marker: Marker::Checkbox(word_wrap),
            enabled: true,
        },
        MenuRow::Item {
            label: "Line Numbers",
            marker: Marker::Checkbox(line_numbers),
            enabled: true,
        },
        MenuRow::Separator,
        MenuRow::Label("Theme"),
        MenuRow::Item {
            label: "Light",
            marker: Marker::Choice(!dark_theme),
            enabled: true,
        },
        MenuRow::Item {
            label: "Dark",
            marker: Marker::Choice(dark_theme),
            enabled: true,
        },
        MenuRow::Separator,
        MenuRow::Item {
            label: "Save (disabled)",
            marker: Marker::None,
            enabled: false,
        },
    ]
}

/// Apply whatever activating row `index` means, returning a message for the status line.
const fn apply_row(
    index: usize,
    word_wrap: &mut bool,
    line_numbers: &mut bool,
    dark_theme: &mut bool,
) -> &'static str {
    match index {
        NEW => "New",
        OPEN => "Open",
        WORD_WRAP => {
            *word_wrap = !*word_wrap;
            if *word_wrap {
                "Word Wrap: on"
            } else {
                "Word Wrap: off"
            }
        }
        LINE_NUMBERS => {
            *line_numbers = !*line_numbers;
            if *line_numbers {
                "Line Numbers: on"
            } else {
                "Line Numbers: off"
            }
        }
        LIGHT => {
            *dark_theme = false;
            "Theme: Light"
        }
        DARK => {
            *dark_theme = true;
            "Theme: Dark"
        }
        _ => "?", // the disabled "Save" row never activates
    }
}

/// State for the menu example: the shared interaction context, the menu's own open/selection
/// state, and the app-level settings a few of its rows toggle.
pub struct MenuDemo {
    interaction: Interaction<Id>,
    menu: MenuState,
    word_wrap: bool,
    line_numbers: bool,
    dark_theme: bool,
    last_action: &'static str,
}

impl Default for MenuDemo {
    fn default() -> Self {
        Self {
            interaction: Interaction::new(),
            menu: MenuState::new(),
            word_wrap: true,
            line_numbers: false,
            dark_theme: true,
            last_action: "(none yet)",
        }
    }
}

impl Example for MenuDemo {
    const NAME: &'static str = "25_menu";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        let events: Vec<Event> = term.drain_events().collect();
        let screen = term.area();
        let mut surface = term.surface();
        let Self {
            interaction,
            menu,
            word_wrap,
            line_numbers,
            dark_theme,
            last_action,
        } = self;

        let trigger_rect = Rect::new(1, 1, 10, 1);
        let popup_size = Size::new(22, 6); // shorter than 11 rows: proves the ^/v indicators
        let popup_area = anchored_rect(trigger_rect, popup_size, Side::Below, screen);

        let mut quit = false;
        interaction.frame(&mut surface, |ui| {
            for event in &events {
                let _ = ui.interaction().handle_event(event);

                let was_open = menu.is_open();
                let row_snapshot = rows(*word_wrap, *line_numbers, *dark_theme);
                if let Some(index) = menu.handle_event(event, row_snapshot.len(), |i| {
                    row_snapshot[i].is_selectable()
                }) {
                    *last_action = apply_row(index, word_wrap, line_numbers, dark_theme);
                }
                if was_open {
                    continue; // the menu owned this event: nav, activation, or an Escape close.
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

            // Each line is kept under 48 columns (this gallery's narrowest grid, 50x25, minus
            // the x=1 margin): a longer string would wrap onto the next row -- see `Surface::print`.
            let style_white = Style::new().fg(Color::WHITE);
            ui.surface().print(
                (1, screen.height() - 3),
                "Click File, or Tab+Enter, to open the menu.",
                style_white,
            );
            ui.surface().print(
                (1, screen.height() - 2),
                "Up/Down navigate, Enter picks, Escape closes.",
                style_white,
            );
            ui.surface().print(
                (1, screen.height() - 1),
                &format!("Last action: {last_action}"),
                style_white,
            );

            let theme = Theme::DARK;
            let button = Button::new("File \u{25be}")
                .style(Style::new().fg(theme.fg).bg(theme.panel_bg))
                .hovered_style(Style::new().fg(theme.fg).bg(theme.hover_bg))
                .pressed_style(Style::new().fg(theme.fg).bg(theme.press_bg))
                .focused_style(Style::new().fg(theme.accent).bg(theme.panel_bg));
            if ui.show(trigger_rect, Id::Trigger, &button).clicked() {
                let row_snapshot = rows(*word_wrap, *line_numbers, *dark_theme);
                menu.toggle(row_snapshot.len(), |i| row_snapshot[i].is_selectable());
            }

            let row_snapshot = rows(*word_wrap, *line_numbers, *dark_theme);
            if let Some(index) = Menu::new(&row_snapshot).show(
                ui,
                screen,
                popup_area,
                Id::MenuContent,
                Id::MenuBackdrop,
                menu,
            ) {
                *last_action = apply_row(index, word_wrap, line_numbers, dark_theme);
            }
        });
        !quit
    }
}

retroglyph_examples::example_main!(MenuDemo);
