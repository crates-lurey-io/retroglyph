//! rg: a terminal/grid rendering library for roguelikes.
//!
//! rg provides a grid of character cells with styled output, input handling,
//! and double-buffered presentation via pluggable backends.

pub mod color;
pub mod style;
pub mod cell;

pub use color::{AnsiColor, Color};
pub use style::{CellModifier, Style};
pub use cell::Cell;
