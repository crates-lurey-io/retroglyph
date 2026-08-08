# Record and replay

`retroglyph-recorder` is a separate crate on top of `retroglyph-core`, with two recorders covering
two different questions: `InputRecorder` for "what happened" (a real session's input stream, for
turning a bug report into a regression test), `FrameRecorder` for "what was shown" (a backend's
drawn output, for turning a real session into a docs GIF). Neither is enabled by default:

```sh
cargo add retroglyph-recorder
```

## `InputRecorder`: turn a bug report into a regression test

Wraps any backend and taps its input stream, so a real session (a bug report, a demo run) can be
captured to a `.rgrec` file:

```rust,no_run
use retroglyph_core::backend::Headless;
use retroglyph_recorder::{InputRecorder, install_panic_recorder};

let recorder = InputRecorder::new(Headless::new(80, 24));
// Reach the recorder from anywhere (e.g. a panic hook) via a cheap, cloneable handle.
install_panic_recorder(recorder.handle(), "crash.rgrec");

// ...drive `recorder` as the backend, then save it whenever you like:
recorder.save("session.rgrec").expect("save recording");
```

[`install_panic_recorder`](https://docs.rs/retroglyph-recorder/latest/retroglyph_recorder/fn.install_panic_recorder.html)
saves an in-progress recording automatically if the process panics, so the input that led to the
crash isn't lost with it -- the exact case a bug report needs.

Replay a saved session back into a regression test with `retroglyph-core`'s own `testing` feature,
against a headless `App`, at faithful (not coarse) timing and with no wall-clock sleeping:

```rust,no_run
# use retroglyph_core::app::{App, Flow, Frame};
use retroglyph_core::testing::TestHarness;
# struct MyApp;
# impl<B: retroglyph_core::backend::Backend> App<B> for MyApp { fn update(&mut self, _t: &mut retroglyph_core::terminal::Terminal<B>, _f: &Frame) -> Flow { Flow::Exit } }

let recording = retroglyph_recorder::read("session.rgrec").expect("read recording");
let mut harness = TestHarness::from_recording(&recording);
let mut app = MyApp;
harness.replay(&recording, &mut app);
assert!(harness.view().contains("expected state"));
```

Or watch it happen on screen, live, through a real backend, with
[`replay_live`](https://docs.rs/retroglyph-recorder/latest/retroglyph_recorder/fn.replay_live.html)
-- see [Test a game](./test-a-game.md) for the rest of `TestHarness`.

## `FrameRecorder`: turn a session into a docs GIF

Wraps any backend and taps its `Output::draw_layers` diff stream -- the same `DrawCell` stream every
backend's `draw_layers` call already receives -- buffering owned frames as it goes:

```rust
use retroglyph_core::backend::{DrawCell, Headless, Output};
use retroglyph_core::color::Style;
use retroglyph_core::grid::Pos;
use retroglyph_core::tile::Tile;
use retroglyph_recorder::{write_cast, FrameRecorder};

let mut recorder = FrameRecorder::new(Headless::new(20, 5));
let tile = Tile::new('!', Style::default());
recorder
    .draw_layers(std::iter::once(DrawCell::new(Pos::new(2, 2), &tile)))
    .expect("draw_layers failed");

let mut cast = Vec::new();
write_cast(&mut cast, recorder.inner().size(), &recorder.frames()).expect("write_cast failed");
```

`write_cast` exports the buffered frames as
[asciicast v3](https://docs.asciinema.org/manual/asciicast/v3/) newline-delimited JSON -- the format
[`agg`](https://docs.asciinema.org/manual/agg/), `asciinema-player`, and `svg-term` already render.
Nothing in `retroglyph-recorder` builds a GIF encoder or an interactive player; the point is
standard output that hands off to that existing ecosystem.

`FrameRecorder`'s captured frames are read through a
[`FrameRecorderHandle`](https://docs.rs/retroglyph-recorder/latest/retroglyph_recorder/struct.FrameRecorderHandle.html)
taken out with `recorder.handle()` _before_ handing the wrapped `Terminal` to a driver like
`retroglyph_core::app::run_on`, which takes it by value and never hands it back -- the handle is how
the captured frames survive a driver call that consumes the recorder.

### Two capture sources, one export format

- The example above is the `TestHarness`-style source: scripted, deterministic, no real terminal
  needed. This is what makes docs GIF generation scriptable, instead of `vhs`'s wall-clock
  `Sleep`/`Type@` timing guesses (see retroglyph#461).
- With the `pty` feature, `capture_pty` is a second capture source: a real
  [`portable-pty`](https://docs.rs/portable-pty) pseudo-terminal plus a real
  [`vt100`](https://docs.rs/vt100) VT-parser, feeding the same `write_cast` path. This is the
  mechanism aimed at matching `vhs`'s real-terminal fidelity, without a real browser/`ttyd`/`ffmpeg`
  in the loop. See `capture_pty`'s own rustdoc for the specific, measured fidelity gap against a
  `TestHarness`-driven capture of an equivalent session (color quantization differs; timing/frame
  boundaries differ under real scheduling) -- both still converge on the same `write_cast` output
  _shape_.

### Generating a real docs GIF

`retroglyph-examples`' `launch::<E>()` wires `FrameRecorder` in generically via `--record <path>`,
for the crossterm and headless-stdout backends (the two text/glyph-oriented ones -- the windowed
software/GL/wgpu backends present pixels, not `DrawCell` diffs, so there's nothing for a text export
to capture there):

```sh
cargo run --example 15_outpost_dashboard --features crossterm -- --record demo.cast
agg demo.cast demo.gif
```

`just assets` runs this same pipeline for the workspace's own docs GIFs (`agg` is invoked as an
external tool, never a `Cargo.toml` dependency -- it's GPL-licensed, and this workspace is MIT).

## Known limitation: whole session in memory

Both `InputRecorder` and `FrameRecorder` buffer their entire session in memory rather than streaming
to disk as it's captured. Fine for the docs/demo-length and bug-report-length captures both exist
for; not a fit for an unbounded, long-running recording.

## See also

- [Test a game](./test-a-game.md): `Headless`/`TestHarness` for tests -- this page's counterpart for
  driving a real recorder instead.
