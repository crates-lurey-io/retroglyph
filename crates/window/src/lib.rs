//! A shared layer for window-based backends (software, GL, wgpu).
//!
//! # Architecture
//!
//! [`retroglyph_core::backend::Input`] and [`retroglyph_core::backend::Output`] are two
//! independent facets of [`Backend`](retroglyph_core::backend::Backend), which fits a terminal process
//! (one type implements both) but not a window: there, an event loop owns input and a renderer
//! owns output separately. This crate keeps that split ([`Presenter`] is an `Output` supertrait,
//! [`WindowBackend`] owns its own `Input` event queue) and reassembles both into one `Backend`:
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
//! - [`Presenter`] is `Output` plus the surface lifecycle
//!   (`init_surface`/`resize_surface`/`present`/`cell_size`). Renderer crates implement only this
//!   trait, which gives them `Output` for free.
//! - <code>[WindowBackend]&lt;P: Presenter&gt;</code> implements `Output` (by delegating to `P`),
//!   `Input` (via its own event queue), and the no-op default `Cursor` (windowed backends have no
//!   text cursor), which together give it `Backend` generically.
//! - The `winit` module (feature-gated, see below) drives the event loop that fills that queue
//!   and calls `Presenter::present` each frame.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! Default features: `winit`.
//!
//! ### `default-font`
//!
//! ⚪ Optional.
//!
//! Embeds the Unscii 16 default font (`font::unscii16`).
//!
//! Off by default so a consumer that supplies its own bitmap font pays nothing for the ~4 KB atlas;
//! the graphical backends' own `default-font` features forward to this one.
//!
//! ### `dev`
//!
//! ⚪ Optional.
//!
//! Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build
//! that would otherwise compile them out (see [`retroglyph_core::dev`]).
//!
//! Forwarded so a consumer of this crate can turn them on without adding a direct dependency on
//! core just to reach the flag.
//!
//! ### `legacy-computing`
//!
//! ⚪ Optional.
//!
//! Embeds a generated block-elements/braille fallback font (`font::legacy_computing`): the 10
//! quadrant, 60 sextant, and 256 braille glyphs CP437 (and so `unscii16`) has no mapping for.
//!
//! A separate opt-in from `default-font` rather than folded into it: this repertoire is a much more
//! niche/specialized addition (subcell image rendering, braille density tricks) than the base text
//! font, so a consumer that only wants CP437 text shouldn't pay for it. Computed at compile time by
//! a `const fn`, so this adds no font asset and no new dependency.
//!
//! ### `tilesets`
//!
//! ⚪ Optional.
//!
//! Shared PNG sprite/tileset support (`tileset` + `sprite_cache` modules, issue #366).
//!
//! Both graphical backends' own `tilesets` features forward to this one.
//!
//! ### `winit`
//!
//! 🟢 Enabled by default.
//!
//! The winit event loop and event translation (`run`, `translate`, `run_windowed`/`run_app`).
//!
//! Renderer crates that only implement [`Presenter`] can disable this and depend solely on
//! `raw-window-handle`; loops other than winit (SDL2, tao, custom) bring their own driver against
//! `Presenter` + `WindowBackend`.
//! <!-- gen-features:end -->
//!
//! # Feature flags
//!
//! [`Presenter`], [`WindowBackend`], and [`WindowHandle`] depend only on
//! [`raw-window-handle`](raw_window_handle) and are always available. The `winit` feature
//! (default on) additionally provides the `winit` module: the event loop, event translation, and
//! the `run_windowed`/`run_app` drivers. Disable it to implement or drive `Presenter` with a
//! different windowing library (SDL2, tao, a custom loop) without pulling in winit.
//!
//! # DPI, scale, and the resize contract
//!
//! [`Presenter::cell_size`] returns the cell size in **physical pixels** (the same pixel
//! space as `winit::dpi::PhysicalSize`), not logical/DPI-scaled ("CSS" or "point") pixels.
//! This crate performs no automatic DPI scaling of it: nothing here changes `cell_size()` in
//! response to a display's scale factor. `SoftwareRenderer`'s cell size, for example, is
//! fixed at construction (glyph size × its integer `scale` config) and never changes on a
//! [`Presenter::scale_factor_changed`] notification. A presenter that wants larger cells on a
//! `HiDPI` display has to opt into that itself from `scale_factor_changed` (e.g. regenerating a
//! font atlas at a new pixel density); until one does, the grid renders at a fixed physical
//! pixel size on every display, `HiDPI` or not.
//!
//! Window resize is clamped to whole cells: a physical size that isn't an exact multiple of
//! `cell_size()` has its sub-cell remainder truncated, not centered or cleared, and the OS
//! window is never resized to compensate: see [`Presenter::resize_surface`]'s doc comment
//! for the full contract, including the unpainted trailing strip this can leave on screen.
//!
//! # Threading model
//!
//! The windowed drivers (`winit::run_windowed`, `winit::run_app`, and their `_with_proxy`
//! variants) are single-threaded: the event loop, every [`Presenter`] call, and the app
//! closure/[`App`](retroglyph_core::app::App) callback all run on the one thread that calls
//! `run_windowed`/`run_app`: the main thread, on platforms (e.g. macOS) that require it for
//! windowing. Neither [`Presenter`] nor [`WindowBackend`] carries a `Send`/`Sync` bound
//! anywhere in this crate, and a presenter is free to hold thread-affine state accordingly
//! (an `Rc`, a non-`Send` GPU context handle). The only supported way to reach the loop from
//! another thread is `winit::EventProxy<T>`, which is `Send + Sync + Clone` for any
//! `T: Send + 'static`: it does not give another thread direct access to the `Presenter` or
//! `Terminal`. With the default `T = u64` (`winit::run_windowed_with_proxy`/
//! `run_app_with_proxy`), the payload surfaces as an opaque
//! [`Event::Custom`](retroglyph_core::event::Event::Custom); a custom `T`
//! (`winit::run_windowed_with_typed_proxy`/`run_app_with_typed_proxy`) bypasses `Event` entirely
//! and goes straight to a caller-supplied handler, since `Event::Custom` itself stays fixed to
//! `u64`.

#![doc(html_logo_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg")]
#![doc(html_favicon_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg")]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod atlas;
// clippy::too_long_first_doc_paragraph is a known-noisy nursery lint (rust-lang/rust-clippy#13441):
// it mis-attributes its span across this outer doc comment plus `backend`'s own inner module doc,
// which grew past the threshold once its intra-doc links became fully qualified (retroglyph#1035).
#[allow(clippy::too_long_first_doc_paragraph)]
/// The generic [`Backend`](retroglyph_core::backend::Backend) for windowed presenters.
pub mod backend;
/// System clipboard read/write ([`Clipboard`], [`SystemClipboard`] on native targets).
pub mod clipboard;
pub mod font;
/// Shared cell/surface pixel geometry ([`CellGeometry`](geometry::CellGeometry)).
pub mod geometry;
/// Canonical default colors ([`DEFAULT_FG`](palette::DEFAULT_FG),
/// [`DEFAULT_BG`](palette::DEFAULT_BG)) shared by the graphical backends.
pub mod palette;
/// The [`Presenter`] trait and [`WindowHandle`](presenter::WindowHandle).
pub mod presenter;
#[cfg(feature = "tilesets")]
pub mod sprite_cache;
#[cfg(feature = "tilesets")]
pub mod tileset;
/// Locates winit's `<canvas>` element via the DOM ([`web::winit_canvas`]).
#[cfg(target_arch = "wasm32")]
pub mod web;
/// The winit event loop, event translation, and app drivers.
#[cfg(feature = "winit")]
pub mod winit;

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation: see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use backend::WindowBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use clipboard::SystemClipboard;
pub use clipboard::{Clipboard, ClipboardError};
pub use geometry::CellGeometry;
pub use presenter::{
    GenericSurfaceError, Presenter, RecoverableError, WindowHandle, cell_art_glyph,
};

// Re-exported so presenters can name the handle traits without adding their
// own raw-window-handle dependency (and so versions can't drift apart).
pub use raw_window_handle;
