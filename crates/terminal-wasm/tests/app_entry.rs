//! Compile-check for `app_entry!` against a real `App<TerminalWasm>` implementation, on an
//! actual `wasm32` build (run via `wasm-pack test --node`, see `just test-wasm`).
//!
//! `app_entry!`'s generated `wasm_app_*` functions are declared inside the macro's own anonymous
//! `const _: () = { ... };` block (mirroring `examples/src/wasm_entry.rs`'s equivalent macros),
//! so they have no Rust-level path any sibling code -- including a `#[wasm_bindgen_test]` in this
//! same file -- can call by name; they only exist as `wasm-bindgen` exports for JS to call, the
//! same reason `examples/` has no Rust-level test of `wasm_terminal_example_*` either. This file
//! therefore only proves the macro type-checks against a real `App<TerminalWasm>` impl (the thing
//! most likely to break: wrong paths, wrong trait bounds, `Frame`/`Flow` misuse) -- `cargo check
//! --target wasm32-unknown-unknown --tests` compiling this crate is the actual assertion. Runtime
//! behavior of the generated FFI is exercised the same way the examples crate's equivalent macro
//! is: end-to-end, through a real browser (the docs gallery), not a unit test.
#![cfg(target_arch = "wasm32")]

use retroglyph_core::{App, Flow, Frame, Style, Terminal};
use retroglyph_terminal_wasm::TerminalWasm;

#[derive(Default)]
struct CountingApp {
    frames: u64,
}

impl App<TerminalWasm> for CountingApp {
    fn update(&mut self, term: &mut Terminal<TerminalWasm>, frame: &Frame) -> Flow {
        self.frames += 1;
        term.surface().put((0, 0), '#', Style::default());
        if frame.frame >= 2 {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

retroglyph_terminal_wasm::app_entry!(CountingApp);
