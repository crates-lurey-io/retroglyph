//! A shared layer for window-based backends (software, GL, wgpu).
//!
//! # Architecture
//!
//! [`Backend`](retroglyph_core::Backend) fuses input (`poll_event`/
//! `push_event`) and output (`draw_layers`/`flush`/...), which fits a
//! terminal process but not a window: there, an event loop owns input and a
//! renderer owns output. This crate splits the two apart and reassembles
//! them into one `Backend`:
//!
//! ```text
//!               ┌─────────────────────────────┐
//!               │     event loop (winit or     │
//!               │      a custom driver)        │
//!               └──────────────┬───────────────┘
//!                    translated events
//!                              │
//!                              v
//! ┌────────────────────────────────────────────────────┐
//! │            WindowBackend<P: Presenter>              │
//! │   (implements Backend: owns the input event queue,  │
//! │              delegates output to P)                 │
//! └───────────────────────┬──────────────────────────────┘
//!                         │ draw / flush / resize / present
//!                         v
//!         ┌───────────────────────────────┐
//!         │      P: Presenter              │
//!         │  (retroglyph-software today;   │
//!         │   wgpu/GL renderers planned)   │
//!         └───────────────────────────────┘
//! ```
//!
//! - [`Presenter`] is the output half: rasterization plus the surface
//!   lifecycle (`init_surface`/`resize_surface`/`present`/`cell_size`).
//!   Renderer crates implement only this trait.
//! - <code>[WindowBackend]&lt;P: Presenter&gt;</code> implements `Backend`
//!   generically, holding the input event queue and delegating output to
//!   `P`.
//! - The `winit` module (feature-gated, see below) drives the event loop
//!   that fills that queue and calls `Presenter::present` each frame.
//!
//! # Feature flags
//!
//! [`Presenter`], [`WindowBackend`], and [`WindowHandle`] depend only on
//! [`raw-window-handle`](raw_window_handle) and are always available. The
//! `winit` feature (default on) additionally provides the `winit` module:
//! the event loop, event translation, and the `run_windowed`/`run_app`
//! drivers. Disable it to implement or drive `Presenter` with a different
//! windowing library (SDL2, tao, a custom loop) without pulling in winit.

/// The generic [`Backend`](retroglyph_core::Backend) for windowed presenters.
pub mod backend;
/// The [`Presenter`] trait and [`WindowHandle`](presenter::WindowHandle).
pub mod presenter;
/// The winit event loop, event translation, and app drivers.
#[cfg(feature = "winit")]
pub mod winit;

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation -- see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use backend::WindowBackend;
pub use presenter::{Presenter, WindowHandle};

// Re-exported so presenters can name the handle traits without adding their
// own raw-window-handle dependency (and so versions can't drift apart).
pub use raw_window_handle;
