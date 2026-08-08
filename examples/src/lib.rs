//! Shared support code for retroglyph's examples: the [`Example`] trait,
//! `launch::<E>()` backend dispatch, and the [`wasm_entry!`]/[`example_main!`]
//! FFI-codegen macros.
//!
//! Not published (`publish = false`). The `[[example]]` targets in this
//! crate exercise the workspace's real crates (`retroglyph-core`,
//! `-crossterm`, `-software`, `-window`, `-terminal-wasm`) the same way a
//! downstream user would.
//!
//! Every example follows the same shape:
//!
//! ```ignore
//! #[derive(Default)] // skip this (and Example::init's default body) if MyExample needs
//!                     // backend-dependent startup state -- see Example::init's doc comment
//! struct MyExample { /* state */ }
//!
//! impl retroglyph_examples::Example for MyExample {
//!     const NAME: &'static str = "my_example";
//!     fn tick<B: retroglyph_core::backend::Backend>(&mut self, term: &mut retroglyph_core::terminal::Terminal<B>) -> bool { todo!() }
//! }
//!
//! retroglyph_examples::example_main!(MyExample);
//! ```

// Internal example-support code, not a public API. Under the old layout this
// lived inside each example *binary* (via `mod util;`), where these
// published-API-hygiene lints never fired; as a shared *lib* they otherwise
// would. Not worth the ceremony for throwaway demo helpers.
#![allow(
    missing_docs,
    clippy::must_use_candidate,
    clippy::missing_panics_doc,
    rustdoc::broken_intra_doc_links,
    rustdoc::private_intra_doc_links
)]

// Only the live backends have a frame rate worth reporting: the headless paths are frame-stepped
// at a fixed synthetic delta, and nothing there constructs the `ExampleApp` driver that draws it.
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
mod fps;
mod launch;
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
mod perf_overlay;
pub mod util;
mod wasm_entry;

pub use launch::{Example, HEADLESS_FRAME_DELTA, render_headless_frames};

pub use launch::launch;
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
pub use launch::render_perf_overlay_rgb;
#[cfg(feature = "crossterm")]
pub use launch::run_crossterm;
#[cfg(feature = "gl")]
pub use launch::run_gl;
pub use launch::run_headless_stdout;
#[cfg(feature = "wgpu")]
pub use launch::run_wgpu;
#[cfg(feature = "software")]
pub use launch::{run_software, run_software_with};
