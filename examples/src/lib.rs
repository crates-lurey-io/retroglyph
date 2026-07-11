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
//!     fn tick<B: retroglyph_core::Backend>(&mut self, term: &mut retroglyph_core::Terminal<B>) -> bool { todo!() }
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

mod launch;
pub mod util;
mod wasm_entry;

pub use launch::{Example, render_headless_frames};

pub use launch::launch;
#[cfg(feature = "crossterm")]
pub use launch::run_crossterm;
pub use launch::run_headless_stdout;
#[cfg(feature = "software")]
pub use launch::run_software;
