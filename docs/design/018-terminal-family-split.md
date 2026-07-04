# ADR 018: Terminal Family Split

**Status:** Accepted **Date:** 2026-07-04 **Amends:**
[ADR 014: Workspace Split](014-workspace-split.md) (specifically its `retroglyph-crossterm` section
and the "Windowed backend family" framing) **Relates to:**
[ADR 011: WASM Portability (Revised)](011-wasm-portability-revised.md)

## Context

ADR 014 placed `retroglyph-crossterm` outside any seam-splitting exercise, stating:
"`retroglyph- crossterm` stays entirely outside this family: no winit, no `Presenter`, implements
`Backend` directly, driven by core's `run_blocking`." That was correct for the single-implementor
state of the world at the time.

A second terminal-family consumer is now wanted: a WASM/browser terminal backend, intended to be
driven from JS via a terminal emulator such as [xterm.js](https://xtermjs.org/) on the JS side (the
Rust crate itself has no dependency on or knowledge of which JS terminal emulator is used -- it just
emits ANSI bytes and receives pushed events). This is new information ADR 014 didn't have: with two
implementors instead of one, the "no seam, it's just `Backend`" answer needs revisiting the same way
ADR 014 revisited the windowed family once a second presenter (a GL prototype) became imminent.

**Key constraint verified by direct build test:** the `crossterm` crate does not compile at all for
`wasm32-unknown-unknown` (`cargo check --target wasm32-unknown-unknown` against a crate that only
depends on `crossterm = "0.28"` fails with multiple `E0046`/`E0308` errors from crossterm's own
`unix`/`windows`-gated modules being compiled out and their public API left with holes). This isn't
a "some crossterm features don't support wasm" situation; the crate is native-only, full stop. Any
wasm terminal backend must have zero dependency on it.

## Why this is not the same shape as `retroglyph-window`

ADR 014 split `retroglyph-window` out ahead of a second windowed presenter because winit is a single
shared runtime driver across every windowed backend (`retroglyph-software` today, `retroglyph-wgpu`/
`retroglyph-gl` planned): one event loop, one input-translation layer, reused by every `Presenter`
implementor. The seam (`Presenter` + `WindowBackend<P>`) exists specifically to let renderer crates
plug into that one shared driver without depending on winit directly.

Crossterm and a WASM/xterm.js-driven terminal share **no such runtime**:

- `retroglyph-crossterm` owns a blocking poll loop against a real TTY (`crossterm::event::poll`/
  `read`), raw mode, the alternate screen, and the kitty keyboard protocol. It is driven by the
  existing generic `run_blocking` (`retroglyph-core`), same as `Headless`.
- A WASM terminal backend has no loop at all in the Rust sense: JS calls into Rust once per
  animation frame (or on demand) to push input and pull rendered output. There is no polling, no raw
  mode, no alternate screen, no TTY concept whatsoever.

There is nothing to factor out of "the loop" the way `Presenter`/`WindowBackend` factor winit's loop
out of the renderer. What genuinely _is_ shared between the two is narrower and lower-level: the
ANSI/SGR cell-diff renderer -- converting `Tile` content into cursor-movement and
`SetForegroundColor`/ `SetBackgroundColor`/SGR-attribute escape codes, tracking cursor position and
last-emitted color/ attribute state across frames so only the deltas are emitted. That logic has no
OS or JS opinion in it; it is a pure `Tile` stream -> ANSI bytes transform.

## Decision

Split the terminal family into a seam crate plus per-implementor crates, mirroring
`retroglyph-window`'s crate-per-implementor shape, but justified by a shared **renderer**, not a
shared **driver**:

```text
retroglyph-terminal          seam: TerminalRenderer<W: Write>, ANSI/SGR cell-diff renderer,
  │                          zero crossterm dependency, zero wasm-bindgen dependency
  ├── retroglyph-crossterm       implementor: crossterm crate, raw mode, alternate screen,
  │                               kitty keyboard protocol, crossterm::event polling
  └── retroglyph-terminal-wasm   implementor: wasm-bindgen, push_event-driven, no polling,
                                  no TTY concept, size set externally from JS
```

### `retroglyph-terminal` (the seam)

Depends only on `retroglyph-core` (`default-features = false`, `features = ["std"]`) and
`unicode-width`. Exposes `TerminalRenderer<W: std::io::Write>`: `draw`/`flush`/
`begin_synchronized_update`/`end_synchronized_update`/`reset_state`, generic over any byte sink.
Color and attribute conversion targets standard ANSI/CSI/SGR codes directly (not crossterm's
`crossterm::style::Color`/`Attributes` enums), since those are ordinary escape sequences with no
crossterm-specific meaning:

- foreground/background: `\x1b[38;...m`/`\x1b[48;...m` (SGR "set extended color" introducers),
  `\x1b[30-37m`/`\x1b[90-97m` (plain/bright ANSI), `\x1b[39m`/`\x1b[49m` (reset to default)
- attributes: `\x1b[1m` (bold), `\x1b[2m` (dim), `\x1b[3m` (italic), `\x1b[4m` (underline),
  `\x1b[5m` (blink), `\x1b[7m` (reverse), `\x1b[8m` (hidden), `\x1b[9m` (strikethrough), always
  preceded by a full `\x1b[0m` reset (SGR has no "set to this exact attribute state in one shot"
  escape; each attribute is an independent toggle)
- cursor movement: `\x1b[{row};{col}H` (1-indexed CSI cursor position)
- synchronized update: `\x1b[?2026h`/`\x1b[?2026l`

**Correctness note found during extraction:** the attribute reset (`\x1b[0m`) also resets fg/bg to
the terminal's default, so it must be emitted _before_ the per-cell color codes, not after. The
pre-split crossterm code happened to get this right by construction (it emitted attrs last, and
`crossterm::style::SetAttributes` doesn't behave like a raw SGR reset). Getting the order backwards
during the initial extraction broke `tests/e2e_snapshots.rs`'s `hex_battle` PTY/vt100 snapshot (a
missing background color on the very first cell drawn); this is now covered by both that
pre-existing e2e snapshot test and a unit regression test in `retroglyph-terminal` documenting the
ordering requirement directly.

### `retroglyph-crossterm` (unchanged name, slimmed down)

Keeps its published name -- it remains _the_ crossterm-based implementor, just no longer bundling
the renderer inline. Depends on `retroglyph-core` + `retroglyph-terminal` + `crossterm`. Owns
exactly the OS/TTY-specific surface: raw mode enable/disable, alternate screen enter/leave, kitty
keyboard protocol push/pop, `crossterm::event::poll`/`read`, `crossterm::terminal::size()`, and the
crossterm-event-to-`retroglyph_core::event`/color/attribute conversion functions. Its `Backend`
impl's `draw`/`flush` delegate to a `TerminalRenderer<BufWriter<Stdout>>` field instead of building
escape sequences inline. `Crossterm::run`, the public struct name, and the `crossterm` feature name
in `retroglyph-examples` are all unchanged -- this is an internal refactor, not an API break for
existing consumers.

### `retroglyph-terminal-wasm` (new)

Depends on `retroglyph-core` + `retroglyph-terminal`, plus `wasm-bindgen` under
`target_arch = "wasm32"`. `TerminalWasm` implements `Backend` directly, following the same "no
runtime driver, push-driven" shape as `Headless`:

- `size()` is a plain stored value, set via `resize()`/`Backend::resize`; never queried from a TTY.
  The host JS is expected to call this whenever its terminal emulator's `fit` logic recomputes
  `cols`/`rows`.
- `poll_event` never blocks and never polls anything; it only drains a `VecDeque` filled by
  `push_event`, called from a JS-facing entry point in response to a browser keyboard event.
- Output renders into an in-memory `Vec<u8>` via `TerminalRenderer<Vec<u8>>`; `take_output()` drains
  it as a `String` once per animation frame for JS to write into its terminal emulator.

A `wasm32`-only `wasm` submodule exposes a minimal `#[wasm_bindgen]` FFI surface
(`wasm_terminal_new`/`_resize`/`_push_key`/`_take_output`) keyed by opaque `u32` handles rather than
handing a `TerminalWasm`/`Terminal<TerminalWasm>` value across the FFI boundary directly --
`retroglyph_core::event::Event` and friends are not `wasm-bindgen`-compatible types. Key events
cross as a `(code: u32, mods: u8)` pair (`decode_key_event`, `NAMED_KEY_*` constants for
non-printable keys), the same "plain integers across the FFI boundary" pattern
`crates/examples/src/util/perf.rs` already uses for `performance.now()`.

This module is a low-level instance registry and event decoder, not a full example harness -- it
does not itself wire up a `Terminal<TerminalWasm>` + `App` + game loop the way
`retroglyph_examples::rg_run!` does for the software backend on wasm. Each example that wants a
WASM/xterm.js demo is expected to drive its own `#[wasm_bindgen(start)]` entry point on top of this
crate's `TerminalWasm`, mirroring how `rg_run!`'s software branch works today.

## What this amends in ADR 014

ADR 014's statement "`retroglyph-crossterm` stays entirely outside this family: no winit, no
`Presenter`, implements `Backend` directly, driven by core's `run_blocking`" remains true of
`retroglyph-crossterm` specifically. What's superseded is the implication that the terminal family
has (and needs) no seam at all: it now has one, `retroglyph-terminal`, deliberately shaped
differently from `Presenter` (a renderer library, not a trait implemented by a driver-owned type)
for the reasons above. `retroglyph-crossterm` is driven by `run_blocking` exactly as before;
`retroglyph-terminal-wasm` is driven by nothing (no driver at all, JS calls in directly), which is
itself further evidence there was never a shared driver here to extract a `Presenter`-style trait
from.

## Non-goals

- **A shared driver/loop abstraction across crossterm and wasm.** Rejected above: there is no shared
  runtime to abstract over.
- **Depending on xterm.js, or any specific JS terminal emulator, from Rust.**
  `retroglyph-terminal-wasm` only emits ANSI bytes and decodes pushed key events; which JS library
  (if any) consumes those bytes is entirely a docs-example/demo-page concern, not a crate
  dependency. See the parent conversation that motivated this ADR for the naming discussion that led
  here (`retroglyph-terminal-wasm` over `retroglyph-xtermjs`/`retroglyph-xterm`, precisely to avoid
  baking in that coupling).
- **A full example/game-loop harness inside `retroglyph-terminal-wasm`.** The `wasm` submodule is
  infrastructure (instance registry, event decoding); wiring a specific game's `App` to it is
  example/demo-page work, out of scope here.
- **Publishing any of these crates.** Deferred to [ADR 017](017-release-and-workspace-tooling.md),
  same as every other crate in the workspace.

## References

- [ADR 014: Workspace Split](014-workspace-split.md) -- the crate-per-implementor precedent
  (`retroglyph-window` + `retroglyph-software`/`wgpu`/`gl`) this ADR mirrors, and the section
  amended here
- [ADR 011: WASM Portability (Revised)](011-wasm-portability-revised.md) -- prior WASM-specific
  constraints and rAF-driven frame gating precedent
- `crates/examples/src/util/perf.rs` -- existing precedent for crossing the `wasm-bindgen` FFI
  boundary with plain integers/floats instead of core types
- [xterm.js](https://xtermjs.org/) -- the reference JS terminal emulator this crate is designed to
  be compatible with, deliberately not depended on
