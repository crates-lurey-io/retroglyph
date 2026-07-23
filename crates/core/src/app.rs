//! The `App`-driven game loop.
//!
//! `App` is the update-side dual of [`Backend`](crate::Backend): where a
//! backend is the output contract, an [`App`] is the per-frame update contract.
//! A game implements [`App`] once and runs on every backend unchanged.
//!
//! The loop decomposes into three pieces:
//!
//! - the contract ([`App`], [`Flow`], [`Frame`]), here in the core;
//! - the generic blocking driver ([`run_blocking`], `std` only), which covers
//!   `Crossterm` (in `retroglyph-crossterm`) and [`Headless`](crate::backend::Headless);
//! - the inverted driver in the windowing layer (the software backend's
//!   `run_app`), which cannot be generic because winit owns the loop instead of
//!   handing control back to a shared driver function.
//!
//! Both drivers share [`step`] as the per-frame body. The low-level
//! [`poll`](crate::Terminal::poll) / [`present`](crate::Terminal::present) API
//! remains available for turn-based games and headless tests.

use crate::backend::Backend;
use crate::terminal::Terminal;
use core::time::Duration;

/// Whether the game loop should continue or stop after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Flow {
    /// Run another frame.
    Continue,
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
/// use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
///
/// struct MyGame;
/// impl<B: Backend> App<B> for MyGame {
///     fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
///         term.put(0, 0, '@');
///         term.present().ok();
///         Flow::Exit
///     }
/// }
/// ```
pub trait App<B: Backend> {
    /// Advance and render one frame.
    ///
    /// Draw into `term`, read input via `term`, and call
    /// [`term.present()`](Terminal::present) to render the frame. Return
    /// [`Flow::Exit`] to stop the loop.
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

/// Drive an [`App`] with a blocking loop until it returns [`Flow::Exit`].
///
/// Generic over the backend, so it powers every non-inverted backend
/// (`Crossterm` in `retroglyph-crossterm`, [`Headless`](crate::backend::Headless))
/// with no per-backend loop code.
/// Inverted backends (software/winit) provide their own driver.
///
/// The terminal is owned and dropped when the loop exits, so backend teardown
/// (for example crossterm's terminal restore) runs on the way out.
#[cfg(feature = "std")]
pub fn run_blocking<B, A>(mut term: Terminal<B>, mut app: A)
where
    B: Backend,
    A: App<B>,
{
    let mut frame_count = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        let delta = now.duration_since(last);
        last = now;
        let frame = Frame {
            delta,
            frame: frame_count,
        };
        frame_count = frame_count.wrapping_add(1);
        // `Flow` is `#[non_exhaustive]`; treat any variant other than `Flow::Exit` the same as
        // `Flow::Continue` (keep looping) rather than exiting on an unknown future value.
        if step(&mut term, &mut app, &frame) == Flow::Exit {
            return;
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
            term.put(0, 0, '#');
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
        run_blocking(term, app);
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
}
