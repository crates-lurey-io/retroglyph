//! 17: Theme switch
//!
//! Proves [`Theme::DARK`]/[`Theme::LIGHT`] runtime switching together with the `.theme()`
//! builder method every widget in `retroglyph-widgets` now has: [`Panel`] (border+fill),
//! [`Tabs`] (unselected/selected), [`List`] (item/selected), [`Button`] (all four interaction
//! states, doubling as the toggle control itself), and [`ProgressBar`] (filled/empty) all
//! re-derive their colors from whichever [`Theme`] is active on every frame -- no widget bakes
//! in a palette of its own. This is the "manual toggle key" scenario [`Theme`]'s own doc comment
//! names as one of several ways an app might pick between the two palettes; `09_widgets_dashboard`
//! and `10_widgets_interaction` pick a single fixed [`Theme::DARK`] and hand-thread `theme.*`
//! into each style knob, which is exactly the boilerplate `.theme()` replaces.
//!
//! ```sh
//! cargo run --example 17_theme_switch --features crossterm
//! cargo run --example 17_theme_switch --features software
//! cargo run --example 17_theme_switch  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Press `t`, or click (or Tab to it, then Enter/Space) the "Switch to ..." button, to flip the
//! active theme; Left/Right switches tabs, Up/Down moves the list selection; `q`/`Escape` quits,
//! or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Frame, Rect, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{
    Button, Interaction, List, ListState, Panel, ProgressBar, Sense, StatefulWidget, Tabs, Theme,
    Widget,
};

/// Identifies the demo's one interactive widget for [`Interaction`]'s hit-testing and focus ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetId {
    ToggleButton,
}

/// The list's fixed items.
const ITEMS: [&str; 4] = ["Alpha", "Bravo", "Charlie", "Delta"];

/// The tab strip's fixed titles.
const TABS: [&str; 2] = ["Info", "Settings"];

/// State for the theme-switch example: which palette is active, the list/tab selections, and
/// the toggle button's [`Interaction`] context.
pub struct ThemeSwitch {
    dark: bool,
    list_state: ListState,
    selected_tab: usize,
    interaction: Interaction<WidgetId>,
}

impl Default for ThemeSwitch {
    fn default() -> Self {
        let mut list_state = ListState::new();
        list_state.select(Some(0));
        Self {
            dark: true,
            list_state,
            selected_tab: 0,
            interaction: Interaction::new(),
        }
    }
}

impl ThemeSwitch {
    /// The currently active palette.
    const fn theme(&self) -> Theme {
        if self.dark { Theme::DARK } else { Theme::LIGHT }
    }

    /// Drains pending input: `t` toggles the theme directly (a keyboard-only path independent of
    /// the button's own click/focus handling in [`Self::draw_toggle_button`]); Left/Right
    /// switches tabs; Up/Down moves the list selection; `q`/`Escape` quits.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            self.interaction.handle_event(&event);
            match event {
                Event::Key(key) if key.is_down() => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Char('t') => self.dark = !self.dark,
                    KeyCode::Left => self.selected_tab = self.selected_tab.saturating_sub(1),
                    KeyCode::Right => {
                        self.selected_tab = (self.selected_tab + 1).min(TABS.len() - 1);
                    }
                    KeyCode::Down => self.list_state.select_next(ITEMS.len()),
                    KeyCode::Up => self.list_state.select_previous(ITEMS.len()),
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws this frame and presents it.
    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let theme = self.theme();

        term.reset_style().fg(theme.accent);
        term.print(
            1,
            0,
            "t toggles theme, Left/Right tabs, Up/Down selects, q/Esc quits",
        );
        term.reset_style();

        let panel_area = Rect::new(0, 1, 50, 24);
        Panel::new()
            .title(if self.dark {
                "Theme: Dark"
            } else {
                "Theme: Light"
            })
            .theme(theme)
            .render(panel_area, term);

        // Panel's own interior inset -- one cell in from the border on every side, the same
        // math `Modal::render` uses to hand back its inner content rect.
        let inner = Rect::new(
            panel_area.left() + 1,
            panel_area.top() + 1,
            panel_area.width() - 2,
            panel_area.height() - 2,
        );

        Tabs::new(&TABS)
            .select(Some(self.selected_tab))
            .theme(theme)
            .render(Rect::new(inner.left(), inner.top(), inner.width(), 1), term);

        let list_area = Rect::new(inner.left(), inner.top() + 2, inner.width(), 4);
        List::new(&ITEMS)
            .theme(theme)
            .render(list_area, term, &mut self.list_state);

        self.draw_toggle_button(term, Rect::new(inner.left(), inner.top() + 7, 20, 1), theme);

        ProgressBar::new(7, 10).theme(theme).render(
            Rect::new(inner.left(), inner.top() + 9, inner.width(), 1),
            term,
        );

        term.present().ok();
    }

    /// Draws the "Switch to ..." [`Button`] and applies its click to `self.dark`. Mirrors
    /// `09_widgets_dashboard`'s `draw_ping_button`/`10_widgets_interaction`'s `draw_button`,
    /// except the four hand-threaded `theme.*` style calls those examples make are replaced here
    /// by the one `.theme(theme)` call this whole feature exists to add.
    fn draw_toggle_button<B: Backend>(&mut self, term: &mut Terminal<B>, rect: Rect, theme: Theme) {
        let response = self
            .interaction
            .interact(rect, WidgetId::ToggleButton, Sense::click());
        let label = if self.dark {
            "Switch to Light"
        } else {
            "Switch to Dark"
        };
        Button::new(label, response).theme(theme).render(rect, term);
        if response.clicked() {
            self.dark = !self.dark;
        }
    }
}

impl Example for ThemeSwitch {
    const NAME: &'static str = "17_theme_switch";

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

retroglyph_examples::example_main!(ThemeSwitch);
