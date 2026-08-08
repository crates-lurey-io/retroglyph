//! Input recording & replay for [retroglyph](https://github.com/crates-lurey-io/retroglyph).
//!
//! Two things live here, both built on `retroglyph-core`'s `testing` feature
//! ([`retroglyph_core::testing::InputRecording`]/[`TestHarness`](retroglyph_core::testing::TestHarness)):
//!
//! - [`InputRecorder`]: wraps any [`Backend`](retroglyph_core::backend::Backend) and taps its
//!   input stream, so a real session (a bug report, a demo run) can be captured to a `.rgrec`
//!   file via [`InputRecorder::save`].
//! - [`replay_live`]: drives a saved recording forward into a live `Terminal`/`App`, once, at
//!   recorded pace -- "watch it happen" on screen. For driving a recording back through a
//!   headless `App` in a test instead (no real backend, no wall-clock sleeping), use
//!   [`TestHarness::replay`](retroglyph_core::testing::TestHarness::replay) directly.
//!
//! [`install_panic_recorder`] wires a [`RecorderHandle`] into `std::panic`'s hook, so an
//! in-progress recording is saved automatically if the process panics instead of being lost.
//!
//! Not here: rendering a recording to an asciicast/GIF, or capturing raw terminal frames (ANSI
//! output) rather than input events -- that's the frame recorder, a separate, not-yet-built
//! crate (see retroglyph#1267's follow-up issue).

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation; see `retroglyph-core`'s matching include for the same
// pattern.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod input_recorder;
mod panic_hook;
mod replay;
mod rgrec;

pub use input_recorder::{InputRecorder, RecorderHandle};
pub use panic_hook::install_panic_recorder;
pub use replay::replay_live;
pub use rgrec::{read, write};
