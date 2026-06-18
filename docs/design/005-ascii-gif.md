# ADR 005: Animated Cast Recording for Test Documentation

**Status:** Draft **Date:** 2026-06-17 **Parent:**
[ADR 004: E2E and Screenshot Testing Strategy](004-testing-strategy.md)

## Context

ADR 004 establishes snapshot testing as the correctness gate: `insta` asserts on the rendered grid
state, and a static SVG provides a visual artifact for GitHub PR diffs. This is sufficient for
catching regressions, but it only shows the _final frame_ of an interactive session.

For a library whose primary purpose is driving interactive terminal UIs, a static snapshot does not
fully communicate what a test covers. A reviewer looking at a snapshot change cannot tell whether
the player moved, an animation played, or the layout reflowed — they see only the end state.

This ADR describes an optional complementary layer: recording the PTY session as an
[asciinema](https://asciinema.org/) `.cast` file and converting it to an animated GIF committed
alongside the existing snapshots.

## Decision

The cast recording is **documentation only**. It is never the basis of an assertion.

Rationale: raw PTY output is unsuitable as a test oracle. It contains terminal init sequences, echo
artifacts, timing jitter, and cursor housekeeping that vary between runs and environments. The
rendered grid state — what the VT100 emulator produces after processing all bytes — is the only
signal stable enough to assert on. The cast answers "what did it look like and how did it move"; the
`.snap` file answers "did the output change."

## Cast format

asciinema v2 is JSON Lines:

```
{"version": 2, "width": 60, "height": 15, "title": "crossterm_demo"}
[0.000, "o", "\u001b[?1049h\u001b[2J..."]
[0.183, "o", "\u001b[5;7H@"]
[0.312, "o", "\u001b[?1049l"]
```

Line 1 is a header object. Each subsequent line is a three-element array:
`[elapsed_seconds, event_type, data]`. For output capture, `event_type` is always `"o"`.

The format is trivial to write without the asciinema CLI — it only requires timestamping each read
chunk from the PTY master and serialising the result.

## Required changes

### 1. Timed PTY capture

`capture_pty` currently calls `read_to_end`, discarding timing. It needs to read in a loop,
recording a `(Duration, Vec<u8>)` tuple per chunk. The rest of the test (VT100 parsing, SVG
rendering, insta assertion) is unchanged; only the capture function grows a return type.

### 2. Cast file writer

A small helper writes the collected chunks to a `.cast` file:

```rust
fn write_cast(path: &Path, rows: u16, cols: u16, title: &str, chunks: &[(Duration, Vec<u8>)]) {
    // header
    // one line per chunk: [elapsed.as_secs_f64(), "o", base64-or-escaped bytes]
}
```

The test calls this alongside the existing SVG write, depositing
`tests/snapshots/crossterm_demo.cast` in the same directory.

### 3. Justfile recipes

```just
# Convert all committed casts to animated GIFs.
# Requires: cargo install agg --root bin/
casts-to-gif:
    bin/bin/agg tests/snapshots/crossterm_demo.cast tests/snapshots/crossterm_demo.gif

# Record a fresh cast by running the demo interactively.
# Requires: brew install asciinema
record name="crossterm_demo":
    cargo build --example {{name}} --features crossterm
    asciinema rec --command target/debug/examples/{{name}} \
        tests/snapshots/{{name}}.cast
```

`casts-to-gif` is a local tool (binary installed under `bin/`). `record` requires `asciinema` on
`$PATH`; it is a viewer/recorder, not a build dependency, so a global install is acceptable.

### 4. Dependency additions

| Tool           | Install                                     | Purpose                                  |
| -------------- | ------------------------------------------- | ---------------------------------------- |
| `agg`          | `cargo install agg --root bin/`             | Cast → animated GIF                      |
| `svg-term-cli` | add to `tools/package.json` devDependencies | Cast → animated SVG (alternative to GIF) |
| `asciinema`    | `brew install asciinema` (global, optional) | Playback and interactive recording       |

`asciinema` is not a build or CI dependency. Any contributor who wants to record a new session or
play one back locally installs it themselves.

### 5. `.gitattributes`

```
tests/snapshots/*.cast linguist-generated
tests/snapshots/*.gif  linguist-generated
```

### 6. Committed snapshot artifacts (full set)

After this ADR is implemented, the committed artifacts for each E2E test scenario are:

| File     | Format       | Purpose                                            |
| -------- | ------------ | -------------------------------------------------- |
| `*.snap` | insta text   | Authoritative assertion; CI fails on mismatch      |
| `*.svg`  | SVG (static) | Final frame; renders inline on GitHub              |
| `*.cast` | asciinema v2 | Full session with timing; `asciinema play` to view |
| `*.gif`  | Animated GIF | Animated; renders inline on GitHub                 |

The `.snap` and `.svg` files are written on every test run. The `.cast` and `.gif` files are written
by explicit Justfile recipes (`just record` / `just casts-to-gif`) and committed manually when a
scenario changes intentionally.

## The broader test stack this points toward

Cast recording is the last piece of a layered test infrastructure that is worth naming explicitly,
because the layers compound: each one makes the next more useful, and together they create a
feedback loop that supports both human review and AI-assisted development.

### Layer 1: Unit tests — logic in isolation

Test individual structs and functions (`Grid`, `Style`, `Cell`, the differ). Fast, deterministic, no
I/O. These are already in place.

### Layer 2: Integration tests — input events against `Headless`

Drive `Terminal<Headless>` with synthetic events and assert on grid state. The existing
`tests/e2e.rs` does this, but the taxonomy of input scenarios is worth expanding deliberately:

- **Key input:** single keys, modifier combinations, sequences (e.g. arrow navigation)
- **Text input:** printable characters, unicode, paste sequences
- **Mouse:** move, click (button + position), drag, scroll
- **Resize:** backend reports a new `Size`, grid reflows correctly
- **Timing:** events arrive after delays, idle frames render correctly

Each scenario is a deterministic, sub-millisecond test — no PTY, no process spawn. These should be
the primary regression surface.

### Layer 3: Golden tests — final rendered state

Spawn the real binary in a PTY, feed it a scripted input sequence, parse the ANSI output with a
VT100 emulator, and snapshot the resulting grid. The `.snap` file is the oracle; the `.svg` is the
human-readable rendering. This is what ADR 004 implements.

The key insight is that the assertion is on the _rendered grid_ — the stable, high-level output —
not on the raw bytes. Raw PTY output contains echo, timing, and cursor housekeeping that vary
between runs. Rendering it first discards all of that noise.

### Layer 4: Playback — the session as an artifact

What this ADR adds. The `.cast` file records the session with timing. It does not assert on
anything; it documents the scenario that produced the golden. A future reader can replay it to
understand what the test covers, and a future change author can record a new cast when intentionally
modifying behavior.

Playback can also be used as a _test driver_: replay a cast as input into a fresh process and assert
that the final state matches the golden. This is a stronger form of the golden test — it verifies
not just that the output is correct but that the same sequence of inputs produces it reproducibly.
This is worth exploring but not required for an initial implementation.

### Layer 5: Tracing — structured observability

Add `tracing` instrumentation to the hot paths: `Terminal::present()` (diff size, cells changed),
`Backend::draw()` (cells written), event dispatch, grid operations. Emit spans and events.

In tests, use a `tracing-subscriber` that captures spans to an in-memory buffer. This gives you:

- Assertions on _behavior_, not just output: "this frame only redrew 3 cells"
- Regression detection for performance characteristics
- A structured log attached to every test run that explains _why_ a golden changed

The trace output also becomes an artifact that can be committed alongside the cast and SVG, creating
a complete picture of any given test scenario: what the user did (cast), what it looked like (SVG),
and what the library did internally (trace).

### Why this stack matters for AI-assisted development

Each layer produces a different kind of artifact:

| Layer       | Artifact         | Readable by   |
| ----------- | ---------------- | ------------- |
| Unit        | test output      | human, CI     |
| Integration | grid state diffs | human, CI, AI |
| Golden      | `.snap`, `.svg`  | human, CI, AI |
| Playback    | `.cast`, `.gif`  | human, AI     |
| Tracing     | structured spans | human, CI, AI |

An AI agent asked to modify the rendering pipeline can read the `.snap` to understand expected
output, watch the `.gif` to understand the interaction being tested, and read the trace to
understand what the library did internally. It can run the test suite and interpret a failure
because the failure message includes a rendered diff of the grid state, not a wall of ANSI escape
sequences.

The test infrastructure is, in this sense, the primary interface between the codebase and any
autonomy working on it. The richer and more legible the artifacts, the more effectively that
autonomy can diagnose failures, propose fixes, and verify its own work. This is the compounding
effect: integration tests catch input handling bugs quickly; golden tests catch rendering
regressions visually; playback lets a reviewer understand what changed and why; tracing explains the
mechanism. Each layer makes the others easier to interpret.

---

## Alternatives considered

**Use the cast as the test oracle.** Rejected. The raw byte stream is not stable — timing, echo, and
init sequences vary. Stripping those to extract comparable content would essentially re-implement
the VT100 emulator approach already in use, with more fragility and no benefit.

**Pixel screenshots via Xvfb.** Rejected in ADR 004. Font rendering and GPU dependencies make these
brittle in CI and produce unreadable binary diffs.

**Record with `script` (BSD/macOS built-in).** `script` produces a timing file + raw bytes in a
non-standard format. asciinema v2 is better supported, has a wider tooling ecosystem, and the format
is human-readable.
