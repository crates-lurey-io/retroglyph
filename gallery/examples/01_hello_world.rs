//! 01: Hello world -- print, present, quit on input
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
//! Press any key (Terminal/Desktop) to quit.

use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};

/// Prints a greeting and quits on the first input event.
struct HelloWorld;

impl<B: Backend> App<B> for HelloWorld {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        term.print(2, 1, "Hello, retroglyph!");

        // Crash on present errors for this simple example.
        term.present().expect("present failed");

        if any_key_pressed_or_window_closed(term) {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

rg_gallery_run!(HelloWorld, "01: Hello World", 30, 5);
