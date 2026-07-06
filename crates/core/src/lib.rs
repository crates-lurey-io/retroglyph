//! retroglyph-core: the `no_std`-compatible foundation of retroglyph.
//!
//! Grid, tile, style, color, text, terminal, and event types, plus the
//! [`Backend`] trait and the dependency-free [`Headless`] test backend, and
//! the `App`/`Flow`/`Frame` game loop contract. Platform backends
//! (`retroglyph-crossterm`, `retroglyph-software`) and drawing helpers
//! (`retroglyph-widgets`) are separate crates that depend on this one.
//!
//! # Architecture
//!
//! [`Terminal<B>`](Terminal) is the drawing API a game calls into (`put`,
//! `print`, `layer`, ...). It owns a double-buffered [`Grid`] and diffs the
//! current frame against the previous one in [`present`](Terminal::present),
//! sending only changed cells to the [`Backend`]. `B` is the only thing that
//! changes between a headless test and a real window or terminal:
//!
//! ```text
//!               ┌───────────────────────────┐
//!               │      App::update(...)      │  game logic, once, generic over B
//!               └──────────────┬─────────────┘
//!                              │ put / print / present
//!                              ▼
//!               ┌───────────────────────────┐
//!               │       Terminal<B>          │  double-buffered Grid, cell diff
//!               └──────────────┬─────────────┘
//!                              │ draw / draw_layers / poll_event
//!                              ▼
//!               ┌───────────────────────────┐
//!               │      B: Backend            │  the only piece that swaps out
//!               └──────────────┬─────────────┘
//!                              │
//!        ┌─────────────────────┼─────────────────────┐
//!        ▼                     ▼                      ▼
//!  Headless (here)      Crossterm                SoftwareRenderer
//!  in-memory grid,      (retroglyph-crossterm)   (retroglyph-software)
//!  synthetic events     real TTY, ANSI output    winit window, pixels
//! ```
//!
//! [`Headless`] stores presented content in memory and lets tests inject
//! synthetic [`Event`]s with [`Headless::push_event`](backend::Headless::push_event);
//! nothing here talks to a real terminal or window. Swapping `Headless` for
//! `Crossterm` or `SoftwareRenderer` changes only the `B` type parameter --
//! `App` implementations, [`Terminal`] calls, and game logic are unchanged.
//! `run_blocking` drives `Terminal<Headless>` and `Terminal<Crossterm>`
//! identically; the software backend's windowed loop drives `Terminal<SoftwareRenderer>`
//! through the same [`App`]/[`step`] contract, inverted because winit owns the
//! event loop instead of handing control back to a driver function.
//!
//! See `examples/headless.rs` (`cargo run -p retroglyph-core --example
//! headless`) for the smallest possible use of [`Headless`], depending on
//! nothing but this crate.
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
