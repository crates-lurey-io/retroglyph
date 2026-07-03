//! retroglyph-core: the `no_std`-compatible foundation of retroglyph.
//!
//! Grid, tile, style, color, text, terminal, and event types, plus the
//! [`Backend`] trait and the dependency-free [`Headless`] test backend, and
//! the `App`/`Flow`/`Frame` game loop contract. Platform backends
//! (`retroglyph-crossterm`, `retroglyph-software`) and drawing helpers
//! (`retroglyph-widgets`) are separate crates that depend on this one.
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
pub use backend::{Backend, Headless};
pub use camera::Camera;
pub use color::{AnsiColor, Color, InvalidAnsiIndex};
pub use event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyState, MouseButton, MouseEvent,
    MouseEventKind, PhysicalPos,
};
pub use frame_clock::FrameClock;
pub use grid::{Grid, Pos, Rect, Size};
#[cfg(feature = "egc")]
pub use layout::{HAlign, TextLayout, TextMetrics, VAlign};
pub use style::{CellModifier, Style};
pub use terminal::Terminal;
pub use text::{Line, Span};
pub use tile::Tile;
