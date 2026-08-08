# Test a game

`retroglyph-core`'s
[`Headless`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/struct.Headless.html)
backend is an in-memory `Backend`: no terminal, no window, no feature flag to enable. It's the
backend to write your tests against, whichever real backend the game ships on.

## Unit tests: drive a `Terminal<Headless>` directly

```rust
use retroglyph_core::backend::Headless;
use retroglyph_core::color::Style;
use retroglyph_core::terminal::Terminal;

let backend = Headless::new(20, 5);
let mut term = Terminal::new(backend);
term.draw(|s| s.put((2, 2), 'X', Style::default()))
    .expect("draw failed");
term.present().expect("present failed");
insta::assert_snapshot!(term.backend().format_view());
```

[`Headless::format_view`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/struct.Headless.html#method.format_view)
converts the in-memory grid to a text string (spaces rendered as `·` so trailing/leading blanks are
visible in a diff), pairing naturally with [`insta::assert_snapshot!`](https://docs.rs/insta) for
layout assertions. Colors and other styling have their own encoder,
[`Headless::format_styled`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/struct.Headless.html#method.format_styled),
for tests that also need to assert on foreground/background/attributes rather than plain glyphs.

## Driving input

[`Headless::push_event`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/struct.Headless.html#method.push_event)
queues a synthetic event for your code's own event loop to drain next tick, the same way a real
backend's driver pushes an OS/terminal event. Prefer this over asserting only on an idle frame:
pushing real key/mouse events through the same `Input::push_event` path a windowed or wasm backend
uses is what actually proves your event handling and decoding logic, not just your draw code.

If the game is built around `retroglyph-core`'s `App` trait rather than a hand-rolled loop,
[`TestHarness`](https://docs.rs/retroglyph-core/latest/retroglyph_core/testing/struct.TestHarness.html)
drives a full `App::update`/present cycle over `Headless` for you, including timing.

## Reviewing snapshots

```sh
cargo install cargo-insta   # one-time
cargo insta test            # run tests and open the review UI
cargo insta accept          # accept all pending snapshots
```

A failing snapshot test means the rendered output actually changed: review the diff
(`cargo insta test` shows it side by side) before accepting, rather than accepting on reflex. Commit
snapshot files alongside the tests that produce them, following the workspace's existing layout
(`crates/core/src/snapshots/`, `examples/tests/snapshots/`).

## Integration and cross-module tests

Unit tests live alongside their modules (`#[cfg(test)] mod tests`, in the same file as the code
under test). A few crates additionally have `tests/*.rs` integration suites for invariants that span
modules (e.g. `crates/core/tests/no_drift.rs`). Run everything with:

```sh
just test          # run everything
just test-v        # with stdout, useful while reviewing snapshot diffs
cargo test --lib   # unit tests only
```

## See also

- [Choose a backend](./choose-a-backend.md): `Headless` is the backend to test against regardless of
  which real backend the game ships on.
- [Record and replay](./record-and-replay.md): `TestHarness::replay`/`InputRecording` (this page)
  cover replaying a recording back through a headless `App` in a test; `retroglyph-recorder` covers
  actually capturing a session to a file (`InputRecorder`) or exporting it as a docs GIF
  (`FrameRecorder`).
