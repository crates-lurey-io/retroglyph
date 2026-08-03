//! Pointer and keyboard focus tracking for interactive widgets, without a
//! retained widget tree.
//!
//! [`ListState`](crate::ListState) answers "where is this list scrolled to
//! and what's selected"; this module answers the sibling question, "what
//! did the user just do to this widget" (hover, click, drag, focus,
//! scroll) for widgets that don't have a natural selection index of their
//! own (buttons, tabs, draggable panes, ...). Four independently usable
//! pieces, composed by [`Interaction`] the way [`ListState`](crate::ListState)
//! composes with [`crate::widget::Table`]:
//!
//! - [`Pointer`]: raw mouse position/button/scroll state from a stream of
//!   [`Event`]s.
//! - [`HitTester`]: resolves a pointer position to the topmost registered
//!   widget id.
//! - [`FocusRing`]: which id holds keyboard focus, plus Tab/Shift+Tab
//!   cycling.
//! - [`Response`]: what [`Interaction::interact`] reports back to a
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
//! assert!(!saved); // nothing clicked yet: no input was fed in
//! ```

mod consumed;
mod density;
mod focus;
mod hit;
mod pointer;
mod response;
mod sense;
mod shortcuts;

pub use consumed::Consumed;
pub use density::Density;
pub use focus::FocusRing;
pub use hit::HitTester;
pub use pointer::Pointer;
pub use response::Response;
pub use sense::Sense;
pub use shortcuts::Shortcuts;

use core::time::Duration;

use retroglyph_core::{Event, Frame, KeyCode, MouseButton, Pos, Rect, Surface, Tween};

use crate::Ui;

/// Default [`Interaction::with_drag_threshold`].
///
/// The pointer must move more than one cell from its press-down position
/// before a [`Sense::DRAG`] widget reports [`Response::dragging`] instead of
/// a click-in-progress.
pub const DEFAULT_DRAG_THRESHOLD: u16 = 1;

/// Default [`Interaction::with_double_click_window`].
///
/// A second [`Response::clicked`] within this many [`begin_frame`](Interaction::begin_frame)
/// calls of the first counts as [`Response::double_clicked`].
pub const DEFAULT_DOUBLE_CLICK_WINDOW: u16 = 30;

/// Ties [`Pointer`], [`HitTester`], and [`FocusRing`] together into the one
/// piece of state a draw pass needs to make its widgets interactive.
///
/// # Frame lifecycle
///
/// [`frame`](Self::frame) is the documented way to drive one frame: it wraps a closure with
/// [`begin_frame`](Self::begin_frame) and [`end_frame`](Self::end_frame), and hands the closure a
/// [`Ui`] pairing the surface passed in with `self`.
///
/// ```
/// use retroglyph_core::{Backend, Headless, Rect, Terminal};
/// use retroglyph_widgets::{Interaction, Sense};
///
/// #[derive(Clone, Copy, PartialEq, Eq)]
/// enum WidgetId {
///     SaveButton,
/// }
///
/// let mut term = Terminal::new(Headless::new(20, 10));
/// let mut interaction = Interaction::<WidgetId>::new();
/// let clicked = interaction.frame(&mut term.surface(), |ui| {
///     let area = Rect::new(0, 0, 10, 1);
///     let response = ui.interaction().interact(area, WidgetId::SaveButton, Sense::click());
///     // ... draw the button, using response.hovered()/focused() to pick a style ...
///     response.clicked()
/// });
/// assert!(!clicked); // nothing clicked yet: no input was fed in
/// ```
///
/// `begin_frame`/`handle_event`/`end_frame` stay public for callers driving the lifecycle
/// themselves (e.g. to interleave event handling between frames rather than all at once), but
/// `frame` is what each step below describes:
///
/// ```text
/// interaction.begin_frame();                 // 1
/// for event in poll_events() {
///     interaction.handle_event(&event);       // 2
/// }
/// draw(&mut term, &mut interaction, &state);  // 3: calls interaction.interact(...)
/// interaction.end_frame();                    // 4
/// ```
///
/// 1. [`begin_frame`](Self::begin_frame) snapshots which id (if any) is
///    under the pointer, and whether it pressed/released/scrolled, using
///    *last* frame's hit registrations and pointer events: this frame's
///    registrations aren't complete until step 3 finishes, and this frame's
///    events haven't arrived yet (they're step 2), so every [`Response`] in
///    a given frame is one frame stale relative to what's being drawn/fed in
///    *this* frame: uniformly for hover, press, release, click, and
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
///    live, since it's plain level state with no hit-testing involved: no staleness to trade
///    off.
/// 2. [`handle_event`](Self::handle_event) updates pointer position/buttons and, by default,
///    cycles focus on Tab/Shift+Tab, then reports whether this interaction [claimed the
///    event](Consumed), resolved against the same last-frame registrations step 1 just read: an
///    app juggling more than one [`Interaction`] can stop routing an event the moment one of them
///    claims it, without waiting for step 3 to run.
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
/// an opaque integer, flexible, but it means two widgets can collide onto
/// the same id at runtime with no compile-time signal, and the id carries
/// no meaning a debugger can show you. `Interaction<Id>` instead asks the
/// app for whatever id type it already has lying around: typically a
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
// machine: see the field-level comment above `resolved_press` for why
// they're snapshotted individually rather than read live off `pointer`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Interaction<Id> {
    pointer: Pointer,
    hits: HitTester<Id>,
    // A copy of last frame's finalized `hits`, taken in `begin_frame` before `hits` is cleared
    // for this frame's `interact` calls. `handle_event` and `wants_pointer` read this rather than
    // `hits`: `hits` itself is empty for most of a frame (from `begin_frame` until the first
    // `interact` call resolves in step 3), but a pointer event delivered any time between
    // `begin_frame` and that first `interact` call still needs a hit-test target, and the only
    // complete one available yet is last frame's.
    prev_hits: HitTester<Id>,
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
    // event) still reflects last frame's pointer position: `active` would
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
    // Counts `begin_frame` calls, used as a frame-count clock for double-click windowing: this
    // module otherwise has no notion of wall-clock time (see the `no_std`-friendly habits
    // documented on `Pointer`), so `double_click_window` is measured in frames rather than a
    // `Duration`, matching how `drag_threshold` is measured in cells rather than pixels.
    frame_count: u64,
    // The id and `frame_count` of the most recent click not yet paired into a double-click. Only
    // one slot is needed: like `active`/`secondary_active`, a click can only resolve on one
    // widget per frame, and a second click within the window immediately consumes this (see
    // `interact`), so there's never more than one pending single-click to remember.
    last_click: Option<(Id, u64)>,
    double_click_window: u16,
    // `focus.focused()` as of the end of the *previous* frame, snapshotted in `begin_frame`
    // before this frame's `handle_event` (Tab cycling) or `interact` (click-to-focus) calls can
    // move it: `gained_focus`/`lost_focus` need last frame's answer to diff against, the same
    // role `resolved_hover`'s pointer-snapshot fields play for hover/press/release.
    prev_focused: Option<Id>,
    // Backs [`animate`](Self::animate): one `(id, last target, Tween)` per `id` that's had
    // `animate` called on it since it last settled at rest. A plain `Vec` scanned linearly, not a
    // `HashMap`, to keep `animate` (like every other method here) usable with any `Id: Copy +
    // PartialEq`, no `Hash`/`Eq` required -- the same call [`HitTester`] already makes for its own
    // per-`Id` registrations. The stored `bool` is `animate`'s last-seen `target`, kept alongside
    // the `Tween` so a flip can be detected without the `Tween` itself exposing its `to`.
    tweens: Vec<(Id, bool, Tween)>,
}

impl<Id> Interaction<Id> {
    /// A fresh interaction context: nothing hovered, focused, or active.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pointer: Pointer::new(),
            hits: HitTester::new(),
            prev_hits: HitTester::new(),
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
            frame_count: 0,
            last_click: None,
            double_click_window: DEFAULT_DOUBLE_CLICK_WINDOW,
            prev_focused: None,
            tweens: Vec::new(),
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

    /// Override how many [`begin_frame`](Self::begin_frame) calls may separate two clicks on the
    /// same widget for the second to report [`Response::double_clicked`]. Defaults to
    /// [`DEFAULT_DOUBLE_CLICK_WINDOW`].
    #[must_use]
    pub const fn with_double_click_window(mut self, frames: u16) -> Self {
        self.double_click_window = frames;
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
    /// The id the pointer resolved to this frame, if any: the same value
    /// [`interact`](Self::interact) compares against to decide each
    /// widget's [`Response::hovered`], resolved from last frame's hit-test
    /// (see [`Interaction`](Self) for why there's a frame of latency).
    ///
    /// Unlike `Response::hovered`, this isn't filtered by the hovered
    /// widget's [`Sense`]: it's the topmost id under the pointer
    /// regardless of what that id is listening for, which is what makes it
    /// useful for drawing a hover-driven readout (a tooltip, a cost
    /// preview) *before* the widget it depends on has been registered this
    /// frame, rather than having to stash the value for next frame by hand.
    #[must_use]
    pub const fn hovered(&self) -> Option<Id> {
        self.resolved_hover
    }

    /// Run one frame: [`begin_frame`](Self::begin_frame), then `f` (given a [`Ui`] pairing
    /// `surface` with `self`), then [`end_frame`](Self::end_frame).
    ///
    /// This is the documented way to drive the [frame lifecycle](Self#frame-lifecycle): the three
    /// calls are easy to get right once and easy to forget (particularly `end_frame`) when spread
    /// across a caller's own draw loop by hand.
    pub fn frame<R>(
        &mut self,
        surface: &mut Surface<'_>,
        f: impl FnOnce(&mut Ui<'_, '_, Id>) -> R,
    ) -> R {
        self.begin_frame();
        let result = f(&mut Ui::new(surface, self));
        self.end_frame();
        result
    }

    /// Resolve hover/press against last frame's registrations, finalize the
    /// focus order, and clear the hit registry for this frame's
    /// [`interact`](Self::interact) calls. Call once per frame, before
    /// processing input or drawing.
    pub fn begin_frame(&mut self) {
        self.frame_count += 1;
        // Taken before `focus.begin_frame` (a no-op on `current`) and before this frame's own
        // `handle_event`/`interact` calls can move focus: see the field comment on `prev_focused`.
        self.prev_focused = self.focus.focused();
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

        // `prev_hits` becomes last frame's finalized registrations (what `hits` held coming into
        // this call, used above to compute `resolved_hover`), and `hits` is left holding
        // whatever `prev_hits` held before, immediately cleared below: this reuses that buffer's
        // capacity for this frame's `interact` calls rather than allocating a fresh one.
        core::mem::swap(&mut self.hits, &mut self.prev_hits);
        self.hits.clear();
        self.focus.begin_frame();
        // Now that this frame's snapshot is taken, clear the one-shot flags
        // so next frame's `handle_event` calls start from a clean slate.
        self.pointer.end_frame();
    }

    /// Feed a raw input event: updates the pointer, and (by default) Tab cycles focus (see
    /// [`FocusRing::handle_event`] if you need to override that), then reports whether this
    /// interaction claimed it.
    ///
    /// Resolved against *last* frame's registrations, the same snapshot
    /// [`wants_pointer`](Self::wants_pointer)/[`wants_keyboard`](Self::wants_keyboard) read: this
    /// frame's [`interact`](Self::interact) calls haven't run yet (they're step 3 of the [frame
    /// lifecycle](Self#frame-lifecycle); `handle_event` is step 2), so `self`'s hit/focus
    /// registrations at the time this runs are still whatever the previous frame left behind.
    ///
    /// A pointer event ([`Event::Mouse`]) is claimed if its position lands on a registered rect,
    /// regardless of that rect's [`Sense`] (even a [`Sense::hover`]-only widget still owns the
    /// pointer at its own position; a wheel event over it shouldn't fall through to whatever's
    /// behind it either). `Tab`/`Shift+Tab` are claimed whenever anything is registered as
    /// focusable, matching [`FocusRing::advance`]/[`retreat`](FocusRing::retreat)'s own condition
    /// for actually moving focus. `Enter`/`Space` are claimed only when they double as
    /// [`Sense::CLICK`] activation, i.e. a [`Sense::FOCUSABLE`] widget currently holds focus:
    /// otherwise the same keys reach an app's own text input or other key handling unclaimed.
    /// Everything else ([`Event::Resize`], [`Event::Paste`], [`Event::FocusGained`]/
    /// [`Event::FocusLost`], and any key this interaction doesn't bind) is never claimed, even
    /// while a widget is focused and active: those need to reach whatever's behind an open
    /// overlay (a resize still has to reflow the screen under a dropdown), which a coarser "is
    /// the overlay open" gate cannot express without also swallowing them.
    #[must_use]
    pub fn handle_event(&mut self, event: &Event) -> Consumed {
        let claimed = match event {
            Event::Mouse(mouse) => self.prev_hits.topmost_at(mouse.position).is_some(),
            Event::Key(key)
                if key.is_down() && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) =>
            {
                self.focus.has_order()
            }
            // A widget's `Sense` (whether Enter/Space double as its activation, per
            // `Sense::FOCUSABLE`/`Sense::CLICK`) isn't visible here, only per-`interact` call: the
            // same approximation `activate_focused` below makes, conservative in the same
            // direction (a focused non-activating widget makes this report claimed even though
            // no `interact` call will actually consume the key as a click).
            Event::Key(_) => is_activation_key(event) && self.focus.focused().is_some(),
            // `Resize`/`Paste`/`FocusGained`/`FocusLost`, plus any future variant: never claimed.
            Event::Resize(..) | Event::Paste(_) | Event::FocusGained | Event::FocusLost | _ => {
                false
            }
        };

        self.pointer.handle_event(event);
        self.focus.handle_event(event);
        self.activate_focused |= is_activation_key(event);

        claimed.into()
    }

    /// Whether the pointer is over a rect registered last frame, so a pointer event delivered
    /// right now would land on a widget rather than fall through to whatever's behind this
    /// interaction.
    ///
    /// Answerable before this frame has drawn anything: [`interact`](Self::interact) registers
    /// each rect for the *next* frame's hit test (see the [frame lifecycle](Self#frame-lifecycle)),
    /// so last frame's registrations, and therefore this answer, are already complete the moment
    /// [`begin_frame`](Self::begin_frame) returns. An app with more than one [`Interaction`] (a
    /// menu bar above a screen stack, say) can call this on the frontmost one before routing a
    /// pointer event anywhere, the same role `io.WantCaptureMouse` plays in Dear `ImGui`.
    ///
    /// Reads the pointer's *live* position, not a frame-stale snapshot: unlike
    /// [`Response::hovered`], which only updates once a frame via `begin_frame`, this is meant to
    /// be called right after [`handle_event`](Self::handle_event) to decide where to route the
    /// event that was just fed in, so it has to reflect that event's effect on the pointer
    /// immediately, not wait for the next `begin_frame`.
    #[must_use]
    pub fn wants_pointer(&self) -> bool {
        self.pointer
            .pos()
            .is_some_and(|pos| self.prev_hits.topmost_at(pos).is_some())
    }

    /// Whether a widget currently holds keyboard focus, so a key event delivered right now is
    /// likely to be consumed rather than fall through.
    ///
    /// Unlike [`wants_pointer`](Self::wants_pointer), this reads live, not a frame-stale
    /// snapshot: focus is plain level state that persists across frames until
    /// [`FocusRing::request`]/[`clear`](FocusRing::clear) moves it, the same reasoning
    /// [`Response::focused`] documents.
    #[must_use]
    pub const fn wants_keyboard(&self) -> bool {
        self.focus.focused().is_some()
    }

    /// Eases toward `1.0` when `target` is `true`, `0.0` otherwise, over `duration`, advancing by
    /// `frame.delta`. Owns one [`Tween`] per `id`, created (at rest, on whichever side `target`
    /// starts on) the first time this is called for that `id`, and dropped once it settles back
    /// at rest at its current target, so this doesn't grow unbounded across a long-running app
    /// with many transient `Id`s (an overlay's per-item ids, say).
    ///
    /// The bridge from [`Response`] to [`retroglyph_core::animate`](retroglyph_core): a widget's
    /// `render` (already handed a `Response`) calls this once per frame with, say,
    /// `response.hovered()` as `target`, and blends its idle/hover style by the result, without
    /// declaring a `Tween` field of its own or hand-diffing this frame's `hovered()` against
    /// last frame's to find the edge that should retarget it -- both of which an app would
    /// otherwise need to do once per animated `Id`, since neither [`Tween`] nor
    /// [`Interaction`](Self) tracks that edge on its own.
    ///
    /// `duration` only takes effect while `id`'s tween is created, i.e. the first call for that
    /// `id`, or the first call after a previous one settled and was pruned: changing it on a
    /// later call while the tween is still in flight has no effect, the same as
    /// [`Tween::retarget`] leaving `duration` untouched.
    pub fn animate(&mut self, id: Id, target: bool, duration: Duration, frame: &Frame) -> f32 {
        let target_value = f32::from(target);
        let index = if let Some(index) = self
            .tweens
            .iter()
            .position(|(existing, ..)| *existing == id)
        {
            let (_, last_target, tween) = &mut self.tweens[index];
            if *last_target != target {
                tween.retarget(target_value);
                *last_target = target;
            }
            tween.update(frame.delta);
            index
        } else {
            self.tweens.push((
                id,
                target,
                Tween::new(target_value, target_value).duration(duration),
            ));
            self.tweens.len() - 1
        };

        let (_, _, tween) = &self.tweens[index];
        let value = tween.value();
        if tween.is_finished() {
            self.tweens.remove(index);
        }
        value
    }

    /// Register `rect` as a barrier: a pointer inside it never resolves to anything registered by
    /// an earlier [`interact`](Self::interact) call, no matter how the two overlap.
    ///
    /// [`Ui::modal`](crate::Ui::modal) is the documented way to use this; call it directly only
    /// when driving [`interact`](Self::interact) without a [`Ui`].
    pub fn push_barrier(&mut self, rect: Rect) {
        self.hits.push_barrier(rect);
    }

    /// Register `id`'s `rect` for whatever `sense` asks for, and report
    /// what happened to it, resolved from *last* frame's input: see the
    /// [`Interaction`] docs for the frame lifecycle this implies.
    pub fn interact(&mut self, rect: Rect, id: Id, sense: Sense) -> Response {
        // `DISABLED` is a modifier, not a capability: it never changes what
        // gets hit-tested (so `hovered` keeps working), only what gets
        // registered with `FocusRing` and what `Response` reports back.
        let disabled = sense.is_disabled();

        if sense.wants_pointer() {
            self.hits.push(rect, id);
        }
        if sense.contains(Sense::FOCUSABLE) && !disabled {
            self.focus.register(id);
        }

        let hovered = sense.wants_pointer() && self.resolved_hover == Some(id);
        let is_active = self.active == Some(id);
        let senses_click = sense.contains(Sense::CLICK) && !disabled;
        let is_focused = !disabled && self.focus.is_focused(id);
        let key_activated =
            senses_click && sense.contains(Sense::FOCUSABLE) && is_focused && self.activate_focused;
        // `is_active` is assigned from `resolved_hover` alone in `begin_frame`,
        // independent of `sense`, so a disabled (or non-`CLICK`-sensing,
        // e.g. hover-only or `NONE`) widget can still become "active"
        // merely by being topmost at press time even though it never asked
        // for `CLICK`: gated on `senses_click` (which already folds in
        // `!disabled`) the same way `held`/`clicked` below are, rather than
        // relying on `is_active` alone.
        let released_here = senses_click && is_active && self.resolved_release;
        // Not gated on `self.pointer.is_down()`: the release
        // frame (where `is_down` just went false) must still see `dragging
        // == true` so `clicked` below correctly stays suppressed for a
        // drag's terminating release, not just the frames in between.
        let dragging =
            is_active && sense.contains(Sense::DRAG) && !disabled && self.past_drag_threshold();

        // Live re-check, not gated on `hovered`/`resolved_hover` the way `pressed`
        // is: those are resolved from *last* frame's hit-test snapshot (see the `Interaction`
        // frame-lifecycle docs), but a slide-off cancellation needs to see the pointer's
        // *current* position the instant it leaves this rect, not one frame later. Mirrors how
        // `dragging` above already reads `self.pointer.pos()` live instead of `resolved_pos`, and
        // how `scroll_delta` below bypasses the single-topmost-winner rule: same "read live
        // state, scoped to my own rect" shape, applied a third time.
        let held = senses_click
            && is_active
            && self.pointer.is_down(MouseButton::Left)
            && self.pointer.pos().is_some_and(|pos| rect.contains_pos(pos));

        if senses_click && sense.contains(Sense::FOCUSABLE) && released_here && hovered && !dragging
        {
            self.focus.request(id);
        }

        // Scroll isn't gated on `hovered` (single topmost
        // winner) the way click/press/release/drag are: a scrollable
        // container's own rect is usually fully covered by its rows/items
        // (each independently sensing HOVER | CLICK so they're individually
        // clickable), which would otherwise shadow the container at every
        // point inside it and make it un-scrollable. Any rect the resolved
        // pointer position falls within gets scroll credit, regardless of
        // what's drawn on top of it, matching how wheel input behaves in
        // most real UIs (it reaches the nearest scrollable ancestor, not
        // just whatever's topmost at the exact pixel).
        let scrollable_here = !disabled
            && sense.wants_pointer()
            && sense.contains(Sense::SCROLL)
            && self.resolved_pos.is_some_and(|pos| rect.contains_pos(pos));

        // The secondary button gets a narrower resolution than the primary
        // one: no drag-threshold suppression (secondary-button drags aren't
        // a gesture this module tracks), and it doesn't drive focus the way
        // a primary click does (see `Response::secondary_clicked`'s doc
        // comment).
        let secondary_is_active = !disabled && self.secondary_active == Some(id);
        let secondary_clicked = sense.contains(Sense::SECONDARY_CLICK)
            && !disabled
            && secondary_is_active
            && self.resolved_secondary_release
            && hovered;

        let clicked = (senses_click && released_here && hovered && !dragging) || key_activated;
        // A click pairs with the *previous* click only if it landed on this same `id` within
        // `double_click_window` frames: checked and consumed here, per `id`, rather than as a
        // single crate-wide "last click" slot, so two widgets clicked in quick succession don't
        // spuriously pair with each other. Consuming the pairing (resetting to `None` instead of
        // leaving it set) means a third click starts counting fresh: each pair of qualifying
        // clicks reports exactly one `double_clicked` frame, not one on every click after the
        // second.
        let double_clicked = clicked
            && self.last_click.is_some_and(|(last_id, last_frame)| {
                last_id == id
                    && self.frame_count.saturating_sub(last_frame)
                        <= u64::from(self.double_click_window)
            });
        if clicked {
            self.last_click = if double_clicked {
                None
            } else {
                Some((id, self.frame_count))
            };
        }

        // `is_active.then_some(self.drag_origin).flatten()`, matching `press_origin` below: not
        // gated on `resolved_pos`/`hovered`, live, matching `held`/`dragging` above,
        // so a drag that has moved outside this widget's own rect still keeps reporting. Gated on
        // `senses_click` for the same reason `released_here`/`held` are above: `is_active` alone
        // says nothing about whether this widget ever asked for `CLICK`.
        let press_origin = (senses_click && is_active)
            .then_some(self.drag_origin)
            .flatten();
        let drag_delta = press_origin.zip(self.pointer.pos()).map(|(origin, pos)| {
            (
                i32::from(pos.x) - i32::from(origin.x),
                i32::from(pos.y) - i32::from(origin.y),
            )
        });

        // Diffed against `prev_focused`, snapshotted in `begin_frame` before this frame's own
        // Tab-cycling/click-to-focus could move it: see that field's doc comment.
        let gained_focus = is_focused && self.prev_focused != Some(id);
        let lost_focus = !is_focused && self.prev_focused == Some(id);

        Response {
            hovered,
            pressed: (senses_click && is_active && self.resolved_press) || key_activated,
            released: released_here || key_activated,
            clicked,
            double_clicked,
            held,
            dragging,
            focused: is_focused,
            gained_focus,
            lost_focus,
            secondary_clicked,
            disabled,
            scroll_delta: if scrollable_here {
                self.resolved_scroll
            } else {
                0
            },
            // Resolved from the same `resolved_pos`/`resolved_hover` snapshot `hovered` comes
            // from above, so the two stay consistent (one frame stale together) rather than
            // mixing a stale hover flag with a live position.
            pointer_pos: hovered.then_some(self.resolved_pos).flatten(),
            // `drag_origin` is set once, in `begin_frame`, when a press lands on `active`, and
            // cleared in `end_frame` on release: live state, not part of the per-`interact`
            // resolved-snapshot fields above, matching how `held` reads `active` directly rather
            // than a snapshot of it.
            press_origin,
            drag_delta,
            rect,
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
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
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
            let _ = interaction.handle_event(event);
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

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
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

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        assert!(!save.dragging()); // hasn't moved yet
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(4, 0), // 2 cells from the press origin
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        assert!(save.dragging());
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
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

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        // frame 2: press resolves against frame 1's registrations, pointer still over Save.
        let (save, _) = frame(&mut interaction);
        assert!(save.held());

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(20, 0), // outside Save's rect, still held down
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        assert!(!save.held()); // slid off before release: cancels immediately

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0), // back over Save, still held down, before release
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        assert!(save.held()); // back inside: held again

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
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

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        // `active` is assigned from whichever id was topmost at press time, regardless of that
        // id's own `Sense` (see `begin_frame`'s `self.active = self.resolved_hover;`), so Save
        // is `is_active` here even though it only sensed `HOVER`: `held` must still stay false.
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::hover());
        interaction.end_frame();
        assert!(!save.held());
    }

    #[test]
    fn hover_only_sense_must_not_report_pressed_or_released() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: Save/Cancel registered with Sense::click()
        click_at(&mut interaction, Pos::new(2, 0)); // full press+release cycle over Save's rect

        interaction.begin_frame();
        // Save is `is_active` here, same as `held_requires_click_sense` above, but this call
        // only senses `HOVER`: `pressed`/`released`/`press_origin`/`drag_delta` must all stay at
        // their not-asked-for defaults, matching `held`'s existing guard.
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::hover());
        interaction.end_frame();

        assert!(save.hovered());
        assert!(!save.pressed());
        assert!(!save.released());
        assert_eq!(save.press_origin(), None);
        assert_eq!(save.drag_delta(), None);
    }

    #[test]
    fn sense_none_returns_response_default() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: Save/Cancel registered with Sense::click()
        click_at(&mut interaction, Pos::new(2, 0)); // full press+release cycle over Save's rect

        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::NONE);
        interaction.end_frame();

        // Per `Sense::NONE`'s own doc, `interact` registers the id nowhere and reports nothing it
        // never asked for. `rect` is the one exception: it always echoes back this frame's
        // `interact` area regardless of `Sense` (see `rect_echoes_back_this_frames_area`), so it's
        // deliberately not asserted here.
        assert!(!save.hovered());
        assert!(!save.pressed());
        assert!(!save.released());
        assert!(!save.clicked());
        assert!(!save.double_clicked());
        assert!(!save.held());
        assert!(!save.dragging());
        assert!(!save.focused());
        assert!(!save.gained_focus());
        assert!(!save.lost_focus());
        assert!(!save.secondary_clicked());
        assert!(!save.disabled());
        assert_eq!(save.scroll_delta(), 0);
        assert_eq!(save.pointer_pos(), None);
        assert_eq!(save.press_origin(), None);
        assert_eq!(save.drag_delta(), None);
    }

    #[test]
    fn pointer_pos_matches_hover_and_is_none_elsewhere() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction);

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        let (save, cancel) = frame(&mut interaction);
        assert_eq!(save.pointer_pos(), Some(Pos::new(2, 0)));
        assert_eq!(cancel.pointer_pos(), None);
    }

    #[test]
    fn press_origin_stays_put_for_the_duration_of_a_drag() {
        let mut interaction = Interaction::<Id>::new().with_drag_threshold(1);
        let _ = frame(&mut interaction);

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        let cancel = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::drag());
        interaction.end_frame();
        assert_eq!(save.press_origin(), Some(Pos::new(2, 0)));
        assert_eq!(cancel.press_origin(), None); // press never landed on Cancel

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(4, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        interaction.end_frame();
        // Still the original press position, not the pointer's current one.
        assert_eq!(save.press_origin(), Some(Pos::new(2, 0)));
    }

    #[test]
    fn drag_delta_keeps_reporting_once_the_pointer_leaves_the_rect() {
        let mut interaction = Interaction::<Id>::new().with_drag_threshold(1);
        let _ = frame(&mut interaction);

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        let cancel = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::drag());
        interaction.end_frame();
        assert_eq!(save.drag_delta(), Some((0, 0))); // press just landed, hasn't moved yet
        assert_eq!(cancel.drag_delta(), None); // press never landed on Cancel

        // Move far past Save's own rect (5 cells wide): drag_delta must keep tracking the full
        // displacement from the press origin, unlike `pointer_pos`, which would go `None` here.
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(20, 3),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::drag());
        interaction.end_frame();
        assert_eq!(save.pointer_pos(), None); // outside Save's rect now
        assert_eq!(save.drag_delta(), Some((18, 3)));
    }

    #[test]
    fn scroll_reports_only_while_hovered_and_sensed() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction);

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
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
    /// topmost-hover winner: see [`Sense::SCROLL`]'s doc comment.
    #[test]
    fn scroll_reaches_a_container_through_an_overlapping_child() {
        let mut interaction = Interaction::<Id>::new();
        interaction.begin_frame();
        // The child (Cancel, standing in for a list row) is registered
        // *after* the container (Save), so it's topmost at any point they
        // share, exactly like a row drawn on top of its list container.
        let _ = interaction.interact(Rect::new(0, 0, 10, 1), Id::Save, Sense::scroll());
        let _ = interaction.interact(
            Rect::new(0, 0, 10, 1),
            Id::Cancel,
            Sense::HOVER | Sense::CLICK,
        );
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
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
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
    }

    #[test]
    fn secondary_click_is_independent_of_the_primary_button() {
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

        let mut interaction = Interaction::<Id>::new();
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

    fn frame_disabled(interaction: &mut Interaction<Id>) -> Response {
        interaction.begin_frame();
        let save = interaction.interact(
            Rect::new(0, 0, 5, 1),
            Id::Save,
            Sense::click() | Sense::SECONDARY_CLICK | Sense::DISABLED,
        );
        interaction.end_frame();
        save
    }

    #[test]
    fn disabled_widget_never_reports_click_press_or_held() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame_disabled(&mut interaction); // frame 1: registers Save

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let save = frame_disabled(&mut interaction); // frame 2: press resolves
        assert!(!save.pressed());
        assert!(!save.held());

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            position: Pos::new(2, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let save = frame_disabled(&mut interaction); // frame 3: release resolves
        assert!(!save.released());
        assert!(!save.clicked());
    }

    #[test]
    fn disabled_widget_never_reports_dragging_or_scroll() {
        // The `!disabled` guards on `dragging` and `scrollable_here` in `interact` are otherwise
        // never exercised, since `frame_disabled` above only senses `CLICK`/`SECONDARY_CLICK`.
        let mut interaction = Interaction::<Id>::new().with_drag_threshold(1);
        let sense = Sense::drag() | Sense::SCROLL | Sense::DISABLED;

        interaction.begin_frame();
        let _ = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, sense);
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        interaction.begin_frame();
        let _ = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, sense);
        interaction.end_frame();

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(4, 0), // 2 cells from the press origin, past the threshold
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            position: Pos::new(4, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, sense);
        interaction.end_frame();

        assert!(!save.dragging());
        assert_eq!(save.scroll_delta(), 0);
        assert!(save.hovered()); // hit-testing keeps working while disabled
    }

    #[test]
    fn disabled_widget_still_reports_hover() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame_disabled(&mut interaction);

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        let save = frame_disabled(&mut interaction);
        assert!(save.hovered()); // hit-testing keeps working while disabled
    }

    #[test]
    fn disabled_widget_never_reports_secondary_click() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame_disabled(&mut interaction);
        right_click_at(&mut interaction, Pos::new(2, 0));

        let save = frame_disabled(&mut interaction);
        assert!(!save.secondary_clicked());
    }

    #[test]
    fn disabled_widget_is_skipped_by_focus_ring() {
        let mut interaction = Interaction::<Id>::new();

        // Frame 1: register Save (disabled) and Cancel (enabled) for the
        // *next* frame's Tab order, mirroring `tab_focuses_then_enter_activates_without_any_pointer`.
        interaction.begin_frame();
        let _ = interaction.interact(
            Rect::new(0, 0, 5, 1),
            Id::Save,
            Sense::click() | Sense::DISABLED,
        );
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();

        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        interaction.begin_frame(); // finalizes frame 1's registrations into the Tab order
        let _ = interaction.handle_event(&tab);
        // Save is registered nowhere in that order while disabled, so the
        // first Tab lands directly on Cancel.
        assert_eq!(interaction.focus().focused(), Some(Id::Cancel));
    }

    #[test]
    fn disabling_a_focused_widget_clears_its_response_and_self_heals_on_the_next_tab() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel, both focusable

        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (save, _) = frame_with_events(&mut interaction, &[tab]);
        assert!(save.focused()); // Save now holds focus

        // frame 3: Save becomes disabled without focus moving anywhere else.
        interaction.begin_frame();
        let save = interaction.interact(
            Rect::new(0, 0, 5, 1),
            Id::Save,
            Sense::click() | Sense::DISABLED,
        );
        let _ = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::click());
        interaction.end_frame();
        assert!(!save.focused()); // Response no longer claims focus...

        // ...and the next Tab moves cleanly onto Cancel, exactly like any
        // other stale-focus-not-in-order recovery.
        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (_, cancel) = frame_with_events(&mut interaction, &[tab]);
        assert!(cancel.focused());
    }

    #[test]
    fn disabled_bit_reports_via_response_even_on_a_hover_only_sense() {
        // `Sense::hover()` never granted click/drag/focus in the first
        // place, so `DISABLED` here changes nothing about what's reported
        // except `Response::disabled` itself.
        let mut interaction = Interaction::<Id>::new();
        interaction.begin_frame();
        let disabled = interaction.interact(
            Rect::new(0, 0, 5, 1),
            Id::Save,
            Sense::hover() | Sense::DISABLED,
        );
        let plain = interaction.interact(Rect::new(6, 0, 5, 1), Id::Cancel, Sense::hover());
        interaction.end_frame();

        assert!(disabled.disabled());
        assert!(!plain.disabled());
        assert_eq!(disabled.hovered(), plain.hovered()); // both unhovered, same either way
    }
    #[test]
    fn wants_pointer_is_false_before_any_frame_has_registered_anything() {
        let interaction = Interaction::<Id>::new();
        assert!(!interaction.wants_pointer());
    }

    #[test]
    fn a_pointer_event_on_a_registered_rect_is_consumed() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel
        let _ = frame(&mut interaction); // frame 2: frame 1's registrations become `prev_hits`

        let consumed = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(2, 0), // over Save
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(consumed, Consumed::Yes);
        assert!(interaction.wants_pointer());
    }

    #[test]
    fn a_pointer_event_on_empty_space_is_not_consumed() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel
        let _ = frame(&mut interaction); // frame 2: frame 1's registrations become `prev_hits`

        let consumed = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(20, 20), // outside both Save and Cancel
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(consumed, Consumed::No);
        assert!(!interaction.wants_pointer());
    }

    /// Regression case for the #598 defect: a coarse "is a widget focused and active" gate would
    /// wrongly swallow events a focused/active widget has no business claiming, starving whatever
    /// needed them: `Resize`/`Paste`/`FocusLost`/`FocusGained`, and any key that isn't an
    /// activation key. `Consumed` must report `No` for all of them even while a widget holds
    /// focus and is mid-press, so a caller routing by [`Consumed`] doesn't reproduce that bug.
    #[test]
    fn unclaimed_events_are_never_consumed_even_while_focused_and_active() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel as focusable

        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (save, _) = frame_with_events(&mut interaction, &[tab]); // frame 2: Tab focuses Save
        assert!(save.focused());

        let _ = interaction.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(2, 0), // over Save, now active and mid-press
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));

        assert_eq!(
            interaction.handle_event(&Event::Resize(80, 24)),
            Consumed::No
        );
        assert_eq!(
            interaction.handle_event(&Event::Paste("hello".to_owned())),
            Consumed::No
        );
        assert_eq!(interaction.handle_event(&Event::FocusLost), Consumed::No);

        // A non-activation key and `FocusGained` also aren't claimed by a focused/active widget:
        // only an activation key while focused, or a pointer event while active, is consumed.
        assert_eq!(
            interaction.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Char('a'),
                KeyModifiers::NONE
            ))),
            Consumed::No
        );
        assert_eq!(
            interaction.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Escape,
                KeyModifiers::NONE
            ))),
            Consumed::No
        );
        assert_eq!(interaction.handle_event(&Event::FocusGained), Consumed::No);
    }

    #[test]
    fn tab_is_consumed_only_when_something_is_registered_as_focusable() {
        let mut interaction = Interaction::<Id>::new();
        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        // Nothing has ever been registered yet.
        assert_eq!(interaction.handle_event(&tab), Consumed::No);

        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel as focusable
        let _ = frame(&mut interaction); // frame 2: frame 1's registrations finalize the order
        assert_eq!(interaction.handle_event(&tab), Consumed::Yes);
    }

    #[test]
    fn rect_echoes_back_this_frames_area() {
        let mut interaction = Interaction::<Id>::new();
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::hover());
        interaction.end_frame();
        assert_eq!(save.rect(), Rect::new(0, 0, 5, 1));
    }

    #[test]
    fn a_second_click_within_the_window_reports_double_clicked() {
        let mut interaction = Interaction::<Id>::new().with_double_click_window(5);
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel

        click_at(&mut interaction, Pos::new(2, 0));
        let (save, _) = frame(&mut interaction); // frame 2: first click resolves
        assert!(save.clicked());
        assert!(!save.double_clicked());

        click_at(&mut interaction, Pos::new(2, 0));
        let (save, _) = frame(&mut interaction); // frame 3: second click, well within the window
        assert!(save.clicked());
        assert!(save.double_clicked());

        // A third click starts counting fresh rather than pairing with the second again.
        click_at(&mut interaction, Pos::new(2, 0));
        let (save, _) = frame(&mut interaction);
        assert!(save.clicked());
        assert!(!save.double_clicked());
    }

    #[test]
    fn a_second_click_outside_the_window_does_not_pair() {
        let mut interaction = Interaction::<Id>::new().with_double_click_window(1);
        let _ = frame(&mut interaction); // frame 1: registers Save/Cancel

        click_at(&mut interaction, Pos::new(2, 0));
        let _ = frame(&mut interaction); // frame 2: first click resolves
        let _ = frame(&mut interaction); // frame 3: window (1 frame) already elapsed

        click_at(&mut interaction, Pos::new(2, 0));
        let (save, _) = frame(&mut interaction); // frame 4: too late to pair with frame 2's click
        assert!(save.clicked());
        assert!(!save.double_clicked());
    }

    #[test]
    fn a_second_click_on_a_different_widget_does_not_pair() {
        let mut interaction = Interaction::<Id>::new().with_double_click_window(5);
        let _ = frame(&mut interaction);

        click_at(&mut interaction, Pos::new(2, 0)); // Save
        let _ = frame(&mut interaction);

        click_at(&mut interaction, Pos::new(7, 0)); // Cancel, not Save
        let (save, cancel) = frame(&mut interaction);
        assert!(!save.double_clicked());
        assert!(cancel.clicked());
        assert!(!cancel.double_clicked());
    }

    #[test]
    fn gained_and_lost_focus_are_one_shot_edges_around_level_focused() {
        let mut interaction = Interaction::<Id>::new();
        let _ = frame(&mut interaction); // registers Save/Cancel as focusable for the *next* frame

        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (save, cancel) = frame_with_events(&mut interaction, core::slice::from_ref(&tab));
        assert!(save.focused());
        assert!(save.gained_focus()); // just became focused this frame
        assert!(!save.lost_focus());
        assert!(!cancel.gained_focus());

        // Still focused next frame, with nothing moving focus: level state stays true, but the
        // one-shot edge does not re-fire.
        let (save, _) = frame(&mut interaction);
        assert!(save.focused());
        assert!(!save.gained_focus());
        assert!(!save.lost_focus());

        // Tab moves focus off Save and onto Cancel.
        let (save, cancel) = frame_with_events(&mut interaction, core::slice::from_ref(&tab));
        assert!(!save.focused());
        assert!(save.lost_focus());
        assert!(!save.gained_focus());
        assert!(cancel.focused());
        assert!(cancel.gained_focus());
    }

    /// Regression test for retroglyph#704: clicking a `Sense::CLICK` widget that never asked for
    /// `Sense::FOCUSABLE` (e.g. a hoverable-but-not-Tab-stoppable row) used to call
    /// `FocusRing::request` unconditionally, silently stealing focus from whatever *was* actually
    /// in the Tab ring, without ever removing that widget from the ring itself.
    #[test]
    fn clicking_a_non_focusable_widget_must_not_steal_focus() {
        let mut interaction = Interaction::<Id>::new();

        // frame 1: Save is focusable (Sense::click()), Cancel is clickable but not focusable.
        interaction.begin_frame();
        let _ = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(
            Rect::new(6, 0, 5, 1),
            Id::Cancel,
            Sense::HOVER | Sense::CLICK,
        );
        interaction.end_frame();

        // Tab focuses Save, the only widget registered with FOCUSABLE.
        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        interaction.begin_frame();
        let _ = interaction.handle_event(&tab);
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let _ = interaction.interact(
            Rect::new(6, 0, 5, 1),
            Id::Cancel,
            Sense::HOVER | Sense::CLICK,
        );
        interaction.end_frame();
        assert!(save.focused());

        // Click Cancel, which never asked for FOCUSABLE.
        click_at(&mut interaction, Pos::new(7, 0));
        interaction.begin_frame();
        let save = interaction.interact(Rect::new(0, 0, 5, 1), Id::Save, Sense::click());
        let cancel = interaction.interact(
            Rect::new(6, 0, 5, 1),
            Id::Cancel,
            Sense::HOVER | Sense::CLICK,
        );
        interaction.end_frame();
        assert!(cancel.clicked());
        assert!(!cancel.focused()); // never registered as focusable: must not report focus either
        assert!(save.focused()); // focus must stay put, not get silently stolen
    }

    fn frame_with_delta(delta: Duration) -> Frame {
        Frame { delta, frame: 0 }
    }

    // Exact float equality is intentional in the `animate_*` tests below, mirroring
    // `retroglyph_core::animate::tween`'s own tests: every value under test is produced by
    // `Easing::Linear` (`Tween`'s default) at exactly-representable fractions of `duration`, not
    // an accumulated or transcendental result where an epsilon comparison would be appropriate.
    #[test]
    #[allow(clippy::float_cmp)]
    fn animate_starts_at_rest_on_whichever_side_target_begins_on() {
        let mut interaction = Interaction::<Id>::new();
        let frame = frame_with_delta(Duration::ZERO);

        assert_eq!(
            interaction.animate(Id::Save, false, Duration::from_millis(100), &frame),
            0.0
        );
        assert_eq!(
            interaction.animate(Id::Cancel, true, Duration::from_millis(100), &frame),
            1.0
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn animate_eases_toward_the_target_once_it_flips() {
        let mut interaction = Interaction::<Id>::new();
        let duration = Duration::from_millis(100);

        // At rest at 0.0 until `target` flips true.
        let value =
            interaction.animate(Id::Save, false, duration, &frame_with_delta(Duration::ZERO));
        assert_eq!(value, 0.0);

        // Flips: retargets toward 1.0, halfway through `duration` after one more update.
        let value = interaction.animate(
            Id::Save,
            true,
            duration,
            &frame_with_delta(Duration::from_millis(50)),
        );
        assert_eq!(value, 0.5); // Easing::Linear (Tween's default), so exactly halfway
        assert!(value < 1.0);

        // Finishes once `duration` has fully elapsed since the flip.
        let value = interaction.animate(
            Id::Save,
            true,
            duration,
            &frame_with_delta(Duration::from_millis(50)),
        );
        assert_eq!(value, 1.0);
    }

    #[test]
    fn animate_prunes_the_entry_once_it_settles_at_rest() {
        let mut interaction = Interaction::<Id>::new();
        let duration = Duration::from_millis(100);

        // Created at rest (`from == to == 0.0`): `is_finished` only becomes true once `duration`
        // has elapsed, exercising the prune path even for a tween that never actually moved.
        let _ = interaction.animate(Id::Save, false, duration, &frame_with_delta(Duration::ZERO));
        assert_eq!(interaction.tweens.len(), 1);

        let _ = interaction.animate(Id::Save, false, duration, &frame_with_delta(duration));
        assert!(
            interaction.tweens.is_empty(),
            "a settled tween must be dropped, not accumulate"
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn animate_tracks_each_id_independently() {
        let mut interaction = Interaction::<Id>::new();
        let duration = Duration::from_millis(100);

        let save = interaction.animate(Id::Save, true, duration, &frame_with_delta(Duration::ZERO));
        let cancel = interaction.animate(
            Id::Cancel,
            false,
            duration,
            &frame_with_delta(Duration::ZERO),
        );
        assert_eq!(save, 1.0);
        assert_eq!(cancel, 0.0);

        // Advancing Save must not perturb Cancel's independently tracked tween.
        let save = interaction.animate(
            Id::Save,
            false,
            duration,
            &frame_with_delta(Duration::from_millis(50)),
        );
        let cancel = interaction.animate(
            Id::Cancel,
            false,
            duration,
            &frame_with_delta(Duration::from_millis(50)),
        );
        assert_eq!(save, 0.5);
        assert_eq!(cancel, 0.0);
    }
}
