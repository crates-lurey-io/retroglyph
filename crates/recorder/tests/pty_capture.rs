//! Cross-checks the `pty` feature's [`capture_pty`] against a
//! [`FrameRecorder`](retroglyph_recorder::FrameRecorder)-driven capture of an equivalent scripted
//! session: both should converge on the same [`write_cast`] output shape for a settled session,
//! per `crates/recorder/src/pty.rs`'s own "Fidelity" docs.
//!
//! An integration test (rather than a `#[cfg(test)]` unit test in `pty.rs`) because it shells out
//! to a real subprocess (`/bin/sh`) under a real PTY -- no dependency on `retroglyph-examples`'
//! own compiled binaries, so this runs anywhere the `pty` feature itself does, without needing
//! `just build-pty-examples` first.

#![cfg(all(feature = "pty", unix))]

use retroglyph_core::backend::{DrawCell, Headless, Output};
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::{HasSize, Pos};
use retroglyph_core::tile::Tile;
use retroglyph_recorder::{FrameRecorder, write_cast};
use std::path::Path;
use std::time::Duration;

/// A script that prints "Hi" in green (matching `SGR 32`) then exits: small and fast enough to
/// settle within [`capture_pty`]'s poll cadence, and simple enough that a `FrameRecorder`-driven
/// `Headless` capture of "the same session" is unambiguous -- one frame, two green cells.
const SCRIPT: &str = "printf '\\033[32mHi\\033[0m'";

#[test]
fn pty_capture_produces_the_same_write_cast_shape_as_frame_recorder() {
    let (size, frames) = retroglyph_recorder::pty::capture_pty(
        Path::new("/bin/sh"),
        &["-c", SCRIPT],
        10,
        1,
        Duration::from_secs(5),
    )
    .expect("pty capture");
    assert!(!frames.is_empty(), "expected at least one captured frame");

    let mut pty_cast = Vec::new();
    write_cast(&mut pty_cast, size, &frames).expect("write_cast for pty capture");
    let pty_text = String::from_utf8(pty_cast).expect("utf8");

    // Same shape: a version-3 header naming this terminal's size, followed by at least one
    // `"o"` event carrying the green "Hi" text -- exactly what a `FrameRecorder`-driven capture
    // of the same visible content produces below. Not the same SGR *code*: `vt100` reports the
    // shell's green as an indexed color (`Idx(2)`, per this crate's `pty` module docs on why it
    // isn't upgraded to the 16-color `Ansi` form), so this checks for `38;5;2` (indexed green)
    // here and `32` (standard-palette green) in the `FrameRecorder` capture below -- same color,
    // different encoding, exactly the documented fidelity gap.
    assert!(pty_text.contains("\"version\":3"));
    assert!(pty_text.contains(&format!("\"cols\":{}", size.width())));
    assert!(pty_text.contains("\"o\""));
    assert!(pty_text.contains("Hi"));
    assert!(pty_text.contains("38;5;2"));

    let mut recorder = FrameRecorder::new(Headless::new(10, 1));
    let style = Style::new().fg(Color::GREEN);
    let h = Tile::new('H', style);
    let i = Tile::new('i', style);
    recorder
        .draw_layers(
            [
                DrawCell::new(Pos::new(0, 0), &h),
                DrawCell::new(Pos::new(1, 0), &i),
            ]
            .into_iter(),
        )
        .expect("draw_layers");

    let mut harness_cast = Vec::new();
    write_cast(
        &mut harness_cast,
        recorder.inner().size(),
        &recorder.frames(),
    )
    .expect("write_cast for harness capture");
    let harness_text = String::from_utf8(harness_cast).expect("utf8");

    assert!(harness_text.contains("\"version\":3"));
    assert!(harness_text.contains("\"o\""));
    assert!(harness_text.contains("Hi"));
    assert!(harness_text.contains("32"));
}
