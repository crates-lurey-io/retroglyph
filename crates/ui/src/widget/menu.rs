//! [`Menu`]: an anchored, scrollable, keyboard-navigable popup, the shared core a dropdown, a
//! combobox's list, and a command palette all draw from.
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::{Pos, Rect};

use super::StatefulWidget;
use crate::Surface;
use crate::align::Align;
use crate::draw::fill_rect;
use crate::interact::Sense;
use crate::state::MenuState;
use crate::text::draw_clipped;
use crate::theme::Theme;
use crate::ui::Ui;

/// A leading marker glyph for a [`MenuRow::Item`], drawn before its label.
///
/// Purely a rendering choice: every [`MenuRow::Item`] activates and closes the menu the same
/// way regardless of its marker (dreadmire's "checkbox items fire and close" is true because
/// there is no special-cased checkbox *behavior* here, only a checkbox *glyph*). Grouping
/// [`Choice`](Self::Choice) rows into a mutually-exclusive set (so selecting one clears the
/// others) is the caller's job, the same way this crate leaves "which tab is active" bookkeeping
/// to whatever drives [`Tabs`](super::Tabs): set `Choice(true)` on exactly one row per group and
/// update it on activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// No marker: the label starts at the row's left edge.
    None,
    /// A checkbox marker (`[x]`/`[ ]`), independently toggleable.
    Checkbox(bool),
    /// A radio/choice marker (`(o)`/`( )`), typically one-of-many within a caller-managed group.
    Choice(bool),
}

impl Marker {
    /// The literal text drawn before the label, including its trailing space.
    const fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Checkbox(true) => "[x] ",
            Self::Checkbox(false) => "[ ] ",
            Self::Choice(true) => "(o) ",
            Self::Choice(false) => "( ) ",
        }
    }
}

/// One row of a [`Menu`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuRow<'a> {
    /// A selectable, activatable row.
    Item {
        /// The row's label text.
        label: &'a str,
        /// A leading checkbox/choice glyph, or [`Marker::None`] for a plain item.
        marker: Marker,
        /// Whether this row can be selected/activated. A disabled item is drawn with
        /// [`Menu::disabled_style`] and skipped by keyboard navigation and clicks alike, the
        /// same as a [`Separator`](Self::Separator)/[`Label`](Self::Label).
        enabled: bool,
    },
    /// A non-selectable divider line.
    Separator,
    /// A non-selectable heading. Drawn like an [`Item`](Self::Item) with no marker, but never
    /// selected by navigation or a click, the same as a disabled item.
    Label(&'a str),
}

impl MenuRow<'_> {
    /// Whether this row can be selected/activated: `true` only for an enabled
    /// [`Item`](Self::Item).
    #[must_use]
    pub const fn is_selectable(&self) -> bool {
        matches!(self, Self::Item { enabled: true, .. })
    }
}

/// How many of a menu's rows a given `capacity` can show, and whether an overflow
/// indicator is needed at either end.
struct Layout {
    show_top: bool,
    show_bottom: bool,
    content_capacity: usize,
}

/// An anchored, scrollable, keyboard-navigable popup: the shared core of a dropdown, a
/// combobox's open list, and a command palette.
///
/// Draws whatever `capacity = surface.height()` of this menu's own rows the current
/// [`MenuState::offset`] window covers, top-aligned. When more rows exist above/below that
/// window, the first/last drawn row becomes a `^`/`v` overflow indicator instead of silently
/// truncating -- call [`MenuState::ensure_visible`] before rendering (as [`Menu::show`] already
/// does) to keep the selection inside the window.
///
/// This is the "anchored-popup core" alone: it draws rows and resolves a click/scroll/keyboard
/// gesture against them, but has no opinion on *where* the popup sits relative to whatever
/// anchored it (a menu-bar label, a combobox field, ...) -- that placement is the caller's job,
/// same as [`Modal`](super::Modal) leaves centering to [`centered_rect`](crate::layout::centered_rect)
/// rather than owning it itself.
///
/// [`Menu::show`] is the documented way to drive one open menu end-to-end each frame (drawing,
/// hover/click/scroll, and click-outside dismissal); [`StatefulWidget::render`] is the lower-level
/// draw-only half for a caller composing its own interaction (e.g. a menu bar switching which of
/// several menus is open).
///
/// `item_style`/`selected_style`/`disabled_style`/`separator_style`/`label_style`/
/// `overflow_style` default to [`Theme::DARK`] (as if [`Menu::theme`] had been called); set a
/// different [`Theme`] with [`Menu::theme`]/[`Menu::theme_on`] or override a single field with
/// the matching builder method.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::state::MenuState;
/// use retroglyph_ui::widget::{Marker, Menu, MenuRow, StatefulWidget};
/// use retroglyph_ui::Surface;
///
/// let rows = [
///     MenuRow::Item { label: "Cut", marker: Marker::None, enabled: false },
///     MenuRow::Separator,
///     MenuRow::Item { label: "Copy", marker: Marker::None, enabled: true },
///     MenuRow::Item { label: "Word Wrap", marker: Marker::Checkbox(true), enabled: true },
/// ];
/// let mut state = MenuState::new();
/// state.open(rows.len(), |i| rows[i].is_selectable());
///
/// let area = Rect::new(0, 0, 16, 4);
/// let mut grid = Grid::new(16, 4);
/// Menu::new(&rows).render(&mut Surface::new(&mut grid, area, 0), &mut state);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Menu<'a> {
    rows: &'a [MenuRow<'a>],
    item_style: Style,
    selected_style: Style,
    disabled_style: Style,
    separator_style: Style,
    label_style: Style,
    overflow_style: Style,
}

impl<'a> Menu<'a> {
    /// A menu over `rows`, styled from [`Theme::DARK`] (as if [`Menu::theme`] had been called).
    #[must_use]
    pub fn new(rows: &'a [MenuRow<'a>]) -> Self {
        Self {
            rows,
            item_style: Style::new(),
            selected_style: Style::new(),
            disabled_style: Style::new(),
            separator_style: Style::new(),
            label_style: Style::new(),
            overflow_style: Style::new(),
        }
        .theme(Theme::DARK)
    }

    /// Set the style of a plain, enabled, unselected [`MenuRow::Item`].
    #[must_use]
    pub const fn item_style(mut self, style: Style) -> Self {
        self.item_style = style;
        self
    }

    /// Set the style of the selected row, including its background fill.
    #[must_use]
    pub const fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Set the style of a disabled [`MenuRow::Item`].
    #[must_use]
    pub const fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set the style of a [`MenuRow::Separator`] line.
    #[must_use]
    pub const fn separator_style(mut self, style: Style) -> Self {
        self.separator_style = style;
        self
    }

    /// Set the style of a [`MenuRow::Label`] row.
    #[must_use]
    pub const fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Set the style of the `^`/`v` overflow indicator rows.
    #[must_use]
    pub const fn overflow_style(mut self, style: Style) -> Self {
        self.overflow_style = style;
        self
    }

    /// Applies `theme`'s named roles: `item_style`/`label_style` become `theme.fg` on
    /// `theme.panel_bg`, `selected_style` becomes `theme.bg` on `theme.accent`, `disabled_style`
    /// becomes `theme.dim` on `theme.panel_bg`, and `separator_style`/`overflow_style` become
    /// `theme.border` on `theme.panel_bg`.
    ///
    /// Call before any manual per-field override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Menu::theme`], but every style is drawn on `bg` instead of `theme.panel_bg`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.item_style = Style::new().fg(theme.fg).bg(bg);
        self.label_style = Style::new().fg(theme.fg).bg(bg);
        self.selected_style = Style::new().fg(theme.bg).bg(theme.accent);
        self.disabled_style = Style::new().fg(theme.dim).bg(bg);
        self.separator_style = Style::new().fg(theme.border).bg(bg);
        self.overflow_style = Style::new().fg(theme.border).bg(bg);
        self
    }
}

impl Menu<'_> {
    /// How many of a menu's rows a `capacity`-row surface can show from `state.offset()`
    /// onward, and whether either end needs an overflow indicator instead of a real row.
    fn layout(&self, state: &MenuState, capacity: usize) -> Layout {
        if capacity == 0 {
            return Layout {
                show_top: false,
                show_bottom: false,
                content_capacity: 0,
            };
        }
        let show_top = state.offset() > 0;
        let mut content_capacity = capacity - usize::from(show_top);
        let show_bottom = self.rows.len().saturating_sub(state.offset()) > content_capacity;
        if show_bottom {
            content_capacity = content_capacity.saturating_sub(1);
        }
        Layout {
            show_top,
            show_bottom,
            content_capacity,
        }
    }

    /// The shared drawing routine.
    fn draw(&self, surface: &mut Surface<'_>, state: &MenuState) {
        let (width, height) = (surface.width(), surface.height());
        if width == 0 || height == 0 || self.rows.is_empty() {
            return;
        }
        let layout = self.layout(state, usize::from(height));

        let mut y: u16 = 0;
        if layout.show_top {
            let _ = draw_clipped(
                surface,
                (0, y),
                width,
                "^",
                Align::Left,
                self.overflow_style,
            );
            y += 1;
        }
        for (index, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(state.offset())
            .take(layout.content_capacity)
        {
            let is_selected = Some(index) == state.selected();
            let style = match row {
                MenuRow::Separator => self.separator_style,
                MenuRow::Label(_) => self.label_style,
                MenuRow::Item { enabled: false, .. } => self.disabled_style,
                MenuRow::Item { .. } if is_selected => self.selected_style,
                MenuRow::Item { .. } => self.item_style,
            };
            if is_selected {
                fill_rect(
                    surface,
                    Rect::new(0, y, width, 1),
                    ' ',
                    Style::new().bg(self.selected_style.background()),
                );
            }
            match row {
                MenuRow::Separator => {
                    fill_rect(surface, Rect::new(0, y, width, 1), '\u{2500}', style);
                }
                MenuRow::Label(text) => {
                    let _ = draw_clipped(surface, (0, y), width, text, Align::Left, style);
                }
                MenuRow::Item { label, marker, .. } => {
                    let marker_text = marker.text();
                    let marker_width = retroglyph_core::text::width(marker_text).min(width);
                    if marker_width > 0 {
                        let _ = draw_clipped(
                            surface,
                            (0, y),
                            marker_width,
                            marker_text,
                            Align::Left,
                            style,
                        );
                    }
                    let _ = draw_clipped(
                        surface,
                        (marker_width, y),
                        width - marker_width,
                        label,
                        Align::Left,
                        style,
                    );
                }
            }
            y += 1;
        }
        if layout.show_bottom {
            let _ = draw_clipped(
                surface,
                (0, y),
                width,
                "v",
                Align::Left,
                self.overflow_style,
            );
        }
    }

    /// The row index at `pos`, given `state`'s current scroll offset, or `None` if `pos` falls
    /// on an overflow indicator, past the last drawn row, or outside `area`.
    fn index_at(&self, area: Rect, state: &MenuState, pos: Pos) -> Option<usize> {
        let capacity = usize::from(area.height());
        let layout = self.layout(state, capacity);
        let row = usize::from(pos.y.checked_sub(area.top())?);
        let content_row = row.checked_sub(usize::from(layout.show_top))?;
        if content_row >= layout.content_capacity {
            return None; // the top/bottom overflow indicator, or past the window entirely
        }
        let index = state.offset() + content_row;
        (index < self.rows.len()).then_some(index)
    }
}

impl StatefulWidget for Menu<'_> {
    type State = MenuState;

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State) {
        self.draw(surface, state);
    }
}

impl Menu<'_> {
    /// Draw and interact with this menu for one frame: the documented way to drive an anchored
    /// popup end-to-end, exercising the same primitives a menu bar/combobox/command palette all
    /// need.
    ///
    /// A no-op (returns `None`, draws nothing) while `state.is_open()` is `false`. Otherwise:
    ///
    /// - Registers `screen` for `backdrop_id` *before* opening a [`Ui::modal`] scoped to
    ///   `popup_area`: because a modal's barrier only blocks hit-testing *inside* `popup_area`
    ///   (see [`Ui::modal`]'s docs), a click landing anywhere in `screen` but outside
    ///   `popup_area` still reaches the backdrop and is treated as "outside every hitbox",
    ///   closing the menu without activating anything. A click inside `popup_area` is claimed by
    ///   the popup's own rows instead, since those are registered *after* the barrier and always
    ///   win inside it.
    /// - Draws the window of rows `state.offset()` selects (calling
    ///   [`MenuState::ensure_visible`] first so the selection is never scrolled out of view), and
    ///   applies a hover/click/wheel-scroll gesture the same way [`Menu::render`] alone would if
    ///   driven through [`Ui::show_stateful`].
    /// - Returns `Some(row_index)` for the one frame a selectable row is clicked, closing the
    ///   menu as a side effect ("checkbox items fire and close"). Returns `None` on every other
    ///   frame, including the one where an outside click closes the menu.
    ///
    /// `content_id` and `backdrop_id` must be distinct: sharing one `Id` between them would make
    /// both resolve the same hit-test winner, breaking the inside/outside distinction above.
    ///
    /// Keyboard navigation/activation (Up/Down/Enter/Escape) is not handled here: feed raw
    /// events to [`MenuState::handle_event`] instead, in your event-handling loop, the same split
    /// [`TextInputState::handle_event`](crate::state::TextInputState::handle_event) has from drawing.
    #[must_use]
    pub fn show<Id: Copy + PartialEq>(
        &self,
        ui: &mut Ui<'_, '_, Id>,
        screen: Rect,
        popup_area: Rect,
        content_id: Id,
        backdrop_id: Id,
        state: &mut MenuState,
    ) -> Option<usize> {
        if !state.is_open() {
            return None;
        }

        let backdrop = ui.region(screen, backdrop_id, Sense::click()).0;

        let mut activated = None;
        ui.modal(popup_area, |inner| {
            let (response, mut surface) = inner.region(
                popup_area,
                content_id,
                Sense::click() | Sense::HOVER | Sense::SCROLL,
            );

            let scroll_delta = response.scroll_delta();
            if scroll_delta != 0 {
                state.scroll_by(scroll_delta);
            }
            if response.clicked()
                && let Some(pos) = response.pointer_pos()
                && let Some(index) = self.index_at(popup_area, state, pos)
                && self.rows[index].is_selectable()
            {
                state.select(Some(index));
                activated = Some(index);
            }

            state.ensure_visible(usize::from(popup_area.height()));
            self.draw(&mut surface, state);
        });

        if activated.is_some() || backdrop.clicked() {
            state.close();
        }

        activated
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::grid::{Grid, Pos};

    use super::*;
    use crate::interact::Interaction;

    const ROWS: [MenuRow<'static>; 5] = [
        MenuRow::Separator,
        MenuRow::Item {
            label: "Cut",
            marker: Marker::None,
            enabled: false,
        },
        MenuRow::Item {
            label: "Copy",
            marker: Marker::None,
            enabled: true,
        },
        MenuRow::Item {
            label: "Word Wrap",
            marker: Marker::Checkbox(true),
            enabled: true,
        },
        MenuRow::Label("More"),
    ];

    fn selectable(i: usize) -> bool {
        ROWS[i].is_selectable()
    }

    #[test]
    fn draws_a_row_per_line() {
        let area = Rect::new(0, 0, 20, 5);
        let mut grid = Grid::new(20, 5);
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable);

        StatefulWidget::render(
            &Menu::new(&ROWS),
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
        );

        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'C'); // "Cut"
        assert_eq!(grid[Pos::new(0, 2)].glyph(), 'C'); // "Copy"
        assert_eq!(grid[Pos::new(0, 3)].glyph(), '['); // checkbox marker
    }

    #[test]
    fn selected_row_is_highlighted() {
        let area = Rect::new(0, 0, 20, 5);
        let mut grid = Grid::new(20, 5);
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable); // selects "Copy" (index 2)

        StatefulWidget::render(
            &Menu::new(&ROWS),
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
        );

        let selected_bg = grid[Pos::new(0, 2)].style().background();
        let plain_bg = grid[Pos::new(0, 3)].style().background();
        assert_ne!(selected_bg, plain_bg);
    }

    #[test]
    fn overflow_draws_indicators_instead_of_silent_truncation() {
        let area = Rect::new(0, 0, 20, 3); // only 3 of 5 rows fit
        let mut grid = Grid::new(20, 3);
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable);
        state.set_offset(1); // window starts at "Cut", so there's overflow both ways

        StatefulWidget::render(
            &Menu::new(&ROWS),
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
        );

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '^');
        assert_eq!(grid[Pos::new(0, 2)].glyph(), 'v');
    }

    #[test]
    fn index_at_skips_the_overflow_indicator_rows() {
        let area = Rect::new(0, 0, 20, 3);
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable);
        state.set_offset(1);

        let menu = Menu::new(&ROWS);
        assert_eq!(menu.index_at(area, &state, Pos::new(0, 0)), None); // '^' row
        assert_eq!(menu.index_at(area, &state, Pos::new(0, 1)), Some(1)); // "Cut"
        assert_eq!(menu.index_at(area, &state, Pos::new(0, 2)), None); // 'v' row
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Id {
        Content,
        Backdrop,
    }

    #[test]
    fn show_returns_none_and_draws_nothing_while_closed() {
        let screen = Rect::new(0, 0, 20, 10);
        let popup = Rect::new(2, 2, 10, 5);
        let mut grid = Grid::new(20, 10);
        let mut state = MenuState::new();
        let mut interaction = Interaction::<Id>::new();

        let activated = interaction.frame(&mut Surface::new(&mut grid, screen, 0), |ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        assert_eq!(activated, None);
        assert_eq!(grid[Pos::new(2, 2)].glyph(), ' ');
    }

    // The three multi-frame tests below drive `Menu::show` through `WidgetHarness` instead of
    // hand-rolling `Interaction`'s register/feed/resolve frame pair: gated behind `testing` (see
    // `crate::testing::WidgetHarness`), since `menu.rs` itself isn't feature-gated but the
    // harness is.
    #[test]
    #[cfg(feature = "testing")]
    fn clicking_a_selectable_row_activates_and_closes() {
        use crate::testing::WidgetHarness;

        let screen = Rect::new(0, 0, 20, 10);
        let popup = Rect::new(0, 0, 20, 5); // "Copy" (index 2) draws at row y=2
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable);
        let mut harness = WidgetHarness::<Id>::new(20, 10);

        // Frame 1: register this frame's hit rects (nothing fed in yet).
        harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        harness.click(1, 2); // "Copy"'s row
        // Frame 2: feeds the click; the flags it sets aren't read until the next `step`.
        let _ = harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });
        // Frame 3: resolves it.
        let activated = harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        assert_eq!(activated, Some(2));
        assert!(!state.is_open());
    }

    #[test]
    #[cfg(feature = "testing")]
    fn clicking_outside_the_popup_closes_without_activating() {
        use crate::testing::WidgetHarness;

        let screen = Rect::new(0, 0, 20, 10);
        let popup = Rect::new(0, 0, 10, 5);
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable);
        let mut harness = WidgetHarness::<Id>::new(20, 10);

        harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        // Well outside `popup` (which only spans columns 0..10, rows 0..5).
        harness.click(15, 8);
        let _ = harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });
        let activated = harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        assert_eq!(activated, None);
        assert!(!state.is_open());
    }

    #[test]
    #[cfg(feature = "testing")]
    fn clicking_inside_the_popup_but_off_every_row_stays_open() {
        // retroglyph#1290: a click that lands inside `popup_area` but past the drawn rows (e.g.
        // in the empty space below a short menu) must not fall through to the backdrop: the
        // barrier blocks it from ever reaching `screen`'s registration.
        use crate::testing::WidgetHarness;

        let screen = Rect::new(0, 0, 20, 10);
        let popup = Rect::new(0, 0, 20, 8); // taller than the 5 rows it actually draws
        let mut state = MenuState::new();
        state.open(ROWS.len(), selectable);
        let mut harness = WidgetHarness::<Id>::new(20, 10);

        harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        harness.click(1, 7); // inside `popup`, past the last drawn row
        let _ = harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });
        let activated = harness.step(|ui| {
            Menu::new(&ROWS).show(ui, screen, popup, Id::Content, Id::Backdrop, &mut state)
        });

        assert_eq!(activated, None);
        assert!(state.is_open()); // still open: the click never reached the backdrop
    }
}
