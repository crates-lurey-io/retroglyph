//! 17: Theme switch
//!
//! Runtime switching between [`Theme::DARK`] and [`Theme::LIGHT`], using the `.theme()` builder
//! method every widget in `retroglyph-widgets` has: [`Panel`] (border and fill), [`Tabs`]
//! (unselected/selected), [`List`] (item/selected), [`Button`] (all four interaction states,
//! doubling as the toggle control itself), and [`ProgressBar`] (filled/empty) all re-derive
//! their colors from whichever [`Theme`] is active on every frame; no widget bakes in a palette
//! of its own. `09_widgets_dashboard` and `10_widgets_interaction` pick a single fixed
//! [`Theme::DARK`] and hand-thread `theme.*` into each style knob; `.theme()` replaces that
//! boilerplate. [`Ui`] pairs the frame's surface with [`Interaction`] (via
//! [`Interaction::frame`]), so drawing each widget into its own area no longer needs a fresh
//! [`Surface`](retroglyph_core::surface::Surface) built by hand.
//!
//! ```sh
//! cargo run --example 17_theme_switch --features crossterm
//! cargo run --example 17_theme_switch --features software
//! cargo run --example 17_theme_switch  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `t`, or click (or Tab to it, then Enter/Space) the "Switch to ..." button, flips the
//! active theme. Left/Right switches tabs, Up/Down moves the list selection. `q` or `Escape`
//! quits, or close the window.

use retroglyph_core::app::Frame;
use retroglyph_core::backend::Backend;
use retroglyph_core::color::Style;
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;
use retroglyph_widgets::{
    Button, Interaction, List, ListState, Panel, ProgressBar, Tabs, Theme, Ui,
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

/// Handles one already-drained event: `t` toggles the theme directly (a keyboard-only path
/// independent of the button's own click/focus handling in [`draw_toggle_button`]); Left/Right
/// switches tabs; Up/Down moves the list selection; `q`/`Escape` quits.
fn handle_event(
    event: &Event,
    dark: &mut bool,
    selected_tab: &mut usize,
    list_state: &mut ListState,
) -> bool {
    match event {
        Event::Key(key) if key.is_down() => match key.code {
            KeyCode::Char('q') | KeyCode::Escape => return false,
            KeyCode::Char('t') => *dark = !*dark,
            KeyCode::Left => *selected_tab = selected_tab.saturating_sub(1),
            KeyCode::Right => *selected_tab = (*selected_tab + 1).min(TABS.len() - 1),
            KeyCode::Down => list_state.select_next(ITEMS.len()),
            KeyCode::Up => list_state.select_previous(ITEMS.len()),
            _ => {}
        },
        Event::Close => return false,
        _ => {}
    }
    true
}

/// Draws the "Switch to ..." [`Button`] and applies its click to `dark`. `ui.show` resolves the
/// click and draws the button from the one `rect`. Mirrors `09_widgets_dashboard`'s
/// `draw_ping_button`/`10_widgets_interaction`'s `draw_button`, except the four hand-threaded
/// `theme.*` style calls those examples make are replaced here by the one `.theme(theme)` call
/// this whole feature exists to add.
fn draw_toggle_button(ui: &mut Ui<'_, '_, WidgetId>, rect: Rect, theme: Theme, dark: &mut bool) {
    let label = if *dark {
        "Switch to Light"
    } else {
        "Switch to Dark"
    };
    let button = Button::new(label).theme(theme);
    if ui.show(rect, WidgetId::ToggleButton, &button).clicked() {
        *dark = !*dark;
    }
}

/// Draws one frame.
fn draw(
    ui: &mut Ui<'_, '_, WidgetId>,
    theme: Theme,
    dark: &mut bool,
    list_state: &mut ListState,
    selected_tab: usize,
) {
    let accent_style = Style::new().fg(theme.accent);
    ui.surface().print(
        (1, 0),
        "t toggles theme, Left/Right tabs, Up/Down selects, q/Esc quits",
        accent_style,
    );

    let panel_area = Rect::new(0, 1, 50, 24);
    let panel = Panel::new()
        .title(if *dark { "Theme: Dark" } else { "Theme: Light" })
        .theme(theme);
    ui.draw(panel_area, &panel);

    // Panel's own interior inset -- one cell in from the border on every side, the same math
    // `Modal::render` uses to hand back its inner content rect.
    let inner = Rect::new(
        panel_area.left() + 1,
        panel_area.top() + 1,
        panel_area.width() - 2,
        panel_area.height() - 2,
    );

    let tabs = Tabs::new(&TABS).select(Some(selected_tab)).theme(theme);
    ui.draw(
        Rect::new(inner.left(), inner.top(), inner.width(), 1),
        &tabs,
    );

    let list_area = Rect::new(inner.left(), inner.top() + 2, inner.width(), 4);
    let list = List::new(&ITEMS).theme(theme);
    ui.draw_stateful(list_area, &list, list_state);

    draw_toggle_button(
        ui,
        Rect::new(inner.left(), inner.top() + 7, 20, 1),
        theme,
        dark,
    );

    let progress_area = Rect::new(inner.left(), inner.top() + 9, inner.width(), 1);
    ui.draw(progress_area, &ProgressBar::new(7, 10).theme(theme));
}

impl Example for ThemeSwitch {
    const NAME: &'static str = "17_theme_switch";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        let events: Vec<Event> = term.drain_events().collect();
        let mut surface = term.surface();
        let Self {
            dark,
            list_state,
            selected_tab,
            interaction,
        } = self;

        let mut keep_going = true;
        interaction.frame(&mut surface, |ui| {
            for event in &events {
                let _ = ui.interaction().handle_event(event);
                if !handle_event(event, dark, selected_tab, list_state) {
                    keep_going = false;
                }
            }
            if keep_going {
                let theme = if *dark { Theme::DARK } else { Theme::LIGHT };
                draw(ui, theme, dark, list_state, *selected_tab);
            }
        });
        keep_going
    }
}

retroglyph_examples::example_main!(ThemeSwitch);
