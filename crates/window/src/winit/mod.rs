//! The winit integration: event loop, event translation, and the windowed
//! app drivers.
//!
//! Everything winit-specific in this crate lives here, behind the `winit`
//! feature (default on). The seam ([`Presenter`](crate::Presenter),
//! [`WindowBackend`](crate::WindowBackend),
//! [`WindowHandle`](crate::WindowHandle)) is winit-free; an integration for
//! another windowing library (SDL2, tao, a custom loop) would be a sibling
//! module with the same shape: create a window, translate events into
//! [`Event`](retroglyph_core::event::Event)s pushed onto the
//! [`WindowBackend`](crate::WindowBackend) queue, and drive
//! [`Presenter::present`](crate::Presenter::present) once per frame.

/// The winit event loop, `WindowConfig`, and the `run_windowed`/`run_app` drivers.
pub mod run;
/// winit-event -> retroglyph-event converters.
pub mod translate;

pub use run::{WindowConfig, run_app, run_windowed};
