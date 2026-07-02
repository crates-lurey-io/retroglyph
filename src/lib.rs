//! rg: a 2D pseudographic terminal library.
//!
//! rg provides a grid of character cells with styled output, input handling,
//! and double-buffered presentation via pluggable backends.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

/// The `App`-driven game loop.
pub mod app;
/// Pluggable rendering backends.
pub mod backend;
/// A scrolling viewport into a world larger than the screen.
pub mod camera;
pub mod color;
pub mod event;
/// Fixed-timestep accumulator for game loops.
pub mod frame_clock;
pub mod grid;
#[cfg(feature = "egc")]
pub mod layout;
pub mod style;
pub mod terminal;
pub mod text;
/// The atomic drawable unit (glyph, style, sub-cell offsets).
pub mod tile;

#[cfg(feature = "std")]
pub use app::run_blocking;
pub use app::{App, Flow, Frame, step};
#[cfg(feature = "crossterm")]
pub use backend::Crossterm;
#[cfg(feature = "software")]
pub use backend::software::SoftwareBackend;
#[cfg(feature = "software-tilesets")]
pub use backend::software::tileset::{Codepage, TilesetBuilder, TilesetError, TilesetOptions};
pub use backend::{Backend, Headless};
pub use camera::Camera;
pub use color::{AnsiColor, Color, InvalidAnsiIndex};
pub use event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, PhysicalPos,
};
pub use frame_clock::FrameClock;
pub use grid::{Grid, Pos, Rect, Size};
#[cfg(feature = "egc")]
pub use layout::{HAlign, TextLayout, TextMetrics, VAlign};
pub use style::{CellModifier, Style};
pub use terminal::Terminal;
pub use text::{Line, Span};
pub use tile::Tile;
