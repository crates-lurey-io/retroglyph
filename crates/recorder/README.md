# retroglyph-recorder

[![crates.io](https://img.shields.io/crates/v/retroglyph-recorder.svg)](https://crates.io/crates/retroglyph-recorder)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-recorder)](https://docs.rs/retroglyph-recorder)
[![license](https://img.shields.io/crates/l/retroglyph-recorder.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

Input recording & replay for [retroglyph](https://github.com/crates-lurey-io/retroglyph): record a
real session's input stream and replay it through a live `App`, re-executing real application logic
against recorded input rather than repainting recorded pixels. A bug report that replays itself into
a regression test, with no hand-transcription.

- [`InputRecorder<B>`] wraps any backend and taps its input stream, so a real session can be
  captured to a `.rgrec` file (line-delimited JSON, one event per line, diffs and reviews like
  source).
- [`replay_live`] drives a saved recording forward into a live `Terminal`/`App`, once, at recorded
  pace: "watch it happen" on screen.
- `retroglyph-core`'s own `testing` feature (`TestHarness::replay`/`InputRecording`) covers driving
  a recording back through a headless `App` in a test, with faithful (not coarse) timing -- no
  wall-clock sleeping, no real backend needed. This crate builds `.rgrec` and the record/live-replay
  path on top of that same `InputRecording`.
- [`install_panic_recorder`] auto-saves an in-progress recording if the process panics, so a crash
  doesn't lose the input that led to it.

Not here: rendering a recording to an asciicast/GIF, or capturing raw terminal frames rather than
input events -- that's the (separate, not-yet-built) frame recorder.

## Quick start

```sh
cargo add retroglyph-core retroglyph-recorder
```

```no_run
use retroglyph_core::backend::Headless;
use retroglyph_recorder::{InputRecorder, install_panic_recorder};

let recorder = InputRecorder::new(Headless::new(80, 24));
// Reach the recorder from anywhere (e.g. a panic hook) via a cheap, cloneable handle.
install_panic_recorder(recorder.handle(), "crash.rgrec");

// ...drive `recorder` as the backend, then save it whenever you like:
recorder.save("session.rgrec").expect("save recording");
```

Replay a saved session through the same `App`, live:

```no_run
use retroglyph_core::backend::Headless;
use retroglyph_core::terminal::Terminal;
use retroglyph_recorder::replay_live;

# use retroglyph_core::app::{App, Flow, Frame};
# struct MyApp;
# impl<B: retroglyph_core::backend::Backend> App<B> for MyApp {
#     fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow { Flow::Exit }
# }
let recording = retroglyph_recorder::read("session.rgrec").expect("read recording");
let term = Terminal::new(Headless::new(recording.width(), recording.height()));
let mut app = MyApp;
replay_live(term, &mut app, &recording).expect("replay");
```

See [docs.rs](https://docs.rs/retroglyph-recorder) for the full API.
