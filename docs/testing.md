# Testing

How retroglyph is tested, and where each kind of test lives. For the exact commands to run, see
`AGENTS.md`'s Correctness gate section, which stays the single source of truth for the command list.

## Unit tests

Unit tests live alongside their modules in each crate (`retroglyph-core` and `retroglyph-widgets`
carry the bulk of them). Pixel-level software-backend regressions live in
`crates/software/src/snapshots/`.

## Snapshot tests (insta)

`Headless::format_view()` renders a grid to text (spaces become `·`). Combined with
`insta::assert_snapshot!`, this is the primary tool for layout assertions: write the drawing code,
snapshot the headless render, and diff future changes against the committed baseline instead of
hand-writing character-grid assertions.

Snapshot files are committed next to their crate (`crates/*/src/snapshots/`,
`examples/tests/snapshots/`).

```sh
cargo insta test    # run and open review UI
cargo insta accept  # accept pending snapshots
```

## Example-driven snapshots (examples crate)

`examples/tests/support/` drives every `Example` implementation through three snapshot types from
one source of truth:

- **Headless text** (insta) — the same `format_view()` mechanism as unit tests, run against the
  example's actual `update()` logic.
- **Software PNG** — a pixel buffer capture of the software backend's rendered output.
- **Crossterm SVG** — a real PTY capture, parsed via the `vt100` crate, verifying the ANSI/SGR
  output an actual terminal would receive.

The crossterm binary each `svg_snapshot` test spawns is built with its own `--target-dir`
(`target/pty-examples/`, see `support::build_crossterm_example`), separate from the workspace's
normal `target/`. `cargo test --workspace --all-features` builds every example with the `software`
feature (unusable in a PTY) before any test runs, so building the crossterm-only variant back into
the same output path would force a relink -- and, on macOS, a real code-signature validation cost of
roughly a second or two -- on every single test run. The isolated target dir keeps that binary
byte-identical (and already validated) across runs instead.

Every example under `examples/examples/*.rs` is also auto-built to three WASM variants (headless /
xterm.js terminal / software canvas) and deployed to the docs gallery by
`.github/workflows/docs.yml` on every push, so each example carries real, ongoing CI cost, not just
a one-time snapshot.

```sh
cargo test -p retroglyph-examples --all-features
```

See `examples/AGENTS.md` for the per-example validation checklist a new example must satisfy before
it's considered complete (all three snapshot types, all three WASM variants, graceful backend
degradation, etc.).
