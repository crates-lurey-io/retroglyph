//! rg: a terminal/grid rendering library for roguelikes.
//!
//! rg provides a grid of character cells with styled output, input handling,
//! and double-buffered presentation via pluggable backends.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

/// Pluggable rendering backends.
pub mod backend;
pub mod cell;
pub mod color;
pub mod event;
pub mod grid;
pub mod style;
pub mod terminal;
pub mod text;

#[cfg(feature = "crossterm")]
pub use backend::Crossterm;
pub use backend::{Backend, Headless};
pub use cell::Cell;
pub use color::{AnsiColor, Color, InvalidAnsiIndex};
pub use event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
pub use grid::{Grid, Position, Rect, Size};
pub use style::{CellModifier, Style};
pub use terminal::Terminal;
pub use text::{Line, Span};
