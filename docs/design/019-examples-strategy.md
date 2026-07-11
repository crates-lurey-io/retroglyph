# ADR 019: Examples Strategy

**Status:** Accepted **Date:** 2026-07-11 **Relates to:**
[ADR 014: Workspace Split](014-workspace-split.md) (the crate boundaries this ADR's examples
exercise), [ADR 004: E2E and Screenshot Testing Strategy](004-testing-strategy.md) (the snapshot
approach this ADR extends to a per-example harness),
[ADR 008: Layers and Sub-cell Offsets](008-layer-composition.md) (`06_layers` is the first example
to exercise layer compositing end to end)

## Context

`examples/examples/` was rebuilt from scratch after the workspace split landed (ADR 014) and
currently holds a single file, `01_hello_world.rs`. The supporting infrastructure around it is
already complete: the `Example` trait (`init`/`tick`, generic over `Backend`), a feature-gated
launch dispatcher (`launch::<E>()` picks software / crossterm / wasm-headless / wasm-terminal /
headless-stdout by priority), WASM FFI codegen macros (`example_main!`, `wasm_entry!`) that turn one
macro call into three browser entry points, and a three-way snapshot test harness in
`examples/tests/support/` (headless text via `insta`, software-rendered PNG, and a crossterm SVG
captured from a real PTY and parsed with `vt100`). `01_hello_world` exercises all of it end to end.

Examples in this crate are dual-purpose, not documentation-only:

1. **Docs-gallery UX surface.** `.github/workflows/docs.yml` discovers every file under
   `examples/examples/*.rs` and builds it to three WASM variants (headless text, xterm.js terminal,
   software canvas), deploying all three to the GitHub Pages gallery. This is the first thing a
   prospective user of the library sees and copies from.
2. **Cross-backend regression suite.** Each example's three committed snapshots pin its rendered
   output on all three backend families. A change that breaks color mapping, event decoding, or
   layer compositing on any one backend fails `cargo test -p retroglyph-examples --all-features`
   before it fails a user's build.

Because every example carries a real, measured CI cost (WASM build time for three variants, plus a
committed snapshot triple), the count and scope of examples is a deliberate product decision, not a
"the more the better" one. This ADR's job is to fix that scope for the first rollout and to record a
staged path beyond it, so contributors don't independently reinvent the "what belongs in examples/"
question.

### Correcting the premise this ADR started from

Earlier drafting of this plan assumed the workspace split was still "planned" and that
`retroglyph-widgets` did not yet exist. Both are wrong. The workspace split (ADR 014) is done: the
repository is a Cargo workspace under
`crates/{core,terminal,crossterm,terminal-wasm,software,window,widgets}`, plus `examples` and
`tools/cargo-bin`, with no single-crate `src/` and no `retroglyph` facade crate.
`retroglyph-widgets` exists and is full-featured -- layout (`split_h`/`split_v`, `Constraint`,
`Flex`, `centered_rect`), interaction (`HitTester`, `FocusRing`, `Interaction`, `Shortcuts`,
`Density`), and a wide widget set (`Table`, `Gauge`, `Sparkline`, `Panel`, `Modal`, `Scrollbar`,
`Log`, `Theme`, and more). Likewise `retroglyph-core` already ships `Camera`, `Grid::from_charmap`,
`animate` (tweens/easing), `frame_clock` (fixed timestep), and the `App`/`Flow`/`Frame` loop
contract. None of this needs to be built before examples can use it.

The practical effect: nothing in the original 13 pre-split examples (dashboard, roguelike,
interaction demo, and the rest, deleted when `examples/` was rebuilt from scratch) is blocked on
missing library capability. The open question was never "can we build this," it is "what should ship
first, and in what order," at a CI cost that stays manageable. That is what this ADR decides.

### The `Style` reality (why there is no `styled_text` example)

`retroglyph_core::Style` has exactly two fields, `fg` and `bg`. Its module documentation states this
is deliberate: retroglyph is a spiritual successor to a hardware text-mode display and cannot fake
most terminal text attributes (no bold font variant to switch to, no underline stroke to draw).
There is no bold, italic, underline, reverse, or blink anywhere in `Style`. A literal "styled text"
example built around those attributes would misrepresent the library and would not compile against
the real type. The styling story retroglyph actually has is color (`fg`/`bg`, including
inverse-video by swapping the two), which belongs in a colors example rather than a separate one.

## Decision

### Tier 1: six core-only examples, all three backends, first PR

Ship exactly six examples for the first rollout. Every one uses only `retroglyph-core` (no
`retroglyph-widgets`, no `software-tilesets`), must build and run correctly on all three backend
families (headless, crossterm/terminal, software/pixel, including their WASM variants), and must
degrade gracefully rather than panic or render blank when a specific backend cannot fully represent
a capability.

| #   | Example                   | Proves (to a reader)                                                                                                                                                                                           | Tests (via committed snapshots)                                                                                                                               |
| --- | ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01  | `01_hello_world` (exists) | minimal setup: `print`, present loop, quit                                                                                                                                                                     | baseline text placement renders identically on all three backends                                                                                             |
| 02  | `02_colors`               | the color model: `Ansi` (16), `Indexed` (256), `Rgb` (24-bit), `Default`; fg vs bg; inverse video via fg/bg swap -- this is retroglyph's styling vocabulary, in place of a `styled_text` example (see Context) | color-to-backend mapping is stable: SGR codes on the terminal backend, exact RGB on software, glyph-only layout on headless                                   |
| 03  | `03_keyboard`             | `Event::Key`/`KeyCode` including arrows, modifiers, and Esc; a drain-events loop                                                                                                                               | key decode and on-screen echo are stable; exercises the WASM `decode_key` FFI path end to end                                                                 |
| 04  | `04_mouse`                | `Event::Mouse`/`MouseEventKind` down/up/move/scroll with cell coordinates                                                                                                                                      | pointer decode and hit feedback are stable; exercises the WASM `decode_mouse` FFI path; the reference implementation of graceful per-backend fallback (below) |
| 05  | `05_layout_grid`          | core `Rect` geometry: subdividing the grid into panes by hand, bordering and labeling each, using manual arithmetic only (no `retroglyph-widgets`)                                                             | pane geometry and border glyphs are stable; establishes the "before" baseline that a later widgets `split_h`/`split_v` example contrasts against              |
| 06  | `06_layers`               | multi-layer compositing: `Terminal::layer(n)`, transparent-empty-tile blit, z-order (a background fill on layer 0, a moving glyph on layer 1)                                                                  | layer ordering and transparency are stable across backends; the only Tier 1 example exercising the layer/blit path (ADR 008)                                  |

No two examples prove the same capability, and no two pin overlapping snapshot content.

**`04_mouse` is the canonical graceful-fallback example.** Terminal mouse-motion reporting may be
unavailable depending on the backend's capability; when motion cannot be reported, the example
degrades to click-only tracking and shows an on-screen note ("motion unavailable on this backend")
rather than failing or rendering nothing. Future examples that hit a backend capability gap should
follow this same pattern: detect, degrade visibly, never panic or blank the frame.

**`05_layout_grid` stays core-only on purpose.** `retroglyph-widgets`'s `split_h`/`split_v`/
`Constraint`/`Flex` would make this example shorter, but Tier 1's whole point is proving what the
core API alone can do. Manual `Rect` math (roughly 30 lines) is the honest answer to "how would a
user lay out a screen before reaching for widgets," and it sets up the contrast for the Tier 2
widgets example below.

### Conventions for every example, every tier

- **Naming:** the existing zero-padded `NN_name.rs` pattern under `examples/examples/`.
- **Shape:** one file, `#[derive(Default)]` state where the example allows it, `impl Example`,
  terminated with `retroglyph_examples::example_main!(Type)`. A sibling `examples/tests/NN_name.rs`
  produces the three snapshots through `examples/tests/support`.
- **Size ceiling:** roughly 150 lines per example (including its top doc comment, excluding the test
  file). `01_hello_world` is about 73 lines; Tier 1 examples should stay in that neighborhood so
  they read as copy-paste templates, not applications. An example that outgrows this ceiling belongs
  in Tier 2, not Tier 1.
- **Top doc comment:** states what the example proves and how to run it, matching
  `01_hello_world.rs`'s existing style.
- **Headless snapshots for input-driven examples (`03_keyboard`, `04_mouse`) drive synthetic
  events** through `Headless::push_event` rather than snapshotting an idle/legend frame. This is
  more work per example but it is what actually proves decode-and-echo correctness, and it is what
  exercises the WASM `decode_key`/`decode_mouse` FFI paths through a real example rather than only
  through their existing unit tests.

### Prerequisite: `run_software_with` escape hatch lands with Tier 1

`examples/src/launch.rs`'s `run_software::<E>()` hardcodes a 50x25 grid at 2x scale, and its own
documentation already carries a TODO describing the fix: a
`run_software_with::<E>(builder: SoftwareBackendBuilder)` variant that accepts a caller-supplied
builder, with `run_software` delegating to it using the default 50x25-at-2x builder. None of the six
Tier 1 examples need a non-default grid size, but this escape hatch is a standing constraint
independent of Tier 1's content, the change is small and low-risk, and it directly unblocks the Tier
2 pixel-only examples below (`animation`, `sprites_tileset`) that do need custom grid sizes. Land it
in the Tier 1 PR while `launch.rs` is already being touched, rather than deferring it to Tier 2.

### CI cost, Tier 1

Six examples x three WASM variants = 18 builds, estimated at roughly five minutes of wall-clock
added to the docs deploy workflow. Acceptable as a first stage.

## Staged roadmap beyond Tier 1

Delivery is staged, not all-at-once: Tier 1 lands in one PR, Tier 2 in a follow-up, Tier 3 (games)
later, with CI wall-clock measured between stages before committing to the next. Tier 2 and Tier 3
are **deferred, not blocked** -- everything they need (`retroglyph-widgets`, `software-tilesets`,
`animate`, `frame_clock`) already exists in the workspace today.

### Tier 2: capability proofs (second PR)

Mapped from the 13 examples that existed before the `examples/` rebuild. Naming continues Tier 1's
zero-padded `NN_name.rs` convention rather than switching to bare descriptive names -- there is no
tier-boundary signal worth the inconsistency, and `bin/runner.rs`'s discovery/listing reads better
sorted.

| #   | Example                  | Origin                | Proves                                                                                                           | Backend / fallback                                                                                                                      |
| --- | ------------------------ | --------------------- | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 07  | `07_sprites_tileset`     | `tileset.rs`          | `retroglyph-software`'s `tilesets` feature: PNG sprite sheet loading, alpha-blended compositing                  | software renders real sprites; terminal and headless fall back to an ASCII/glyph representation (mandatory under the all-backends rule) |
| 08  | `08_animation`           | `subpixel.rs`, tamed  | `animate` tweens plus `frame_clock` fixed-timestep driving, sub-cell `put_offset` on the software backend        | software shows true sub-cell offsets; terminal/headless fall back to whole-cell movement                                                |
| 09  | `09_widgets_dashboard`   | `dashboard.rs`        | the first `retroglyph-widgets` showcase: `Table`, `Gauge`, `Sparkline`, `split_h`/`split_v`, `BoxStyle`, `Theme` | all backends -- widgets is backend-generic; this is the payoff of "deferred, not blocked"                                               |
| 10  | `10_widgets_interaction` | `interaction_demo.rs` | `Interaction`, `HitTester`, `FocusRing`, `Shortcuts`, `Density`                                                  | all backends, mouse-driven; pairs with `04_mouse`                                                                                       |

`09_widgets_dashboard` and `10_widgets_interaction` bring `retroglyph-widgets` into
`examples/Cargo.toml` for the first time, as a plain unconditional dependency (like
`retroglyph-core`), not behind a new Cargo feature -- widgets is backend-generic, so there is no
backend axis to gate it on, and every other Tier 2 example already needs the software/crossterm
feature flags that exist for backend selection, not capability selection.

**No `benchmark` example, and no `layers_compositing` example.** Two items considered for Tier 2 are
deliberately cut from this crate's scope:

- **`benchmark`** (from `sprite_stress.rs`, `PerfOverlay` FPS/throughput under load) is dropped from
  `examples/`. Performance measurement and the docs-gallery/regression-suite purpose this ADR
  defines for examples are different concerns and should not be conflated: an examples-crate
  "example" is judged by its committed snapshots (does it render correctly), not by a throughput
  number, and nothing in this ADR's harness (`insta`, PNG, SVG snapshots) is built to gate on
  performance. If a perf-regression benchmark is wanted, it belongs in a `cargo bench`/criterion
  setup scoped by its own decision, not as a `retroglyph-examples` entry. `PerfOverlay` itself is
  unaffected and remains available for any example that wants to display live FPS as incidental UI.
- **`layers_compositing`** (from `dirty_viz.rs`, deeper multi-layer / dirty-diff visualization) is
  dropped as a near-duplicate of Tier 1's `06_layers`: both exercise the same layer/blit path, and a
  dirty-diff heatmap is a debugging visualization of the same capability rather than a new one worth
  a separate committed snapshot triple.

Tier 2 is therefore four examples, not six.

**`Example::configure_software` extends the Tier 1 escape hatch.** `run_software_with` (the Tier 1
prerequisite above) takes a caller-supplied `SoftwareBackendBuilder`, but nothing called it except a
hand-written `main`, which would have broken every example's single `example_main!` call site.
`07_sprites_tileset` (the one Tier 2 example that needs a builder customization -- its PNG tileset)
instead overrides a new `Example::configure_software` default method (mirroring `Example::init`'s
existing shape), and `run_software` threads the example's own builder through it before calling
`run_software_with`. `example_main!`'s one-call convention holds for every example, Tier 1 or Tier
2, tileset or not.

**Tier 2's size ceiling is looser than Tier 1's.** Tier 1's ~150-line ceiling exists to keep those
six examples reading as copy-paste templates. Tier 2 examples are explicitly capability proofs, not
templates -- `09_widgets_dashboard` alone covers `Table`, `Gauge`, `Sparkline`, `split_h`/`split_v`,
`BoxStyle`, and `Theme`, and forcing that into ~150 lines would make it either incomplete or
unreadable. Tier 2 examples get a ~300-line ceiling instead. The top-doc-comment and
`examples/tests/support`-driven three-way-snapshot conventions are unchanged.

CI cost: Tier 1 (18 builds) plus four more examples brings the total to roughly 30 WASM builds,
estimated at about eight minutes of wall-clock. Before this stage is scheduled, measure the actual
docs-deploy wall-clock from the merged Tier 1 PR's CI run and confirm it lands near the ~5 minute
estimate; if it runs meaningfully over, revisit the ~12-minute mitigation threshold (matrix-
parallelize `docs.yml`'s WASM build job) before adding Tier 2's builds on top rather than after.

### Tier 3: games (later PRs)

`sokoban` first, as the pilot: pure grid-and-core logic, no external dependencies, no FOV or
pathfinding needed. Followed by a roguelike (`scrolling_roguelike`/`roguelike_dungeon` in the
original set) and `hex_battle` (hex coordinates plus tileset sprites with an ASCII fallback).

CI cost: roughly 39 WASM builds total once all three land (30 from Tiers 1-2 plus 9 for three game
examples), estimated at about eleven minutes of wall-clock. If a stage's addition pushes the docs
deploy workflow meaningfully past that, the next step is matrix-parallelizing the WASM build job in
`docs.yml` rather than trimming examples -- record that as the mitigation to try first, not
scope-cutting.

### Open gate: FOV and pathfinding for the Tier 3 roguelike (undecided)

Neither field-of-view (shadowcasting/raycasting) nor pathfinding (BFS/A\*) exists in any workspace
crate today. `sokoban` needs neither and is deliberately sequenced first so Tier 3 is not blocked
waiting on this decision. Before the roguelike example is scheduled, one of the following needs to
be chosen, and this ADR intentionally does not choose it:

- **Inline the algorithms in the example file.** No new dependency, but bloats the example past the
  template ideal and duplicates logic if a second game example needs the same algorithm.
- **Add an external, examples-only dev-dependency** (for example `bracket-pathfinding` or the
  `pathfinding` crate) to `examples/Cargo.toml` only. Fast and idiomatic, but introduces a gamedev
  dependency to the examples crate's build, including its WASM variants.
- **Offer FOV/pathfinding as real retroglyph API**, a new module or crate. This is a product
  decision about what the library offers, not an examples-scope decision, and is out of bounds for
  this ADR.

This is recorded here as an explicit open gate on Tier 3, to be resolved when Tier 3 is actually
scheduled, not now.

## Per-example validation gates

Every example, in every tier, must pass all of the following before merge:

1. Compiles for all three WASM variants (`wasm-headless`, `wasm-terminal`, `software`); a failure
   here blocks the docs deploy workflow, so verify locally before pushing.
2. Builds and runs on all three native backends (`--features crossterm`, `--features software`, and
   the headless-stdout fallback with no backend feature enabled).
3. All three snapshots are committed (headless text via `insta`, software PNG, crossterm SVG), and
   `cargo test -p retroglyph-examples --all-features` is green.
4. Any backend-specific capability gap degrades visibly (an on-screen note) rather than panicking or
   rendering a blank frame -- verified by hand for examples that have a fallback path.
5. `cargo run -p retroglyph-examples --bin runner` lists the example (it discovers
   `examples/examples/*.rs` at runtime, so this is a smoke check that the file is in the right place
   and compiles) and can launch it on each backend.
6. `just check` is green (fmt, clippy including `pedantic`/`nursery`, compile, tests, doc, llms),
   and Tier 1 examples stay within the roughly 150-line ceiling.
7. The example carries a top doc comment stating what it proves and how to run it.

## Alternatives considered

**A. An even leaner Tier 1 of three** (`hello_world`, `colors`, `keyboard`), deferring mouse,
layout, and layers to Tier 2. Would cut Tier 1 CI to about nine builds and further shrink the first
PR. Rejected as the default: mouse is the example with the trickiest cross-backend fallback story,
and deferring it only postpones that risk rather than removing it; layout and layers are cheap and
high-signal enough that dropping them buys little. Kept as a fallback if Tier 1's measured CI cost
turns out worse than estimated.

**B. Resurrect all 13 deleted originals at once.** Maximum ecosystem proof, highest-fidelity
gallery, and the least new design work, since all 13 already existed and worked. Rejected: it
directly contradicts the chosen staged, restrained scope; 13 examples x 3 WASM variants is about 39
builds (roughly twelve minutes) landing in a single PR; a single WASM regression anywhere in that
set blocks the entire docs deploy; and the roguelike/hex examples would immediately hit the
unresolved FOV/pathfinding gate before Tier 1 has even proven the core snapshot pipeline at small
scale. The staged roadmap in this ADR reaches the same destination, sequenced so each stage's cost
is measured before committing to the next.

**C. One kitchen-sink demo instead of many small examples.** A single example paging through colors,
input, layout, layers, and widgets in tabs would cost only three WASM builds and one gallery entry.
Rejected: it is a poor copy-paste template, which is the actual purpose of an example; one snapshot
cannot isolate which capability regressed when it breaks; a bug anywhere in it breaks the whole demo
instead of one focused test; and it hides per-feature backend parity rather than proving it per
capability. Every comparable library surveyed for this decision (ratatui, bracket-lib, FTXUI) ships
many small, focused examples rather than one composite demo, for the same reasons. A later Tier 3
game (`sokoban`, the roguelike) already serves the legitimate version of "show it all working
together."

## Consequences

- Tier 1 ships six small, single-capability examples that are both the docs gallery's entry point
  and a committed cross-backend regression suite, at roughly five minutes of added CI cost.
- There is no `styled_text` example; `02_colors` carries that role, matching what `Style` actually
  supports.
- `05_layout_grid` is intentionally more verbose than it would be with `retroglyph-widgets`, as a
  deliberate "before" baseline for a Tier 2 widgets example.
- `run_software_with` becomes available to every example after Tier 1, not just the ones that use it
  first.
- Tier 2 and Tier 3 are explicitly not blocked on any missing library capability; their only open
  question is the Tier 3 FOV/pathfinding dependency, deferred by design until Tier 3 is scheduled.
- Tier 2 ships four examples, not six: `benchmark` is cut entirely (performance measurement is a
  separate concern from the snapshot-based examples suite and does not belong in this crate) and
  `layers_compositing` is cut as a near-duplicate of `06_layers`.
- Tier 2 introduces `retroglyph-widgets` as an unconditional `examples/Cargo.toml` dependency and
  relaxes the per-example size ceiling to ~300 lines (from Tier 1's ~150), since Tier 2 examples are
  capability proofs rather than copy-paste templates.
- CI wall-clock for the docs deploy workflow grows with each stage (about 5 -> 8 -> 11 minutes) and
  must be watched; the agreed mitigation if a stage exceeds roughly twelve minutes is to
  matrix-parallelize the WASM build job, not to cut examples. Tier 2 should not be scheduled until
  Tier 1's actual measured docs-deploy wall-clock (not just the estimate) is confirmed from CI.

## Non-goals

- Deciding the Tier 3 FOV/pathfinding approach now. Recorded as an open gate, not resolved here.
- Building or wiring any example code. This ADR is scope and sequencing only.
- Revisiting the `Style` fg/bg-only design. That is accepted as given; examples work within it.
- A facade crate or any change to crate boundaries. Examples depend on whichever workspace crates
  each one needs, per ADR 014's "no facade, for now" decision.

## References

- [ADR 014: Workspace Split](014-workspace-split.md) -- the crate boundaries (`retroglyph-core`,
  `retroglyph-widgets`, `retroglyph-software`, and the rest) that this ADR's examples are scoped
  against.
- [ADR 004: E2E and Screenshot Testing Strategy](004-testing-strategy.md) -- the snapshot-over-pixel
  testing philosophy this ADR's three-way per-example harness (headless text, software PNG,
  crossterm SVG) continues.
- [ADR 008: Layers and Sub-cell Offsets](008-layer-composition.md) -- the layer/compositing model
  `06_layers` exercises.
- `.github/workflows/docs.yml` -- discovers `examples/examples/*.rs` and builds each to three WASM
  variants deployed to the docs gallery; the source of every example's real CI cost referenced
  throughout this ADR.
