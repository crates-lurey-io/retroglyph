//! 01: Hello world
//!
//! A small cross-backend retroglyph program.
//!
//! Implements [`App`] once, generic over [`Backend`], and runs it with the backend provided.
//!
//! ```sh
//! cargo run --example 01_hello_world                                                          # Headless (prints a few frames)
//! cargo run --example 01_hello_world --features crossterm                                     # Terminal
//! cargo run --example 01_hello_world --features default-font                                  # Desktop window
//! cargo run --example 01_hello_world --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press any key (Terminal/Desktop) to quit. The Headless fallback has no real input source, so it
//! just prints a fixed number of frames and exits (override the count with `RG_HEADLESS_FRAMES`).

use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_gallery::rg_gallery_run;

/// Prints a greeting and quits on the first input event.
struct HelloWorld;

impl<B: Backend> App<B> for HelloWorld {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.print(2, 1, "Hello, retroglyph!");
        term.present().ok();

        if term.has_input() {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

rg_gallery_run!(HelloWorld, "01: Hello World", 30, 5);
