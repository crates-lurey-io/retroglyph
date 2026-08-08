//! Headless widget-level test harness: [`WidgetHarness`] drives a bare [`Interaction<Id>`]/[`Ui`]
//! against [`Headless`], without an
//! [`App`](retroglyph_core::app::App).
//!
//! `retroglyph_core::testing::TestHarness` is the right tool once a whole app exists to drive,
//! but it's generic over `A: App<Headless>`, so a widget or a bare `Interaction<Id>` needs a
//! throwaway `App` impl just to reach it. A widget test that only needs a few multi-frame
//! sequences gets away with `Interaction::new()`/`begin_frame()` inline instead (see
//! `crates/ui/src/widget/button.rs`'s own tests), but that stops scaling once a widget's suite is
//! built from repeated click/assert sequences: `Menu`'s tests (`crates/ui/src/widget/menu.rs`)
//! are exactly that shape, each hand-rolling the same register-hits/feed-events/resolve frame
//! pair `WidgetHarness` now does once. Feature-gated (`testing`), no effect on release builds.

use alloc::collections::VecDeque;
use alloc::string::String;

use retroglyph_core::backend::Headless;
use retroglyph_core::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph_core::grid::Pos;
use retroglyph_core::terminal::Terminal;

use crate::interact::Interaction;
use crate::ui::Ui;

/// Drives a bare [`Interaction<Id>`] against a [`Headless`] backend: queues synthetic input,
/// steps a frame through a caller-supplied draw closure, and reads back the rendered view.
///
/// The sibling of `retroglyph_core::testing::TestHarness` for testing a widget (or a bare
/// [`Interaction<Id>`]) directly, without wrapping it in an [`App`](retroglyph_core::app::App)
/// first. Where `TestHarness` hands each frame to `App::update`, `WidgetHarness` hands each frame
/// a [`Ui<'_, '_, Id>`](Ui) built from its own [`Interaction<Id>`] and [`Headless`] surface, and
/// leaves whatever state a widget needs (a `MenuState`, a selection index, ...) owned by the
/// caller's closure instead of an `App` impl.
///
/// # Frame timing differs from `TestHarness`
///
/// `TestHarness::step` drains at most one queued [`Event`] per call, because it's reproducing how
/// a real `App` sees input trickle in one `Backend` push at a time. `WidgetHarness::step` feeds
/// [`Interaction::handle_event`] directly, with no `Backend` queue in between, so nothing forces
/// that one-event-per-frame pacing: [`step`](Self::step) drains its *entire* queue in one call,
/// between [`begin_frame`](Interaction::begin_frame) and the draw closure -- [`Interaction`]'s
/// own docs call this "the realistic call pattern" (see `Interaction`'s
/// `press_and_release_in_separate_frames_still_resolves_a_click` test), and it's load-bearing for
/// Tab: `FocusRing`'s registration order only finalizes inside `begin_frame`, so a Tab fed before
/// it would still cycle through the previous frame's stale order. One consequence: a pointer
/// gesture (click, hover) fed to `step` resolves on the *following* `step` call, not the one that
/// fed it, since the flags `handle_event` sets aren't read until the next `begin_frame`; a key
/// press that moves focus (Tab) or a live-state field like
/// [`wants_pointer`](Interaction::wants_pointer) update immediately, within the same `step`. A
/// gesture that needs to be observed mid-flight (a drag, a hover that switches mid-scroll) still
/// needs its own [`mouse_move`](Self::mouse_move)/[`step`](Self::step) pair per intermediate
/// position, exactly like driving [`Interaction`] by hand.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::Rect;
/// use retroglyph_ui::testing::WidgetHarness;
/// use retroglyph_ui::{Button, InteractiveWidget};
///
/// #[derive(Clone, Copy, PartialEq, Eq)]
/// enum Id {
///     Save,
/// }
///
/// let mut harness = WidgetHarness::<Id>::new(10, 3);
/// let area = Rect::new(0, 0, 7, 1);
/// let button = Button::new("Save");
///
/// // Frame 1: register the button's hit rect (nothing queued yet).
/// harness.step(|ui| ui.show(area, Id::Save, &button));
///
/// harness.click(2, 0);
/// // Frame 2: feeds the click; the flags it sets aren't read until the next `begin_frame`.
/// harness.step(|ui| ui.show(area, Id::Save, &button));
/// // Frame 3: resolves it.
/// let response = harness.step(|ui| ui.show(area, Id::Save, &button));
/// assert!(response.clicked());
/// ```
pub struct WidgetHarness<Id> {
    term: Terminal<Headless>,
    interaction: Interaction<Id>,
    queued: VecDeque<Event>,
}

impl<Id> WidgetHarness<Id> {
    /// Creates a harness with a `width` x `height` [`Headless`] backend and a fresh
    /// [`Interaction<Id>`].
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            term: Terminal::new(Headless::new(width, height)),
            interaction: Interaction::new(),
            queued: VecDeque::new(),
        }
    }

    /// Queues a synthetic event for the next [`step`](Self::step) call.
    ///
    /// Only queues: nothing sees it until a frame runs. Prefer the typed helpers
    /// ([`click`](Self::click), [`key`](Self::key), [`mouse_move`](Self::mouse_move)) unless the
    /// [`Event`] variant needed isn't one of them.
    pub fn push_event(&mut self, event: Event) {
        self.queued.push_back(event);
    }

    /// Queues a left-button click (press then release) at `(x, y)`, with no modifiers.
    ///
    /// Both events are drained by the same [`step`](Self::step) call, but (see "Frame timing" on
    /// [`WidgetHarness`]) that only sets the underlying flags: the click itself resolves on the
    /// `step` *after* that one, once its `begin_frame` reads them.
    pub fn click(&mut self, x: u16, y: u16) {
        self.click_button(x, y, MouseButton::Left);
    }

    /// Queues a click (press then release) with `button` at `(x, y)`, with no modifiers.
    pub fn click_button(&mut self, x: u16, y: u16, button: MouseButton) {
        let position = Pos::new(x, y);
        for kind in [MouseEventKind::Down(button), MouseEventKind::Up(button)] {
            self.push_event(Event::Mouse(MouseEvent::new(
                kind,
                position,
                KeyModifiers::NONE,
            )));
        }
    }

    /// Queues a pointer move to `(x, y)`, with no buttons held.
    pub fn mouse_move(&mut self, x: u16, y: u16) {
        self.push_event(Event::Mouse(MouseEvent::new(
            MouseEventKind::Moved,
            Pos::new(x, y),
            KeyModifiers::NONE,
        )));
    }

    /// Queues a key press of `code`, with no modifiers.
    pub fn key(&mut self, code: KeyCode) {
        self.key_with(code, KeyModifiers::NONE);
    }

    /// Queues a key press of `code` with `modifiers`.
    pub fn key_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.push_event(Event::Key(KeyEvent::new(code, modifiers)));
    }

    /// The underlying [`Interaction<Id>`], for anything not wrapped directly (focus queries,
    /// [`Interaction::hovered`](crate::interact::Interaction::hovered)).
    #[must_use]
    pub const fn interaction(&self) -> &Interaction<Id> {
        &self.interaction
    }

    /// The rendered view as of the last [`step`](Self::step) call (see
    /// [`Headless::format_view`]).
    #[must_use]
    pub fn view(&self) -> String {
        self.term.backend().format_view()
    }

    /// [`view`](Self::view), with [`Headless::SPACE_GLYPH`] converted back to a literal space.
    ///
    /// See `retroglyph_core::testing::TestHarness::readable_view` for why this exists: `view`
    /// renders a blank cell as `·` so layout is visible in text diffs, which defeats an ordinary
    /// plain-text assertion like `view.contains("Save")`.
    #[must_use]
    pub fn readable_view(&self) -> String {
        self.view().replace(Headless::SPACE_GLYPH, " ")
    }
}

impl<Id: Copy + PartialEq> WidgetHarness<Id> {
    /// Runs exactly one frame: drains every currently queued event into
    /// [`Interaction::handle_event`], then runs `f` inside
    /// [`Interaction::begin_frame`]/[`end_frame`](Interaction::end_frame) and presents.
    ///
    /// See "Frame timing" on [`WidgetHarness`] for why the whole queue drains at once rather than
    /// one event per call.
    pub fn step<R>(&mut self, f: impl FnOnce(&mut Ui<'_, '_, Id>) -> R) -> R {
        self.interaction.begin_frame();
        // Fed between `begin_frame` and `f`, not before `begin_frame`: `Interaction`'s own docs
        // call this "the realistic call pattern" (see `press_and_release_in_separate_frames...`
        // on `Interaction`), and it's load-bearing for Tab -- `FocusRing`'s registration order
        // only finalizes *inside* `begin_frame`, so a Tab fed before it would still cycle through
        // the previous frame's stale order.
        for event in self.queued.drain(..) {
            let _ = self.interaction.handle_event(&event);
        }
        let result = f(&mut Ui::new(
            &mut self.term.surface(),
            &mut self.interaction,
        ));
        self.interaction.end_frame();
        // `Headless::Error` is `Infallible`: absorbed here so callers never see a `Result` for a
        // `present` call that cannot fail, mirroring `TestHarness::step` (retroglyph#612).
        let Ok(()) = self.term.present();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::color::Style;
    use retroglyph_core::grid::Rect;

    use crate::{Response, Sense};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Id {
        Save,
        Cancel,
    }

    fn register(harness: &mut WidgetHarness<Id>, area: Rect) {
        let _ = harness.step(|ui| ui.interaction().interact(area, Id::Save, Sense::click()));
    }

    /// A widget's own interact call, factored out so a test can call it once to feed a queued
    /// gesture and once more to observe the frame after: pointer flags fed via `handle_event`
    /// resolve on the *next* `step`, not the one that fed them (see [`step`](WidgetHarness::step)'s
    /// doc comment).
    fn interact(harness: &mut WidgetHarness<Id>, area: Rect) -> Response<Id> {
        harness.step(|ui| ui.interaction().interact(area, Id::Save, Sense::click()))
    }

    #[test]
    fn click_resolves_one_step_after_it_was_queued() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let area = Rect::new(0, 0, 7, 1);

        register(&mut harness, area); // frame 1: nothing queued yet, nothing resolves

        harness.click(2, 0);
        let _ = interact(&mut harness, area); // frame 2: feeds the click, doesn't resolve it yet
        let response = interact(&mut harness, area); // frame 3: resolves it

        assert!(response.clicked());
    }

    #[test]
    fn click_button_queues_a_non_left_button() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let area = Rect::new(0, 0, 7, 1);
        register(&mut harness, area);

        harness.click_button(2, 0, MouseButton::Right);
        let _ = interact(&mut harness, area);
        let response = interact(&mut harness, area);

        // `Sense::click()` only resolves the left button: this proves `click_button` actually
        // queued a right-button gesture rather than silently falling back to `click`'s left one.
        assert!(!response.clicked());
    }

    #[test]
    fn mouse_move_updates_hover_without_a_click() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let area = Rect::new(0, 0, 7, 1);
        register(&mut harness, area);

        harness.mouse_move(2, 0);
        let _ = interact(&mut harness, area);
        let response = interact(&mut harness, area);

        assert!(response.hovered());
        assert!(!response.clicked());
    }

    #[test]
    fn key_feeds_a_press_with_no_modifiers() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let area = Rect::new(0, 0, 7, 1);
        register(&mut harness, area); // registers Save as focusable for the *next* frame

        harness.key(KeyCode::Tab);
        let response = harness.step(|ui| ui.interaction().interact(area, Id::Save, Sense::click()));

        assert!(response.focused());
    }

    #[test]
    fn step_presents_before_view_reflects_it() {
        let mut harness = WidgetHarness::<Id>::new(3, 1);
        assert!(harness.view().starts_with('·'));

        harness.step(|ui| {
            ui.surface().put((0, 0), '@', Style::default());
        });

        assert!(harness.view().starts_with('@'));
    }

    #[test]
    fn readable_view_converts_the_middle_dot_back_to_a_literal_space() {
        let mut harness = WidgetHarness::<Id>::new(12, 1);
        harness.step(|ui| {
            ui.surface().print((0, 0), "Save Game", Style::default());
        });

        assert!(!harness.view().contains("Save Game"));
        assert!(harness.readable_view().contains("Save Game"));
    }

    #[test]
    fn push_event_queues_an_arbitrary_event() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let area = Rect::new(0, 0, 7, 1);
        register(&mut harness, area);

        harness.push_event(Event::Mouse(MouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            Pos::new(2, 0),
            KeyModifiers::NONE,
        )));
        harness.push_event(Event::Mouse(MouseEvent::new(
            MouseEventKind::Up(MouseButton::Left),
            Pos::new(2, 0),
            KeyModifiers::NONE,
        )));
        let _ = interact(&mut harness, area);
        let response = interact(&mut harness, area);

        assert!(response.clicked());
    }

    #[test]
    fn interaction_exposes_the_underlying_context() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let area = Rect::new(0, 0, 7, 1);
        register(&mut harness, area);

        harness.mouse_move(2, 0);
        let _ = interact(&mut harness, area);
        let _ = interact(&mut harness, area);

        assert_eq!(harness.interaction().hovered(), Some(Id::Save));
    }

    #[test]
    fn distinct_ids_track_independently() {
        let mut harness = WidgetHarness::<Id>::new(10, 3);
        let save_area = Rect::new(0, 0, 4, 1);
        let cancel_area = Rect::new(5, 0, 4, 1);
        let frame = |harness: &mut WidgetHarness<Id>| {
            harness.step(|ui| {
                let save = ui
                    .interaction()
                    .interact(save_area, Id::Save, Sense::click());
                let cancel = ui
                    .interaction()
                    .interact(cancel_area, Id::Cancel, Sense::click());
                (save, cancel)
            })
        };
        let _ = frame(&mut harness);

        harness.click(6, 0);
        let _ = frame(&mut harness);
        let (save, cancel) = frame(&mut harness);

        assert!(!save.clicked());
        assert!(cancel.clicked());
    }
}
