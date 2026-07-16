//! 10: Widgets interaction
//!
//! [`Interaction`] (composing [`HitTester`] and [`FocusRing`] internally -- see their own doc
//! comments for the pieces this ties together), [`Shortcuts`] (a global keyboard binding
//! independent of focus), [`Density`] (sizing the buttons' hit targets), and [`Button`] (the
//! style-by-[`Response`] widget this example used to hand-roll). Pairs with `04_mouse`: that
//! example proved raw pointer decode; this one proves what a real widget does with it -- hover,
//! click, drag-suppressed-click, and Tab/Shift+Tab keyboard focus with Enter/Space activation,
//! all through one [`Interaction`] context, on three [`Button`]s.
//!
//! ```sh
//! cargo run --example 10_widgets_interaction --features crossterm
//! cargo run --example 10_widgets_interaction --features software
//! cargo run --example 10_widgets_interaction  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Click a button, or Tab to it and press Enter/Space; `r` resets the counter regardless of
//! focus (a [`Shortcuts`] global binding); `q` or `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode, KeyModifiers};
use retroglyph_core::{Backend, Color, Frame, Rect, Style, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{Button, Density, Interaction, Sense, Shortcuts, Theme, Widget};

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
            density: Density::Relaxed,
            count: 0,
        }
    }
}

impl WidgetsInteraction {
    /// Feeds every event to [`Interaction`]/[`Shortcuts`] and reports whether the user asked to
    /// quit. Must run between [`Interaction::begin_frame`] and the draw pass's
    /// [`Interaction::interact`] calls -- see [`Interaction`]'s own doc comment for the frame
    /// lifecycle this follows.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            self.interaction.handle_event(&event);
            if self
                .shortcuts
                .resolve(&event, self.interaction.focus().focused())
                == Some(Action::Reset)
            {
                self.count = 0;
            }
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

    /// Draws one button, colored by hover/press/focus state via [`Button`], and applies its
    /// click to `count`. The app still owns `interact`ing (it needs `response.clicked()` for the
    /// counter logic below); `Button` only turns the resulting `Response` into a styled label,
    /// replacing what used to be this method's own bg/fg-by-response wiring.
    fn draw_button<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        rect: Rect,
        id: ButtonId,
        label: &str,
    ) {
        let response = self.interaction.interact(rect, id, Sense::click());
        let theme = Theme::DARK;

        Button::new(label, response)
            .style(Style::new().fg(theme.fg).bg(theme.panel_bg))
            .hovered_style(Style::new().fg(theme.fg).bg(theme.hover_bg))
            .pressed_style(Style::new().fg(theme.fg).bg(theme.press_bg))
            .focused_style(Style::new().fg(theme.accent).bg(theme.panel_bg))
            .render(rect, term);

        match (id, response.clicked()) {
            (ButtonId::Increment, true) => self.count += 1,
            (ButtonId::Decrement, true) => self.count -= 1,
            (ButtonId::Reset, true) => self.count = 0,
            (ButtonId::Increment | ButtonId::Decrement | ButtonId::Reset, false) => {}
        }
    }

    /// Draws this frame and presents it. Must run between the frame's own
    /// [`Interaction::begin_frame`] and [`Interaction::end_frame`].
    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        term.reset_style().fg(Color::WHITE);
        term.print(
            1,
            1,
            "Tab/Shift+Tab focuses, Enter/Space or click activates, r resets, q/Escape quits.",
        );
        term.reset_style();

        let btn_h = self.density.min_target_size().height;
        let btn_w = 16u16;
        let y = 4;
        for (i, &(id, label)) in BUTTONS.iter().enumerate() {
            let x = 2 + u16::try_from(i).expect("BUTTONS.len() fits u16") * (btn_w + 2);
            self.draw_button(term, Rect::new(x, y, btn_w, btn_h), id, label);
        }

        term.reset_style().fg(Color::WHITE);
        term.print(2, y + btn_h + 1, &format!("Count: {}", self.count));
        term.reset_style();

        term.present().ok();
    }
}

impl Example for WidgetsInteraction {
    const NAME: &'static str = "10_widgets_interaction";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        self.interaction.begin_frame();
        if !self.handle_events(term) {
            self.interaction.end_frame();
            return false;
        }
        self.draw(term);
        self.interaction.end_frame();
        true
    }
}

retroglyph_examples::example_main!(WidgetsInteraction);
