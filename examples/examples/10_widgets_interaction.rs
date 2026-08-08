//! 10: Widgets interaction
//!
//! [`Interaction`] (composing [`HitTester`] and [`FocusRing`] internally; see their own doc
//! comments for the pieces this ties together), [`Shortcuts`] (a global keyboard binding
//! independent of focus), [`Density`] (sizing the buttons' hit targets), [`Ui`] (pairing a
//! frame's surface with `Interaction`, via [`Interaction::frame`]), and [`Button`] (the
//! style-by-[`Response`] widget). `04_mouse` proves raw pointer decode; this example shows what
//! a real widget does with it: hover, click, drag-suppressed-click, and Tab/Shift+Tab keyboard
//! focus with Enter/Space activation, all through one [`Interaction`] context, on three
//! [`Button`]s.
//!
//! ```sh
//! cargo run --example 10_widgets_interaction --features crossterm
//! cargo run --example 10_widgets_interaction --features software
//! cargo run --example 10_widgets_interaction  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: click a button, or Tab to it and press Enter/Space. `r` resets the counter regardless
//! of focus (a [`Shortcuts`] global binding). `q` or `Escape` quits, or close the window.

use retroglyph::app::Frame;
use retroglyph::backend::Backend;
use retroglyph::color::{Color, Style};
use retroglyph::event::{Event, KeyCode, KeyModifiers};
use retroglyph::grid::{HasSize, Rect};
use retroglyph::terminal::Terminal;
use retroglyph::ui::{Button, Density, Interaction, Shortcuts, Theme, Ui};
use retroglyph_examples::Example;

/// Identifies each button for [`Interaction`]'s hit-testing and focus ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonId {
    Increment,
    Decrement,
    Reset,
}

/// What a [`Shortcuts`] binding resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Reset,
}

/// The three buttons, in the order they're laid out left to right.
const BUTTONS: [(ButtonId, &str); 3] = [
    (ButtonId::Increment, "Increment (+1)"),
    (ButtonId::Decrement, "Decrement (-1)"),
    (ButtonId::Reset, "Reset"),
];

/// State for the interaction example: the shared interaction context, a global shortcut table,
/// and the counter the buttons drive.
pub struct WidgetsInteraction {
    interaction: Interaction<ButtonId>,
    shortcuts: Shortcuts<ButtonId, Action>,
    density: Density,
    count: i32,
}

impl Default for WidgetsInteraction {
    fn default() -> Self {
        let mut shortcuts = Shortcuts::new();
        shortcuts.bind_global(KeyCode::Char('r'), KeyModifiers::NONE, Action::Reset);
        Self {
            interaction: Interaction::new(),
            shortcuts,
            density: Density::Mouse,
            count: 0,
        }
    }
}

/// Draws one button, colored by hover/press/focus state via [`Button`], and applies its click to
/// `count`. `ui.show` resolves the click and draws the button from the one `rect`; the caller
/// only needs `response.clicked()` for the counter logic below.
fn draw_button(ui: &mut Ui<'_, '_, ButtonId>, rect: Rect, id: ButtonId, label: &str) -> bool {
    let theme = Theme::DARK;
    let button = Button::new(label)
        .style(Style::new().fg(theme.fg).bg(theme.panel_bg))
        .hovered_style(Style::new().fg(theme.fg).bg(theme.hover_bg))
        .pressed_style(Style::new().fg(theme.fg).bg(theme.press_bg))
        .focused_style(Style::new().fg(theme.accent).bg(theme.panel_bg));
    ui.show(rect, id, &button).clicked()
}

impl Example for WidgetsInteraction {
    const NAME: &'static str = "10_widgets_interaction";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        let events: Vec<Event> = term.drain_events().collect();
        let mut surface = term.surface();
        let Self {
            interaction,
            shortcuts,
            density,
            count,
        } = self;

        let mut quit = false;
        interaction.frame(&mut surface, |ui| {
            // Feeds every event to `ui`'s `Interaction`/`shortcuts`. Every `Response` `ui`
            // resolves below already reflects these events -- see `Interaction`'s own doc
            // comment for the frame lifecycle this follows.
            for event in &events {
                let _ = ui.interaction().handle_event(event);
                if shortcuts.resolve(event, ui.interaction().focus().focused())
                    == Some(Action::Reset)
                {
                    *count = 0;
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
                (1, 1),
                "Tab/Shift+Tab focuses, Enter/Space or click activates, r resets, q/Escape quits.",
                style_white,
            );

            let btn_h = density.min_target_size().height();
            let btn_w = 16u16;
            let y = 4;
            for (i, &(id, label)) in BUTTONS.iter().enumerate() {
                let x = 2 + u16::try_from(i).expect("BUTTONS.len() fits u16") * (btn_w + 2);
                if draw_button(ui, Rect::new(x, y, btn_w, btn_h), id, label) {
                    match id {
                        ButtonId::Increment => *count += 1,
                        ButtonId::Decrement => *count -= 1,
                        ButtonId::Reset => *count = 0,
                    }
                }
            }

            ui.surface()
                .print((2, y + btn_h + 1), &format!("Count: {count}"), style_white);
        });
        !quit
    }
}

retroglyph_examples::example_main!(WidgetsInteraction);
