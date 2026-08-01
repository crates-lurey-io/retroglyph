//! [`Ui`]: a per-frame context pairing a [`Surface`] with an [`Interaction`].

use retroglyph_core::{Rect, Surface};

use crate::interact::{Interaction, Response, Sense};
use crate::widget::{InteractiveWidget, StatefulWidget, Widget};

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
}

impl<'s, 'g, Id> Ui<'s, 'g, Id> {
    /// A `Ui` pairing `surface` with `interaction` for one frame.
    #[must_use]
    pub const fn new(surface: &'s mut Surface<'g>, interaction: &'s mut Interaction<Id>) -> Self {
        Self {
            surface,
            interaction,
        }
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

impl<Id: Copy + PartialEq> Ui<'_, '_, Id> {
    /// Hit-test `area` for `id` with `widget`'s own [`Sense`], then draw `widget` into `area`.
    ///
    /// This is the one-`id`-one-`area` guarantee the [`InteractiveWidget`]/[`Ui`] split exists
    /// for: `area` is registered for hit-testing and used to scope the surface the widget draws
    /// into from the same value, so the two cannot disagree.
    #[must_use]
    pub fn show(
        &mut self,
        area: Rect,
        id: Id,
        widget: &impl InteractiveWidget<State = ()>,
    ) -> Response {
        let response = self.interaction.interact(area, id, widget.sense());
        widget.render(&mut self.surface.scope(area), &mut (), response);
        response
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
        let response = self.interaction.interact(area, id, widget.sense());
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
        let response = self.interaction.interact(area, id, sense);
        (response, self.surface.scope(area))
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Event, Grid, KeyModifiers, MouseEvent, MouseEventKind, Pos, Style};

    use super::*;
    use crate::widget::Widget;

    fn move_pointer(interaction: &mut Interaction<Id>, pos: Pos) {
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Id {
        Button,
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
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(retroglyph_core::MouseButton::Left),
            position: Pos::new(1, 1),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.handle_event(&Event::Mouse(MouseEvent {
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
}
