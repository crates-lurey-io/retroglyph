//! Headless test harness driving an [`App`] with synthetic input.
//!
//! Also home to [`conformance`](crate::testing::conformance), the cross-backend harness that
//! tests a raw [`Backend`](crate::backend::Backend) facet against its own trait contract.
//!
//! [`TestHarness`] exists so every consumer stops rewriting the same loop by hand
//! (retroglyph#612): `Headless` gives a test a backend and [`Headless::push_event`]; everything
//! between that and an assertion used to be the consumer's own problem. Feature-gated
//! (`testing`), no effect on release builds. Not a UI-testing framework: no assertions, no
//! matchers, no fixtures, just the loop and the input synthesis that otherwise gets rewritten per
//! consumer. See ["Driving an `App` with `TestHarness`"](https://github.com/crates-lurey-io/retroglyph/blob/main/docs/testing.md#driving-an-app-with-testharness)
//! for the full workflow.
//!
//! [`conformance`](crate::testing::conformance) is a different tool for a different job: it drives a backend directly (no
//! `App`, no `Terminal`) through the obligations [`Output`](crate::backend::Output),
//! [`Cursor`](crate::backend::Cursor), and [`Input`](crate::backend::Input) each promise but
//! that a lone `impl` block never states, catching the five backends in this workspace (and any
//! future one) disagreeing on them (retroglyph#763).

pub mod conformance;

use crate::app::{App, Flow, Frame};
use crate::backend::Headless;
use crate::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crate::grid::Pos;
use crate::terminal::Terminal;
use alloc::collections::VecDeque;
use alloc::string::String;
use core::fmt;
use core::time::Duration;

/// Fixed per-frame delta [`TestHarness::step`] hands to [`App::update`].
///
/// Headless tests have no wall clock; this exists only so [`Frame::delta`]-driven code (tweens,
/// [`FrameClock`](crate::frame_clock::FrameClock)) advances instead of stalling.
pub const STEP_DELTA: Duration = Duration::from_millis(16);

/// Default step budget for [`TestHarness::run`].
pub const DEFAULT_MAX_STEPS: u32 = 64;

/// Drives an [`App`] against a [`Headless`] backend: queues synthetic input, steps frames, and
/// reads back the rendered view.
///
/// # The two-frame rule
///
/// A press and a release queued together resolve a frame later than the same gesture arriving
/// from real input, because hit-testing (e.g. `retroglyph-widgets`' `Interaction`) snapshots the
/// *previous* frame's pointer state before this frame's queued events are applied. [`click`](
/// Self::click) queues both events for you, but resolving them still costs two frames: call
/// [`run`](Self::run) or [`settle`](Self::settle) after queuing input, not a single
/// [`step`](Self::step).
///
/// # Presenting
///
/// [`step`](Self::step) presents automatically: skipped on [`Flow::Idle`], skipped as a no-op if
/// `update` already presented (mirroring [`run_blocking`](crate::app::run_blocking)'s own
/// behavior). Nothing queued is visible in [`view`](Self::view) until a `step` call has run.
///
/// # Examples
///
/// ```
/// use retroglyph_core::testing::TestHarness;
/// use retroglyph_core::{App, Backend, Flow, Frame, Style, Terminal};
///
/// struct Counter(u32);
///
/// impl<B: Backend> App<B> for Counter {
///     fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
///         if term.has_input() {
///             self.0 += 1;
///         }
///         term.surface()
///             .put((0, 0), char::from_digit(self.0, 10).unwrap_or('?'), Style::default());
///         if frame.frame > 10 {
///             Flow::Exit
///         } else {
///             Flow::Continue
///         }
///     }
/// }
///
/// let mut harness = TestHarness::new(10, 1);
/// let mut app = Counter(0);
///
/// harness.key(retroglyph_core::event::KeyCode::Char(' '));
/// harness.run(&mut app);
///
/// assert_eq!(app.0, 1);
/// assert!(harness.view().starts_with('1'));
/// ```
pub struct TestHarness {
    term: Terminal<Headless>,
    frame: u64,
    queued: VecDeque<Event>,
}

impl TestHarness {
    /// Creates a harness with a `width` x `height` [`Headless`] backend.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            term: Terminal::new(Headless::new(width, height)),
            frame: 0,
            queued: VecDeque::new(),
        }
    }

    /// Queues a synthetic event for the next [`step`](Self::step) call.
    ///
    /// Only queues: the app does not see it until a frame runs. Prefer the typed helpers
    /// ([`click`](Self::click), [`key`](Self::key), [`mouse_move`](Self::mouse_move)) unless the
    /// [`Event`] variant needed isn't one of them.
    pub fn push_event(&mut self, event: Event) {
        self.queued.push_back(event);
    }

    /// Queues a left-button click (press then release) at `(x, y)`, with no modifiers.
    ///
    /// See "the two-frame rule" on [`TestHarness`] before asserting after a single
    /// [`step`](Self::step): use [`run`](Self::run)/[`settle`](Self::settle) instead.
    pub fn click(&mut self, x: u16, y: u16) {
        self.click_button(x, y, MouseButton::Left);
    }

    /// Queues a click (press then release) with `button` at `(x, y)`, with no modifiers.
    pub fn click_button(&mut self, x: u16, y: u16, button: MouseButton) {
        let position = Pos::new(x, y);
        for kind in [MouseEventKind::Down(button), MouseEventKind::Up(button)] {
            self.push_event(Event::Mouse(MouseEvent {
                kind,
                position,
                // `Headless` is a character-mode backend: it has no sub-cell pixel position to
                // report, matching the crossterm backend's own convention (see `MouseEvent`'s
                // docs) rather than guessing one.
                pixel_position: None,
                modifiers: KeyModifiers::NONE,
            }));
        }
    }

    /// Queues a pointer move to `(x, y)`, with no buttons held.
    pub fn mouse_move(&mut self, x: u16, y: u16) {
        self.push_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(x, y),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Queues a key press of `code`, with no modifiers.
    pub fn key(&mut self, code: KeyCode) {
        self.key_with(code, KeyModifiers::NONE);
    }

    /// Queues a key press of `code` with `modifiers`.
    pub fn key_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.push_event(Event::Key(KeyEvent::new(code, modifiers)));
    }

    /// Resizes the backend and queues the matching [`Event::Resize`] a real terminal would also
    /// deliver.
    ///
    /// Unlike calling <code>[term_mut](Self::term_mut)().[resize](Terminal::resize)</code>
    /// directly, this also queues the event, matching what a real backend delivers alongside its
    /// own resize.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.term.resize(width, height);
        self.push_event(Event::Resize(width, height));
    }

    /// Runs exactly one frame: pops at most one queued event into the backend, calls
    /// [`App::update`], and presents unless `update` returned [`Flow::Idle`] or already presented.
    ///
    /// Draining only one queued event per call, rather than the whole queue at once, is what
    /// reproduces the two-frame rule described on [`TestHarness`] instead of masking it.
    pub fn step<A: App<Headless>>(&mut self, app: &mut A) -> Flow {
        if let Some(event) = self.queued.pop_front() {
            self.term.backend_mut().push_event(event);
        }
        let frame = Frame {
            delta: STEP_DELTA,
            frame: self.frame,
        };
        self.frame = self.frame.wrapping_add(1);
        let present_count_before = self.term.present_count();
        let flow = app.update(&mut self.term, &frame);
        if flow != Flow::Idle && self.term.present_count() == present_count_before {
            // `Headless::Error` is `Infallible`: absorbed here so callers never see a `Result`
            // for a `present` call that cannot fail, rather than each one writing its own
            // "never panics in practice" doc (retroglyph#612).
            let Ok(()) = self.term.present();
        }
        flow
    }

    /// Runs [`step`](Self::step) until the event queue is empty (with at least one frame run),
    /// stopping early on [`Flow::Exit`], bounded by `max_steps`.
    ///
    /// This is the "run until settled" primitive: queuing input only stages it, `settle` resolves
    /// the two-frame rule (see [`TestHarness`]) instead of requiring two manual `step` calls per
    /// gesture.
    ///
    /// # Errors
    ///
    /// Returns [`RunError::ExceededMaxSteps`] if the queue is still non-empty after `max_steps`
    /// steps: an app that never drains its input is a bug in the test or the app, not a case to
    /// loop on forever.
    pub fn settle<A: App<Headless>>(
        &mut self,
        app: &mut A,
        max_steps: u32,
    ) -> Result<u32, RunError> {
        let mut steps = 0;
        loop {
            let flow = self.step(app);
            steps += 1;
            if flow == Flow::Exit || self.queued.is_empty() {
                return Ok(steps);
            }
            if steps >= max_steps {
                return Err(RunError::ExceededMaxSteps { max_steps });
            }
        }
    }

    /// [`settle`](Self::settle) with [`DEFAULT_MAX_STEPS`], panicking instead of returning an
    /// error.
    ///
    /// # Panics
    ///
    /// Panics if the queue is still non-empty after [`DEFAULT_MAX_STEPS`] steps.
    pub fn run<A: App<Headless>>(&mut self, app: &mut A) -> u32 {
        match self.settle(app, DEFAULT_MAX_STEPS) {
            Ok(steps) => steps,
            Err(err) => panic!("{err}"),
        }
    }

    /// Runs a fixed number of frames, regardless of queue state or the [`Flow`] each one returns.
    ///
    /// For tests asserting on the app still running after N frames (e.g. an idle animation)
    /// rather than on input settling; [`run`](Self::run)/[`settle`](Self::settle) cover the
    /// input-resolution case.
    pub fn run_steps<A: App<Headless>>(&mut self, app: &mut A, steps: u32) {
        for _ in 0..steps {
            self.step(app);
        }
    }

    /// The rendered view as of the last [`step`](Self::step) call (see
    /// [`Headless::format_view`]).
    #[must_use]
    pub fn view(&self) -> String {
        self.term.backend().format_view()
    }

    /// The underlying [`Terminal`], for anything not wrapped directly (cursor position,
    /// [`Terminal::grid`], a manual [`Terminal::draw`] outside the `App` loop).
    #[must_use]
    pub const fn term(&self) -> &Terminal<Headless> {
        &self.term
    }

    /// The underlying [`Terminal`], mutably.
    #[must_use]
    pub const fn term_mut(&mut self) -> &mut Terminal<Headless> {
        &mut self.term
    }
}

/// Error returned by [`TestHarness::settle`] when the queue never drained within the step budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunError {
    /// The queue still had pending events after `max_steps` [`TestHarness::step`] calls.
    ExceededMaxSteps {
        /// The budget that was exceeded.
        max_steps: u32,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExceededMaxSteps { max_steps } => write!(
                f,
                "TestHarness::settle did not drain its event queue within {max_steps} steps"
            ),
        }
    }
}

impl core::error::Error for RunError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Flow;
    use crate::backend::Backend;
    use crate::color::Style;
    use ixy::HasSize;

    struct Clicker {
        clicks: u32,
    }

    impl<B: Backend> App<B> for Clicker {
        fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
            for event in term.drain_events() {
                if matches!(
                    event,
                    Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        ..
                    })
                ) {
                    self.clicks += 1;
                }
            }
            term.surface().put((0, 0), 'x', Style::default());
            Flow::Continue
        }
    }

    #[test]
    fn step_presents_before_view_reflects_it() {
        struct Drawer;
        impl<B: Backend> App<B> for Drawer {
            fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                term.surface().put((0, 0), '@', Style::default());
                Flow::Continue
            }
        }

        let mut harness = TestHarness::new(3, 1);
        let mut app = Drawer;
        assert!(harness.view().starts_with('·'));
        harness.step(&mut app);
        assert!(harness.view().starts_with('@'));
    }

    #[test]
    fn click_resolves_after_settle_not_after_one_step() {
        let mut harness = TestHarness::new(5, 1);
        let mut app = Clicker { clicks: 0 };
        harness.click(0, 0);

        // A single step only delivers one of the two queued events (see the two-frame rule).
        harness.step(&mut app);
        assert_eq!(
            app.clicks, 1,
            "the queued Down event resolves on the first step"
        );

        harness.run(&mut app);
        assert!(harness.view().starts_with('x'));
    }

    #[test]
    fn settle_reports_exceeded_max_steps() {
        struct NeverDrains;
        impl<B: Backend> App<B> for NeverDrains {
            fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                Flow::Continue
            }
        }

        let mut harness = TestHarness::new(2, 1);
        let mut app = NeverDrains;
        harness.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        harness.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::NONE,
        )));

        // `settle`'s loop bound is the harness's own queue length, independent of whether `app`
        // drains anything from `term`; a 0-step budget with a non-empty queue always exceeds it
        // after the first (mandatory) step, so this is deterministic regardless of app behavior.
        let err = harness.settle(&mut app, 0).unwrap_err();
        assert_eq!(err, RunError::ExceededMaxSteps { max_steps: 0 });
    }

    #[test]
    fn run_steps_ignores_flow_exit() {
        struct AlwaysExits {
            calls: u32,
        }
        impl<B: Backend> App<B> for AlwaysExits {
            fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                self.calls += 1;
                Flow::Exit
            }
        }

        let mut harness = TestHarness::new(2, 1);
        let mut app = AlwaysExits { calls: 0 };
        harness.run_steps(&mut app, 3);
        assert_eq!(app.calls, 3);
    }

    #[test]
    fn resize_updates_backend_and_queues_event() {
        struct Resized {
            seen: Option<(u16, u16)>,
        }
        impl<B: Backend> App<B> for Resized {
            fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                for event in term.drain_events() {
                    if let Event::Resize(w, h) = event {
                        self.seen = Some((w, h));
                    }
                }
                Flow::Continue
            }
        }

        let mut harness = TestHarness::new(4, 4);
        let mut app = Resized { seen: None };
        harness.resize(8, 2);
        harness.run(&mut app);
        assert_eq!(harness.term().size().width(), 8);
        assert_eq!(app.seen, Some((8, 2)));
    }

    #[test]
    fn run_error_display_message() {
        let err = RunError::ExceededMaxSteps { max_steps: 5 };
        assert_eq!(
            err.to_string(),
            "TestHarness::settle did not drain its event queue within 5 steps"
        );
    }
}
