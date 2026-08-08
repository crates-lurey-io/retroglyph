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

// ── `run_default`: one call, no per-app `#[cfg]` table ──────────────────────────────
//
// Four near-identical items below, one per backend, `#[cfg]`-gated so exactly one is compiled
// for any given feature set (never zero, never more than one: each arm excludes every
// higher-priority feature via `not(any(...))`, the same shape `retroglyph-examples`' own
// `launch::<E>()` dispatch table already uses). A single generic function spanning every
// backend isn't expressible instead: `Launch::Backend` differs per implementor, and stable Rust
// has no way to write "`A` implements `App<B>` for whichever `B` this arm turns out to be" as
// one bound (no non-lifetime higher-ranked trait bounds; see rust#108185, and `Launch`'s own
// doc comment above for the same limitation). Priority: `software` > `gl` > `wgpu` >
// `crossterm`, matching the tile-demo gallery's dispatch table this replaces (retroglyph#1295),
// with `wgpu` slotted between the other two windowed backends and the real-terminal one.

/// Picks a backend from this crate's enabled Cargo features and drives `app` on it, using that
/// backend's own default configuration.
///
/// See the priority order above. Each backend is built with [`PresenterBuilder::new()`] for a
/// windowed backend, paced by [`RunOptions::default()`](app::RunOptions), or
/// [`Crossterm::builder()`](crossterm::Crossterm::builder) for the terminal one.
///
/// This is the zero-config fast path, not a general-purpose driver: an app that needs a specific
/// grid size, window title, tileset, or pacing has outgrown "default" and should call that
/// backend's own [`Launch::launch`] directly instead, e.g. `Windowed::new(builder, "My
/// Game").launch(app, RunOptions::animated(60))` or `Crossterm::builder().launch(app, options)`.
///
/// Absent entirely (not a compile error) when none of `crossterm`, `software`, `gl`, or `wgpu`
/// is enabled: there is no backend left to pick.
///
/// # Errors
///
/// Returns the picked backend's own [`Launch::Error`] if it fails to launch; see that backend's
/// `Launch` impl for the exact conditions (a real terminal that can't enter raw mode, a
/// presenter that fails to build, or a window/event loop that fails to start or run).
#[cfg(feature = "software")]
pub fn run_default<A>(
    app: A,
) -> Result<(), <Windowed<software::config::SoftwareBackendBuilder> as Launch>::Error>
where
    A: app::App<<Windowed<software::config::SoftwareBackendBuilder> as Launch>::Backend> + 'static,
{
    Windowed::new(
        software::config::SoftwareBackendBuilder::new(),
        "retroglyph",
    )
    .launch(app, app::RunOptions::default())
}

/// See [`run_default`]'s `software`-enabled overload. `gl` is the other GPU windowed backend;
/// `software` wins if both happen to be enabled.
///
/// # Errors
///
/// See [`run_default`]'s `software`-enabled overload.
#[cfg(all(feature = "gl", not(feature = "software")))]
pub fn run_default<A>(
    app: A,
) -> Result<(), <Windowed<gl::config::GlBackendBuilder> as Launch>::Error>
where
    A: app::App<<Windowed<gl::config::GlBackendBuilder> as Launch>::Backend> + 'static,
{
    Windowed::new(gl::config::GlBackendBuilder::new(), "retroglyph")
        .launch(app, app::RunOptions::default())
}

/// See [`run_default`]'s `software`-enabled overload. `wgpu` is the other GPU windowed backend;
/// it loses to `software`/`gl` if either is also enabled.
///
/// # Errors
///
/// See [`run_default`]'s `software`-enabled overload.
#[cfg(all(feature = "wgpu", not(any(feature = "software", feature = "gl"))))]
pub fn run_default<A>(
    app: A,
) -> Result<(), <Windowed<wgpu::config::WgpuBackendBuilder> as Launch>::Error>
where
    A: app::App<<Windowed<wgpu::config::WgpuBackendBuilder> as Launch>::Backend> + 'static,
{
    Windowed::new(wgpu::config::WgpuBackendBuilder::new(), "retroglyph")
        .launch(app, app::RunOptions::default())
}

/// See [`run_default`]'s `software`-enabled overload. `crossterm` is the real-terminal backend;
/// it loses to any windowed backend that's also enabled.
///
/// # Errors
///
/// See [`run_default`]'s `software`-enabled overload.
///
/// # Examples
///
/// ```no_run
/// use retroglyph::crossterm::Crossterm;
/// use retroglyph::prelude::*;
///
/// struct Game;
///
/// impl App<Crossterm> for Game {
///     fn update(&mut self, term: &mut Terminal<Crossterm>, _frame: &Frame) -> Flow {
///         term.surface().put((5, 5), '@', Style::new().fg(Color::GREEN));
///         Flow::Continue
///     }
/// }
///
/// fn main() -> Result<(), std::io::Error> {
///     retroglyph::run_default(Game)
/// }
/// ```
#[cfg(all(
    feature = "crossterm",
    not(any(feature = "software", feature = "gl", feature = "wgpu"))
))]
pub fn run_default<A>(app: A) -> Result<(), <crossterm::CrosstermOptions as Launch>::Error>
where
    A: app::App<<crossterm::CrosstermOptions as Launch>::Backend> + 'static,
{
    crossterm::Crossterm::builder().launch(app, app::RunOptions::default())
}
