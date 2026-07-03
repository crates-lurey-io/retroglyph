//! Shared support code for retroglyph's examples: a small game-loop adapter,
//! the `rg_run!`/`rg_run_software!` backend-selection macros, and demo helpers
//! (FOV, RNG, input maps, perf counters).
//!
//! Not published (`publish = false`). The `[[example]]` targets, integration
//! tests, and benches in this crate exercise the workspace's real crates
//! (`retroglyph-core`, `-crossterm`, `-software`, `-window`, `-widgets`) the
//! same way a downstream user would.

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

pub mod util;
