//! Headless test harness driving an [`App`](crate::app::App) with synthetic input.
//!
//! Also home to [`conformance`](crate::testing::conformance), the cross-backend harness that
//! tests a raw [`Backend`](crate::backend::Backend) facet against its own trait contract.
//!
//! [`TestHarness`](crate::testing::TestHarness) owns the drive-until-settled loop that a test
//! would otherwise hand-roll around `Headless` (retroglyph#612): `Headless` supplies the backend
//! and [`Headless::push_event`](crate::backend::Headless::push_event), and the harness supplies
//! everything between that and the assertion. Feature-gated
//! (`testing`), no effect on release builds. Not a UI-testing framework: no assertions, no
//! matchers, no fixtures, just the loop and the input synthesis that otherwise gets rewritten per
//! consumer: push synthetic events onto `Headless` with `push_event`, drive them through an `App`
//! with `TestHarness` until the loop settles, then assert on the resulting `Headless` frame
//! directly.
//!
//! [`conformance`](crate::testing::conformance) is a different tool for a different job: it drives a backend directly (no
//! `App`, no `Terminal`) through the obligations [`Output`](crate::backend::Output),
//! [`Cursor`](crate::backend::Cursor), and [`Input`](crate::backend::Input) each promise but
//! that a lone `impl` block never states, catching the five backends in this workspace (and any
//! future one) disagreeing on them (retroglyph#763).

pub mod conformance;
mod recording;

pub use recording::InputRecording;

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

/// Fixed per-frame delta [`TestHarness::step`](crate::testing::TestHarness::step) hands to [`App::update`](crate::app::App::update).
///
/// Headless tests have no wall clock; this exists only so [`Frame::delta`](crate::app::Frame::delta)-driven code (tweens,
/// [`FrameClock`](crate::frames::FrameClock)) advances instead of stalling. 16ms is one frame at
/// ~60fps; the value is otherwise arbitrary, but it is load-bearing for any test that counts steps
/// to reach an animation state: a duration-D animation finishes in `ceil(D / 16ms)` steps, so
/// changing this shifts those step counts.
pub const STEP_DELTA: Duration = Duration::from_millis(16);

/// Default step budget for [`TestHarness::run`](crate::testing::TestHarness::run) before it treats a
/// non-draining event queue as a stuck app and panics.
///
/// Sized to comfortably clear any single queued gesture (a click is two events plus the trailing
/// event-free frame the two-frame rule costs), with headroom, while still failing fast on an app
/// that never drains its input. The exact value is picked by feel, not measured; a test with a
/// legitimately long settle should call [`settle`](crate::testing::TestHarness::settle) with a
/// larger budget rather than raise this shared default.
pub const DEFAULT_MAX_STEPS: u32 = 64;

/// Drives an [`App`](crate::app::App) against a [`Headless`](crate::backend::Headless) backend: queues synthetic input, steps frames, and
/// reads back the rendered view.
///
/// # The two-frame rule
///
/// A press and a release queued together resolve a frame later than the same gesture arriving
/// from real input, because hit-testing (e.g. `retroglyph-ui`' `Interaction`) snapshots the
/// *previous* frame's pointer state before this frame's queued events are applied, and because
/// [`step`](Self::step) drains at most one queued event per call, `click`'s Down and Up land in
/// two separate frames rather than one, costing a third, event-free frame before that snapshot
/// catches up: `resolved_press` from Down's frame latches `active` on Up's frame, and only the
/// frame after *that* sees `resolved_release`. [`click`](Self::click) queues both events for you;
/// [`run`](Self::run) and [`settle`](Self::settle) already run that trailing event-free frame, so
/// call one of those after queuing input rather than a fixed number of manual
/// [`step`](Self::step) calls.
///
/// # Presenting
///
/// [`step`](Self::step) presents automatically: skipped on [`Flow::Idle`](crate::app::Flow::Idle), skipped as a no-op if
/// `update` already presented (mirroring [`run_on`](crate::app::run_on)'s own
/// behavior). Nothing queued is visible in [`view`](Self::view) until a `step` call has run.
///
/// # Examples
///
/// ```
/// use retroglyph_core::testing::TestHarness;
/// use retroglyph_core::app::{App, Flow, Frame};
/// use retroglyph_core::backend::Backend;
/// use retroglyph_core::color::Style;
/// use retroglyph_core::terminal::Terminal;
///
/// struct Counter(u32);
///
/// impl<B: Backend> App<B> for Counter {
///     fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
///         // Draining (not just `has_input`, which leaves the event buffered) matters here:
///         // `run` follows the queue's last event with a trailing event-free frame (see "the
///         // two-frame rule" above), and an undrained event would still be visible to
///         // `has_input` on that next frame too, double-counting it.
///         if term.drain_events().count() > 0 {
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
    step_delta: Duration,
}

impl TestHarness {
    /// Creates a harness with a `width` x `height` [`Headless`](crate::backend::Headless) backend.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            term: Terminal::new(Headless::new(width, height)),
            frame: 0,
            queued: VecDeque::new(),
            step_delta: STEP_DELTA,
        }
    }

    /// Overrides the [`Frame::delta`](crate::app::Frame::delta) [`step`](Self::step) hands to
    /// [`App::update`](crate::app::App::update), replacing [`STEP_DELTA`] (builder style).
    ///
    /// For an app under test whose animation timing is calibrated against a different fixed
    /// delta than [`STEP_DELTA`]'s 16ms (a `retroglyph_ui::Tween`/[`FrameClock`](crate::frames::FrameClock)
    /// tuned in real seconds, for example): matching that calibration here keeps however many
    /// `step` calls an animation takes to settle the same as what tuned it, rather than retuning
    /// the animation (or the test's assertions) around the harness's own default instead.
    #[must_use]
    pub const fn with_step_delta(mut self, delta: Duration) -> Self {
        self.step_delta = delta;
        self
    }

    /// Queues a synthetic event for the next [`step`](Self::step) call.
    ///
    /// Only queues: the app does not see it until a frame runs. Prefer the typed helpers
    /// ([`click`](Self::click), [`key`](Self::key), [`mouse_move`](Self::mouse_move)) unless the
    /// [`Event`](crate::event::Event) variant needed isn't one of them.
    pub fn push_event(&mut self, event: Event) {
        self.queued.push_back(event);
    }

    /// Queues a left-button click (press then release) at `(x, y)`, with no modifiers.
    ///
    /// See "the two-frame rule" on [`TestHarness`](crate::testing::TestHarness) before asserting after a single
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

    /// Resizes the backend and queues the matching [`Event::Resize`](crate::event::Event::Resize) a real terminal would also
    /// deliver.
    ///
    /// Unlike calling <code>[term_mut](Self::term_mut)().[resize](crate::terminal::Terminal::resize)</code>
    /// directly, this also queues the event, matching what a real backend delivers alongside its
    /// own resize.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.term.resize(width, height);
        self.push_event(Event::Resize(width, height));
    }

    /// Runs exactly one frame: pops at most one queued event into the backend, calls
    /// [`App::update`](crate::app::App::update), and presents unless `update` returned [`Flow::Idle`](crate::app::Flow::Idle) or already presented.
    ///
    /// Draining only one queued event per call, rather than the whole queue at once, is what
    /// reproduces the two-frame rule described on [`TestHarness`](crate::testing::TestHarness) instead of masking it.
    pub fn step<A: App<Headless>>(&mut self, app: &mut A) -> Flow {
        if let Some(event) = self.queued.pop_front() {
            self.term.backend_mut().push_event(event);
        }
        let frame = Frame {
            delta: self.step_delta,
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

    /// Runs [`step`](Self::step) until the event queue is empty *and* one further event-free
    /// frame has run past that (with at least one frame run total), stopping early on
    /// [`Flow::Exit`](crate::app::Flow::Exit), bounded by `max_steps`.
    ///
    /// This is the "run until settled" primitive: queuing input only stages it, `settle` resolves
    /// the two-frame rule (see [`TestHarness`](crate::testing::TestHarness)) instead of requiring
    /// manual `step` calls per gesture. The trailing event-free frame matters because hit-testing
    /// (e.g. `retroglyph-ui`'s `Interaction`) reads a one-shot flag latched by the *previous*
    /// frame's input: stopping the instant the queue empties (right after the last queued event's
    /// own frame) would return before that flag is ever observed, leaving a queued click's
    /// `clicked()` structurally unreachable through this API.
    ///
    /// # Errors
    ///
    /// Returns [`RunError::ExceededMaxSteps`](crate::testing::RunError::ExceededMaxSteps) if the queue is still non-empty after `max_steps`
    /// steps: an app that never drains its input is a bug in the test or the app, not a case to
    /// loop on forever.
    pub fn settle<A: App<Headless>>(
        &mut self,
        app: &mut A,
        max_steps: u32,
    ) -> Result<u32, RunError> {
        let mut steps = 0;
        let mut ran_trailing_frame = false;
        loop {
            let flow = self.step(app);
            steps += 1;
            if flow == Flow::Exit {
                return Ok(steps);
            }
            if self.queued.is_empty() {
                // The queue draining doesn't mean the app has resolved everything it saw: one
                // more event-free frame is what lets a one-shot flag latched by the last queued
                // event's own frame (e.g. `resolved_release`) finally be observed. Run exactly one
                // before stopping, rather than stopping the instant the queue empties.
                if ran_trailing_frame {
                    return Ok(steps);
                }
                ran_trailing_frame = true;
            } else {
                ran_trailing_frame = false;
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

    /// Runs a fixed number of frames, regardless of queue state or the [`Flow`](crate::app::Flow) each one returns.
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
    /// [`Headless::format_view`](crate::backend::Headless::format_view)).
    #[must_use]
    pub fn view(&self) -> String {
        self.term.backend().format_view()
    }

    /// [`view`](Self::view), with [`Headless::SPACE_GLYPH`] converted back to a literal space.
    ///
    /// `view` renders a blank cell as `·` rather than a space so layout is visible in text
    /// diffs (see [`Headless::format_view`](crate::backend::Headless::format_view)); that's the
    /// right default for snapshot tests, but it defeats an ordinary plain-text assertion like
    /// `view.contains("New Game")`, since the label's actual space never appears literally. Use
    /// this instead of `view` for that case.
    #[must_use]
    pub fn readable_view(&self) -> String {
        self.view().replace(Headless::SPACE_GLYPH, " ")
    }

    /// The [`Pos`] of `needle`'s first occurrence in the current [`view`](Self::view), or
    /// `None` if it doesn't appear.
    ///
    /// "First" is row-major: top row before bottom, left before right within a row. `needle` is
    /// matched against a single row at a time (a label wrapped across rows by [`Headless::draw_text`](crate::backend::Headless)
    /// won't match) and must consist of single-cell glyphs (a wide glyph's spacer cell, per
    /// [`Headless::format_view`](crate::backend::Headless::format_view), renders blank, not as a second copy of the glyph, so a
    /// `needle` containing one can never match).
    ///
    /// A space in `needle` is matched against [`format_view`](crate::backend::Headless::format_view)'s own `·`
    /// stand-in for a blank cell, so a multi-word `needle` (`"Save Game"`) matches the view's
    /// rendered `"Save·Game"` without the caller having to know about that substitution.
    #[must_use]
    pub fn find_text(&self, needle: &str) -> Option<Pos> {
        if needle.is_empty() {
            return None;
        }
        // `format_view` stands `·` in for a plain space so layout is visible in text diffs
        // (`Headless::display_glyph`); matching against that convention here is what lets a
        // caller pass a natural, space-separated label instead of pre-encoding it themselves.
        let needle: String = needle
            .chars()
            .map(|c| if c == ' ' { Headless::SPACE_GLYPH } else { c })
            .collect();
        for (y, row) in self.view().lines().enumerate() {
            if let Some(byte_index) = row.find(needle.as_str()) {
                // `str::find` returns a byte offset; the harness's coordinates are cell (char)
                // indices, and every glyph `format_view` emits is one `char`, so counting chars
                // up to that byte offset converts one to the other.
                let x = row[..byte_index].chars().count();
                // Both indices are bounded by the backend's own grid size, which is a `u16` x
                // `u16` extent (`Headless::new`'s own parameters): `y` never exceeds the number
                // of rows `format_view` emits, and `x` never exceeds one row's cell count.
                #[allow(clippy::cast_possible_truncation)]
                return Some(Pos::new(x as u16, y as u16));
            }
        }
        None
    }

    /// [`click`](Self::click)s the first occurrence of `needle` in the current [`view`](Self::view).
    ///
    /// See [`find_text`](Self::find_text) for what "first occurrence" means and its matching
    /// rules (row-major, single-row, `·`-for-space).
    ///
    /// # Errors
    ///
    /// Returns [`ClickTextError::NotFound`] if `needle` doesn't appear in the current view.
    pub fn click_text(&mut self, needle: &str) -> Result<(), ClickTextError> {
        let pos = self
            .find_text(needle)
            .ok_or_else(|| ClickTextError::NotFound {
                needle: needle.into(),
            })?;
        self.click(pos.x, pos.y);
        Ok(())
    }

    /// Creates a harness sized to `recording`'s recorded backend dimensions, ready for
    /// [`replay`](Self::replay).
    #[must_use]
    pub fn from_recording(recording: &InputRecording) -> Self {
        Self::new(recording.width(), recording.height())
    }

    /// Drives every event in `recording` into `app`, reproducing each recorded gap as whole
    /// [`STEP_DELTA`] steps rather than collapsing it to zero.
    ///
    /// Each recorded `(delay, event)` pair becomes `round(delay / STEP_DELTA)` event-free
    /// [`run_steps`](Self::run_steps) calls, then `event` is queued and settled with
    /// [`run`](Self::run). This is what lets a replay reproduce a bug that depends on elapsed
    /// time between inputs (a [`FrameClock`](crate::frames::FrameClock)/tween-driven cooldown, a
    /// hold-to-charge mechanic): a naive replay that ran one `run()` per event with no delay in
    /// between would silently fail to reproduce any of those. Sub-`STEP_DELTA` jitter in the
    /// original recording is rounded away, which is expected and harmless -- nothing in this
    /// harness has finer time resolution than one step to begin with.
    ///
    /// # Panics
    ///
    /// Panics if [`run`](Self::run) does (an event fails to drain within
    /// [`DEFAULT_MAX_STEPS`]); see that method.
    pub fn replay<A: App<Headless>>(&mut self, recording: &InputRecording, app: &mut A) {
        for (delay, event) in recording.events() {
            self.run_steps(app, Self::steps_for_delay(*delay));
            self.push_event(event.clone());
            self.run(app);
        }
    }

    /// Rounds `delay` to the nearest whole number of [`STEP_DELTA`] steps.
    ///
    /// No zero-`step_nanos` guard: [`STEP_DELTA`] is a fixed, non-zero constant (16ms), not a
    /// runtime value, so that branch would be dead code no test could ever exercise.
    fn steps_for_delay(delay: Duration) -> u32 {
        let step_nanos = STEP_DELTA.as_nanos();
        let steps = (delay.as_nanos() + step_nanos / 2) / step_nanos;
        u32::try_from(steps).unwrap_or(u32::MAX)
    }

    /// The underlying [`Terminal`](crate::terminal::Terminal), for anything not wrapped directly (cursor position,
    /// [`Terminal::grid`](crate::terminal::Terminal::grid), a manual [`Terminal::draw`](crate::terminal::Terminal::draw) outside the `App` loop).
    #[must_use]
    pub const fn term(&self) -> &Terminal<Headless> {
        &self.term
    }

    /// The underlying [`Terminal`](crate::terminal::Terminal), mutably.
    #[must_use]
    pub const fn term_mut(&mut self) -> &mut Terminal<Headless> {
        &mut self.term
    }
}

/// Error returned by [`TestHarness::settle`](crate::testing::TestHarness::settle) when the queue never drained within the step budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunError {
    /// The queue still had pending events after `max_steps` [`TestHarness::step`](crate::testing::TestHarness::step) calls.
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

/// Error returned by [`TestHarness::click_text`](crate::testing::TestHarness::click_text) when `needle` doesn't appear in the
/// current [`view`](crate::testing::TestHarness::view).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClickTextError {
    /// `needle` did not appear anywhere in [`TestHarness::view`](crate::testing::TestHarness::view) at the time of the call.
    NotFound {
        /// The text that was searched for.
        needle: String,
    },
}

impl fmt::Display for ClickTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { needle } => write!(f, "{needle:?} not found in view"),
        }
    }
}

impl core::error::Error for ClickTextError {}

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
    fn with_step_delta_overrides_the_delta_step_hands_to_update() {
        struct RecordsDelta {
            seen: Option<Duration>,
        }
        impl<B: Backend> App<B> for RecordsDelta {
            fn update(&mut self, _term: &mut Terminal<B>, frame: &Frame) -> Flow {
                self.seen = Some(frame.delta);
                Flow::Continue
            }
        }

        let mut harness = TestHarness::new(2, 1).with_step_delta(Duration::from_millis(100));
        let mut app = RecordsDelta { seen: None };
        harness.step(&mut app);
        assert_eq!(app.seen, Some(Duration::from_millis(100)));
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

    /// Draws two fixed labels, and (like [`Clicker`]) counts left-button-down events, so
    /// [`click_text`](TestHarness::click_text) tests can assert it actually clicked, not merely
    /// that it located the right cell.
    struct Labels {
        clicks: u32,
    }

    impl<B: Backend> App<B> for Labels {
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
            term.surface().print((2, 1), "Quit", Style::default());
            term.surface().print((2, 2), "Save Game", Style::default());
            Flow::Continue
        }
    }

    #[test]
    fn find_text_locates_first_occurrence_row_major() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        assert_eq!(harness.find_text("Quit"), Some(Pos::new(2, 1)));
    }

    #[test]
    fn find_text_matches_a_space_against_the_view_middle_dot() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        // `"Save Game"` never appears literally: `format_view` renders the space between the
        // words as `·`. This only passes if `find_text` accounts for that substitution itself.
        assert_eq!(harness.find_text("Save Game"), Some(Pos::new(2, 2)));
    }

    #[test]
    fn readable_view_converts_the_middle_dot_back_to_a_literal_space() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        // `view()` renders the space in "Save Game" as `·`, so a naive `.contains("Save Game")`
        // against it would fail even though the label is on screen; `readable_view` is what lets
        // that assertion pass without the caller hand-rolling the substitution themselves.
        assert!(!harness.view().contains("Save Game"));
        assert!(harness.readable_view().contains("Save Game"));
    }

    #[test]
    fn find_text_returns_none_when_absent() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        assert_eq!(harness.find_text("Cancel"), None);
    }

    #[test]
    fn find_text_returns_none_for_an_empty_needle() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        // An empty needle matches every row trivially (`str::find("")` returns `Some(0)`), which
        // would make `find_text("")` report a match at `(0, 0)` regardless of the view's actual
        // content; treating it as absent instead avoids that meaningless result.
        assert_eq!(harness.find_text(""), None);
    }

    #[test]
    fn click_text_clicks_the_located_cell() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        harness.click_text("Quit").unwrap();
        harness.run(&mut app);

        assert_eq!(app.clicks, 1);
    }

    #[test]
    fn click_text_errs_when_absent() {
        let mut harness = TestHarness::new(12, 4);
        let mut app = Labels { clicks: 0 };
        harness.step(&mut app);

        let err = harness.click_text("Cancel").unwrap_err();
        assert_eq!(err.to_string(), "\"Cancel\" not found in view");
    }

    /// Registers a key press only if a cooldown started by the previous accepted press has fully
    /// elapsed (100ms), tracked via [`Frame::delta`] rather than a step count -- exactly the
    /// class of timer-driven state [`TestHarness::replay`]'s faithful-timing design exists to
    /// reproduce correctly.
    struct CooldownGate {
        cooldown_remaining: Duration,
        hits: u32,
    }

    impl CooldownGate {
        const COOLDOWN: Duration = Duration::from_millis(100);

        const fn new() -> Self {
            Self {
                cooldown_remaining: Duration::ZERO,
                hits: 0,
            }
        }
    }

    impl App<Headless> for CooldownGate {
        fn update(&mut self, term: &mut Terminal<Headless>, frame: &Frame) -> Flow {
            self.cooldown_remaining = self.cooldown_remaining.saturating_sub(frame.delta);
            for event in term.drain_events() {
                if matches!(event, Event::Key(_)) && self.cooldown_remaining.is_zero() {
                    self.hits += 1;
                    self.cooldown_remaining = Self::COOLDOWN;
                }
            }
            Flow::Continue
        }
    }

    fn key_press(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    /// Proves `replay`'s timing is faithful, not coarse: the same two key presses, 200ms apart
    /// in the recording, register as two hits when the recorded gap is honored (the cooldown
    /// from the first has expired by the time the second arrives) but collapse to one hit when
    /// replayed back-to-back with the delay discarded (a naive "one `run()` per event" replay
    /// would get this wrong).
    #[test]
    fn replay_reproduces_recorded_timing_not_a_coarse_back_to_back_replay() {
        let mut recording = InputRecording::new(4, 1);
        recording.push(Duration::ZERO, key_press('a'));
        recording.push(Duration::from_millis(200), key_press('b'));

        let mut faithful = TestHarness::from_recording(&recording);
        let mut faithful_app = CooldownGate::new();
        faithful.replay(&recording, &mut faithful_app);
        assert_eq!(
            faithful_app.hits, 2,
            "the recorded 200ms gap should let the cooldown expire before the second press"
        );

        let mut coarse = TestHarness::new(recording.width(), recording.height());
        let mut coarse_app = CooldownGate::new();
        for (_, event) in recording.events() {
            coarse.push_event(event.clone());
            coarse.run(&mut coarse_app);
        }
        assert_eq!(
            coarse_app.hits, 1,
            "discarding the recorded delay should leave the cooldown still active for the second press"
        );
    }

    #[test]
    fn from_recording_sizes_the_harness_to_the_recording() {
        let recording = InputRecording::new(7, 3);
        let harness = TestHarness::from_recording(&recording);
        assert_eq!(harness.term().size().width(), 7);
        assert_eq!(harness.term().size().height(), 3);
    }
}
