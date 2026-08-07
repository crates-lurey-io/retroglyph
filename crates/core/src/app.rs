//! The `App`-driven game loop.
//!
//! Where [`Backend`](crate::backend::Backend) is the output contract, [`App`](crate::app::App) is the per-frame update
//! contract. A game implements [`App`](crate::app::App) once and runs on every backend unchanged.
//!
//! The loop decomposes into three pieces:
//!
//! - the contract ([`App`](crate::app::App), [`Flow`](crate::app::Flow), [`Frame`](crate::app::Frame)), here in the core;
//! - the generic blocking driver ([`run`](crate::app::run)/[`run_with`](crate::app::run_with), and the
//!   `Terminal`-taking [`run_on`](crate::app::run_on)/[`run_on_with`](crate::app::run_on_with)
//!   they're built on, `std` only), which covers `Crossterm` (in `retroglyph-crossterm`) and
//!   [`Headless`](crate::backend::Headless);
//! - the inverted driver in the windowing layer (the software backend's
//!   `run_app`), which cannot be generic because winit owns the loop instead of
//!   handing control back to a shared driver function.
//!
//! ```text
//!                        +-----------------------------+
//!                        |  App, Flow, Frame (core)    |
//!                        +-----------------------------+
//!                                     |
//!                                App::update
//!                                     |
//!               +---------------------+---------------------+
//!               |                                           |
//!   run_on / run_on_with              windowing layer's run_app
//!   (std only; owns the loop)                     (winit owns the loop instead)
//!               |                                           |
//!      crossterm, headless                           software backend
//! ```
//!
//! Both drivers call [`App::update`](crate::app::App::update) as the per-frame body and present automatically after it
//! returns, skipping the present on [`Flow::Idle`](crate::app::Flow::Idle) or when `update` already presented itself. The
//! low-level [`poll`](crate::terminal::Terminal::poll) / [`present`](crate::terminal::Terminal::present) API remains
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
    /// Run another frame, but nothing changed: skip [`present`](crate::terminal::Terminal::present) and leave the
    /// previous frame on screen.
    ///
    /// For turn-based apps that only need to redraw in response to player input, not on every
    /// tick of the driver's loop. Returning `Idle` while a `retroglyph_ui::Tween`- or
    /// [`FrameClock`](crate::frames::FrameClock)-driven animation is still in flight is an
    /// app bug, not a valid use: an in-progress animation has something new to show every frame,
    /// which is exactly what `Idle` tells the driver isn't true.
    Idle,
    /// Stop the loop. The driver returns and the terminal unwinds normally, so
    /// backend `Drop` logic (for example crossterm's terminal restore) runs.
    Exit,
}

/// Per-frame context handed to [`App::update`](crate::app::App::update).
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
/// use retroglyph_core::app::{App, Flow, Frame};
/// use retroglyph_core::backend::Backend;
/// use retroglyph_core::color::Style;
/// use retroglyph_core::terminal::Terminal;
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
    /// Draw into `term`, read input via `term`, and return [`Flow::Exit`](crate::app::Flow::Exit) to stop the loop.
    ///
    /// Draw via [`term.surface()`](crate::terminal::Terminal::surface) or [`term.draw()`](crate::terminal::Terminal::draw) (though
    /// `draw` presents itself, which usually conflicts with the driver's own automatic present
    /// below; prefer `surface()` inside `update`). Every driver ([`run_on`](crate::app::run_on) and
    /// `retroglyph-window`'s windowed drivers) presents the frame automatically right after this
    /// method returns, unless it returned [`Flow::Idle`](crate::app::Flow::Idle), in which case the driver skips
    /// [`present`](crate::terminal::Terminal::present) entirely. Calling `present` yourself inside `update` remains
    /// fine (the driver detects it already ran via [`present_count`](crate::terminal::Terminal::present_count) and
    /// skips its own call) but is never required. [`run_on`](crate::app::run_on) and [`run_on_with`](crate::app::run_on_with) link
    /// back here rather than restating this contract.
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow;
}

/// Drive an [`App`](crate::app::App) with a blocking, event-driven loop until it returns [`Flow::Exit`](crate::app::Flow::Exit).
///
/// Generic over the backend, so it powers every non-inverted backend
/// (`Crossterm` in `retroglyph-crossterm`, [`Headless`](crate::backend::Headless))
/// with no per-backend loop code.
/// Inverted backends (software/winit) provide their own driver.
///
/// The terminal is owned and dropped when the loop exits, so backend teardown
/// (for example crossterm's terminal restore) runs on the way out.
///
/// See [`App::update`](crate::app::App::update) for the present/idle contract this and every other driver follows.
/// Equivalent to `run_on_with(term, app, RunOptions::default())`: on [`Flow::Idle`](crate::app::Flow::Idle), blocks
/// on input rather than calling `update` again immediately, so a turn-based app that's idle most
/// of the time costs approximately nothing. Use [`run_on_with`](crate::app::run_on_with) with [`RunOptions::animated`](crate::app::RunOptions::animated)
/// for a continuously-rendering app instead.
///
/// # Errors
///
/// Returns the backend's error if the automatic `present()` call fails. The loop stops and the
/// terminal is dropped (running backend teardown) before the error is returned.
#[cfg(feature = "std")]
pub fn run_on<B, A>(term: Terminal<B>, app: A) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    run_on_with(term, app, RunOptions::default())
}

/// Options controlling [`run_on_with`](crate::app::run_on_with)'s pacing and idle behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunOptions {
    target_fps: Option<u32>,
    event_driven: bool,
    idle_wake: Option<Duration>,
}

impl RunOptions {
    /// Options for a continuously-rendering, [`target_fps`](Self::target_fps)-paced loop.
    ///
    /// [`event_driven`](Self::event_driven) is `false`: [`Flow::Idle`](crate::app::Flow::Idle) only skips `present`, it
    /// never blocks. Use this for apps that drive a `retroglyph_ui::Tween`/
    /// [`FrameClock`](crate::frames::FrameClock) from [`Frame::delta`](crate::app::Frame::delta) and need `update`
    /// called every tick regardless of input.
    ///
    /// `target_fps` becomes [`RunOptions::target_fps`](crate::app::RunOptions::target_fps) verbatim, including `0`: passing `0` here
    /// builds without panicking, but [`run_on_with`](crate::app::run_on_with) panics once it constructs the
    /// [`FrameClock`](crate::frames::FrameClock) that paces it (see that function's
    /// `# Panics` section).
    #[must_use]
    pub const fn animated(target_fps: u32) -> Self {
        Self {
            target_fps: Some(target_fps),
            event_driven: false,
            idle_wake: None,
        }
    }

    /// Caps the loop at this many [`App::update`](crate::app::App::update) calls per second whenever a frame actually
    /// runs, using a [`FrameClock`](crate::frames::FrameClock) internally to pace them
    /// evenly. `None` (the default) runs uncapped: as fast as `update` allows for back-to-back
    /// [`Flow::Continue`](crate::app::Flow::Continue) frames, or immediately after whatever woke an
    /// [`event_driven`](Self::event_driven) loop from [`Flow::Idle`](crate::app::Flow::Idle).
    #[must_use]
    pub const fn with_target_fps(mut self, target_fps: u32) -> Self {
        self.target_fps = Some(target_fps);
        self
    }

    /// Returns the configured [`target_fps`](Self::with_target_fps) cap, if any.
    #[must_use]
    pub const fn target_fps(&self) -> Option<u32> {
        self.target_fps
    }

    /// On [`Flow::Idle`](crate::app::Flow::Idle), block on input instead of calling `update` again immediately.
    ///
    /// `true` (the default) is right for turn-based, event-driven apps that are idle most of the
    /// time: an idle frame costs approximately nothing, blocked in the backend's input read
    /// rather than spinning `update` as fast as the host can manage. `false` keeps `Flow::Idle`
    /// non-blocking (skip `present`, keep looping at whatever rate
    /// [`target_fps`](Self::target_fps) allows): right for apps that animate from
    /// [`Frame::delta`](crate::app::Frame::delta) and only return `Idle` between animation-driven `Continue` frames, where
    /// blocking would freeze the animation until the next stray input event. See
    /// [`RunOptions::animated`](crate::app::RunOptions::animated) for that shape.
    #[must_use]
    pub const fn event_driven(mut self, event_driven: bool) -> Self {
        self.event_driven = event_driven;
        self
    }

    /// Returns whether [`Flow::Idle`](crate::app::Flow::Idle) blocks on input rather than looping immediately.
    #[must_use]
    pub const fn is_event_driven(&self) -> bool {
        self.event_driven
    }

    /// When [`is_event_driven`](Self::is_event_driven) is `true`, the longest an idle loop blocks
    /// before calling `update` again anyway, even with no input. `None` (the default) blocks
    /// indefinitely: right for apps with nothing to redraw until input arrives. `Some(d)`
    /// additionally wakes the loop every `d`, for apps that need a periodic idle redraw (a
    /// blinking cursor, a clock) without paying full frame-rate cost. Ignored when
    /// [`is_event_driven`](Self::is_event_driven) is `false`.
    #[must_use]
    pub const fn with_idle_wake(mut self, idle_wake: Duration) -> Self {
        self.idle_wake = Some(idle_wake);
        self
    }

    /// Returns the configured [`idle_wake`](Self::with_idle_wake) interval, if any.
    #[must_use]
    pub const fn idle_wake(&self) -> Option<Duration> {
        self.idle_wake
    }
}

impl Default for RunOptions {
    /// Event-driven, uncapped, blocks indefinitely on [`Flow::Idle`](crate::app::Flow::Idle): see [`run_on`](crate::app::run_on).
    fn default() -> Self {
        Self {
            target_fps: None,
            event_driven: true,
            idle_wake: None,
        }
    }
}

/// Drive an [`App`](crate::app::App) with a blocking loop until it returns [`Flow::Exit`](crate::app::Flow::Exit), paced by `options`.
///
/// The zero-config [`run_on`](crate::app::run_on) is equivalent to `run_on_with(term, app,
/// RunOptions::default())`. Pass [`RunOptions::animated`](crate::app::RunOptions::animated) for a continuously-rendering loop
/// capped at a fixed rate instead, using a [`FrameClock`](crate::frames::FrameClock)
/// internally so `update` is called at even intervals rather than however fast the host can
/// spin.
///
/// With [`RunOptions::is_event_driven`](crate::app::RunOptions::is_event_driven) `true` (the default), [`Flow::Idle`](crate::app::Flow::Idle) blocks the loop on
/// input (via [`Terminal::wait_for_input`](crate::terminal::Terminal::wait_for_input)) instead of calling `update` again immediately:
/// an idle app has nothing new to show, so there is no reason to burn CPU polling it at all,
/// let alone faster than any configured rate. With `event_driven` `false`, an idle loop still
/// waits out the remainder of the current `target_fps` interval (if set) before calling `update`
/// again, rather than looping immediately, but never blocks on input.
///
/// # Errors
///
/// Returns the backend's error if the automatic `present()` call fails. The loop stops and the
/// terminal is dropped (running backend teardown) before the error is returned.
///
/// # Panics
///
/// Panics if `options.target_fps` is `Some(0)`: pacing at a `FrameClock` internally, which
/// requires a non-zero rate (see [`FrameClock::new`](crate::frames::FrameClock::new)).
#[cfg(feature = "std")]
pub fn run_on_with<B, A>(
    mut term: Terminal<B>,
    mut app: A,
    options: RunOptions,
) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    let mut clock = options.target_fps().map(crate::frames::FrameClock::new);
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
        let flow = app.update(&mut term, &frame);
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
        if flow == Flow::Idle && options.is_event_driven() {
            // The heart of the fix for retroglyph#603: block here instead of immediately
            // re-entering the loop, so an idle frame costs approximately nothing rather than
            // spinning `update` as fast as the host allows. `wait_for_input` buffers any event it
            // finds rather than consuming it, so the app's own `update` still observes it on the
            // next iteration; this call only answers "did something happen", it doesn't steal
            // the event. A `target_fps` clock (if set) still gets its top-of-loop sleep on the
            // next iteration; it isn't bypassed by waking early.
            term.wait_for_input(options.idle_wake().unwrap_or(Duration::MAX));
        }
    }
}

/// Builds a [`Terminal`](crate::terminal::Terminal) over `backend` and drives `app` with
/// [`run_on`](crate::app::run_on).
///
/// The canonical entry point for a blocking-loop backend (`Crossterm` in
/// `retroglyph-crossterm`, [`Headless`](crate::backend::Headless), and any future backend with a
/// loop it can enter and return from): construct the backend, hand it here with the app, and the
/// terminal is built and owned for you. Backends whose control flow is inverted (winit owns the
/// loop) or push-driven (wasm's `requestAnimationFrame`) keep their own drivers instead; see
/// this module's doc comment for that split.
///
/// # Errors
///
/// Returns `backend`'s error if it fails to build a [`Terminal`](crate::terminal::Terminal) over
/// itself, or if the automatic `present()` call fails while `app` is running. See
/// [`run_on`](crate::app::run_on) for the exact loop behavior.
#[cfg(feature = "std")]
pub fn run<B, A>(backend: B, app: A) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    run_on(Terminal::new(backend), app)
}

/// A backend's own entry point for driving an [`App`](crate::app::App), named at the call site
/// rather than selected by which Cargo features happen to be enabled.
///
/// Each backend crate implements this on whichever type already gathers the configuration it
/// needs to start: `CrosstermOptions` in `retroglyph-crossterm` (reachable via
/// `Crossterm::builder()`), and a small `Windowed` wrapper around each windowed backend's
/// `PresenterBuilder` in `retroglyph-window` (reachable via `retroglyph-software`,
/// `retroglyph-gl`, and `retroglyph-wgpu`). Both shapes let two backends live in the same binary
/// with no conflict, since Cargo features are additive: a `run()` dispatched by `#[cfg(feature =
/// "crossterm")]` would silently change which backend a binary runs the moment any dependency
/// anywhere in the graph turned that feature on for its own reasons, and `--all-features` would
/// only ever type-check one arm of it. Naming the concrete backend in code, via `Launch::launch`,
/// makes every additive feature harmless instead.
///
/// A single generic `fn run<A>(app: A) where A: for<B: Backend> App<B>` isn't expressible on
/// stable Rust either way (no non-lifetime binders; see rust#108185), so there was never a way to
/// accept "an app that runs on any backend" from one function signature. Each `Launch` impl names
/// one concrete [`Backend`]; an app written as `impl<B: Backend> App<B> for MyGame` satisfies
/// every one of them without change.
///
/// This trait intentionally has no unified error type spanning every backend: [`launch`](Self::launch)
/// returns [`Self::Error`], whatever the implementing backend's own error is (`std::io::Error`
/// for crossterm, a small enum spanning the presenter builder's error and winit's
/// `EventLoopError` for the windowed backends). A facade crate that depends on more than one
/// backend and wants one error type spanning all of them can wrap this trait; this crate, which
/// only ever sees one backend's impl at a time, does not.
///
/// Unlike [`run`](crate::app::run)/[`run_on`](crate::app::run_on) and their `_with` counterparts,
/// this trait is not gated behind the `std` feature: the shape declared here (an associated
/// `Backend`, an associated `Error`, and `launch`'s signature) uses nothing from `std`, only
/// [`Backend`], [`App`], and [`RunOptions`], all of which are already available without it. Only
/// the concrete impls need `std` in practice (`retroglyph-crossterm`'s and
/// `retroglyph-window`'s both do, since they delegate to `run_on_with`/`run_app_on`), and each of
/// those lives in its own backend crate, not here; nothing stops a future `no_std` backend from
/// implementing `Launch` too.
///
/// # Examples
///
/// ```no_run
/// use retroglyph_core::app::{App, Flow, Frame, Launch, RunOptions};
/// use retroglyph_core::backend::Backend;
/// use retroglyph_core::terminal::Terminal;
///
/// // A game written once, generic over the backend, satisfies every `Launch` impl unchanged.
/// struct MyGame;
/// impl<B: Backend> App<B> for MyGame {
///     fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
///         Flow::Exit
///     }
/// }
///
/// // Stand-ins for two real backends' entry points: `retroglyph-crossterm`'s
/// // `CrosstermOptions` (reachable via `Crossterm::builder()`) and `retroglyph-window`'s
/// // `Windowed<B>` (reachable via `retroglyph-software`/`-gl`/`-wgpu`). Each names one concrete
/// // `Backend`, so both can `launch(MyGame, ..)` unmodified -- see those crates' own docs for
/// // the real impls this sketches. Written by hand here (rather than delegating to
/// // `run_on_with`, which both real impls actually use) purely so this doctest itself stays
/// // `std`-free, proving `Launch` doesn't need it -- the real impls' own docs are the proof they
/// // work end to end.
/// # use retroglyph_core::backend::Headless;
/// fn drive_to_exit<A: App<Headless> + 'static>(mut app: A) -> Result<(), core::convert::Infallible> {
///     let mut term = Terminal::new(Headless::new(80, 24));
///     let mut frame_count = 0u64;
///     loop {
///         let frame = Frame { delta: core::time::Duration::ZERO, frame: frame_count };
///         frame_count += 1;
///         if app.update(&mut term, &frame) == Flow::Exit {
///             return Ok(());
///         }
///     }
/// }
///
/// struct TerminalOptions;
/// impl Launch for TerminalOptions {
///     type Backend = Headless;
///     type Error = core::convert::Infallible;
///     fn launch<A>(self, app: A, _options: RunOptions) -> Result<(), Self::Error>
///     where
///         A: App<Self::Backend> + 'static,
///     {
///         drive_to_exit(app)
///     }
/// }
///
/// struct WindowedOptions;
/// impl Launch for WindowedOptions {
///     type Backend = Headless;
///     type Error = core::convert::Infallible;
///     fn launch<A>(self, app: A, _options: RunOptions) -> Result<(), Self::Error>
///     where
///         A: App<Self::Backend> + 'static,
///     {
///         drive_to_exit(app)
///     }
/// }
///
/// # fn call_sites() -> Result<(), core::convert::Infallible> {
/// TerminalOptions.launch(MyGame, RunOptions::default())?;
/// WindowedOptions.launch(MyGame, RunOptions::animated(60))?;
/// # Ok(())
/// # }
/// ```
pub trait Launch {
    /// The backend this impl launches [`App`](crate::app::App) on.
    type Backend: Backend;
    /// The error this impl's [`launch`](Self::launch) can fail with; each backend surfaces its
    /// own, unwrapped (see this trait's docs for why there is no unified error here).
    type Error;

    /// Builds this backend and drives `app` on it with `options`, blocking until `app` returns
    /// [`Flow::Exit`](crate::app::Flow::Exit).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if this backend fails to build, or if driving `app` fails once
    /// running; see the implementing type's own docs for the exact conditions.
    fn launch<A>(self, app: A, options: RunOptions) -> Result<(), Self::Error>
    where
        A: App<Self::Backend> + 'static;
}

/// Builds a [`Terminal`](crate::terminal::Terminal) over `backend` and drives `app` with
/// [`run_on_with`](crate::app::run_on_with), paced by `options`.
///
/// The `options`-taking counterpart to [`run`](crate::app::run); see that function for which
/// backends this suits, and [`RunOptions`](crate::app::RunOptions) for the available pacing and
/// idle-blocking controls.
///
/// # Errors
///
/// Returns `backend`'s error if it fails to build a [`Terminal`](crate::terminal::Terminal) over
/// itself, or if the automatic `present()` call fails while `app` is running. See
/// [`run_on_with`](crate::app::run_on_with) for the exact loop behavior.
///
/// # Panics
///
/// Panics if `options.target_fps` is `Some(0)`; see [`run_on_with`](crate::app::run_on_with).
#[cfg(feature = "std")]
pub fn run_with<B, A>(backend: B, app: A, options: RunOptions) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    run_on_with(Terminal::new(backend), app, options)
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
                .put((0, 0), '#', crate::color::Style::default());
            term.present().expect("present");
            // Quit when a key is pending, or after a safety cap.
            if term.has_input() || frame.frame >= 100 {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn run_on_exits_on_flow_exit() {
        let mut backend = Headless::new(4, 1);
        backend.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        let term = Terminal::new(backend);
        let app = Counter { frames: 0 };
        // Runs until the queued key is observed. Reaching the next line proves
        // the loop terminated on Flow::Exit rather than spinning forever.
        run_on(term, app).expect("run_on");
    }

    /// An app that never draws and always returns `Idle` except on the last frame: proves
    /// `run_on` skips `present()` for `Idle` frames rather than erasing an untouched grid.
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

    #[cfg(feature = "std")]
    #[test]
    fn run_on_skips_present_on_idle() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = AlwaysIdle { frames: 0 };
        // `update` never draws or presents; if the driver called `present()` on an `Idle` frame
        // anyway it would be harmless here (nothing to erase), so this mainly documents intent --
        // the presenting behavior itself is covered by `run_on_with_options_presents_frames`.
        run_on(term, app).expect("run_on");
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
                .put((0, 0), 'x', crate::color::Style::default());
            if frame.frame >= self.exit_at {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn run_on_presents_automatically() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 0,
        };
        run_on(term, app).expect("run_on");
        // No assertion on backend content is possible here: `term` is consumed by `run_on`.
        // Coverage that the automatic present actually reaches the backend lives in
        // `retroglyph-window`'s own driver tests, which retain the terminal after the loop.
    }

    #[cfg(feature = "std")]
    #[test]
    fn run_on_with_default_options_matches_run_on() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 2,
        };
        run_on_with(term, app, RunOptions::default()).expect("run_on_with");
    }

    #[cfg(feature = "std")]
    #[test]
    fn run_on_with_animated_options_runs_to_completion() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 2,
        };
        // A high cap keeps this test fast; the point is that a paced loop still terminates on
        // `Flow::Exit` and delivers the same number of updates as an uncapped loop would.
        run_on_with(term, app, RunOptions::animated(1000)).expect("run_on_with");
    }

    #[cfg(feature = "std")]
    #[test]
    fn run_builds_the_terminal_and_exits_on_flow_exit() {
        let mut backend = Headless::new(4, 1);
        backend.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        let app = Counter { frames: 0 };
        // `run` takes the bare backend rather than a `Terminal`, unlike `run_on`; reaching
        // the next line proves it still builds one and drives the loop to `Flow::Exit`.
        run(backend, app).expect("run");
    }

    #[cfg(feature = "std")]
    #[test]
    fn run_with_builds_the_terminal_and_honors_options() {
        let backend = Headless::new(2, 1);
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 2,
        };
        // Same proof as `run_on_with_animated_options_runs_to_completion`, but starting
        // from a bare backend to cover `run_with`'s own `Terminal::new` call.
        run_with(backend, app, RunOptions::animated(1000)).expect("run_with");
    }

    #[test]
    fn run_options_animated_sets_fields() {
        let animated = RunOptions::animated(30);
        assert_eq!(animated.target_fps(), Some(30));
        assert!(!animated.is_event_driven());
        assert_eq!(animated.idle_wake(), None);

        let default = RunOptions::default();
        assert_eq!(default.target_fps(), None);
        assert!(default.is_event_driven());
        assert_eq!(default.idle_wake(), None);
    }

    #[test]
    fn run_options_setters_override_defaults() {
        let options = RunOptions::default()
            .with_target_fps(60)
            .event_driven(false)
            .with_idle_wake(Duration::from_millis(250));
        assert_eq!(options.target_fps(), Some(60));
        assert!(!options.is_event_driven());
        assert_eq!(options.idle_wake(), Some(Duration::from_millis(250)));
    }

    /// An app that returns `Idle` for its first frame, then `Exit`. The queued key is only
    /// pushed into the backend *after* the driver would have already woken from the idle wait
    /// (`Headless::poll_event` ignores its timeout and returns immediately either way), so this
    /// mainly documents the contract at the type level: `event_driven: false` is accepted and the
    /// loop still terminates, i.e. the non-blocking `Idle` shape is a supported option for
    /// animated apps. Real blocking behavior (`event_driven: true` actually parking
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

    #[cfg(feature = "std")]
    #[test]
    fn run_on_with_non_event_driven_options_does_not_block_on_idle() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = IdleThenExit { frames: 0 };
        let options = RunOptions {
            target_fps: None,
            event_driven: false,
            idle_wake: None,
        };
        run_on_with(term, app, options).expect("run_on_with");
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
    fn run_on_event_driven_idle_wait_does_not_consume_the_waking_event() {
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
        // Can't recover `app` through `run_on` (it takes the app by value and drops it with
        // the terminal), so drive the loop by hand via `step`, mirroring what `run_on_with`
        // does around the `Flow::Idle` branch.
        let mut term = term;
        let frame0 = Frame {
            delta: Duration::ZERO,
            frame: 0,
        };
        assert_eq!(app.update(&mut term, &frame0), Flow::Idle);
        // This is the exact call `run_on_with` makes on `Flow::Idle` when `event_driven` is
        // `true`: it must buffer the event, not return/consume it, so `update`'s own `has_input`
        // still finds it below.
        assert!(term.wait_for_input(Duration::MAX));
        let frame1 = Frame {
            delta: Duration::ZERO,
            frame: 1,
        };
        assert_eq!(app.update(&mut term, &frame1), Flow::Exit);
        assert!(app.saw_input_after_idle);
    }
}
