//! Placeholder example for the new `retroglyph-examples` crate.
//!
//! This crate is being rebuilt from scratch after `gallery/` and the old
//! `crates/examples/` were deleted. Run with:
//! `cargo run -p retroglyph-examples --example hello_world`

use retroglyph_core::{Headless, Terminal};

fn main() {
    let mut term = Terminal::new(Headless::new(20, 3));
    term.print(1, 1, "Hello, retroglyph!");
    term.present().unwrap();
    println!("{}", term.backend().format_view());
}
