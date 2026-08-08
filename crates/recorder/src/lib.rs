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
//! A different pair records "what was shown" instead of "what happened":
//!
//! - [`FrameRecorder`]: wraps any [`Output`](retroglyph_core::backend::Output) backend and taps
//!   its [`DrawCell`](retroglyph_core::backend::DrawCell) diff stream, buffering owned
//!   [`CapturedFrame`]s.
//! - [`write_cast`]: exports those frames as
//!   [asciicast v3](https://docs.asciinema.org/manual/asciicast/v3/) newline-delimited JSON, so
//!   the existing external ecosystem (`agg`, `asciinema-player`, `svg-term`) renders it --
//!   retroglyph never builds a GIF encoder or an interactive player of its own.
//!
//! With the `pty` feature, [`pty::capture_pty`] is a second capture source for the same
//! [`write_cast`] path: it drives a real backend under a real pseudo-terminal (rather than a
//! `TestHarness`-style synthetic `Headless`), aiming for the fidelity a real-terminal tool like
//! `vhs` gets from a real terminal emulator, without one in this crate's own dependency tree.
//!
//! Not here: a first-class cross-backend replay helper for frame recordings (standard asciicast
//! output already covers the common case), or embedding `asciinema-player` in the docs site
//! (straightforward once there's a concrete need) -- both deliberately deferred, see
//! retroglyph#1268.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! This crate has no default features; every feature below is optional and off unless enabled.
//!
//! ### `pty`
//!
//! ⚪ Optional.
//!
//! A real-pseudo-terminal capture source (`pty::capture_pty`) feeding the same `write_cast` path
//! `FrameRecorder` does, for `vhs`-like real-terminal fidelity. Adds `portable-pty`/`vt100`.
//! <!-- gen-features:end -->

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation; see `retroglyph-core`'s matching include for the same
// pattern.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod cast;
mod frame_recorder;
mod input_recorder;
mod owned_cell;
mod panic_hook;
#[cfg(feature = "pty")]
pub mod pty;
mod replay;
mod rgrec;

pub use cast::write_cast;
pub use frame_recorder::{CapturedFrame, FrameRecorder, FrameRecorderHandle};
pub use input_recorder::{InputRecorder, RecorderHandle};
pub use owned_cell::OwnedCell;
pub use panic_hook::install_panic_recorder;
pub use replay::replay_live;
pub use rgrec::{read, write};
