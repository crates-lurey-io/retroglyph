//! [`MenuBar`]: a horizontal row of labels, each anchoring a [`Menu`] popup.
use alloc::vec::Vec;

use retroglyph_core::color::Style;
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::{Pos, Rect, Size};
use retroglyph_core::text::truncate_measured;

use super::{Menu, MenuRow};
use crate::Surface;
use crate::align::Align;
use crate::draw::fill_rect;
use crate::interact::Sense;
use crate::layout::{Side, anchored_rect};
use crate::state::MenuBarState;
use crate::text::draw_clipped;
use crate::theme::Theme;
use crate::ui::Ui;

/// The three [`Id`](crate::interact::Interaction)s [`MenuBar::show`] needs.
///
/// One for the label row itself, and the two [`Menu::show`] needs internally for the popup it
/// opens (its own content, and the full-`screen` backdrop that closes it on an outside click).
/// Bundled into one argument rather than three separate ones so [`MenuBar::show`] itself stays
/// under the `clippy` argument-count lint its unbundled predecessor tripped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuBarIds<Id> {
    /// Identifies the label row for hit-testing.
    pub label: Id,
    /// Identifies the open popup's own content region; see [`Menu::show`].
    pub popup_content: Id,
    /// Identifies the open popup's full-`screen` backdrop; see [`Menu::show`].
    pub popup_backdrop: Id,
}

/// A horizontal row of `titles`, each anchoring the [`Menu`] built from the matching slice of
/// `rows`: a "File Edit View" menu bar.
///
/// The app-specific layer a dropdown's own anchored-popup core ([`Menu`]) doesn't have an
/// opinion on. `titles` and `rows` are parallel slices (`rows[i]` is the popup shown for `titles[i]`); a
/// mismatched length is a caller bug the same way it would be for two `Vec`s meant to stay in
/// lockstep, not something this widget can check for you.
///
/// Interaction lives entirely in [`MenuBar::show`]/[`MenuBar::handle_event`], mirroring the
/// click-and-keyboard half of [`Menu::show`]/[`MenuState::handle_event`](crate::state::MenuState::handle_event):
///
/// - A click on a label [toggles](crate::state::MenuBarState::toggle) that label's popup open or
///   closed.
/// - Hovering a *different* label while one is already open switches to it. A cold hover (no
///   popup open yet) does nothing: this is deliberately not a `Button`-style hover highlight
///   that opens on its own, matching how a desktop menu bar only starts following the pointer
///   once you've clicked into it once.
/// - [`MenuBar::handle_event`]'s Left/Right step which label is open (clamped at the ends, not
///   wrapping, matching [`MenuState::select_next`](crate::state::MenuState::select_next)'s own
///   default); Up/Down/Enter/Escape forward to the open label's own
///   [`MenuState::handle_event`](crate::state::MenuState::handle_event).
///
/// `style`/`open_style` name the unopened/opened label styles (not `selected_style`: nothing
/// here is a persistent selection the way [`Tabs`](super::Tabs)'s is, it's literally "this
/// label's popup is open right now" and reverts the instant it closes). Both default to
/// [`Theme::DARK`] (as if [`MenuBar::theme`] had been called), and that same theme is forwarded
/// to the [`Menu`] popup [`MenuBar::show`] builds internally, so the bar and its popups always
/// read as one themed unit.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::state::MenuBarState;
/// use retroglyph_ui::widget::{Marker, MenuBar, MenuRow};
/// use retroglyph_ui::Surface;
///
/// let file_rows = [
///     MenuRow::Item { label: "New", marker: Marker::None, enabled: true },
///     MenuRow::Item { label: "Open", marker: Marker::None, enabled: true },
/// ];
/// let edit_rows = [
///     MenuRow::Item { label: "Cut", marker: Marker::None, enabled: true },
/// ];
/// let titles = ["File", "Edit"];
/// let rows: [&[MenuRow]; 2] = [&file_rows, &edit_rows];
///
/// let bar = MenuBar::new(&titles, &rows);
/// let mut state = MenuBarState::new();
///
/// let area = Rect::new(0, 0, 20, 1);
/// let mut grid = Grid::new(20, 1);
/// bar.draw(&mut Surface::new(&mut grid, area, 0), state.open_label());
/// ```
#[derive(Clone, Copy, Debug)]
pub struct MenuBar<'a> {
    titles: &'a [&'a str],
    rows: &'a [&'a [MenuRow<'a>]],
    style: Style,
    open_style: Style,
    column_spacing: u16,
    theme: Theme,
}

impl<'a> MenuBar<'a> {
    /// A menu bar over `titles`, each anchoring the popup built from the matching entry of
    /// `rows`, styled from [`Theme::DARK`] (as if [`MenuBar::theme`] had been called).
    #[must_use]
    pub fn new(titles: &'a [&'a str], rows: &'a [&'a [MenuRow<'a>]]) -> Self {
        Self {
            titles,
            rows,
            style: Style::new(),
            open_style: Style::new(),
            column_spacing: 1,
            theme: Theme::DARK,
        }
        .theme(Theme::DARK)
    }

    /// Set the style of an unopened label.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style of the label whose popup is currently open.
    #[must_use]
    pub const fn open_style(mut self, style: Style) -> Self {
        self.open_style = style;
        self
    }

    /// Set the number of blank columns between labels.
    #[must_use]
    pub const fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Applies `theme`'s named roles to this bar and the [`Menu`] popup [`MenuBar::show`] builds
    /// from it: `style` becomes `theme.dim` on `theme.panel_bg`, `open_style` becomes `theme.bg`
    /// on `theme.accent` (the same unselected/selected mapping [`Tabs::theme`](super::Tabs::theme)
    /// uses), and the popup itself is themed via [`Menu::theme`].
    ///
    /// Call before any manual [`MenuBar::style`]/[`MenuBar::open_style`] override you want to
    /// keep.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.style = Style::new().fg(theme.dim).bg(theme.panel_bg);
        self.open_style = Style::new().fg(theme.bg).bg(theme.accent);
        self.theme = theme;
        self
    }
}

impl MenuBar<'_> {
    /// Each drawn label's `(index, start_x, text_width)`, left to right, stopping once a title
    /// would start past `area`'s right edge: the same layout [`draw`](Self::draw) paints and
    /// [`label_at`](Self::label_at) hit-tests against. `x` values are absolute grid coordinates,
    /// matching `area`'s own space (the same space [`Response::pointer_pos`](crate::interact::Response::pointer_pos)
    /// reports in), the same convention [`Tabs::tab_columns`](super::Tabs) uses.
    fn label_columns(&self, area: Rect) -> Vec<(usize, u16, u16)> {
        let mut columns = Vec::with_capacity(self.titles.len());
        let mut x = area.left();
        for (index, &title) in self.titles.iter().enumerate() {
            if x >= area.right() {
                break;
            }
            // x < area.right() per the break check above, so this subtraction fits a u16.
            let avail = area.right() - x;
            let (_text, text_width) = truncate_measured(title, avail);
            columns.push((index, x, text_width));
            x = x.saturating_add(text_width);

            if index + 1 < self.titles.len() {
                x = x.saturating_add(self.column_spacing);
            }
        }
        columns
    }

    /// Draw this bar into `surface.area()`, highlighting `open` (if any) with
    /// [`MenuBar::open_style`].
    pub fn draw(&self, surface: &mut Surface<'_>, open: Option<usize>) {
        let area = surface.area();
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        let columns = self.label_columns(area);
        for &(index, abs_x, text_width) in &columns {
            let x = abs_x - area.left();
            let title = self.titles[index];
            let style = if Some(index) == open {
                self.open_style
            } else {
                self.style
            };
            if Some(index) == open && text_width > 0 {
                fill_rect(
                    surface,
                    Rect::new(x, 0, text_width, 1),
                    ' ',
                    Style::new().bg(style.background()),
                );
            }
            let _ = draw_clipped(surface, (x, 0), text_width, title, Align::Left, style);
        }
    }

    /// The index of the label whose column range contains `pos`, or `None` if `pos` falls in the
    /// spacing between labels, past the last drawn label, or outside `area` entirely.
    fn label_at(&self, area: Rect, pos: Pos) -> Option<usize> {
        if !area.contains_pos(pos) {
            return None;
        }
        self.label_columns(area)
            .into_iter()
            .find(|&(_, x, width)| pos.x >= x && pos.x < x + width)
            .map(|(index, _, _)| index)
    }

    /// `label`'s own rect within `area`: `label_columns`' entry for it, or `area` unchanged if
    /// `label` is out of range (defensive only; every caller here gets `label` from
    /// [`MenuBarState::open_label`](crate::state::MenuBarState::open_label), which never reports
    /// an index [`MenuBar::show`] didn't just draw).
    fn label_rect(&self, area: Rect, label: usize) -> Rect {
        self.label_columns(area)
            .into_iter()
            .find(|&(index, ..)| index == label)
            .map_or(area, |(_, x, text_width)| {
                Rect::new(x, area.top(), text_width.max(1), area.height())
            })
    }

    /// `label`'s row count and selectable predicate, as [`MenuState`](crate::state::MenuState)'s
    /// own methods want them.
    fn rows_of(&self, label: usize) -> (usize, impl Fn(usize) -> bool + '_) {
        let rows = self.rows[label];
        (rows.len(), move |i: usize| rows[i].is_selectable())
    }

    /// Draw and interact with this bar, and whichever label's popup [`MenuBarState::open_label`]
    /// reports, for one frame.
    ///
    /// The popup (if any) is shown *before* the label row itself is registered/drawn, the
    /// reverse of [`Menu::show`]'s own internal ordering of its backdrop-then-content: `show`
    /// there registers a full-`screen` backdrop so a click anywhere outside the popup closes it,
    /// but that backdrop would otherwise sit on top of (and shadow hit-testing for) every label
    /// in this bar, including the very one whose click should switch or close it. Drawing/
    /// registering the label row after the popup puts the bar back on top for hit-testing (see
    /// [`Ui::modal`]'s "later registered wins" rule), so a click or hover on any label -- open
    /// one included -- always reaches the bar, and only a click truly outside both the bar and
    /// the popup reaches the backdrop.
    ///
    /// A consequence of that ordering: the popup for `state`'s own change (open, close, or
    /// switch) applied in this same call -- from a click resolved below, or from
    /// [`MenuBar::handle_event`] called earlier this frame -- isn't drawn until the *next*
    /// frame's `show`, since this call already decided what to draw from `state.open_label()`
    /// before that change happens. One extra frame on top of the usual one-frame click-latency
    /// this crate already has everywhere (see [`Interaction`](crate::interact::Interaction)),
    /// not a bug: correctly prioritizing the bar over the popup's own backdrop for every
    /// *subsequent* frame (so a click or hover can still reach and switch/close it) requires
    /// registering the bar after the popup, which is exactly what makes the fresh transition
    /// itself one frame slower to render.
    ///
    /// `popup_size` is used for every label's popup; `bar_area` anchors each label's own popup
    /// [`Side::Below`] itself via [`anchored_rect`], clamped to `screen`.
    ///
    /// Returns `Some((label, row))` for the one frame a selectable row in `label`'s popup is
    /// clicked, the same "checkbox items fire and close" contract [`Menu::show`] has.
    #[must_use]
    pub fn show<Id: Copy + PartialEq>(
        &self,
        ui: &mut Ui<'_, '_, Id>,
        screen: Rect,
        bar_area: Rect,
        popup_size: Size,
        ids: MenuBarIds<Id>,
        state: &mut MenuBarState,
    ) -> Option<(usize, usize)> {
        let mut activated = None;

        if let Some(open) = state.open_label() {
            let anchor = self.label_rect(bar_area, open);
            let popup_area = anchored_rect(anchor, popup_size, Side::Below, screen);
            if let Some(row) = Menu::new(self.rows[open]).theme(self.theme).show(
                ui,
                screen,
                popup_area,
                ids.popup_content,
                ids.popup_backdrop,
                state.menu_mut(),
            ) {
                activated = Some((open, row));
            }
        }

        let (response, mut surface) = ui.region(bar_area, ids.label, Sense::click() | Sense::HOVER);
        self.draw(&mut surface, state.open_label());

        let hovered_label = response
            .pointer_pos()
            .and_then(|pos| self.label_at(bar_area, pos));

        if response.clicked()
            && let Some(label) = hovered_label
        {
            let (len, selectable) = self.rows_of(label);
            state.toggle(label, len, selectable);
        }

        match hovered_label {
            Some(label) => {
                let (len, selectable) = self.rows_of(label);
                state.hover(Some(label), len, selectable);
            }
            None => state.hover(None, 0, |_| false),
        }

        activated
    }

    /// Apply one keyboard event to `state`: Left/Right move which label is open, clamped at the
    /// ends (see the type docs); every other event forwards to the open label's own
    /// [`MenuState::handle_event`](crate::state::MenuState::handle_event), returning
    /// `Some((label, row))` for the one frame Enter activates a row.
    ///
    /// A no-op, on every key, while nothing is open: Left/Right don't open a label from cold any
    /// more than a hover does (see the type docs), and there is no open label's `MenuState` to
    /// forward anything else to.
    pub fn handle_event(&self, event: &Event, state: &mut MenuBarState) -> Option<(usize, usize)> {
        let open = state.open_label()?;

        if let Event::Key(key) = event
            && key.is_down()
        {
            let delta = match key.code {
                KeyCode::Left => -1,
                KeyCode::Right => 1,
                _ => 0,
            };
            if delta != 0 {
                self.step_open_label(state, open, delta);
                return None;
            }
        }

        let (len, selectable) = self.rows_of(open);
        let row = state.menu_mut().handle_event(event, len, selectable)?;
        Some((open, row))
    }

    /// Move the open label by `delta` (`1` or `-1`), clamped to `0..titles.len()`, reopening the
    /// new label's popup. A no-op if that clamps back to `current` (already at an end) or there
    /// are no labels at all.
    fn step_open_label(&self, state: &mut MenuBarState, current: usize, delta: i32) {
        let Ok(len) = i32::try_from(self.titles.len()) else {
            return; // absurdly large `titles`; leave the open label alone
        };
        if len == 0 {
            return;
        }
        let Ok(current_i32) = i32::try_from(current) else {
            return;
        };
        let next = (current_i32 + delta).clamp(0, len - 1);
        // `0 <= next < len`, and `len` came from a `usize` above, so this fits a `usize`.
        #[allow(clippy::cast_sign_loss)]
        let next = next as usize;
        if next != current {
            let (len, selectable) = self.rows_of(next);
            state.open(next, len, selectable);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::event::{
        Event, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use retroglyph_core::grid::Grid;

    use super::*;
    use crate::interact::Interaction;
    use crate::widget::Marker;

    const FILE_ROWS: [MenuRow<'static>; 2] = [
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
    ];
    const EDIT_ROWS: [MenuRow<'static>; 1] = [MenuRow::Item {
        label: "Cut",
        marker: Marker::None,
        enabled: true,
    }];
    const VIEW_ROWS: [MenuRow<'static>; 1] = [MenuRow::Item {
        label: "Zoom",
        marker: Marker::None,
        enabled: true,
    }];

    const TITLES: [&str; 3] = ["File", "Edit", "View"];

    fn rows() -> [&'static [MenuRow<'static>]; 3] {
        [&FILE_ROWS, &EDIT_ROWS, &VIEW_ROWS]
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Id {
        Bar,
        PopupContent,
        PopupBackdrop,
    }

    fn show(
        bar: &MenuBar<'_>,
        interaction: &mut Interaction<Id>,
        grid: &mut Grid,
        screen: Rect,
        state: &mut MenuBarState,
    ) -> Option<(usize, usize)> {
        interaction.frame(&mut Surface::new(grid, screen, 0), |ui| {
            bar.show(
                ui,
                screen,
                Rect::new(0, 0, 20, 1),
                Size::new(10, 4),
                MenuBarIds {
                    label: Id::Bar,
                    popup_content: Id::PopupContent,
                    popup_backdrop: Id::PopupBackdrop,
                },
                state,
            )
        })
    }

    fn click(interaction: &mut Interaction<Id>, pos: Pos) {
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            pos,
            KeyModifiers::NONE,
        )));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent::new(
            MouseEventKind::Up(MouseButton::Left),
            pos,
            KeyModifiers::NONE,
        )));
    }

    fn hover(interaction: &mut Interaction<Id>, pos: Pos) {
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent::new(
            MouseEventKind::Moved,
            pos,
            KeyModifiers::NONE,
        )));
    }

    #[test]
    fn draws_every_title_left_to_right() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        bar.draw(&mut Surface::new(&mut grid, area, 0), None);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'F'); // "File"
        // "File" (4) + column_spacing (1) = "Edit" starts at column 5.
        assert_eq!(grid[Pos::new(5, 0)].glyph(), 'E');
    }

    #[test]
    fn cold_hover_does_not_open_anything() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        hover(&mut interaction, Pos::new(0, 0)); // over "File", but never clicked

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        assert_eq!(state.open_label(), None);
    }

    #[test]
    fn click_opens_the_clicked_label() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        click(&mut interaction, Pos::new(0, 0)); // over "File"

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        assert_eq!(state.open_label(), Some(0));
    }

    #[test]
    fn click_again_closes_the_same_label() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        click(&mut interaction, Pos::new(0, 0)); // over "File" again

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        assert_eq!(state.open_label(), None);
    }

    #[test]
    fn hovering_another_label_while_one_is_open_switches_to_it() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        hover(&mut interaction, Pos::new(5, 0)); // over "Edit", no click

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        assert_eq!(state.open_label(), Some(1));
    }

    #[test]
    fn clicking_a_row_activates_it_and_closes_the_bar() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        // Frame 1 registers the open popup's rows.
        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        // "File"'s popup is anchored below `bar_area` (y=1), "Open" is its second row (y=2).
        click(&mut interaction, Pos::new(1, 2));

        let mut grid = Grid::new(20, 10);
        let activated = show(&bar, &mut interaction, &mut grid, screen, &mut state);

        assert_eq!(activated, Some((0, 1)));
        assert_eq!(state.open_label(), None);
    }

    #[test]
    fn clicking_outside_the_bar_and_popup_closes_without_activating() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        click(&mut interaction, Pos::new(19, 9)); // far outside both the bar and the popup

        let mut grid = Grid::new(20, 10);
        let activated = show(&bar, &mut interaction, &mut grid, screen, &mut state);

        assert_eq!(activated, None);
        assert_eq!(state.open_label(), None);
    }

    #[test]
    fn right_arrow_moves_to_the_next_label() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        let right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        let activated = bar.handle_event(&right, &mut state);

        assert_eq!(activated, None);
        assert_eq!(state.open_label(), Some(1));
    }

    #[test]
    fn left_right_clamp_at_the_ends_without_wrapping() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        let left = Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        bar.handle_event(&left, &mut state);
        assert_eq!(state.open_label(), Some(0)); // clamped: already at the first label
    }

    #[test]
    fn left_right_are_a_no_op_while_nothing_is_open() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut state = MenuBarState::new();

        let right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        bar.handle_event(&right, &mut state);
        assert_eq!(state.open_label(), None); // no cold-open, matching a cold hover
    }

    #[test]
    fn arrow_down_then_enter_navigates_and_activates_the_open_popup() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut state = MenuBarState::new();
        state.open(0, FILE_ROWS.len(), |i| FILE_ROWS[i].is_selectable());

        let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        bar.handle_event(&down, &mut state);
        assert_eq!(state.menu().selected(), Some(1)); // moved from "New" to "Open"

        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let activated = bar.handle_event(&enter, &mut state);
        assert_eq!(activated, Some((0, 1)));
        assert_eq!(state.open_label(), None);
    }

    /// A pointer that never moves off the label it opened must not immediately undo a
    /// keyboard-driven `handle_event` switch to a different label: re-resolving "is the pointer
    /// over a different label than what's open" fresh every frame (rather than only on a
    /// genuinely new hover) would otherwise snap `open_label` straight back the very next frame
    /// `show` runs, since the stale, unmoved pointer position still resolves over the label that
    /// was open *before* the keypress.
    #[test]
    fn a_stationary_pointer_does_not_fight_a_keyboard_driven_switch() {
        let rows = rows();
        let bar = MenuBar::new(&TITLES, &rows);
        let mut interaction = Interaction::<Id>::new();
        let screen = Rect::new(0, 0, 20, 10);
        let mut state = MenuBarState::new();

        // Frame 1: register the bar's hitbox, with the pointer already resolved over "File".
        hover(&mut interaction, Pos::new(0, 0));
        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);

        // Click "File" open; the pointer stays at the same position throughout (no further
        // `hover` call).
        click(&mut interaction, Pos::new(0, 0));
        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);
        assert_eq!(state.open_label(), Some(0));

        // Keyboard-driven switch to "Edit", with the pointer still resting over "File".
        let right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        bar.handle_event(&right, &mut state);
        assert_eq!(state.open_label(), Some(1));

        // The next `show` must not resolve the still-stationary pointer over "File" as a fresh
        // hover and switch back to it.
        let mut grid = Grid::new(20, 10);
        show(&bar, &mut interaction, &mut grid, screen, &mut state);
        assert_eq!(state.open_label(), Some(1));
    }
}
