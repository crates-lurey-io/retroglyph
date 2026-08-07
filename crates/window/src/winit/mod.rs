//! Everything winit-specific in this crate: the event loop, event translation, and the windowed
//! app drivers.
//!
//! A driver for another windowing library (SDL2, tao, a custom loop) would be a sibling module
//! with the same shape: create a window, translate its events into
//! [`Event`](retroglyph_core::event::Event)s pushed onto [`WindowBackend`](crate::WindowBackend),
//! and call [`Presenter::present`](crate::Presenter::present) once per frame.

/// The winit event loop, `WindowConfig`, and the `run_windowed`/`run_app` drivers.
pub mod run;
/// winit-event -> retroglyph-event converters.
pub mod translate;
/// wasm32-only DPR/viewport/pointer-rescaling helpers used internally by `run`.
mod web;

pub use run::{
    EventProxy, EventProxyClosed, WindowConfig, run_app, run_app_on, run_app_with_proxy,
    run_app_with_typed_proxy, run_windowed, run_windowed_with_proxy, run_windowed_with_typed_proxy,
};
pub use windowed::{Windowed, WindowedLaunchError};

/// [`Windowed`], the [`Launch`](retroglyph_core::app::Launch) wrapper pairing a windowed
/// backend's `PresenterBuilder` with a window title.
mod windowed;

// Re-exported so downstream crates can name the error returned by the drivers above without
// adding their own winit dependency (and so versions can't drift apart).
pub use ::winit::error::EventLoopError;
