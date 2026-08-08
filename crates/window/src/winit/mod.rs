//! Everything winit-specific in this crate: the event loop, event translation, and the windowed
//! app drivers.
//!
//! A driver for another windowing library (SDL2, tao, a custom loop) would be a sibling module
//! with the same shape: create a window, translate its events into
//! [`Event`](retroglyph_core::event::Event)s pushed onto [`WindowBackend`](crate::backend::WindowBackend),
//! and call [`Presenter::present`](crate::presenter::Presenter::present) once per frame.

/// [`EventLoopError`], the opaque wrapper around winit's own event-loop error.
mod error;
/// The winit event loop, `WindowConfig`, and the `run_windowed`/`run_app` drivers.
pub mod run;
/// winit-event -> retroglyph-event converters.
pub mod translate;
/// wasm32-only DPR/viewport/pointer-rescaling helpers used internally by `run`.
mod web;

pub use error::EventLoopError;
pub use run::{
    EventProxy, EventProxyClosed, WindowConfig, run_app, run_app_on, run_app_with_proxy,
    run_app_with_typed_proxy, run_windowed, run_windowed_with_proxy, run_windowed_with_typed_proxy,
};
pub use windowed::{Windowed, WindowedLaunchError};

/// [`Windowed`], the [`Launch`](retroglyph_core::app::Launch) wrapper pairing a windowed
/// backend's `PresenterBuilder` with a window title.
mod windowed;
