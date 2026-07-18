//! Pointer and keyboard focus tracking for interactive widgets, without a
//! retained widget tree.
//!
//! [`ListState`](crate::ListState) answers "where is this list scrolled to
//! and what's selected"; this module answers the sibling question, "what
//! did the user just do to this widget" -- hover, click, drag, focus,
//! scroll -- for widgets that don't have a natural selection index of their
//! own (buttons, tabs, draggable panes, ...). Four independently usable
//! pieces, composed by [`Interaction`] the way [`ListState`](crate::ListState)
//! composes with [`crate::widget::Table`]:
//!
//! - [`Pointer`] -- raw mouse position/button/scroll state from a stream of
//!   [`Event`]s.
//! - [`HitTester`] -- resolves a pointer position to the topmost registered
//!   widget id.
//! - [`FocusRing`] -- which id holds keyboard focus, plus Tab/Shift+Tab
//!   cycling.
//! - [`Response`] -- what [`Interaction::interact`] reports back to a
//!   widget call site, gated by what it asked for via [`Sense`].
//!
//! # Example
//!
//! ```
//! use retroglyph_core::{Backend, Headless, Rect, Terminal};
//! use retroglyph_widgets::{Interaction, Sense};
//!
//! #[derive(Clone, Copy, PartialEq, Eq)]
//! enum WidgetId {
//!     SaveButton,
//! }
//!
//! fn draw<B: Backend>(
//!     term: &mut Terminal<B>,
//!     interaction: &mut Interaction<WidgetId>,
//! ) -> bool {
//!     let area = Rect::new(0, 0, 10, 1);
//!     let response = interaction.interact(area, WidgetId::SaveButton, Sense::click());
//!     // ... draw the button, using response.hovered()/focused() to pick a style ...
//!     response.clicked()
//! }
//!
//! let mut term = Terminal::new(Headless::new(20, 10));
//! let mut interaction = Interaction::<WidgetId>::new();
//! interaction.begin_frame();
//! let saved = draw(&mut term, &mut interaction);
//! interaction.end_frame();
//! assert!(!saved); // nothing clicked yet -- no input was fed in
//! ```

mod density;
mod focus;
mod hit;
mod pointer;
mod response;
mod sense;
mod shortcuts;

pub use density::Density;
pub use focus::FocusRing;
pub use hit::HitTester;
pub use pointer::Pointer;
pub use response::Response;
pub use sense::Sense;
pub use shortcuts::Shortcuts;

use retroglyph_core::{Event, KeyCode, MouseButton, Pos, Rect};

/// Default [`Interaction::with_drag_threshold`].
///
/// The pointer must move more than one cell from its press-down position
/// before a [`Sense::DRAG`] widget reports [`Response::dragging`] instead of
/// a click-in-progress.
pub const DEFAULT_DRAG_THRESHOLD: u16 = 1;

/// Ties [`Pointer`], [`HitTester`], and [`FocusRing`] together into the one
/// piece of state a draw pass needs to make its widgets interactive.
///
/// # Frame lifecycle
///
/// ```text
/// interaction.begin_frame();                 // 1
/// for event in poll_events() {
///     interaction.handle_event(&event);       // 2
/// }
/// draw(&mut term, &mut interaction, &state);  // 3 -- calls interaction.interact(...)
/// interaction.end_frame();                    // 4
/// ```
///
/// 1. [`begin_frame`](Self::begin_frame) snapshots which id (if any) is
///    under the pointer, and whether it pressed/released/scrolled, using
///    *last* frame's hit registrations and pointer events: this frame's
///    registrations aren't complete until step 3 finishes, and this frame's
///    events haven't arrived yet (they're step 2), so every [`Response`] in
///    a given frame is one frame stale relative to what's being drawn/fed in
///    *this* frame -- uniformly for hover, press, release, click, and
///    scroll, all resolved from that one snapshot. At typical redraw rates
///    this is imperceptible; it's the same kind of trade-off
///    [`ListState::ensure_visible`](crate::ListState::ensure_visible)
///    documents for a different reason (only the caller knows the current
///    viewport height), applied here because only the *previous* frame
///    knows the full hit list and the pointer's position as of the input
///    that's about to be processed. `dragging` and [`Response::held`] are exceptions: both
///    re-check the pointer's *live* position (via [`Pointer::pos`]/[`Pointer::is_down`]) rather
///    than the frame-stale snapshot, because a drag-in-progress or a press-cancel needs to react
///    the instant the pointer moves, not one frame later. Keyboard focus is the remaining
///    exception: [`Response::focused`] and Enter/Space activation read [`FocusRing`]'s `current`
///    live, since it's plain level state with no hit-testing involved -- no staleness to trade
///    off.
/// 2. [`handle_event`](Self::handle_event) updates pointer position/buttons
///    and, by default, cycles focus on Tab/Shift+Tab.
/// 3. Each widget calls [`interact`](Self::interact) with its rect, a
///    caller-chosen id, and a [`Sense`] describing what it cares about; it
///    gets back a [`Response`] and, as a side effect, registers itself for
///    step 1 of the *next* frame.
/// 4. [`end_frame`](Self::end_frame) releases the active widget if step 1
///    saw the pointer go up.
///
/// One consequence worth knowing: a full press-then-release gesture that
/// arrives as two events in the *same* [`handle_event`](Self::handle_event)
/// batch (both fed in during step 2 of one frame, e.g. a synthetic test
/// firing them back to back) takes an extra frame to resolve versus a
/// realistic press and release arriving in separate frames, because step
/// 1's hover snapshot for that frame still reflects the pointer's
/// position from *before* those events. Real input rarely lands this way
/// (a physical click's down and up are milliseconds apart, i.e. several
/// frames at typical redraw rates), so this only tends to show up in tests.
///
/// # Why `Id` is a type parameter, not a hash
///
/// Immediate-mode toolkits like egui derive a widget's identity from its
/// call-site source location (optionally salted with data) hashed down to
/// an opaque integer -- flexible, but it means two widgets can collide onto
/// the same id at runtime with no compile-time signal, and the id carries
/// no meaning a debugger can show you. `Interaction<Id>` instead asks the
/// app for whatever id type it already has lying around -- typically a
/// small `Copy` enum like the hand-rolled hit-target enum an app would
/// otherwise define anyway. Collisions become unrepresentable if the enum
/// is exhaustive, and `{:?}`-printing an id tells you exactly which widget
/// it is. The cost is one generic parameter; `Id: Copy + PartialEq` is all
/// any of this module asks for.
///
/// Consistently with that: everything here holds its state in a plain,
/// explicitly-owned struct threaded through `&mut self`, the same
/// convention [`ListState`](crate::ListState) uses, rather than the
/// interior-mutability/global-context pattern egui's `Memory` relies on to
/// keep its implicit ids from needing to be threaded everywhere.
// Several of these are independent one-shot snapshots (primary/secondary
// press/release, keyboard activation), not states of a single state
// machine -- see the field-level comment above `resolved_press` for why
// they're snapshotted individually rather than read live off `pointer`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Interaction<Id> {
    pointer: Pointer,
    hits: HitTester<Id>,
    focus: FocusRing<Id>,
    resolved_hover: Option<Id>,
    // The pointer position `resolved_hover` was computed from, kept
    // alongside it so `interact` can independently ask "was *my* rect under
    // the pointer" (see `scroll_delta` below) without needing `resolved_hover`
    // to have picked this id as the single topmost winner.
    resolved_pos: Option<Pos>,
    // Snapshots of the pointer's one-shot flags, taken once in `begin_frame`
    // and read by every `interact` call for the rest of this frame. Not read
    // straight off `pointer` during `interact`: `handle_event` runs *between*
    // `begin_frame` and `interact` calls (see the frame lifecycle docs), so a
    // press/release arriving this frame would otherwise be visible to
    // `interact` immediately while `resolved_hover` (computed before that
    // event) still reflects last frame's pointer position -- `active` would
    // then latch onto whatever was hovered *last* frame, not the widget the
    // fresh press actually landed on. Resolving everything from one
    // consistent snapshot keeps hover/press/release/click/scroll uniformly
    // one frame behind the input that produced them, matching the docs.
    resolved_press: bool,
    resolved_release: bool,
    resolved_secondary_press: bool,
    resolved_secondary_release: bool,
    resolved_scroll: i32,
    active: Option<Id>,
    // Tracked separately from `active`: a secondary press can land on one
    // widget while the primary button is mid-drag on another (or not
    // pressed at all), so the two buttons need independent "which widget
    // did this press originate on" state.
    secondary_active: Option<Id>,
    drag_origin: Option<Pos>,
    drag_threshold: u16,
    activate_focused: bool,
}

impl<Id> Interaction<Id> {
    /// A fresh interaction context: nothing hovered, focused, or active.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pointer: Pointer::new(),
            hits: HitTester::new(),
            focus: FocusRing::new(),
            resolved_hover: None,
            resolved_pos: None,
            resolved_press: false,
            resolved_release: false,
            resolved_secondary_press: false,
            resolved_secondary_release: false,
            resolved_scroll: 0,
            active: None,
            secondary_active: None,
            drag_origin: None,
            drag_threshold: DEFAULT_DRAG_THRESHOLD,
            activate_focused: false,
        }
    }

    /// Override how far (in cells) the pointer must move from its press
    /// origin before a [`Sense::DRAG`] widget reports
    /// [`Response::dragging`] rather than a click-in-progress. Defaults to
    /// [`DEFAULT_DRAG_THRESHOLD`].
    #[must_use]
    pub const fn with_drag_threshold(mut self, cells: u16) -> Self {
        self.drag_threshold = cells;
        self
    }

    /// Read access to the pointer's current position/button/scroll state,
    /// e.g. to draw a custom cursor glyph.
    #[must_use]
    pub const fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    /// Read access to the focus ring, e.g. to render a "press Tab to
    /// begin" hint when nothing is focused yet.
    #[must_use]
    pub const fn focus(&self) -> &FocusRing<Id> {
        &self.focus
    }

    /// Mutable access to the focus ring, e.g. to drive it from a gamepad
    /// shoulder button instead of (or in addition to) Tab/Shift+Tab.
    pub const fn focus_mut(&mut self) -> &mut FocusRing<Id> {
        &mut self.focus
    }
}

impl<Id: Copy + PartialEq> Interaction<Id> {
    /// Resolve hover/press against last frame's registrations, finalize the
    /// focus order, and clear the hit registry for this frame's
    /// [`interact`](Self::interact) calls. Call once per frame, before
    /// processing input or drawing.
    pub fn begin_frame(&mut self) {
        self.resolved_pos = self.pointer.pos();
        self.resolved_hover = self.resolved_pos.and_then(|pos| self.hits.topmost_at(pos));
        self.resolved_press = self.pointer.pressed(MouseButton::Left);
        self.resolved_release = self.pointer.released(MouseButton::Left);
        self.resolved_secondary_press = self.pointer.pressed(MouseButton::Right);
        self.resolved_secondary_release = self.pointer.released(MouseButton::Right);
        self.resolved_scroll = self.pointer.scroll_delta();

        if self.resolved_press {
            self.active = self.resolved_hover;
            self.drag_origin = self.resolved_pos;
        }
        if self.resolved_secondary_press {
            self.secondary_active = self.resolved_hover;
        }

        self.hits.clear();
        self.focus.begin_frame();
        // Now that this frame's snapshot is taken, clear the one-shot flags
        // so next frame's `handle_event` calls start from a clean slate.
        self.pointer.end_frame();
    }

    /// Feed a raw input event: updates the pointer, and (by default) Tab
    /// cycles focus -- see [`FocusRing::handle_event`] if you need to
    /// override that.
    pub fn handle_event(&mut self, event: &Event) {
        self.pointer.handle_event(event);
        self.focus.handle_event(event);
        self.activate_focused |= is_activation_key(event);
    }

    /// Register `id`'s `rect` for whatever `sense` asks for, and report
    /// what happened to it, resolved from *last* frame's input -- see the
    /// [`Interaction`] docs for the frame lifecycle this implies.
    pub fn interact(&mut self, rect: Rect, id: Id, sense: Sense) -> Response {
        if sense.wants_pointer() {
            self.hits.push(rect, id);
        }
        if sense.contains(Sense::FOCUSABLE) {
            self.focus.register(id);
        }

        let hovered = sense.wants_pointer() && self.resolved_hover == Some(id);
        let is_active = self.active == Some(id);
        let senses_click = sense.contains(Sense::CLICK);
        let key_activated = senses_click
            && sense.contains(Sense::FOCUSABLE)
            && self.focus.is_focused(id)
            && self.activate_focused;
        let released_here = is_active && self.resolved_release;
        // Deliberately not gated on `self.pointer.is_down()`: the release
        // frame (where `is_down` just went false) must still see `dragging
        // == true` so `clicked` below correctly stays suppressed for a
        // drag's terminating release, not just the frames in between.
        let dragging = is_active && sense.contains(Sense::DRAG) && self.past_drag_threshold();

        // Live re-check, deliberately not gated on `hovered`/`resolved_hover` the way `pressed`
        // is: those are resolved from *last* frame's hit-test snapshot (see the `Interaction`
        // frame-lifecycle docs), but a slide-off cancellation needs to see the pointer's
        // *current* position the instant it leaves this rect, not one frame later. Mirrors how
        // `dragging` above already reads `self.pointer.pos()` live instead of `resolved_pos`, and
        // how `scroll_delta` below bypasses the single-topmost-winner rule -- same "read live
        // state, scoped to my own rect" shape, applied a third time.
        let held = senses_click
            && is_active
            && self.pointer.is_down(MouseButton::Left)
            && self.pointer.pos().is_some_and(|pos| rect.contains_pos(pos));

        if senses_click && released_here && hovered && !dragging {
            self.focus.request(id);
        }

        // Scroll deliberately isn't gated on `hovered` (single topmost
        // winner) the way click/press/release/drag are: a scrollable
        // container's own rect is usually fully covered by its rows/items
        // (each independently sensing HOVER | CLICK so they're individually
        // clickable), which would otherwise shadow the container at every
        // point inside it and make it un-scrollable. Any rect the resolved
        // pointer position falls within gets scroll credit, regardless of
        // what's drawn on top of it -- matching how wheel input behaves in
        // most real UIs (it reaches the nearest scrollable ancestor, not
        // just whatever's topmost at the exact pixel).
        let scrollable_here = sense.wants_pointer()
            && sense.contains(Sense::SCROLL)
            && self.resolved_pos.is_some_and(|pos| rect.contains_pos(pos));

        // The secondary button gets a narrower resolution than the primary
        // one: no drag-threshold suppression (secondary-button drags aren't
        // a gesture this module tracks), and it doesn't drive focus the way
        // a primary click does (see `Response::secondary_clicked`'s doc
        // comment).
        let secondary_is_active = self.secondary_active == Some(id);
        let secondary_clicked = sense.contains(Sense::SECONDARY_CLICK)
            && secondary_is_active
            && self.resolved_secondary_release
            && hovered;

        Response {
            hovered,
            pressed: (is_active && self.resolved_press) || key_activated,
            released: released_here || key_activated,
            clicked: (senses_click && released_here && hovered && !dragging) || key_activated,
            held,
            dragging,
            focused: self.focus.is_focused(id),
            secondary_clicked,
            scroll_delta: if scrollable_here {
                self.resolved_scroll
            } else {
                0
            },
        }
    }

    /// Release the active widget (both primary and secondary), e.g. so a
    /// later [`focus_mut`](Self::focus_mut)-driven Tab handling starts
    /// clean. Call once per frame, after drawing.
    pub const fn end_frame(&mut self) {
        if self.resolved_release {
            self.active = None;
            self.drag_origin = None;
        }
        if self.resolved_secondary_release {
            self.secondary_active = None;
        }
        self.activate_focused = false;
    }

    fn past_drag_threshold(&self) -> bool {
        let (Some(origin), Some(pos)) = (self.drag_origin, self.pointer.pos()) else {
            return false;
        };
        origin.x.abs_diff(pos.x).max(origin.y.abs_diff(pos.y)) > self.drag_threshold
    }
}

impl<Id> Default for Interaction<Id> {
    fn default() -> Self {
        Self::new()
    }
}

const fn is_activation_key(event: &Event) -> bool {
    let Event::Key(key) = event else {
        return false;
    };
    key.is_down() && matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Id {
        Save,
        Cancel,
    }

    fn click_at(interaction: &mut Interaction<Id>, pos: Pos) {
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Registers `Save`/`Cancel` at fixed rects and returns their responses,
    /// modeling one full frame (see the [`Interaction`] docs for the
    /// lifecycle). `events` are fed in between `begin_frame` and the
    /// `interact` calls, exactly where the documented lifecycle puts them --
    /// e.g. a `Tab` press only affects focus registered as of the *start*
    /// of this call, and a click resolves against hits registered by the
    /// *previous* `frame`/`frame_with_events` call.
    fn frame_with_events(
        interaction: &mut Interaction<Id>,
        events: &[Event],
    ) -> (Response, Response) {
        interaction.begin_frame();
        for event in events {
            interaction.handle_event(event);
        }
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let cancel = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        (save, cancel)
    }

    fn frame(interaction: &mut Interaction<Id>) -> (Response, Response) {
        frame_with_events(interaction, &[])
    }

    #[test]
    fn click_is_resolved_one_frame_after_the_pointer_event() {
        let mut interaction = Interaction::<Id>::new();

        // Frame 1: nothing registered yet, so nothing can resolve.
        let (save1, _) = frame(&mut interaction);
        assert!(!save1.clicked());

        // Click lands between frame 1 and frame 2, over "Save"'s rect.
        click_at(&mut interaction, Pos::new(2, 0));

        // Frame 2: resolves against frame 1's registrations.
        let (save2, cancel2) = frame(&mut interaction);
        assert!(save2.clicked());
        assert!(!cancel2.clicked());
    }

    /// Regression test for a real bug caught while building the
    /// `interaction_demo` example: pointer flags used to get cleared in
    /// `end_frame` (the same frame `handle_event` set them in), so a press
    /// recorded by `handle_event` was always gone by the time the *next*
    /// frame's `begin_frame` went looking for it, and `active` could never
    /// be set at all. Fixed by moving flag consumption into `begin_frame`
    /// itself. This mirrors the realistic call pattern (`handle_event`
    /// between `begin_frame` and drawing, once per frame) rather than
    /// `click_at`'s frame-boundary-agnostic style above.
    #[test]
    fn press_and_release_in_separate_frames_still_resolves_a_click() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel

        let down = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        // frame 2: press delivered via handle_event, same as a real tick.
        let (save2, _) = frame_with_events(&mut interaction, &[down]);
        assert!(!save2.pressed()); // this frame's hover snapshot predates the event

        // frame 3: begin_frame now sees frame 2's press against frame 2's
        // (correctly positioned) hit registrations.
        let (save3, _) = frame(&mut interaction);
        assert!(save3.pressed());

        let up = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        // frame 4: release delivered the same way.
        let _ = frame_with_events(&mut interaction, &[up]);

        // frame 5: resolves the release.
        let (save5, _) = frame(&mut interaction);
        assert!(save5.clicked());
    }

    #[test]
    fn hover_follows_the_pointer_without_a_click() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction);

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(7, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        let (save, cancel) = frame(&mut interaction);
        assert!(!save.hovered());
        assert!(cancel.hovered());
        assert!(!cancel.clicked());
    }

    #[test]
    fn tab_focuses_then_enter_activates_without_any_pointer() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // registers Save/Cancel as focusable for the *next* frame

        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (save, _) = frame_with_events(&mut interaction, &[tab]);
        assert!(save.focused());
        assert!(!save.clicked());

        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let (save, cancel) = frame_with_events(&mut interaction, &[enter]);
        assert!(save.clicked());
        assert!(!cancel.clicked());
    }

    #[test]
    fn drag_past_threshold_suppresses_the_click() {
        let mut interaction = Interaction::<Id>::new().with_drag_threshold(1);
        let _ = frame(&mut interaction);

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        assert!(!save.dragging()); // hasn't moved yet
        interaction.end_frame();

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(4, 0), // 2 cells from the press origin
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        assert!(save.dragging());
        interaction.end_frame();

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(4, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        assert!(!save.clicked()); // released after dragging, not a click
        assert!(save.released());
    }

    #[test]
    fn held_is_true_while_pressed_and_hovering_and_false_once_the_pointer_slides_off() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        // frame 2: press resolves against frame 1's registrations, pointer still over Save.
        let (save, _) = frame(&mut interaction);
        assert!(save.held());

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(20, 0), // outside Save's rect, still held down
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        assert!(!save.held()); // slid off before release -- cancels immediately

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0), // back over Save, still held down, before release
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        assert!(save.held()); // back inside -- held again

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        assert!(!save.held());
        assert!(save.released());
        assert!(save.clicked());
    }

    #[test]
    fn held_requires_click_sense() {
        let mut interaction = Interaction::<Id>::new();
        interaction.begin_frame();
        let _ = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::hover());
        interaction.end_frame();

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        // `active` is assigned from whichever id was topmost at press time, regardless of that
        // id's own `Sense` (see `begin_frame`'s `self.active = self.resolved_hover;`), so Save
        // is `is_active` here even though it only sensed `HOVER` -- `held` must still stay false.
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::hover());
        interaction.end_frame();
        assert!(!save.held());
    }

    #[test]
    fn scroll_reports_only_while_hovered_and_sensed() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction);

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::scroll());
        let cancel = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::scroll());
        interaction.end_frame();

        assert_eq!(save.scroll_delta(), 1);
        assert_eq!(cancel.scroll_delta(), 0); // outside Cancel's rect
    }

    /// Regression test for a real bug caught while building the
    /// `interaction_demo` example: a scrollable container whose rows are
    /// individually `Sense::HOVER | Sense::CLICK`-sensed (so they're each
    /// clickable) covers its own rect completely, so under the old
    /// "scroll only reports for the single topmost-hovered id" rule the
    /// container could never win hover against its own rows and would
    /// never see a scroll. Fixed by making `SCROLL` independent of the
    /// topmost-hover winner -- see [`Sense::SCROLL`]'s doc comment.
    #[test]
    fn scroll_reaches_a_container_through_an_overlapping_child() {
        let mut interaction = Interaction::<Id>::new();
        interaction.begin_frame();
        // The child (Cancel, standing in for a list row) is registered
        // *after* the container (Save), so it's topmost at any point they
        // share -- exactly like a row drawn on top of its list container.
        let _ = interaction.interact(Rect::new(0, 0, 10, 1), Id::Save, Sense::scroll());
        let _ = interaction.interact(
            Rect::new(0, 0, 10, 1),
            Id::Cancel,
            Sense::HOVER | Sense::CLICK,
        );
        interaction.end_frame();

        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        let container = interaction.interact(Rect::new(0, 0, 10, 1), Id::Save, Sense::scroll());
        let child = interaction.interact(
            Rect::new(0, 0, 10, 1),
            Id::Cancel,
            Sense::HOVER | Sense::CLICK,
        );
        interaction.end_frame();

        assert_eq!(container.scroll_delta(), 1);
        assert!(child.hovered()); // the child still wins plain hover/click resolution
    }

    #[test]
    fn hover_only_sense_never_reports_clicked() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction);
        click_at(&mut interaction, Pos::new(2, 0));

        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::hover());
        interaction.end_frame();

        assert!(save.hovered());
        assert!(!save.clicked());
    }

    fn right_click_at(interaction: &mut Interaction<Id>, pos: Pos) {
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
    }

    #[test]
    fn secondary_click_is_independent_of_the_primary_button() {
        let mut interaction = Interaction::<Id>::new();

        fn frame_secondary(interaction: &mut Interaction<Id>) -> (Response, Response) {
            interaction.begin_frame();
            let save = interaction.interact(
                Rect::new(0, 0, 5, 1),
                Id::Save,
                Sense::click() | Sense::SECONDARY_CLICK,
            );
            let cancel = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
            interaction.end_frame();
            (save, cancel)
        }

        let _ = frame_secondary(&mut interaction); // frame 1: register
        right_click_at(&mut interaction, Pos::new(2, 0)); // over Save

        let (save, cancel) = frame_secondary(&mut interaction); // frame 2: resolves
        assert!(save.secondary_clicked());
        assert!(!save.clicked()); // primary button never touched
        assert!(!cancel.secondary_clicked());
    }

    #[test]
    fn secondary_click_not_sensed_never_reports_even_when_right_clicked() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // Save/Cancel sensed with Sense::click() only
        right_click_at(&mut interaction, Pos::new(2, 0));

        let (save, _) = frame(&mut interaction);
        assert!(!save.secondary_clicked()); // not sensed, so never reported
    }
}
