//! Shared plumbing for the gallery's numbered examples.
//!
//! Each example implements [`App<B>`](retroglyph_core::App) once, generically over the backend,
//! then calls [`rg_gallery_run!`] to generate the per-backend `main` functions (crossterm
//! terminal, software desktop/WASM window, and a stdout-printing Headless fallback) instead of
//! hand-writing them. See `examples/01_hello_world.rs` for the shape this expects.

mod keys;
mod press_any_key;
mod run_macro;

use retroglyph_core::{App, Flow, Frame, Headless, Terminal};

pub use keys::pressed_key;
pub use press_any_key::any_key_pressed_or_window_closed;

/// Synthetic per-frame duration [`run_headless`] feeds as [`Frame::delta`]. Headless has no real
/// timing loop to measure elapsed wall time from -- it just calls `update` a fixed number of
/// times -- so this is a nominal stand-in (20 logic frames per simulated second) rather than
/// anything measured, chosen only so `Frame::delta`-driven examples accumulate *some* nonzero
/// elapsed time per call instead of staying frozen at `Duration::ZERO` forever.
const HEADLESS_DT: std::time::Duration = std::time::Duration::from_millis(50);

/// Headless fallback: ticks `app` a handful of frames against a fresh `Terminal<Headless>` of
/// `cols`x`rows`, printing each frame's grid to stdout. No terminal or window is involved, and no
/// input is ever injected, so purely time-driven examples show motion across frames (whether
/// driven by [`Frame::frame`] or, via [`HEADLESS_DT`], [`Frame::delta`]) while input-driven ones
/// just repeat their initial state.
///
/// Frame count defaults to 3; override with `RG_HEADLESS_FRAMES`.
#[doc(hidden)]
pub fn run_headless<A: App<Headless>>(cols: u16, rows: u16, mut app: A) {
    let frames: u32 = std::env::var("RG_HEADLESS_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3);

    let mut term = Terminal::new(Headless::new(cols, rows));
    for frame_count in 0..u64::from(frames) {
        let frame = Frame {
            delta: HEADLESS_DT,
            frame: frame_count,
        };
        let flow = retroglyph_core::step(&mut term, &mut app, &frame);
        println!("--- Frame {frame_count} ---");
        println!("{}", term.backend().grid());
        if flow == Flow::Exit {
            break;
        }
    }
}
