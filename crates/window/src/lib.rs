//! retroglyph-window: the shared winit windowing layer for retroglyph's
//! windowed backend family (software today; wgpu/GL planned).
//!
//! # Architecture (ADR 014, ADR 015)
//!
//! The [`Backend`](retroglyph_core::Backend) trait fuses input
//! (`poll_event`/`push_event`) and output (`draw_layers`/`flush`/...). That is
//! right for terminal backends, where one process owns both. In a windowed
//! backend the winit event loop owns input and a per-renderer surface owns
//! output, so this crate splits them:
//!
//! - [`Presenter`] is the output half plus the surface seam
//!   (`init_surface`/`resize_surface`/`present`/`cell_size`). Renderer crates
//!   (`retroglyph-software`, future `retroglyph-wgpu`/`retroglyph-gl`)
//!   implement only this.
//! - <code>[WindowBackend]&lt;P: Presenter&gt;</code> implements `Backend` generically: it
//!   owns the input event queue (filled by the winit loop, drained by
//!   `poll_event`) and delegates output to `P`.
//! - [`winit::run_windowed`] / [`winit::run_app`] own the winit `EventLoop`
//!   (native) or the `requestAnimationFrame`-driven loop (wasm), translate
//!   winit events via [`winit::translate`], and drive the app.
//!
//! This is the same crate shape as egui's `egui-winit` (event translation +
//! window glue) with the painter seam (`egui_glow`/`egui-wgpu`) folded into
//! one [`Presenter`] trait, sized for retroglyph's single-window scope.
//!
//! # Feature flags
//!
//! - `winit` (default) -- the [`winit`] module: event loop, event
//!   translation, and the `run_windowed`/`run_app` drivers. The seam itself
//!   ([`Presenter`], [`WindowBackend`], [`WindowHandle`]) is always available
//!   and depends only on [`raw-window-handle`](raw_window_handle): renderer
//!   crates that *implement* the seam can disable this feature and stay
//!   winit-free, and non-winit loops (SDL2, tao, custom) can drive the same
//!   presenters.

/// `WindowBackend<P>`: the generic `Backend` for windowed presenters.
pub mod backend;
/// The `Presenter` trait: rasterization + surface seam for renderer crates.
pub mod presenter;
/// The winit integration: event loop, event translation, and app drivers.
#[cfg(feature = "winit")]
pub mod winit;

pub use backend::WindowBackend;
pub use presenter::{Presenter, WindowHandle};

// Re-exported so presenters can name the handle traits without adding their
// own raw-window-handle dependency (and so versions can't drift apart).
pub use raw_window_handle;
