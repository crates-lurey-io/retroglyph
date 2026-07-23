//! The [`Presenter`] trait: what a renderer crate implements to rasterize a grid and present it
//! to a window surface.
//!
//! `Presenter` is an [`Output`](retroglyph_core::backend::Output) supertrait plus window-surface
//! operations, with no input methods: the event loop owns input, and
//! [`WindowBackend`](crate::WindowBackend) forwards translated events into its own queue instead.
//!
//! | Presenter | `present()` | `init_surface()` |
//! |---|---|---|
//! | `SoftwareRenderer` (retroglyph-software) | Copies pixel buffer to softbuffer surface | Creates `softbuffer::Context` + `Surface` |
//! | `WgpuRenderer` (future) | Submits render pass + presents swap chain | Creates `wgpu::Surface` + `Device` |
//! | `GlRenderer` (future) | Draws full-screen quad + swaps buffers | Creates GL context from the window |
//!
//! See the crate-level docs (`crate` root, "DPI, scale, and the resize contract" and
//! "Threading model" sections) for the physical-pixel/no-auto-scaling contract on
//! [`cell_size`](Presenter::cell_size), the sub-cell-remainder behavior on
//! [`resize_surface`](Presenter::resize_surface), and the single-threaded execution model
//! every `Presenter` implementation runs under.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use retroglyph_core::backend::Output;
use std::sync::Arc;

/// A window/display handle pair, as one trait.
///
/// Presenters receive [`raw-window-handle`](raw_window_handle) types, not a concrete
/// `winit::window::Window`: softbuffer, wgpu, and glutin all accept these handles directly, so
/// any windowing library that produces them can drive the same presenter, and only this crate
/// depends on winit itself.
///
/// `raw-window-handle` has no combined trait, and surface libraries need to *own* the handle
/// (softbuffer stores it for the surface's lifetime), so presenters receive `Arc<dyn
/// WindowHandle>` -- rwh implements the handle traits for `Arc<H: ?Sized>`, so the trait object
/// passes straight into `softbuffer::Surface::new` / `wgpu::Instance::create_surface`.
pub trait WindowHandle: HasWindowHandle + HasDisplayHandle {}

impl<T: HasWindowHandle + HasDisplayHandle + ?Sized> WindowHandle for T {}

/// A surface-lifecycle error that can optionally signal whether it's worth retrying.
///
/// [`Presenter::SurfaceError`] is a per-implementation associated type: softbuffer's error enum
/// has no `Lost`/`Outdated`/`Timeout` discrimination the way `wgpu::SurfaceError` does, so today's
/// only backend (`SoftwareRenderer`) has no structured way to say "this specific failure is
/// fatal, don't bother retrying." [`is_recoverable`](Self::is_recoverable) is that hook: a
/// presenter with real error categories can override it to return `false` for a truly fatal
/// failure, while every presenter that doesn't need the distinction (including every backend that
/// exists in this crate today) can implement this trait with an empty body and inherit the
/// default `true`.
///
/// Deliberately not blanket-implemented for every `Debug + Display` type: that would make it
/// impossible for any concrete error type to override [`is_recoverable`](Self::is_recoverable) at
/// all (a specific `impl` would conflict with the blanket one), defeating the point of the trait.
/// Instead, each `SurfaceError` type needs one explicit (and usually empty) `impl
/// RecoverableError for ...` block -- see `retroglyph_software`'s `SurfaceError` for the minimal
/// case that just inherits the default.
pub trait RecoverableError: core::fmt::Debug + core::fmt::Display {
    /// Whether this error represents a transient failure worth retrying, as opposed to a fatal
    /// one.
    ///
    /// Defaults to `true`: absent any structured error categorization, every failure is treated
    /// as potentially transient, matching the generic consecutive-failure recovery heuristic
    /// `winit::run::present_failure_action` already applies. Override to return `false` only for
    /// an error variant known to be unrecoverable regardless of retries (e.g. a `wgpu::SurfaceError
    /// ::Lost` variant that persists until the surface is fully rebuilt from a different code
    /// path than a simple retry).
    #[must_use]
    fn is_recoverable(&self) -> bool {
        true
    }
}

// `Infallible` is uninhabited -- no value of it can ever exist, so `is_recoverable` can never
// actually be called on one -- but a presenter that can't fail (e.g. a test mock) still needs
// `type SurfaceError = core::convert::Infallible` to satisfy the `RecoverableError` bound, so
// this impl exists purely for that convenience.
impl RecoverableError for core::convert::Infallible {}

/// A renderer that rasterizes grid content and presents it to a window surface.
///
/// A supertrait of [`Output`], adding the surface lifecycle (`init_surface`, `resize_surface`,
/// `present`, `cell_size`) that the event loop drives. Every `Presenter` implementation is an
/// `Output` implementation for free: [`WindowBackend`](crate::WindowBackend) delegates its own
/// `Output` impl straight through to `P: Presenter`, with no duplicated method bodies.
pub trait Presenter: Output {
    /// Surface lifecycle error (context creation, buffer acquisition,
    /// present).
    type SurfaceError: RecoverableError;

    /// Initialize the window surface.
    ///
    /// Called once from the loop's `resumed` handler. The presenter creates
    /// its platform surface (softbuffer surface, wgpu device+surface, GL
    /// context) from the raw window/display handles.
    ///
    /// # Errors
    ///
    /// Returns [`Self::SurfaceError`] if surface or context creation fails.
    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), Self::SurfaceError>;

    /// Resize the window surface to a new physical pixel size.
    ///
    /// Called on every window resize event with `width`/`height` already resolved by the
    /// caller -- for the `winit` driver (see `winit::run::WindowApp::resize_to`), that means
    /// `cols * cell_w` x `rows * cell_h`, where `cols`/`rows` are the window's physical size
    /// divided down to whole cells. Any sub-cell remainder is truncated, not centered or
    /// cleared: when the window's physical size isn't an exact multiple of the cell size,
    /// `width`/`height` here are the largest whole-cell-multiple that fits, which can be
    /// smaller than the window's actual physical size. The OS window itself is never resized
    /// to compensate, so a non-exact-multiple resize leaves a thin strip at the window's
    /// trailing edge outside the surface -- retroglyph does not paint or clear that strip;
    /// whatever the OS/windowing backend leaves there remains visible until a subsequent
    /// resize covers it.
    fn resize_surface(&mut self, width: u32, height: u32);

    /// Notify the presenter that the window's scale factor (DPI) changed.
    ///
    /// Called when the window moves to a display with a different pixel density, or the
    /// system DPI setting changes. The event loop follows this with
    /// [`resize_surface`](Self::resize_surface) for the window's new physical size, so
    /// this hook only needs to handle DPI-dependent state that isn't a plain buffer
    /// resize (e.g. regenerating a font atlas rasterized for a particular scale).
    ///
    /// Defaults to a no-op: presenters whose rasterization doesn't depend on DPI (like
    /// `SoftwareRenderer`'s integer `scale` config, set once at construction) need no
    /// action here.
    fn scale_factor_changed(&mut self, _scale_factor: f64) {}

    /// Present the rasterized frame to the window surface.
    ///
    /// Called after each app tick. A lost frame is not fatal; the caller
    /// logs the error and continues.
    ///
    /// # Errors
    ///
    /// Returns [`Self::SurfaceError`] if the surface buffer can't be acquired
    /// or presented (e.g. context lost on wasm, page flip pending on
    /// DRI/KMS).
    fn present(&mut self) -> Result<(), Self::SurfaceError>;

    /// Cell size in physical pixels `(width, height)`.
    ///
    /// Physical pixels, not logical/DPI-scaled pixels, and never auto-scaled by this crate for
    /// display DPI -- see the crate-level "DPI, scale, and the resize contract" docs. A presenter
    /// whose cells should grow on a `HiDPI` display must change what this returns itself (from
    /// [`resize`](Output::resize) or [`scale_factor_changed`](Self::scale_factor_changed)); absent
    /// that, it stays constant for the presenter's lifetime.
    ///
    /// `(u32, u32)` rather than [`Size`](retroglyph_core::grid::Size) because grid coordinates
    /// are `u16` but pixel arithmetic uses `u32` (winit `PhysicalSize`).
    #[must_use]
    fn cell_size(&self) -> (u32, u32);
}
