//! [`Ui`]: a per-frame context pairing a [`Surface`] with an [`Interaction`].

use retroglyph_core::{Rect, Surface};

use crate::interact::{Interaction, Response, Sense};
use crate::widget::{InteractiveWidget, Measure, StatefulWidget, Widget};

/// Which way a cursor-based child `Ui` (see [`Ui::vertical`]/[`Ui::horizontal`]) stacks the
/// areas it allocates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Vertical,
    Horizontal,
}

/// The mutable allocation state behind a cursor-based child `Ui`: which axis it stacks along,
/// and how much of its area is still unclaimed.
struct Cursor {
    axis: Axis,
    remaining: Rect,
}

impl Cursor {
    /// Carve `primary` cells off the leading edge of `remaining` along `axis` (top for
    /// [`Axis::Vertical`], left for [`Axis::Horizontal`]), clipped to whatever's left -- like
    /// [`split_v`](crate::split_v)/[`split_h`](crate::split_h), this never overflows `remaining`
    /// -- and shrinks `remaining` by the same amount.
    fn allocate(&mut self, primary: u16) -> Rect {
        match self.axis {
            Axis::Vertical => {
                let h = primary.min(self.remaining.height());
                let rect = Rect::new(
                    self.remaining.left(),
                    self.remaining.top(),
                    self.remaining.width(),
                    h,
                );
                self.remaining = Rect::new(
                    self.remaining.left(),
                    self.remaining.top().saturating_add(h),
                    self.remaining.width(),
                    self.remaining.height() - h,
                );
                rect
            }
            Axis::Horizontal => {
                let w = primary.min(self.remaining.width());
                let rect = Rect::new(
                    self.remaining.left(),
                    self.remaining.top(),
                    w,
                    self.remaining.height(),
                );
                self.remaining = Rect::new(
                    self.remaining.left().saturating_add(w),
                    self.remaining.top(),
                    self.remaining.width() - w,
                    self.remaining.height(),
                );
                rect
            }
        }
    }

    /// Take everything left, leaving nothing for a later allocation from this cursor.
    fn allocate_rest(&mut self) -> Rect {
        let primary = match self.axis {
            Axis::Vertical => self.remaining.height(),
            Axis::Horizontal => self.remaining.width(),
        };
        self.allocate(primary)
    }
}

/// One frame's drawing surface and interaction state, together, so a call site names an
/// `area`/`id` once and gets both hit-testing and drawing from it: see [`show`](Self::show).
///
/// # Why two lifetimes
///
/// `Surface<'g>` holds a `&'g mut Grid`, which makes `Surface` invariant in `'g`: nothing can
/// shrink or otherwise reinterpret that lifetime once it is fixed. The surface borrow and the
/// grid borrow are therefore kept as two separate lifetime parameters here, `'s` (how long this
/// `Ui` itself, and the `&'s mut Surface` it holds, lives) and `'g` (how long the underlying grid
/// is borrowed for). Collapsing them into one, e.g. writing the field as `&'a mut Surface<'a>`,
/// forces `'a` to cover both uses at once: the invariance in `'g` then makes the borrow of the
/// surface last exactly as long as the grid borrow it is invariant over, so the surface (and the
/// grid behind it) stay borrowed, and therefore unusable, for the rest of `'a` even after the
/// `Ui` that held them is dropped. Two parameters let `'s` end (releasing the `Ui`'s borrow of
/// the surface) while `'g` keeps going, which is exactly what [`Interaction::frame`] relies on:
/// the surface passed in is usable again once the closure returns.
pub struct Ui<'s, 'g, Id> {
    surface: &'s mut Surface<'g>,
    interaction: &'s mut Interaction<Id>,
    enabled: bool,
    /// The active [`vertical`](Self::vertical)/[`horizontal`](Self::horizontal) allocation
    /// cursor, if any. `None` for a `Ui` fresh from [`new`](Self::new), or a child of
    /// [`enabled`](Self::enabled)/[`modal`](Self::modal): those two deliberately don't inherit
    /// an active cursor (see their docs), so [`show_sized`](Self::show_sized)/
    /// [`draw_sized`](Self::draw_sized)/[`show_auto`](Self::show_auto)/
    /// [`draw_auto`](Self::draw_auto)/[`vertical_sized`](Self::vertical_sized)/
    /// [`horizontal_sized`](Self::horizontal_sized) must be called on the `Ui`
    /// `vertical`/`horizontal` hand to your closure, not through those two.
    cursor: Option<&'s mut Cursor>,
}

impl<'s, 'g, Id> Ui<'s, 'g, Id> {
    /// A `Ui` pairing `surface` with `interaction` for one frame, enabled.
    #[must_use]
    pub const fn new(surface: &'s mut Surface<'g>, interaction: &'s mut Interaction<Id>) -> Self {
        Self {
            surface,
            interaction,
            enabled: true,
            cursor: None,
        }
    }

    /// Whether [`show`](Ui::show)/[`show_stateful`](Ui::show_stateful)/[`region`](Ui::region)
    /// calls through this context report their widgets as enabled: `retroglyph#602`.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The surface, for drawing that no widget in this crate covers.
    #[must_use]
    pub const fn surface(&mut self) -> &mut Surface<'g> {
        self.surface
    }

    /// The interaction context, for hit-testing/focus queries no method here covers.
    #[must_use]
    pub const fn interaction(&mut self) -> &mut Interaction<Id> {
        self.interaction
    }

    /// The region this `Ui`'s surface represents; see [`Surface::area`].
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.surface.area()
    }
}

impl<'g, Id: Copy + PartialEq> Ui<'_, 'g, Id> {
    /// A child context whose [`show`](Self::show)/[`show_stateful`](Self::show_stateful)/
    /// [`region`](Self::region) calls report `enabled`: `retroglyph#602`.
    ///
    /// Nesting only ever tightens: a child of a context already disabled via `enabled(false)`
    /// stays disabled regardless of what `enabled` this call passes, matching how egui's and
    /// `Dear ImGui`'s disabled scopes compose.
    ///
    /// The returned `Ui` never inherits an active [`vertical`](Self::vertical)/
    /// [`horizontal`](Self::horizontal) cursor, even if `self` has one: call
    /// [`show_sized`](Self::show_sized)/[`draw_sized`](Self::draw_sized)/etc. directly on the
    /// cursor `Ui`, and reach for `enabled` on the widget-level `show`/`show_stateful` instead
    /// if you need both.
    #[must_use]
    pub const fn enabled(&mut self, enabled: bool) -> Ui<'_, 'g, Id> {
        Ui {
            surface: self.surface,
            interaction: self.interaction,
            enabled: self.enabled && enabled,
            cursor: None,
        }
    }

    /// Hit-test `area` for `id` with `widget`'s own [`Sense`], then draw `widget` into `area`.
    ///
    /// This is the one-`id`-one-`area` guarantee the [`InteractiveWidget`]/[`Ui`] split exists
    /// for: `area` is registered for hit-testing and used to scope the surface the widget draws
    /// into from the same value, so the two cannot disagree.
    ///
    /// If this context is [`disabled`](Self::enabled), the returned [`Response`] still reports
    /// [`hovered`](Response::hovered) but never an activation: see
    /// [`Sense::DISABLED`](crate::Sense::DISABLED).
    #[must_use]
    pub fn show(
        &mut self,
        area: Rect,
        id: Id,
        widget: &impl InteractiveWidget<State = ()>,
    ) -> Response {
        self.show_stateful(area, id, widget, &mut ())
    }

    /// Like [`show`](Self::show), for an [`InteractiveWidget`] with externally owned `state`.
    #[must_use]
    pub fn show_stateful<W: InteractiveWidget + ?Sized>(
        &mut self,
        area: Rect,
        id: Id,
        widget: &W,
        state: &mut W::State,
    ) -> Response {
        let sense = widget.sense().disabled_if(!self.enabled);
        let response = self.interaction.interact(area, id, sense);
        widget.render(&mut self.surface.scope(area), state, response);
        response
    }

    /// Draw a non-interactive `widget` into `area`.
    pub fn draw(&mut self, area: Rect, widget: &impl Widget) {
        widget.render(&mut self.surface.scope(area));
    }

    /// Like [`draw`](Self::draw), for a [`StatefulWidget`] with externally owned `state`.
    pub fn draw_stateful<W: StatefulWidget + ?Sized>(
        &mut self,
        area: Rect,
        widget: &W,
        state: &mut W::State,
    ) {
        widget.render(&mut self.surface.scope(area), state);
    }

    /// Register `area` for `id` with `sense`, and hand back both the resolved [`Response`] and a
    /// surface scoped to `area`, for drawing this crate has no widget for.
    ///
    /// Like [`show`](Self::show), `area` is committed once, by this call, for both hit-testing
    /// and drawing, so the two cannot disagree.
    #[must_use]
    pub fn region(&mut self, area: Rect, id: Id, sense: Sense) -> (Response, Surface<'_>) {
        let sense = sense.disabled_if(!self.enabled);
        let response = self.interaction.interact(area, id, sense);
        (response, self.surface.scope(area))
    }

    /// Run `f` with a [`Ui`] whose widgets sit above everything shown so far, and whose pointer
    /// hits inside `area` never reach a widget registered *before* `f` (a menu bar, the screen
    /// behind a dropdown, ...), so an app doesn't have to answer "is the thing under this overlay
    /// still supposed to see this event" with its own bookkeeping. A widget registered *after*
    /// `f` still wins inside `area`, so an overlay has to be shown after the content it covers,
    /// not before it.
    ///
    /// The barrier is scoped to `area`: a pointer *outside* it reaches whatever's registered
    /// outside `f` exactly as if `modal` hadn't been called. A modal claims the region it covers,
    /// not the whole screen; pass a full-screen `area` for a blocking overlay. Drawing is
    /// unaffected either way, `f`'s `Ui` still draws to this `Ui`'s full surface unless it also
    /// narrows with [`show`](Self::show)/[`draw`](Self::draw)/[`scope`](retroglyph_core::Surface::scope)
    /// itself: `modal` only changes hit-testing.
    pub fn modal<R>(&mut self, area: Rect, f: impl FnOnce(&mut Ui<'_, 'g, Id>) -> R) -> R {
        self.interaction.push_barrier(area);
        let mut inner = Ui {
            surface: self.surface,
            interaction: self.interaction,
            enabled: self.enabled,
            cursor: None,
        };
        f(&mut inner)
    }

    /// Allocate `primary` cells from this `Ui`'s own cursor; see [`show_sized`](Self::show_sized).
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor: see [`vertical`](Self::vertical)/
    /// [`horizontal`](Self::horizontal).
    fn allocate(&mut self, primary: u16) -> Rect {
        self.cursor
            .as_deref_mut()
            .expect(
                "Ui::show_sized/draw_sized/vertical_sized/horizontal_sized require an active \
                 cursor: call them on the `Ui` that `vertical`/`horizontal` hands to your \
                 closure, not on a `Ui` fresh from `new`/`enabled`/`modal`",
            )
            .allocate(primary)
    }

    /// Take everything left in this `Ui`'s own cursor, or [`area`](Self::area) if it has none.
    fn allocate_rest(&mut self) -> Rect {
        self.cursor
            .as_deref_mut()
            .map_or_else(|| self.surface.area(), Cursor::allocate_rest)
    }

    /// The remaining width of this `Ui`'s own [`vertical`](Self::vertical) cursor, for
    /// [`show_auto`](Self::show_auto)/[`draw_auto`](Self::draw_auto) to measure against.
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor, or its cursor is [`horizontal`](Self::horizontal):
    /// [`Measure`] reports a height for a given width, not the other way around, so a
    /// horizontal cursor has nothing to size a column by.
    fn vertical_cursor_width(&self) -> u16 {
        match &self.cursor {
            Some(cursor) if cursor.axis == Axis::Vertical => cursor.remaining.width(),
            Some(_) => panic!(
                "Ui::show_auto/draw_auto require a vertical cursor: `Measure::height_for` takes \
                 a width, so a horizontal cursor has nothing to size a column by"
            ),
            None => panic!(
                "Ui::show_auto/draw_auto require an active cursor: call them on the `Ui` that \
                 `vertical`/`horizontal` hands to your closure, not on a `Ui` fresh from \
                 `new`/`enabled`/`modal`"
            ),
        }
    }

    /// Run `f` with a fresh vertical cursor claiming whatever's left: the rest of this `Ui`'s
    /// own cursor if it has one (so a `vertical` nested inside a `horizontal` row claims that
    /// row's remaining width), or the whole of [`area`](Self::area) if it doesn't (a `vertical`
    /// called directly inside [`Interaction::frame`], say).
    ///
    /// Widgets shown/drawn through the closure's `Ui` via [`show_sized`](Self::show_sized)/
    /// [`draw_sized`](Self::draw_sized)/[`show_auto`](Self::show_auto)/
    /// [`draw_auto`](Self::draw_auto) stack top-to-bottom, each claiming a horizontal strip of
    /// the cursor's remaining area sized by an explicit height or, for `show_auto`/`draw_auto`,
    /// by [`Measure::height_for`], and advancing the cursor by that strip's height -- so the
    /// call site never computes a `Rect` by hand. Content past the bottom of the cursor's area
    /// clips, the same way [`split_v`](crate::split_v) clips a pane that overflows `area`.
    ///
    /// Nests with [`horizontal`](Self::horizontal): the `Ui` handed to `f` is a plain `Ui`, so
    /// it can call `vertical`/`horizontal` again to start a flow along the other axis, scoped to
    /// whatever this cursor has left at that point.
    pub fn vertical<R>(&mut self, f: impl FnOnce(&mut Ui<'_, 'g, Id>) -> R) -> R {
        let area = self.allocate_rest();
        let mut cursor = Cursor {
            axis: Axis::Vertical,
            remaining: area,
        };
        let mut inner = Ui {
            surface: self.surface,
            interaction: self.interaction,
            enabled: self.enabled,
            cursor: Some(&mut cursor),
        };
        f(&mut inner)
    }

    /// Like [`vertical`](Self::vertical), stacking left-to-right instead of top-to-bottom.
    pub fn horizontal<R>(&mut self, f: impl FnOnce(&mut Ui<'_, 'g, Id>) -> R) -> R {
        let area = self.allocate_rest();
        let mut cursor = Cursor {
            axis: Axis::Horizontal,
            remaining: area,
        };
        let mut inner = Ui {
            surface: self.surface,
            interaction: self.interaction,
            enabled: self.enabled,
            cursor: Some(&mut cursor),
        };
        f(&mut inner)
    }

    /// Like [`vertical`](Self::vertical), but claims only `height` rows of this `Ui`'s own
    /// cursor (clipped to whatever's left) instead of everything remaining, so a fixed-size
    /// nested flow -- a one-row-tall horizontal button bar inside a vertical column, say --
    /// leaves the rest of the outer cursor for whatever comes after it.
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor: see [`vertical`](Self::vertical).
    pub fn vertical_sized<R>(
        &mut self,
        height: u16,
        f: impl FnOnce(&mut Ui<'_, 'g, Id>) -> R,
    ) -> R {
        let area = self.allocate(height);
        let mut cursor = Cursor {
            axis: Axis::Vertical,
            remaining: area,
        };
        let mut inner = Ui {
            surface: self.surface,
            interaction: self.interaction,
            enabled: self.enabled,
            cursor: Some(&mut cursor),
        };
        f(&mut inner)
    }

    /// Like [`vertical_sized`](Self::vertical_sized), claiming `width` columns instead of
    /// `height` rows.
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor: see [`vertical`](Self::vertical).
    pub fn horizontal_sized<R>(
        &mut self,
        width: u16,
        f: impl FnOnce(&mut Ui<'_, 'g, Id>) -> R,
    ) -> R {
        let area = self.allocate(width);
        let mut cursor = Cursor {
            axis: Axis::Horizontal,
            remaining: area,
        };
        let mut inner = Ui {
            surface: self.surface,
            interaction: self.interaction,
            enabled: self.enabled,
            cursor: Some(&mut cursor),
        };
        f(&mut inner)
    }

    /// Like [`show`](Self::show), but allocates the area from this `Ui`'s own cursor instead of
    /// taking one explicitly: `size` is a height on a [`vertical`](Self::vertical) cursor, a
    /// width on a [`horizontal`](Self::horizontal) one, clipped to whatever's left (like
    /// [`split_v`](crate::split_v)/[`split_h`](crate::split_h)), and the cursor advances by that
    /// amount so the next `show_sized`/`draw_sized`/`show_auto`/`draw_auto` call claims the
    /// space right after it.
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor: see [`vertical`](Self::vertical).
    #[must_use]
    pub fn show_sized(
        &mut self,
        id: Id,
        widget: &impl InteractiveWidget<State = ()>,
        size: u16,
    ) -> Response {
        let area = self.allocate(size);
        self.show(area, id, widget)
    }

    /// Like [`draw`](Self::draw), sized from this `Ui`'s own cursor; see
    /// [`show_sized`](Self::show_sized).
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor: see [`vertical`](Self::vertical).
    pub fn draw_sized(&mut self, widget: &impl Widget, size: u16) {
        let area = self.allocate(size);
        self.draw(area, widget);
    }

    /// Like [`show_sized`](Self::show_sized), sized by [`Measure::height_for`] instead of an
    /// explicit size, for a `widget` that can report its own height.
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor, or its cursor is [`horizontal`](Self::horizontal):
    /// [`Measure::height_for`] takes a width, so a horizontal cursor has nothing to size a
    /// column by.
    #[must_use]
    pub fn show_auto(
        &mut self,
        id: Id,
        widget: &(impl InteractiveWidget<State = ()> + Measure),
    ) -> Response {
        let height = widget.height_for(self.vertical_cursor_width());
        self.show_sized(id, widget, height)
    }

    /// Like [`draw_sized`](Self::draw_sized), sized by [`Measure::height_for`] instead of an
    /// explicit size; see [`show_auto`](Self::show_auto).
    ///
    /// # Panics
    ///
    /// If this `Ui` has no active cursor, or its cursor is [`horizontal`](Self::horizontal).
    pub fn draw_auto(&mut self, widget: &(impl Widget + Measure)) {
        let height = widget.height_for(self.vertical_cursor_width());
        self.draw_sized(widget, height);
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Event, Grid, KeyModifiers, MouseEvent, MouseEventKind, Pos, Style};

    use super::*;
    use crate::widget::Widget;

    fn move_pointer(interaction: &mut Interaction<Id>, pos: Pos) {
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Id {
        Button,
        Behind,
        InModal,
    }

    struct Dot;

    impl InteractiveWidget for Dot {
        type State = ();

        fn sense(&self) -> Sense {
            Sense::click()
        }

        fn render(&self, surface: &mut Surface<'_>, _state: &mut Self::State, response: Response) {
            let glyph = if response.hovered() { '*' } else { '.' };
            surface.put((0, 0), glyph, Style::new());
        }
    }

    struct Fill(char);

    impl Widget for Fill {
        fn render(&self, surface: &mut Surface<'_>) {
            let area = surface.area();
            for y in 0..area.height() {
                for x in 0..area.width() {
                    surface.put((x, y), self.0, Style::new());
                }
            }
        }
    }

    /// `Ui::show` registers the same rect it draws into: a pointer landing inside the shown area
    /// hits it on the *next* frame's hit-test, one outside it does not.
    #[test]
    fn show_registers_the_area_it_draws_into() {
        let mut grid = Grid::new(10, 10);
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(2, 2, 3, 3);

        // Frame 1 registers `area` for `Id::Button`.
        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| {
                let _ = ui.show(area, Id::Button, &Dot);
            },
        );

        // The pointer moves inside `area` between frames.
        move_pointer(&mut interaction, Pos::new(3, 3));

        // Frame 2 resolves hover against frame 1's registration.
        let response = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| ui.show(area, Id::Button, &Dot),
        );
        assert!(response.hovered());

        // The pointer moves outside `area` between frames.
        move_pointer(&mut interaction, Pos::new(8, 8));
        let response = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| ui.show(area, Id::Button, &Dot),
        );
        assert!(!response.hovered());
    }

    /// `Ui::region` likewise commits one rect for both hit-testing and the surface it hands
    /// back.
    #[test]
    fn region_registers_the_area_it_scopes_the_surface_to() {
        let mut grid = Grid::new(10, 10);
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(2, 2, 3, 3);

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| {
                let (_response, mut surface) = ui.region(area, Id::Button, Sense::hover());
                assert_eq!(surface.area(), area);
                // `put` addresses this surface's own local coordinates: (0, 0) is `area`'s own
                // top-left, grid-absolute (2, 2).
                surface.put((0, 0), 'x', Style::new());
            },
        );

        assert_eq!(grid[Pos::new(2, 2)].glyph(), 'x');

        move_pointer(&mut interaction, Pos::new(3, 3));
        let response = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| ui.region(area, Id::Button, Sense::hover()).0,
        );
        assert!(response.hovered());
    }

    /// `Interaction::frame` calls `begin_frame`/`end_frame` exactly once around the closure.
    #[test]
    fn frame_calls_begin_and_end_exactly_once() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(0, 0, 4, 4);

        // Frame 1 registers `area` for `Id::Button`.
        interaction.frame(&mut Surface::new(&mut grid, area, 0), |ui| {
            let _ = ui.show(area, Id::Button, &Dot);
        });

        // A press-then-release inside `area`, fed in between frames.
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(retroglyph_core::MouseButton::Left),
            position: Pos::new(1, 1),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(retroglyph_core::MouseButton::Left),
            position: Pos::new(1, 1),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        // Frame 2, if `frame` ran begin/end exactly once, resolves the click against frame 1's
        // registration and reports it.
        let clicked = interaction.frame(&mut Surface::new(&mut grid, area, 0), |ui| {
            ui.show(area, Id::Button, &Dot).clicked()
        });
        assert!(clicked);
    }

    /// `Ui::enabled` only ever tightens: an `enabled(true)` child of an `enabled(false)`
    /// context stays disabled, matching egui's/Dear `ImGui`'s disabled-scope composition.
    #[test]
    fn enabled_nesting_only_tightens() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(0, 0, 4, 4);
        move_pointer(&mut interaction, Pos::new(0, 0));

        // Register once so the second frame has something to resolve against.
        interaction.frame(&mut Surface::new(&mut grid, area, 0), |ui| {
            let mut disabled = ui.enabled(false);
            let mut re_enabled = disabled.enabled(true);
            assert!(!re_enabled.is_enabled());
            let _ = re_enabled.show(area, Id::Button, &Dot);
        });

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(retroglyph_core::MouseButton::Left),
            position: Pos::new(0, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(retroglyph_core::MouseButton::Left),
            position: Pos::new(0, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        let response = interaction.frame(&mut Surface::new(&mut grid, area, 0), |ui| {
            let mut disabled = ui.enabled(false);
            let mut re_enabled = disabled.enabled(true);
            re_enabled.show(area, Id::Button, &Dot)
        });
        assert!(response.disabled());
        assert!(!response.clicked());
        assert!(response.hovered());
    }

    /// A `Ui` borrow released at the end of `Interaction::frame` leaves the surface usable
    /// afterwards: this is the two-lifetime property `Ui` exists for, checked at compile time by
    /// the fact that this test compiles at all.
    #[test]
    fn surface_is_usable_after_frame_returns() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();
        let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);

        interaction.frame(&mut surface, |ui| {
            ui.draw(Rect::new(0, 0, 4, 4), &Fill('.'));
        });

        // `surface` is still a live `&mut Surface` here, not moved or borrowed by `frame`.
        surface.put((0, 0), 'x', Style::new());
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'x');
    }

    /// A widget shown inside `Ui::modal` wins a hit over a widget registered earlier at the same
    /// position, even though the earlier one alone would otherwise win by being drawn under the
    /// pointer (the usual topmost-wins rule): the modal's barrier makes the earlier registration
    /// unreachable at that position for the rest of this frame.
    #[test]
    fn modal_wins_a_hit_over_an_earlier_widget_at_the_same_position() {
        let mut grid = Grid::new(10, 10);
        let mut interaction = Interaction::<Id>::new();
        let area = Rect::new(2, 2, 3, 3);

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| {
                let _ = ui.show(area, Id::Behind, &Dot);
                ui.modal(area, |ui| {
                    let _ = ui.show(area, Id::InModal, &Dot);
                });
            },
        );

        move_pointer(&mut interaction, Pos::new(3, 3));

        let (behind, in_modal) = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| {
                let behind = ui.show(area, Id::Behind, &Dot);
                let in_modal = ui.modal(area, |ui| ui.show(area, Id::InModal, &Dot));
                (behind, in_modal)
            },
        );
        assert!(!behind.hovered()); // the barrier shadows it
        assert!(in_modal.hovered());
    }

    /// A widget registered outside `Ui::modal`'s `area` is unaffected by the barrier and still
    /// wins hits at its own position: a modal claims the region it covers, not the whole screen.
    #[test]
    fn a_widget_outside_the_modal_area_still_wins_hits_at_its_own_position() {
        let mut grid = Grid::new(10, 10);
        let mut interaction = Interaction::<Id>::new();
        let modal_area = Rect::new(2, 2, 3, 3);
        let outside_area = Rect::new(7, 7, 2, 2);

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| {
                let _ = ui.show(outside_area, Id::Behind, &Dot);
                ui.modal(modal_area, |ui| {
                    let _ = ui.show(modal_area, Id::InModal, &Dot);
                });
            },
        );

        move_pointer(&mut interaction, Pos::new(7, 7));

        let behind = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0),
            |ui| {
                let behind = ui.show(outside_area, Id::Behind, &Dot);
                let _ = ui.modal(modal_area, |ui| ui.show(modal_area, Id::InModal, &Dot));
                behind
            },
        );
        assert!(behind.hovered()); // outside the modal's own rect: unaffected by its barrier
    }

    /// A widget that reports a fixed height regardless of width, for [`show_auto`]/[`draw_auto`]
    /// tests: real `Measure` widgets (e.g. `Paragraph`) wrap at their given width instead, but
    /// that wrapping behavior isn't what these tests are checking.
    struct FixedHeight(u16);

    impl Widget for FixedHeight {
        fn render(&self, surface: &mut Surface<'_>) {
            let area = surface.area();
            for y in 0..area.height() {
                for x in 0..area.width() {
                    surface.put((x, y), '#', Style::new());
                }
            }
        }
    }

    impl InteractiveWidget for FixedHeight {
        type State = ();

        fn sense(&self) -> Sense {
            Sense::click()
        }

        fn render(&self, surface: &mut Surface<'_>, _state: &mut Self::State, _response: Response) {
            Widget::render(self, surface);
        }
    }

    impl Measure for FixedHeight {
        fn height_for(&self, _width: u16) -> u16 {
            self.0
        }
    }

    fn glyphs_in_row(grid: &Grid, y: u16, width: u16) -> String {
        (0..width).map(|x| grid[Pos::new(x, y)].glyph()).collect()
    }

    /// `Ui::vertical` stacks `draw_sized` calls top-to-bottom, each claiming a horizontal strip
    /// the height of its `size` argument and advancing the cursor by that much.
    #[test]
    fn vertical_stacks_draw_sized_rows_top_to_bottom() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.vertical(|ui| {
                    ui.draw_sized(&Fill('a'), 1);
                    ui.draw_sized(&Fill('b'), 1);
                });
            },
        );

        assert_eq!(glyphs_in_row(&grid, 0, 4), "aaaa");
        assert_eq!(glyphs_in_row(&grid, 1, 4), "bbbb");
        assert_eq!(glyphs_in_row(&grid, 2, 4), "    "); // untouched: nothing claimed it
    }

    /// `Ui::horizontal` stacks `draw_sized` calls left-to-right instead of top-to-bottom.
    #[test]
    fn horizontal_stacks_draw_sized_columns_left_to_right() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.horizontal(|ui| {
                    ui.draw_sized(&Fill('a'), 1);
                    ui.draw_sized(&Fill('b'), 1);
                });
            },
        );

        assert_eq!(glyphs_in_row(&grid, 0, 4), "ab  ");
        assert_eq!(glyphs_in_row(&grid, 3, 4), "ab  ");
    }

    /// A cursor clips content that overflows its remaining space, the same way `split_v` clips a
    /// pane that overflows `area`: the third row here would run past the 2-row-tall area, so it
    /// gets zero height instead of drawing out of bounds.
    #[test]
    fn vertical_clips_when_rows_overflow_the_cursor_area() {
        let mut grid = Grid::new(4, 2);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 2), 0),
            |ui| {
                ui.vertical(|ui| {
                    ui.draw_sized(&Fill('a'), 1);
                    ui.draw_sized(&Fill('b'), 1);
                    // Nothing left: this claims a zero-height area and draws nothing.
                    ui.draw_sized(&Fill('c'), 1);
                });
            },
        );

        assert_eq!(glyphs_in_row(&grid, 0, 4), "aaaa");
        assert_eq!(glyphs_in_row(&grid, 1, 4), "bbbb");
    }

    /// `Ui::show_sized` commits the same allocated rect for both hit-testing and drawing, just
    /// like `Ui::show` does for an explicit `area`.
    #[test]
    fn show_sized_registers_the_area_it_allocates() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.vertical(|ui| {
                    let _ = ui.show_sized(Id::Button, &Dot, 1);
                });
            },
        );

        // `show_sized`'s row is the first row, (0, 0)-(4, 1): the pointer inside it hits.
        move_pointer(&mut interaction, Pos::new(1, 0));
        let response = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| ui.vertical(|ui| ui.show_sized(Id::Button, &Dot, 1)),
        );
        assert!(response.hovered());

        // Outside the allocated row: no hit.
        move_pointer(&mut interaction, Pos::new(1, 2));
        let response = interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| ui.vertical(|ui| ui.show_sized(Id::Button, &Dot, 1)),
        );
        assert!(!response.hovered());
    }

    /// `Ui::show_auto`/`Ui::draw_auto` size their row by `Measure::height_for` instead of an
    /// explicit size.
    #[test]
    fn draw_auto_sizes_by_measure_height_for() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.vertical(|ui| {
                    ui.draw_auto(&FixedHeight(2));
                    ui.draw_sized(&Fill('x'), 1); // right after the 2-row-tall widget above
                });
            },
        );

        assert_eq!(glyphs_in_row(&grid, 0, 4), "####");
        assert_eq!(glyphs_in_row(&grid, 1, 4), "####");
        assert_eq!(glyphs_in_row(&grid, 2, 4), "xxxx");
    }

    /// `vertical`/`horizontal` nest: a `horizontal` row started inside a `vertical` column is
    /// scoped to that column's remaining area (its full width, whatever height is left), and
    /// stacking within it advances a cursor along the other axis.
    #[test]
    fn horizontal_nested_in_vertical_is_scoped_to_the_outer_cursors_remaining_area() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.vertical(|ui| {
                    ui.draw_sized(&Fill('t'), 1); // a title row
                    ui.vertical_sized(1, |ui| {
                        ui.horizontal(|ui| {
                            ui.draw_sized(&Fill('a'), 1);
                            ui.draw_sized(&Fill('b'), 1);
                        });
                    });
                });
            },
        );

        assert_eq!(glyphs_in_row(&grid, 0, 4), "tttt");
        assert_eq!(glyphs_in_row(&grid, 1, 4), "ab  ");
        assert_eq!(glyphs_in_row(&grid, 2, 4), "    "); // untouched: nothing claimed it
    }

    /// `Ui::show_sized` panics with a clear message when called on a `Ui` with no active cursor
    /// (fresh from `new`/`enabled`/`modal`), instead of silently misbehaving.
    #[test]
    #[should_panic(expected = "require an active cursor")]
    fn show_sized_without_an_active_cursor_panics() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                let _ = ui.show_sized(Id::Button, &Dot, 1);
            },
        );
    }

    /// `Ui::enabled`'s child doesn't inherit an active cursor, even from a `vertical`/
    /// `horizontal` parent: it's a fresh `Ui` like `new`'s, by design (see `Ui::enabled`'s docs).
    #[test]
    #[should_panic(expected = "require an active cursor")]
    fn enabled_child_of_a_cursor_ui_has_no_active_cursor() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.vertical(|ui| {
                    let _ = ui.enabled(true).show_sized(Id::Button, &Dot, 1);
                });
            },
        );
    }

    /// `Ui::show_auto`/`Ui::draw_auto` panic on a `horizontal` cursor: `Measure::height_for`
    /// takes a width, so a horizontal cursor has nothing to size a column by.
    #[test]
    #[should_panic(expected = "require a vertical cursor")]
    fn draw_auto_on_a_horizontal_cursor_panics() {
        let mut grid = Grid::new(4, 4);
        let mut interaction = Interaction::<Id>::new();

        interaction.frame(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0),
            |ui| {
                ui.horizontal(|ui| {
                    ui.draw_auto(&FixedHeight(1));
                });
            },
        );
    }
}
