//! rg: a terminal/grid rendering library for roguelikes.
//!
//! rg provides a grid of character cells with styled output, input handling,
//! and double-buffered presentation via pluggable backends.
#![no_std]
extern crate alloc;

pub mod color;
pub mod style;
pub mod cell;
pub mod grid;
/// Pluggable rendering backends.
pub mod backend;
pub mod event;
pub mod terminal;

pub use color::{AnsiColor, Color};
pub use style::{CellModifier, Style};
pub use cell::Cell;
pub use grid::{Grid, Position, Size, Rect};
pub use backend::{Backend, Headless};
pub use terminal::Terminal;
pub use event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
