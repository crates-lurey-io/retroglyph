//! rg: a terminal/grid rendering library for roguelikes.
//!
//! rg provides a grid of character cells with styled output, input handling,
//! and double-buffered presentation via pluggable backends.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

/// Pluggable rendering backends.
pub mod backend;
/// The atomic drawable unit (glyph, style, sub-cell offsets).
pub mod tile;
pub mod color;
pub mod event;
pub mod grid;
#[cfg(feature = "egc")]
pub mod layout;
pub mod style;
pub mod terminal;
pub mod text;

#[cfg(feature = "crossterm")]
pub use backend::Crossterm;
#[cfg(feature = "software")]
pub use backend::software::SoftwareBackend;
pub use backend::{Backend, Headless};
pub use tile::Tile;
pub use color::{AnsiColor, Color, InvalidAnsiIndex};
pub use event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
pub use grid::{Grid, Pos, Rect, Size};
#[cfg(feature = "egc")]
pub use layout::{HAlign, TextLayout, TextMetrics, VAlign};
pub use style::{CellModifier, Style};
pub use terminal::Terminal;
pub use text::{Line, Span};
