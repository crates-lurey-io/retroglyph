//! 24: Settings form
//!
//! Every other widget example in this gallery proves one widget in isolation. This one proves
//! [`FocusRing`](retroglyph::ui::interact::FocusRing) (composed into [`Interaction`] the way every
//! other interactive example uses it) actually holds up once more than one `Id` variant is
//! competing for it: an inventory [`List`] with a draggable [`Scrollbar`] on the left, two
//! [`TextInput`] fields and a [`Button`] on the right, all sharing one `Interaction<FieldId>`.
//! Tab/Shift+Tab cycles all five in draw order, clicking any of them focuses it, and Enter saves
//! regardless of which one is focused.
//!
//! `TextInput` does implement [`InteractiveWidget`](retroglyph::ui::widget::InteractiveWidget)
//! (see `retroglyph#1331`), the same as `List`, `Scrollbar`, and `Button`, but each field here is
//! still wired by hand via [`Ui::region`] rather than `Ui::show_stateful`: this form draws a
//! focus-highlighted [`Panel`] border around each field, and `TextInput`'s own impl only owns the
//! field's own area, not a wrapping border. `Sense::click()` still registers it in the focus ring
//! and still turns a click (or Enter/Space while focused) into a focus request, exactly as
//! `InteractiveWidget::sense`/`render` would resolve for it, this form just also needs to draw
//! something around it.
//!
//! ```sh
//! cargo run --example 24_settings_form --features crossterm
//! cargo run --example 24_settings_form --features software
//! cargo run --example 24_settings_form  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: Tab/Shift+Tab moves focus, or click any field/row/button/scrollbar to focus it
//! directly. Up/Down moves the inventory selection while it's focused. Type to edit whichever
//! text field is focused. Enter saves. Escape quits, or close the window.

use retroglyph::app::Frame;
use retroglyph::backend::Backend;
use retroglyph::color::Style;
use retroglyph::event::{Event, KeyCode};
use retroglyph::grid::Rect;
use retroglyph::terminal::Terminal;
use retroglyph::ui::interact::{Interaction, Sense};
use retroglyph::ui::layout::{Constraint, split_h, split_v};
use retroglyph::ui::state::{ListState, ScrollState, TextInputState};
use retroglyph::ui::theme::Theme;
use retroglyph::ui::ui::Ui;
use retroglyph::ui::widget::{Button, List, Panel, Scrollbar, StatefulWidget, TextInput, Widget};
use retroglyph_examples::Example;

/// Identifies every focusable/clickable widget for the one shared [`Interaction`]'s hit-testing
/// and focus ring. Draw order below (`Items`, `ItemsScroll`, `Name`, `Notes`, `Save`) is Tab
/// order: [`FocusRing`](retroglyph::ui::interact::FocusRing) walks whatever order things were
/// registered in, not this enum's declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldId {
    Items,
    ItemsScroll,
    Name,
    Notes,
    Save,
}

/// Fixed inventory the left `List` shows: more rows than fit in the visible area, so the
/// `Scrollbar` actually has something to do.
const ITEMS: [&str; 10] = [
    "Potion",
    "Sword",
    "Shield",
    "Torch",
    "Rope",
    "Bandage",
    "Map",
    "Iron Key",
    "Coin Pouch",
    "Lantern",
];

/// State for the settings-form example: the shared interaction context, the inventory
/// list/scroll position, the two text fields, and the last-saved summary line.
pub struct SettingsForm {
    interaction: Interaction<FieldId>,
    list_state: ListState,
    scroll_state: ScrollState,
    name: TextInputState,
    notes: TextInputState,
    status: String,
}

impl Default for SettingsForm {
    fn default() -> Self {
        let mut list_state = ListState::new();
        list_state.select(Some(0));
        Self {
            interaction: Interaction::new(),
            list_state,
            scroll_state: ScrollState::new(),
            name: TextInputState::new(),
            notes: TextInputState::new(),
            status: "Not saved yet.".to_owned(),
        }
    }
}

/// Draws one bordered `TextInput` field, highlighting its border when focused. `ui.show_stateful`
/// would draw `TextInput`'s own [`retroglyph::ui::widget::InteractiveWidget`] impl directly, but
/// that impl only owns the field's own area, not a wrapping border, so this hand-rolls what
/// `ui.show_stateful` does for one automatically: [`Ui::region`] registers `area`/`id` with
/// `Sense::click()` (hit-testing, focus-ring membership, and click-to-focus) and hands back a
/// surface scoped to it, then the border and field are drawn into that same surface -- the "one
/// `id`-one `area`" guarantee `Ui::show`'s own doc comment describes, kept by hand.
fn draw_field(
    ui: &mut Ui<'_, '_, FieldId>,
    area: Rect,
    id: FieldId,
    title: &str,
    placeholder: &str,
    state: &mut TextInputState,
) {
    let theme = Theme::DARK;
    let (response, mut surface) = ui.region(area, id, Sense::click());
    let border_style = if response.focused() {
        Style::new().fg(theme.accent)
    } else {
        Style::new().fg(theme.border)
    };
    let panel = Panel::new().title(title).border_style(border_style);
    panel.render(&mut surface);

    let inner = panel.inner(area);
    state.ensure_visible(inner.width());
    TextInput::new()
        .placeholder(placeholder)
        .render(&mut surface.scope(inner), state);
}

/// Draws the left inventory column: a `List` plus a `Scrollbar` kept in sync with its own
/// selection/scroll offset, the same two-widget-one-position wiring a real inventory panel needs.
fn draw_inventory(
    ui: &mut Ui<'_, '_, FieldId>,
    area: Rect,
    list_state: &mut ListState,
    scroll_state: &mut ScrollState,
) {
    let theme = Theme::DARK;
    let cols = split_h(area, &[Constraint::Fill(1), Constraint::Fixed(1)]);
    let (list_area, scrollbar_area) = (cols[0], cols[1]);

    list_state.ensure_visible(list_area.height_usize());
    let list = List::new(&ITEMS)
        .item_style(Style::new().fg(theme.fg))
        .selected_style(Style::new().fg(theme.bg).bg(theme.accent));
    let _ = ui.show_stateful(list_area, FieldId::Items, &list, list_state);

    let max_offset = ITEMS.len().saturating_sub(list_area.height_usize());
    #[allow(clippy::cast_precision_loss)] // ITEMS.len() stays well under 2^24
    scroll_state.set_offset(list_state.offset() as f32, max_offset as f32);
    let scrollbar = Scrollbar::new(ITEMS.len(), list_area.height_usize());
    let _ = ui.show_stateful(
        scrollbar_area,
        FieldId::ItemsScroll,
        &scrollbar,
        scroll_state,
    );
    list_state.set_offset(scroll_state.integer_offset());
}

/// Draws one whole frame: header, the inventory column, the two fields, and the `Save` button.
/// Returns whether `Save` was clicked (by mouse, or Enter/Space while it holds focus).
#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut Ui<'_, '_, FieldId>,
    list_state: &mut ListState,
    scroll_state: &mut ScrollState,
    name: &mut TextInputState,
    notes: &mut TextInputState,
    status: &str,
) -> bool {
    let theme = Theme::DARK;
    let area = Rect::new(0, 0, 50, 25);
    let rows = split_v(
        area,
        &[
            Constraint::Fixed(1),
            Constraint::Fixed(1),
            Constraint::Fill(1),
            Constraint::Fixed(1),
        ],
    );
    let (title_area, focus_area, body_area, status_area) = (rows[0], rows[1], rows[2], rows[3]);

    ui.surface().print(
        (title_area.left(), title_area.top()),
        "Tab/Shift-Tab, click focus. Enter=save. Esc=quit.",
        Style::new().fg(theme.accent),
    );
    // Read before this frame's `interact()` calls below run, so a click that focuses a widget
    // this same frame (see `draw_field`'s doc comment) shows up here one frame later than the
    // typed/keyboard routing that already sees it, matching the `Interaction` frame-lifecycle
    // read-before-write shape the rest of this module follows.
    let focused = match ui.interaction().focus().focused() {
        Some(FieldId::Items) => "Items",
        Some(FieldId::ItemsScroll) => "Items Scrollbar",
        Some(FieldId::Name) => "Name",
        Some(FieldId::Notes) => "Notes",
        Some(FieldId::Save) => "Save",
        None => "(none)",
    };
    ui.surface().print(
        (focus_area.left(), focus_area.top()),
        &format!("Focused: {focused}"),
        Style::new().fg(theme.dim),
    );

    let cols = split_h(body_area, &[Constraint::Fixed(24), Constraint::Fill(1)]);
    let (left, right) = (cols[0], cols[1]);
    let left_rows = split_v(
        left,
        &[
            Constraint::Fixed(1),
            Constraint::Fixed(8),
            Constraint::Fill(1),
        ],
    );
    ui.surface().print(
        (left_rows[0].left(), left_rows[0].top()),
        "Inventory:",
        Style::new().fg(theme.dim),
    );
    draw_inventory(ui, left_rows[1], list_state, scroll_state);

    let right_rows = split_v(
        right,
        &[
            Constraint::Fixed(3),
            Constraint::Fixed(3),
            Constraint::Fixed(1),
            Constraint::Fill(1),
        ],
    );
    draw_field(
        ui,
        right_rows[0],
        FieldId::Name,
        "Name",
        "Character name...",
        name,
    );
    draw_field(
        ui,
        right_rows[1],
        FieldId::Notes,
        "Notes",
        "Notes...",
        notes,
    );

    let button = Button::new("Save")
        .style(Style::new().fg(theme.fg).bg(theme.panel_bg))
        .hovered_style(Style::new().fg(theme.fg).bg(theme.hover_bg))
        .pressed_style(Style::new().fg(theme.fg).bg(theme.press_bg))
        .focused_style(Style::new().fg(theme.accent).bg(theme.panel_bg));
    let save_area = Rect::new(right_rows[2].left(), right_rows[2].top(), 10, 1);
    let clicked = ui.show(save_area, FieldId::Save, &button).clicked();

    ui.surface().print(
        (status_area.left(), status_area.top()),
        status,
        Style::new().fg(theme.fg),
    );
    clicked
}

/// Routes one already-claimed event to whatever needs it beyond `Interaction::handle_event`
/// (called by the caller first): Up/Down move the inventory selection while it's focused, typed
/// input reaches whichever text field is focused (`TextInputState::handle_event` never consumes
/// Enter/Escape/Tab, so those still fall through to `save`/`quit` below regardless of which field
/// has focus), and Escape quits. Returns `(keep_going, save)`.
fn handle_event(
    event: &Event,
    list_state: &mut ListState,
    name: &mut TextInputState,
    notes: &mut TextInputState,
    focused: Option<FieldId>,
) -> (bool, bool) {
    let mut keep_going = true;
    let mut save = false;
    match event {
        Event::Key(key) if key.is_down() => match key.code {
            KeyCode::Escape => keep_going = false,
            KeyCode::Enter => save = true,
            KeyCode::Up if focused == Some(FieldId::Items) => {
                list_state.select_previous(ITEMS.len());
            }
            KeyCode::Down if focused == Some(FieldId::Items) => {
                list_state.select_next(ITEMS.len());
            }
            _ => {}
        },
        Event::Close => keep_going = false,
        _ => {}
    }
    match focused {
        Some(FieldId::Name) => {
            name.handle_event(event);
        }
        Some(FieldId::Notes) => {
            notes.handle_event(event);
        }
        _ => {}
    }
    (keep_going, save)
}

impl Example for SettingsForm {
    const NAME: &'static str = "24_settings_form";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        let events: Vec<Event> = term.drain_events().collect();
        let mut surface = term.surface();
        let Self {
            interaction,
            list_state,
            scroll_state,
            name,
            notes,
            status,
        } = self;

        let mut keep_going = true;
        let mut save = false;
        interaction.frame(&mut surface, |ui| {
            for event in &events {
                let _ = ui.interaction().handle_event(event);
                let focused = ui.interaction().focus().focused();
                let (still_going, wants_save) =
                    handle_event(event, list_state, name, notes, focused);
                keep_going &= still_going;
                save |= wants_save;
            }
            if keep_going {
                save |= draw(ui, list_state, scroll_state, name, notes, status);
            }
        });

        if keep_going && save {
            *status = format!(
                "Saved: name={:?} notes={:?} item={:?}",
                name.value(),
                notes.value(),
                list_state
                    .selected()
                    .and_then(|i| ITEMS.get(i))
                    .copied()
                    .unwrap_or("(none)"),
            );
        }
        keep_going
    }
}

retroglyph_examples::example_main!(SettingsForm);
