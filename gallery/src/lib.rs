//! Shared plumbing for the gallery's numbered examples.
//!
//! Each example implements [`App<B>`](retroglyph_core::App) once, generically
//! over the backend, then calls [`rg_gallery_run!`] to generate the
//! per-backend `main` functions (crossterm terminal, software desktop/WASM
//! window, and a stdout-printing Headless fallback) instead of hand-writing
//! them. See `examples/01_hello_world.rs` for the shape this expects.
//!
//! This is intentionally much smaller than `crates/examples`'s `rg_run!`:
//! no `ClosureApp` init/tick split, no `wasm-headless`/`wasm-terminal`
//! browser-native-render branches -- just the four `main`s every example
//! needs, generated once instead of copy-pasted.

mod run_macro;

use retroglyph_core::{App, Flow, Frame, Headless, Terminal};

/// Headless fallback: ticks `app` a handful of frames against a fresh
/// `Terminal<Headless>` of `cols`x`rows`, printing each frame's grid to
/// stdout. No terminal or window is involved, and no input is ever
/// injected, so purely time-driven examples show motion across frames
/// while input-driven ones just repeat their initial state.
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
    for number in 1..=u64::from(frames) {
        let frame = Frame {
            dt: std::time::Duration::ZERO,
            number,
        };
        let flow = retroglyph_core::step(&mut term, &mut app, &frame);
        println!("--- Frame {number} ---");
        println!("{}", term.backend().grid());
        if flow == Flow::Exit {
            break;
        }
    }
}
