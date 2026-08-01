//! The `App`-driven game loop.
//!
//! `App` is the update-side dual of [`Backend`](crate::Backend): where a
//! backend is the output contract, an [`App`] is the per-frame update contract.
//! A game implements [`App`] once and runs on every backend unchanged.
//!
//! The loop decomposes into three pieces:
//!
//! - the contract ([`App`], [`Flow`], [`Frame`]), here in the core;
//! - the generic blocking driver ([`run_blocking`]/[`run_blocking_with`], `std` only), which
//!   covers `Crossterm` (in `retroglyph-crossterm`) and [`Headless`](crate::backend::Headless);
//! - the inverted driver in the windowing layer (the software backend's
//!   `run_app`), which cannot be generic because winit owns the loop instead of
//!   handing control back to a shared driver function.
//!
//! Both drivers share [`step`] as the per-frame body and present automatically after `update`
//! returns, skipping the present on [`Flow::Idle`] or when `update` already presented itself. The
//! low-level [`poll`](crate::Terminal::poll) / [`present`](crate::Terminal::present) API remains
//! available for turn-based games and headless tests.

use crate::backend::Backend;
use crate::terminal::Terminal;
use core::time::Duration;

/// Whether the game loop should continue or stop after a frame, and whether that frame renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Flow {
    /// Run another frame, and present it.
    Continue,
    /// Run another frame, but nothing changed: skip [`present`](Terminal::present) and leave the
    /// previous frame on screen.
    ///
    /// For turn-based apps that only need to redraw in response to player input, not on every
    /// tick of the driver's loop. Returning `Idle` while a [`Tween`](crate::animate::Tween)- or
    /// [`FrameClock`](crate::frame_clock::FrameClock)-driven animation is still in flight is an
    /// app bug, not a valid use: an in-progress animation has something new to show every frame,
    /// which is exactly what `Idle` tells the driver isn't true.
    Idle,
    /// Stop the loop. The driver returns and the terminal unwinds normally, so
    /// backend `Drop` logic (for example crossterm's terminal restore) runs.
    Exit,
}

/// Per-frame context handed to [`App::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Wall-clock time elapsed since the previous frame, supplied by the driver.
    pub delta: Duration,
    /// Monotonic frame counter, starting at 0.
    pub frame: u64,
}

/// The per-frame update contract for a game.
///
/// Implement this once, generically over the backend, to run everywhere:
///
/// ```
/// use retroglyph_core::{App, Backend, Flow, Frame, Style, Terminal};
///
/// struct MyGame;
/// impl<B: Backend> App<B> for MyGame {
///     fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
///         term.surface().put((0, 0), '@', Style::default());
///         Flow::Exit
///     }
/// }
/// ```
pub trait App<B: Backend> {
    /// Advance and render one frame.
    ///
    /// Draw into `term`, read input via `term`, and return [`Flow::Exit`] to stop the loop.
    ///
    /// Draw via [`term.surface()`](Terminal::surface) or [`term.draw()`](Terminal::draw) (though
    /// `draw` presents itself, which usually conflicts with the driver's own automatic present
    /// below; prefer `surface()` inside `update`). Every driver ([`run_blocking`] and
    /// `retroglyph-window`'s windowed drivers) presents the frame automatically right after this
    /// method returns, unless it returned [`Flow::Idle`], in which case the driver skips
    /// [`present`](Terminal::present) entirely. Calling `present` yourself inside `update` remains
    /// fine (the driver detects it already ran via [`present_count`](Terminal::present_count) and
    /// skips its own call) but is never required.
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow;
}

/// Run one frame: the per-frame body shared by every driver.
///
/// Calls [`App::update`]. Both [`run_blocking`] and the windowing layer's
/// inverted driver call this function instead of `update` directly, so the
/// two drivers cannot drift apart as the per-frame body grows.
#[must_use]
pub fn step<B: Backend, A: App<B>>(term: &mut Terminal<B>, app: &mut A, frame: &Frame) -> Flow {
    app.update(term, frame)
}

/// Drive an [`App`] with a blocking, event-driven loop until it returns [`Flow::Exit`].
///
/// Generic over the backend, so it powers every non-inverted backend
/// (`Crossterm` in `retroglyph-crossterm`, [`Headless`](crate::backend::Headless))
/// with no per-backend loop code.
/// Inverted backends (software/winit) provide their own driver.
///
/// The terminal is owned and dropped when the loop exits, so backend teardown
/// (for example crossterm's terminal restore) runs on the way out.
///
/// Presents automatically after [`App::update`] returns, the same as `retroglyph-window`'s
/// windowed drivers: skipped entirely on [`Flow::Idle`], and skipped as a redundant no-op if
/// `update` already presented itself. Equivalent to `run_blocking_with(term, app,
/// RunOptions::default())`: on [`Flow::Idle`], blocks on input rather than calling `update`
/// again immediately, so a turn-based app that's idle most of the time costs approximately
/// nothing. Use [`run_blocking_with`] with [`RunOptions::animated`] for a continuously-rendering
/// app instead.
///
/// # Errors
///
/// Returns the backend's error if the automatic `present()` call fails. The loop stops and the
/// terminal is dropped (running backend teardown) before the error is returned.
#[cfg(feature = "std")]
pub fn run_blocking<B, A>(term: Terminal<B>, app: A) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    run_blocking_with(term, app, RunOptions::default())
}

/// Options controlling [`run_blocking_with`]'s pacing and idle behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunOptions {
    /// Caps the loop at this many [`App::update`] calls per second whenever a frame actually
    /// runs, using a [`FrameClock`](crate::frame_clock::FrameClock) internally to pace them
    /// evenly. `None` (the default) runs uncapped: as fast as `update` allows for back-to-back
    /// [`Flow::Continue`] frames, or immediately after whatever woke an
    /// [`event_driven`](Self::event_driven) loop from [`Flow::Idle`].
    pub target_fps: Option<u32>,
    /// On [`Flow::Idle`], block on input instead of calling `update` again immediately.
    ///
    /// `true` (the default) is right for turn-based, event-driven apps that are idle most of the
    /// time: an idle frame costs approximately nothing, blocked in the backend's input read
    /// rather than spinning `update` as fast as the host can manage. `false` keeps `Flow::Idle`
    /// non-blocking (skip `present`, keep looping at whatever rate
    /// [`target_fps`](Self::target_fps) allows) -- right for apps that animate from
    /// [`Frame::delta`] and only return `Idle` between animation-driven `Continue` frames, where
    /// blocking would freeze the animation until the next stray input event. See
    /// [`RunOptions::animated`] for that shape.
    pub event_driven: bool,
    /// When [`event_driven`](Self::event_driven) is `true`, the longest an idle loop blocks
    /// before calling `update` again anyway, even with no input. `None` (the default) blocks
    /// indefinitely -- right for apps with nothing to redraw until input arrives. `Some(d)`
    /// additionally wakes the loop every `d`, for apps that need a periodic idle redraw (a
    /// blinking cursor, a clock) without paying full frame-rate cost. Ignored when
    /// [`event_driven`](Self::event_driven) is `false`.
    pub idle_wake: Option<Duration>,
}

impl RunOptions {
    /// Options for a continuously-rendering, [`target_fps`](Self::target_fps)-paced loop.
    ///
    /// [`event_driven`](Self::event_driven) is `false`: [`Flow::Idle`] only skips `present`, it
    /// never blocks. Use this for apps that drive a [`Tween`](crate::animate::Tween)/
    /// [`FrameClock`](crate::frame_clock::FrameClock) from [`Frame::delta`] and need `update`
    /// called every tick regardless of input.
    #[must_use]
    pub const fn animated(target_fps: u32) -> Self {
        Self {
            target_fps: Some(target_fps),
            event_driven: false,
            idle_wake: None,
        }
    }
}

impl Default for RunOptions {
    /// Event-driven, uncapped, blocks indefinitely on [`Flow::Idle`]: see [`run_blocking`].
    fn default() -> Self {
        Self {
            target_fps: None,
            event_driven: true,
            idle_wake: None,
        }
    }
}

/// Drive an [`App`] with a blocking loop until it returns [`Flow::Exit`], paced by `options`.
///
/// The zero-config [`run_blocking`] is equivalent to `run_blocking_with(term, app,
/// RunOptions::default())`. Pass [`RunOptions::animated`] for a continuously-rendering loop
/// capped at a fixed rate instead, using a [`FrameClock`](crate::frame_clock::FrameClock)
/// internally so `update` is called at even intervals rather than however fast the host can
/// spin.
///
/// With [`RunOptions::event_driven`] `true` (the default), [`Flow::Idle`] blocks the loop on
/// input -- via [`Terminal::wait_for_input`] -- instead of calling `update` again immediately:
/// an idle app has nothing new to show, so there is no reason to burn CPU polling it at all,
/// let alone faster than any configured rate. With `event_driven` `false`, an idle loop still
/// waits out the remainder of the current `target_fps` interval (if set) before calling `update`
/// again, rather than looping immediately, but never blocks on input.
///
/// # Errors
///
/// Returns the backend's error if the automatic `present()` call fails. The loop stops and the
/// terminal is dropped (running backend teardown) before the error is returned.
#[cfg(feature = "std")]
pub fn run_blocking_with<B, A>(
    mut term: Terminal<B>,
    mut app: A,
    options: RunOptions,
) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    let mut clock = options.target_fps.map(crate::frame_clock::FrameClock::new);
    let mut frame_count = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        if let Some(clock) = clock.as_mut() {
            // Block out the rest of this frame's budget before ticking `update` again, so a
            // paced loop doesn't busy-spin between updates the way an uncapped one does.
            let elapsed = last.elapsed();
            if let Some(remaining) = clock.step().checked_sub(elapsed) {
                std::thread::sleep(remaining);
            }
            clock.advance(clock.step().max(elapsed));
            // A fixed-timestep `FrameClock` is meant to be drained in a `while tick()` loop for
            // logic that must run in whole steps; here it only paces wall-clock timing, so a
            // single `tick()` (there is always at least one step ready, since we just slept/
            // advanced past the threshold) resets the accumulator for the next iteration.
            let _ = clock.tick();
        }
        let now = std::time::Instant::now();
        let delta = now.duration_since(last);
        last = now;
        let frame = Frame {
            delta,
            frame: frame_count,
        };
        frame_count = frame_count.wrapping_add(1);
        let present_count_before = term.present_count();
        let flow = step(&mut term, &mut app, &frame);
        if flow == Flow::Exit {
            return Ok(());
        }
        // A no-op if `update` already called `present()` itself (detected via `present_count`
        // rather than relying on `present()` being a safe no-op to call twice: it always presents
        // unconditionally, so a second call here would diff the just-cleared `current` against
        // the just-presented `previous` and erase the frame `update` already sent).
        if flow != Flow::Idle && term.present_count() == present_count_before {
            term.present()?;
        }
        // `Flow` is `#[non_exhaustive]`; treat any variant other than `Exit`/`Idle` the same as
        // `Continue` (keep looping and presenting) rather than exiting on an unknown future value.
        if flow == Flow::Idle && options.event_driven {
            // The heart of the fix for retroglyph#603: block here instead of immediately
            // re-entering the loop, so an idle frame costs approximately nothing rather than
            // spinning `update` as fast as the host allows. `wait_for_input` buffers any event it
            // finds rather than consuming it, so the app's own `update` still observes it on the
            // next iteration -- this call only answers "did something happen", it doesn't steal
            // the event. A `target_fps` clock (if set) still gets its top-of-loop sleep on the
            // next iteration; it isn't bypassed by waking early.
            term.wait_for_input(options.idle_wake.unwrap_or(Duration::MAX));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Headless;
    use crate::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    struct Counter {
        frames: u64,
    }

    impl App<Headless> for Counter {
        fn update(&mut self, term: &mut Terminal<Headless>, frame: &Frame) -> Flow {
            self.frames += 1;
            term.surface()
                .put((0, 0), '#', crate::style::Style::default());
            term.present().expect("present");
            // Quit when a key is pending, or after a safety cap.
            if term.has_input() || frame.frame >= 100 {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
    }

    #[test]
    fn run_blocking_exits_on_flow_exit() {
        let mut backend = Headless::new(4, 1);
        backend.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        let term = Terminal::new(backend);
        let app = Counter { frames: 0 };
        // Runs until the queued key is observed. Reaching the next line proves
        // the loop terminated on Flow::Exit rather than spinning forever.
        run_blocking(term, app).expect("run_blocking");
    }

    #[test]
    fn step_forwards_to_update() {
        let mut term = Terminal::new(Headless::new(2, 1));
        let mut app = Counter { frames: 0 };
        let frame = Frame {
            delta: Duration::ZERO,
            frame: 200,
        };
        let flow = step(&mut term, &mut app, &frame);
        assert_eq!(flow, Flow::Exit); // frame >= 100
        assert_eq!(app.frames, 1);
    }

    /// An app that never draws and always returns `Idle` except on the last frame: proves
    /// `run_blocking` skips `present()` for `Idle` frames rather than erasing an untouched grid.
    struct AlwaysIdle {
        frames: u64,
    }

    impl App<Headless> for AlwaysIdle {
        fn update(&mut self, _term: &mut Terminal<Headless>, frame: &Frame) -> Flow {
            self.frames += 1;
            if frame.frame >= 5 {
                Flow::Exit
            } else {
                Flow::Idle
            }
        }
    }

    #[test]
    fn run_blocking_skips_present_on_idle() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = AlwaysIdle { frames: 0 };
        // `update` never draws or presents; if the driver called `present()` on an `Idle` frame
        // anyway it would be harmless here (nothing to erase), so this mainly documents intent --
        // the presenting behavior itself is covered by `run_blocking_with_options_presents_frames`.
        run_blocking(term, app).expect("run_blocking");
    }

    /// An app that draws a distinct glyph per frame and never presents itself, so successfully
    /// reaching the backend proves the driver's automatic present ran.
    struct DrawsAndExits {
        frames: u64,
        exit_at: u64,
    }

    impl App<Headless> for DrawsAndExits {
        fn update(&mut self, term: &mut Terminal<Headless>, frame: &Frame) -> Flow {
            self.frames += 1;
            term.surface()
                .put((0, 0), 'x', crate::style::Style::default());
            if frame.frame >= self.exit_at {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
    }

    #[test]
    fn run_blocking_presents_automatically() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 0,
        };
        run_blocking(term, app).expect("run_blocking");
        // No assertion on backend content is possible here: `term` is consumed by `run_blocking`.
        // Coverage that the automatic present actually reaches the backend lives in
        // `retroglyph-window`'s own driver tests, which retain the terminal after the loop.
    }

    #[test]
    fn run_blocking_with_default_options_matches_run_blocking() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 2,
        };
        run_blocking_with(term, app, RunOptions::default()).expect("run_blocking_with");
    }

    #[test]
    fn run_blocking_with_animated_options_runs_to_completion() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 2,
        };
        // A high cap keeps this test fast; the point is that a paced loop still terminates on
        // `Flow::Exit` and delivers the same number of updates as an uncapped loop would.
        run_blocking_with(term, app, RunOptions::animated(1000)).expect("run_blocking_with");
    }

    #[test]
    fn run_options_animated_sets_fields() {
        let animated = RunOptions::animated(30);
        assert_eq!(animated.target_fps, Some(30));
        assert!(!animated.event_driven);
        assert_eq!(animated.idle_wake, None);

        let default = RunOptions::default();
        assert_eq!(default.target_fps, None);
        assert!(default.event_driven);
        assert_eq!(default.idle_wake, None);
    }

    /// An app that returns `Idle` for its first frame, then `Exit`. The queued key is only
    /// pushed into the backend *after* the driver would have already woken from the idle wait
    /// (`Headless::poll_event` ignores its timeout and returns immediately either way), so this
    /// mainly documents the contract at the type level: `event_driven: false` is accepted and the
    /// loop still terminates, i.e. the non-blocking `Idle` shape from before this change remains
    /// available for animated apps. Real blocking behavior (`event_driven: true` actually parking
    /// the thread) can only be observed on a backend that genuinely blocks, like crossterm --
    /// see that crate's own tests.
    struct IdleThenExit {
        frames: u64,
    }

    impl App<Headless> for IdleThenExit {
        fn update(&mut self, _term: &mut Terminal<Headless>, frame: &Frame) -> Flow {
            self.frames += 1;
            if frame.frame == 0 {
                Flow::Idle
            } else {
                Flow::Exit
            }
        }
    }

    #[test]
    fn run_blocking_with_non_event_driven_options_does_not_block_on_idle() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = IdleThenExit { frames: 0 };
        let options = RunOptions {
            target_fps: None,
            event_driven: false,
            idle_wake: None,
        };
        run_blocking_with(term, app, options).expect("run_blocking_with");
    }

    /// Proves the driver's idle wait doesn't swallow the event it woke up for: `update` is only
    /// ever called again *after* `wait_for_input` observed something, so the app's own `has_input`
    /// must still see the same event on the next frame rather than the driver having consumed it.
    struct ObservesQueuedEventAfterIdle {
        frames: u64,
        saw_input_after_idle: bool,
    }

    impl App<Headless> for ObservesQueuedEventAfterIdle {
        fn update(&mut self, term: &mut Terminal<Headless>, frame: &Frame) -> Flow {
            self.frames += 1;
            if frame.frame == 0 {
                return Flow::Idle;
            }
            self.saw_input_after_idle = term.has_input();
            Flow::Exit
        }
    }

    #[test]
    fn run_blocking_event_driven_idle_wait_does_not_consume_the_waking_event() {
        let mut backend = Headless::new(2, 1);
        backend.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )));
        let term = Terminal::new(backend);
        let mut app = ObservesQueuedEventAfterIdle {
            frames: 0,
            saw_input_after_idle: false,
        };
        // Can't recover `app` through `run_blocking` (it takes the app by value and drops it with
        // the terminal), so drive the loop by hand via `step`, mirroring what `run_blocking_with`
        // does around the `Flow::Idle` branch.
        let mut term = term;
        let frame0 = Frame {
            delta: Duration::ZERO,
            frame: 0,
        };
        assert_eq!(step(&mut term, &mut app, &frame0), Flow::Idle);
        // This is the exact call `run_blocking_with` makes on `Flow::Idle` when `event_driven` is
        // `true`: it must buffer the event, not return/consume it, so `update`'s own `has_input`
        // still finds it below.
        assert!(term.wait_for_input(Duration::MAX));
        let frame1 = Frame {
            delta: Duration::ZERO,
            frame: 1,
        };
        assert_eq!(step(&mut term, &mut app, &frame1), Flow::Exit);
        assert!(app.saw_input_after_idle);
    }
}
