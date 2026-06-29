//! Interactive demo of the rg library.
//!
//! The backend is selected at compile time via feature flags:
//!
//! - Terminal:  `cargo run --example dungeon_room --features crossterm`
//! - Desktop:   `cargo run --example dungeon_room --features software-default-font`
//! - WASM:      `cargo build --example dungeon_room --target wasm32-unknown-unknown --features software-default-font`

mod util;

use util::game::{GameState, tick};

rg_run!(GameState, GameState::new, tick);
