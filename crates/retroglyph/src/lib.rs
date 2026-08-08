//! `retroglyph`: a 2D pseudographic terminal library, one dependency and one `use`.
//!
//! Add this crate and you get the double-buffered `Terminal`/`App` game loop, styled cells, text
//! and layout helpers, and input events -- `app`, `color`, `event`, `frames`, `grid`, `layout`,
//! `surface`, `terminal`, `text`, `tile`, `symbols` -- plus a [`prelude`] with the handful of
//! names a program can't avoid, and one feature-gated module per backend (`crossterm`,
//! `software`, `gl`, `wgpu`, `ui`). Writing a new backend instead of a game? Depend on
//! [`retroglyph-core`](retroglyph_core) directly for its lower-level `backend`, `dev`, and `math`
//! modules.
//!
//! # Quick start
//!
//! ```sh
//! cargo add retroglyph
//! ```
//!
//! ```rust,no_run
//! use retroglyph::crossterm::Crossterm;
//! use retroglyph::prelude::*;
//!
//! struct Game;
//!
//! impl App<Crossterm> for Game {
//!     fn update(&mut self, term: &mut Terminal<Crossterm>, _frame: &Frame) -> Flow {
//!         term.surface().put((5, 5), '@', Style::new().fg(Color::GREEN));
//!
//!         if let Some(Event::Key(k)) = term.poll(std::time::Duration::from_secs(1)) {
//!             if k.code == KeyCode::Char('q') {
//!                 return Flow::Exit;
//!             }
//!         }
//!         Flow::Continue
//!     }
//! }
//!
//! fn main() -> std::io::Result<()> {
//!     retroglyph::app::run(Crossterm::new()?, Game)
//! }
//! ```
//!
//! Want a native window or a browser tab instead of a real terminal? Enable the `software`, `gl`,
//! or `wgpu` feature instead of (or alongside) `crossterm`: same `Terminal`/`App` contract, a
//! different `Backend` type. See each backend module's own docs (and
//! [`WindowConfig`]/[`run_app`]) for the windowed quick start.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! Default features: `crossterm`, `ui`.
//!
//! ### `crossterm`
//!
//! 🟢 Enabled by default.
//!
//! Re-exports `retroglyph-crossterm` as [`crossterm`]: a real-terminal `Backend`
//! via `crossterm`.
//!
//! ### `default-font`
//!
//! ⚪ Optional.
//!
//! Forwards each enabled backend's own `default-font` feature (an embedded Unscii 16 bitmap font),
//! so a caller doesn't need to know which backend crate actually owns it.
//!
//! ### `gl`
//!
//! ⚪ Optional.
//!
//! Re-exports `retroglyph-gl` as [`gl`]: a GPU `Backend` via `glow` (OpenGL 3.3 native,
//! WebGL2 wasm). Also pulls in the curated windowed re-exports (`WindowConfig`, `PresenterBuilder`,
//! `Windowed`, `WindowedLaunchError`, `run_app`, `run_app_on`).
//!
//! ### `software`
//!
//! ⚪ Optional.
//!
//! Re-exports `retroglyph-software` as [`software`]: a CPU pixel `Backend` via
//! `softbuffer`. Also pulls in the curated windowed re-exports (`WindowConfig`, `PresenterBuilder`,
//! `Windowed`, `WindowedLaunchError`, `run_app`, `run_app_on`).
//!
//! ### `testing`
//!
//! ⚪ Optional.
//!
//! Enables [`TestHarness`] and its error, the published headless `App` driver for testing your own
//! `App`. Forwards to `retroglyph-core`'s own `testing` feature.
//!
//! ### `tracing`
//!
//! ⚪ Optional.
//!
//! Forwards to `retroglyph-crossterm`'s `tracing` feature: instruments `draw`/`flush`/`poll_event`
//! with `tracing` spans for profiling render/input time.
//!
//! ### `ui`
//!
//! 🟢 Enabled by default.
//!
//! Re-exports `retroglyph-ui` as [`ui`]: the immediate-mode widget/layout toolkit.
//!
//! ### `wgpu`
//!
//! ⚪ Optional.
//!
//! Re-exports `retroglyph-wgpu` as [`wgpu`]: a GPU `Backend` via `wgpu` (Vulkan,
//! Metal, D3D12, WebGPU). Also pulls in the curated windowed re-exports (`WindowConfig`,
//! `PresenterBuilder`, `Windowed`, `WindowedLaunchError`, `run_app`, `run_app_on`).
//! <!-- gen-features:end -->
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Compile the code blocks in both this crate's own README and the workspace root README as
// doctests so the quick-start examples are type-checked on every test run and cannot silently
// rot. The `cfg(doctest)` gate keeps these out of the rendered crate documentation. The workspace
// root README's quick start now demonstrates this crate (previously `retroglyph-crossterm`,
// before this crate existed), so it's doctested here instead.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct WorkspaceReadmeDoctests;

pub use retroglyph_core::{
    app, backend, color, event, frames, grid, layout, surface, symbols, terminal, text, tile,
};

/// The trait a backend's own builder implements to be driven end to end by
/// [`launch`](Launch::launch): the user names the backend (`CrosstermOptions`, `Windowed<B>`, ...)
/// and gets back that backend's own unwrapped error, rather than a facade-wide one. See this
/// trait's own docs for why there is no unified error here (`retroglyph::LaunchError`, tracked by
/// #430, is only needed if the facade grows an entry point that can return either backend's
/// error).
pub use retroglyph_core::app::Launch;

// `retroglyph_core::testing` also holds `conformance` (`Observable`, `assert_output_contract`,
// ...), the harness a *new backend* uses to prove it satisfies the `Output`/`Input`/`Cursor`
// contracts. That's backend-author surface, not game-author surface, so only the two
// game-facing items are re-exported here, individually, rather than the whole module.
//
// `RunError` is re-exported by its own name rather than a facade-specific alias: a future
// unified driver error (`LaunchError`, tracked by #430) must not also be named `RunError`, or the
// two would collide right here at `retroglyph::RunError`.
#[cfg(feature = "testing")]
pub use retroglyph_core::testing::{RunError, TestHarness};

// A `pub mod` line's own doc comment and its target module's inner `//!` doc concatenate into one
// rendered page (see `retroglyph-core`'s matching comment in its own `lib.rs`), and the combined
// first paragraph here grows past this noisy nursery lint's threshold.
#[allow(clippy::too_long_first_doc_paragraph)]
/// The names a program cannot avoid, glob-importable in one line.
pub mod prelude;

/// A real-terminal [`Backend`](retroglyph_core::backend::Backend) via
/// [`crossterm`](https://crates.io/crates/crossterm).
#[cfg(feature = "crossterm")]
pub use retroglyph_crossterm as crossterm;
/// A GPU [`Backend`](retroglyph_core::backend::Backend) via
/// [`glow`](https://crates.io/crates/glow): OpenGL 3.3 (native) and WebGL2 (wasm).
#[cfg(feature = "gl")]
pub use retroglyph_gl as gl;
/// A CPU pixel [`Backend`](retroglyph_core::backend::Backend) via
/// [`softbuffer`](https://crates.io/crates/softbuffer).
#[cfg(feature = "software")]
pub use retroglyph_software as software;
/// The immediate-mode widget/layout toolkit: panels, gauges, tables, input/focus, theming,
/// animation.
#[cfg(feature = "ui")]
pub use retroglyph_ui as ui;
/// A GPU [`Backend`](retroglyph_core::backend::Backend) via
/// [`wgpu`](https://crates.io/crates/wgpu): Vulkan, Metal, D3D12, and WebGPU.
#[cfg(feature = "wgpu")]
pub use retroglyph_wgpu as wgpu;

// The curated windowed surface (issue #1203): a windowed backend's own quick-start needs a way
// to build a window and drive its event loop, without reaching past this crate into
// `retroglyph-window` for the whole `winit` module. `run_windowed`/`run_windowed_with_proxy`/
// `run_windowed_with_typed_proxy`/`run_app_with_proxy`/`run_app_with_typed_proxy` stay
// reachable only through `retroglyph_window::winit` directly: they're cross-thread event
// injection power tools, not quick-start material.
#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
pub use retroglyph_window::presenter_builder::PresenterBuilder;
#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
pub use retroglyph_window::winit::{
    WindowConfig, Windowed, WindowedLaunchError, run_app, run_app_on,
};
