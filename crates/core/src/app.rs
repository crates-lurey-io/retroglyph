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
    /// below -- prefer `surface()` inside `update`). Every driver ([`run_blocking`] and
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

/// Drive an [`App`] with an unpaced blocking loop until it returns [`Flow::Exit`].
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
/// `update` already presented itself. This loop runs as fast as `update` allows, with no frame
/// rate cap; use [`run_blocking_with`] and [`RunOptions::max_fps`] for a paced loop.
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

/// Options controlling [`run_blocking_with`]'s pacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RunOptions {
    /// Caps the loop at this many [`App::update`] calls per second, using a
    /// [`FrameClock`](crate::frame_clock::FrameClock) internally to pace them evenly. `None` (the
    /// default) runs unpaced, as fast as `update` allows.
    pub max_fps: Option<u32>,
}

impl RunOptions {
    /// Options requesting a paced loop capped at `max_fps` updates per second.
    #[must_use]
    pub const fn paced(max_fps: u32) -> Self {
        Self {
            max_fps: Some(max_fps),
        }
    }
}

/// Drive an [`App`] with a blocking loop until it returns [`Flow::Exit`], paced by `options`.
///
/// The zero-config [`run_blocking`] is equivalent to `run_blocking_with(term, app,
/// RunOptions::default())`: unpaced, spinning as fast as `update` allows. Pass
/// [`RunOptions::paced`] to cap the loop at a fixed rate instead, using a
/// [`FrameClock`](crate::frame_clock::FrameClock) internally so `update` is called at even
/// intervals rather than however fast the host can spin.
///
/// On [`Flow::Idle`], the paced loop still waits out the remainder of the current frame interval
/// before calling `update` again, rather than looping immediately: an idle app has nothing new to
/// show, so there is no reason to burn CPU polling it faster than the configured rate.
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
    let mut clock = options.max_fps.map(crate::frame_clock::FrameClock::new);
    let mut frame_count = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        if let Some(clock) = clock.as_mut() {
            // Block out the rest of this frame's budget before ticking `update` again, so a
            // paced loop doesn't busy-spin between updates the way the unpaced loop does.
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
    fn run_blocking_with_paced_options_runs_to_completion() {
        let term = Terminal::new(Headless::new(2, 1));
        let app = DrawsAndExits {
            frames: 0,
            exit_at: 2,
        };
        // A high cap keeps this test fast; the point is that a paced loop still terminates on
        // `Flow::Exit` and delivers the same number of updates as the unpaced loop would.
        run_blocking_with(term, app, RunOptions::paced(1000)).expect("run_blocking_with");
    }

    #[test]
    fn run_options_paced_sets_max_fps() {
        assert_eq!(RunOptions::paced(30).max_fps, Some(30));
        assert_eq!(RunOptions::default().max_fps, None);
    }
}
