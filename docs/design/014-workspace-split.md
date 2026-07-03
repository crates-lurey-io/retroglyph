# ADR 014: Workspace Split

**Status:** Draft **Date:** 2026-07-02 **Supersedes:**
[ADR 001 §1: "single crate, split later"](001-architecture.md) **Amends:**
[ADR 016: Widget-Trait Verdict](016-widget-trait-verdict.md) (revives `retroglyph-widgets` as a
published crate ahead of the "second consumer" bar ADR 016 set) **Relates to:**
[ADR 015: Cross-Backend Rendering and Loop Consistency](015-cross-backend-consistency.md) (Accepted,
landed first as planned)

## Context

ADR 001 chose a single-crate structure with the expectation that it would split "when the API
stabilizes and compile times or dependency isolation warrant it." ADR 015 has landed (`Tile`'s
public shape, `Backend::composites_layers`, and the `App`/`Flow`/`Frame`/`run_blocking`/`FrameClock`
loop surface are settled), which was the explicit precondition. The split is unblocked.

Reasons to split, unchanged from the original draft:

1. **Dependency bloat.** `software-tilesets` pulls in winit, softbuffer, image, and alpha-blend.
   Separate crates make the dependency tree explicit at the Cargo.toml level instead of hiding
   behind feature-gated optional deps in one crate.

2. **Compile-time isolation.** The software backend is 2,863 lines across 6 files, nearly as large
   as the rest of the library combined. Touching `grid.rs` recompiles everything today. In a
   workspace, `retroglyph-software` only recompiles when its own sources or `retroglyph-core`
   change.

3. **Independent versioning.** Backend crates can release breaking changes (e.g. winit 0.30 -> 0.31)
   without bumping the core crate.

4. **Clearer ownership boundaries.** Core types are nearly stable; the software backend is still
   evolving. Separate crates make this visible.

A fifth reason emerged during ADR 015 and the dashboard/roguelike examples: the `Backend` trait
fuses input (`poll_event`/`push_event`) and output (`draw_layers`/`flush`/`size`/`resize`/cursor)
into one contract. That's fine for terminal backends, where a single process owns both. It's wrong
for windowed backends, where a shared winit event loop owns input and a per-renderer surface
(softbuffer today, wgpu/GL soon) owns output. This ADR resolves that now rather than deferring it,
because a GL backend prototype is imminent and two real consumers are the right number to validate
the seam against (see "Windowed backend family" below).

## Internal dependency graph (current, single crate)

```text
color ─────────────┐
                   ▼
style ◄──── color  │
  │                │
  ▼                │
tile ◄──── style   │
  │                │
  ▼                ▼
grid ◄──── tile, style, (egc: TileFlags, cap_grapheme)
  │
  ├──► camera ◄──── grid::{Pos, Rect, Size}   (pure geometry, no rendering opinion)
  │
  ▼
event ◄──── grid::Pos
  │
  ▼
text ◄──── style
  │
  ▼
layout ◄──── text, grid::Rect, style, terminal, Backend
  │
  ▼
terminal ◄──── Backend, color, event, grid, style, text, tile
  │
  ├──► backend::Backend  (trait, depends on event, grid, tile)
  ├──► backend::Headless  (depends on Backend, event, grid, tile)
  ├──► backend::Crossterm (depends on Backend, event, grid, tile, color, style)
  └──► backend::software  (depends on Backend, color, event, grid, style, tile,
                            + winit, softbuffer, image, alpha-blend, log)
```

Everything above the `backend::` line, plus `camera`, forms the core. Each backend depends on the
core but not on other backends. This is the natural crate boundary.

## Decision

Split into a Cargo workspace with **five published crates**: `retroglyph-core`,
`retroglyph-crossterm`, `retroglyph-window`, `retroglyph-software`, and `retroglyph-widgets`. One
internal, unpublished tools crate (`cargo-bin`) stays as-is. **No facade crate (`retroglyph`) in
this pass** — see "No facade, for now" below.

### Workspace layout

```text
Cargo.toml              (workspace root, no [package])
crates/
  core/                  retroglyph-core
    src/
      lib.rs
      color.rs
      style.rs
      tile.rs
      grid.rs
      camera.rs          Camera: world/screen viewport geometry, no rendering opinion
      event.rs
      text.rs
      layout.rs
      terminal.rs
      app.rs              App/Flow/Frame, step, run_blocking (std) -- ADR 015
      frame_clock.rs      FrameClock accumulator (no_std)
      backend/
        mod.rs            Backend trait + Headless
        headless.rs
  crossterm/               retroglyph-crossterm
    src/
      lib.rs               Crossterm backend
  window/                  retroglyph-window  (new, extracted now, not deferred)
    src/
      lib.rs
      loop_.rs             winit event loop + wasm requestAnimationFrame variant
      translate.rs         winit-event -> retroglyph::Event translation
      run.rs               inverted run(app) driver (ADR 015 Decision 2, piece 3)
      presenter.rs         Presenter trait (rasterization seam)
      backend.rs           WindowBackend<P: Presenter>: implements Backend by
                            owning input + delegating output to P
  software/                retroglyph-software
    src/
      lib.rs
      renderer.rs          SoftwareRenderer: implements Presenter via softbuffer
      bitmap_font.rs
      config.rs
      sprite_cache.rs
      tileset.rs
  widgets/                 retroglyph-widgets
    src/
      lib.rs               Widget helpers: panel, gauge, list, tabs, sparkline;
                            layout splitter (split_v/split_h + Constraint)
  (future)                 retroglyph-wgpu, retroglyph-gl -- Presenter impls, not yet built
tools/
  cargo-bin/               (existing, stays)
examples/                  workspace root; depend directly on retroglyph-core +
                            retroglyph-crossterm / retroglyph-window+software as needed
tests/                     workspace root integration tests
```

### Crate responsibilities

#### `retroglyph-core`

The `no_std`-compatible foundation. Contains:

- `Color`, `AnsiColor`, `Style`, `CellModifier`
- `Tile`, `TileFlags`
- `Grid`, `Pos`, `Rect`, `Size`
- `Camera` (see "Where Camera lives" below)
- `Event`, `KeyEvent`, `MouseEvent`, `KeyCode`, `KeyModifiers`
- `Span`, `Line`, `TextLayout`, `HAlign`, `VAlign`, `TextMetrics`
- `Terminal<B>`
- `Backend` trait + `Headless` backend
- `App`, `Flow`, `Frame` loop contract; `step` + `run_blocking` generic driver (`std`); `FrameClock`
  accumulator

Dependencies: `ixy`, `grixy`, `unicode-width`, `bitflags`, and optionally `unicode-segmentation`
(behind `egc` feature). No platform-specific deps.

Feature flags:

- `std` (default) -- enables std-dependent code
- `egc` (default) -- extended grapheme cluster support

#### Where Camera lives

`Camera` converts world coordinates to screen coordinates for a viewport smaller than the world it
scrolls over. It has no rendering opinion, no allocation, and depends only on `Pos`/`Rect`/`Size`,
which are already in core. Three prior-art patterns exist for this kind of thing:

- **bracket-lib (RLTK)** ships no Camera at all. Its own roguelike tutorial has readers hand-write
  one _in the game_, tracking `left_x`/`right_x`/`top_y`/`bottom_y` by hand. Treated as
  game-specific, not a library concern.
- **python-tcod-camera** is a standalone, optional package, explicitly decoupled: it works with or
  without tcod and has no rendering opinion, by design, so non-tcod users can use it too.
- **ratatui**'s `Viewport` (a different but related concept: which region of the terminal ratatui
  draws into) lives in `ratatui-core`. The signal from ratatui's split is that surface-mapping math
  belongs in core, not in a widgets/extras crate.

Options considered:

| Option                                   | Verdict                                                                                                                                                                                        |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Stay in `retroglyph-core` (current)      | **Chosen.** Zero new deps, no breaking change to the existing `retroglyph::Camera` path, matches ratatui's precedent that surface-mapping math is core, not extras.                            |
| Move to `retroglyph-widgets`             | Rejected: shape mismatch (Camera does no drawing, unlike gauge/panel/list which all take `&mut Terminal<B>, Rect`); would force Camera-only users to depend on widgets and its future deps.    |
| New standalone `retroglyph-camera` crate | Rejected: this ADR's own non-goals section already rejects splitting tightly related, small, dependency-free things into many tiny crates. A ~200-line dep-free struct doesn't clear that bar. |
| Core, but feature-gated                  | Rejected: zero deps means zero compile-time cost to gate; feature-gating adds config surface for no benefit.                                                                                   |

`camera.rs` moves into `retroglyph-core` alongside `grid.rs`, unchanged, in the "what moves where"
table below.

#### `retroglyph-crossterm`

Crossterm backend. Depends on `retroglyph-core` and `crossterm`. Always requires `std`. Implements
`Backend` directly (not `WindowBackend`/`Presenter` -- crossterm has no window, no winit, and is
entirely outside the windowed family).

Exposes `Crossterm` and the `From`/`TryFrom` conversions for events and colors. No feature flags of
its own.

#### `retroglyph-window` (new, extracted now)

The shared surface for every windowed backend (software today; wgpu and GL prototypes soon). Depends
on `retroglyph-core` and `winit`. Owns:

- the winit window + event loop, and the wasm `requestAnimationFrame` variant,
- winit-event -> `retroglyph::Event` translation (keys, mouse, modifiers, resize, scale factor,
  close),
- the inverted `run(app)` driver (ADR 015 Decision 2, piece 3),
- cursor, DPI, and resize handling,
- the `Presenter` trait (the rasterization seam), and
- `WindowBackend<P: Presenter>`, a generic `Backend` impl that owns input and delegates output to
  `P`. This is the concrete answer to the trait-fusion problem: `Backend` itself is unchanged and
  stays fused for terminal-family backends (crossterm, headless), but windowed backends get their
  `Backend` impl "for free" from `WindowBackend`, and only need to implement the smaller `Presenter`
  trait (`draw_layers`, `flush`, `size`, `resize`, cursor-shape-if-supported).

```rust
// retroglyph-window/src/backend.rs (sketch)
pub trait Presenter {
    fn draw_layers(&mut self, layers: &[&Grid]);
    fn flush(&mut self);
    fn size(&self) -> Size;
    fn resize(&mut self, size: Size);
}

pub struct WindowBackend<P: Presenter> {
    window: winit::window::Window,
    events: VecDeque<Event>, // filled by the winit loop, drained by poll_event
    presenter: P,
}

impl<P: Presenter> Backend for WindowBackend<P> {
    // poll_event/push_event: shared, from the winit loop
    // draw_layers/flush/size/resize: delegate to self.presenter
}
```

`retroglyph-software` becomes a `Presenter` implementation (via softbuffer), not a `Backend`
implementation. `retroglyph-wgpu` and `retroglyph-gl`, when built, do the same.

**Accepted risk:** the ADR's earlier draft argued against extracting this now, on the grounds that a
single consumer (softbuffer) can't validate a seam that also needs to fit wgpu's async device/queue
init and GL's context-current-on-thread requirement. We're proceeding anyway because a GL prototype
is coming soon and will exercise the seam quickly. If the GL prototype reveals `Presenter` doesn't
fit, `retroglyph-window` is young enough (0.x, low adoption) to take a breaking change without the
coordination cost it would have after a wider release. Tracked as a risk below, not re-litigated
here.

#### `retroglyph-software`

Software rendering backend, now a thin `Presenter` implementation. Depends on `retroglyph-core`,
`retroglyph-window`, `softbuffer`, `log`. Always requires `std`. No longer depends on `winit`
directly (that's `retroglyph-window`'s job).

Exposes `SoftwareRenderer` (implements `Presenter`), `BitmapFont`, and the sprite/tileset types. A
type alias `pub type SoftwareBackend = WindowBackend<SoftwareRenderer>;` preserves the
`SoftwareBackend` name users already know.

Feature flags:

- `tilesets` -- PNG sprite sheet support (adds `image`, `alpha-blend`)
- `default-font` -- embedded VGA 8x16 bitmap font

#### `retroglyph-widgets` (shipped now, amends ADR 016)

ADR 016 concluded free functions in `examples/util/draw.rs` were sufficient and declined to extract
a crate, absent a second consumer. That verdict is **overridden here**: `retroglyph-widgets` ships
as part of this split regardless of a second consumer showing up. Depends on `retroglyph-core` only.

Contains the `Widget` helpers (panel/border, gauge, list, tabs, sparkline) and the layout splitter
(`split_v`/`split_h` + `Constraint::{Fixed, Percent, Fill}`) that graduate from
`examples/util/draw.rs` and `examples/util/layout.rs`. `Camera` does **not** move here (see above).
Games that draw manually pay nothing -- this is an optional, separately-versioned crate.

No feature flags of its own initially.

### No facade, for now

The original draft of this ADR planned a `retroglyph` facade crate re-exporting core plus
feature-gated backends, matching the pre-split single-crate API (`use retroglyph::Terminal`,
`use retroglyph::Crossterm`). **That's deferred out of this pass.**

Rationale: with no external consumers yet, the facade's only job today would be internal -- letting
`examples/` depend on one crate instead of several. That's not worth the re-export-churn cost (new
public types in a backend crate need a matching feature-gated re-export in the facade, and it's easy
for that to go stale silently) before there's a real external user who benefits from it. Instead:

- `examples/` and `tests/` depend directly on `retroglyph-core`, `retroglyph-crossterm`,
  `retroglyph-window` + `retroglyph-software`, and `retroglyph-widgets`, per-example, as needed.
- The `retroglyph` name stays reserved on crates.io (unpublished) for a future facade.
- Revisit as its own ADR once there's a concrete external-user reason to want a single re-export
  crate again (same "split/unify when it warrants it" logic as ADR 001 and the deferred
  `retroglyph-app`).

### What moves where

| Current path                       | Destination crate      | Notes                                            |
| ---------------------------------- | ---------------------- | ------------------------------------------------ |
| `src/color.rs`                     | `retroglyph-core`      |                                                  |
| `src/style.rs`                     | `retroglyph-core`      |                                                  |
| `src/tile.rs`                      | `retroglyph-core`      |                                                  |
| `src/grid.rs`                      | `retroglyph-core`      |                                                  |
| `src/camera.rs`                    | `retroglyph-core`      | Pure geometry, depends only on grid types        |
| `src/event.rs`                     | `retroglyph-core`      |                                                  |
| `src/text.rs`                      | `retroglyph-core`      |                                                  |
| `src/layout.rs`                    | `retroglyph-core`      | Depends on `Terminal`, stays with it             |
| `src/terminal.rs`                  | `retroglyph-core`      |                                                  |
| `src/backend/mod.rs`               | `retroglyph-core`      | `Backend` trait only                             |
| `src/backend/headless.rs`          | `retroglyph-core`      | Zero external deps, used in tests                |
| `src/backend/crossterm.rs`         | `retroglyph-crossterm` |                                                  |
| `src/backend/software/window.rs`   | `retroglyph-window`    | winit loop, event translation, inverted `run`    |
| `src/backend/software/*.rs` (rest) | `retroglyph-software`  | Becomes a `Presenter` impl, not a `Backend` impl |
| `examples/util/draw.rs`            | `retroglyph-widgets`   |                                                  |
| `examples/util/layout.rs`          | `retroglyph-widgets`   |                                                  |

### Headless stays in core

`Headless` has no external dependencies and is the primary testing backend. Every crate that tests
`Terminal<B>` needs it. Putting it in core avoids a circular dependency and means users get snapshot
testing support without adding a dev-dependency.

## Migration plan

### Step 1: Create workspace skeleton

- Move `[package]` out of the root `Cargo.toml` into `crates/core/Cargo.toml`.
- Root `Cargo.toml` becomes workspace-only with a `[workspace]` members list.
- Create `crates/core/`, `crates/crossterm/`, `crates/window/`, `crates/software/`,
  `crates/widgets/`.
- Move source files per the table above, including `camera.rs` and the widget helpers.
- Replace `use crate::` with `use retroglyph_core::` in backend crates.

### Step 2: Extract the Presenter seam

- Define `Presenter` and `WindowBackend<P: Presenter>` in `retroglyph-window`.
- Port `retroglyph-software`'s existing `Backend` impl to a `Presenter` impl (`SoftwareRenderer`).
- Add the `SoftwareBackend = WindowBackend<SoftwareRenderer>` type alias to keep the public name
  stable.
- Verify `tests/software_renderer.rs` and `tests/e2e_snapshots.rs` still pass unchanged in behavior.

### Step 3: Move examples and tests

- Examples stay at workspace root, depend directly on the backend crate(s) each example needs (no
  facade).
- Integration tests depend on specific backend crates as dev-dependencies.
- Unit tests within each module move with their source files.

### Step 4: Update CI and tooling

- `just check` runs `cargo check --workspace --all-features`.
- `just test` runs `cargo test --workspace --all-features`.
- `just clippy` runs against the workspace.
- Add `cargo-hack` for feature-powerset checking (`--each-feature` at minimum), wired into **both**
  a local `just` recipe and CI -- not CI-only. The local recipe must be fast enough to run before
  every push, not just in CI, per the "don't abandon the local dev story" principle.
- Codecov flags (ADR 013) map one flag per crate; update `codecov.yml` path filters as part of the
  PR that adds each new crate (checklist item, not automated yet).
- `cargo-llvm-cov --workspace` generates a single report; `codecov.yml` path filters split it.

### Step 5: Publish

Deferred. See [ADR 017: Release Process and Workspace Tooling](017-release-and-workspace-tooling.md)
(stub). Publishing is **not** part of this split; the workspace can exist, build, and be tested
entirely from git without ever being pushed to crates.io. When it does happen, order is
dependency-driven:

1. `retroglyph-core` (no in-workspace deps)
2. `retroglyph-window` (depends on `retroglyph-core`)
3. `retroglyph-crossterm` (depends on `retroglyph-core`)
4. `retroglyph-widgets` (depends on `retroglyph-core`)
5. `retroglyph-software` (depends on `retroglyph-core`, `retroglyph-window`)

(2)-(4) can publish in any order relative to each other; (5) must come after (2).

## Versioning strategy

All five crates start at `0.2.0` (the split itself is the breaking change from the single-crate
`0.1.x`), whenever the first publish happens (see ADR 017).

After that, crates version independently:

- `retroglyph-core` bumps affect all downstream crates (they depend on it).
- `retroglyph-crossterm` can bump for crossterm upgrades without touching anything else.
- `retroglyph-window` can bump for winit upgrades; ripples to `retroglyph-software` and any future
  wgpu/GL crate, but not to `retroglyph-crossterm`.
- `retroglyph-software` can bump for softbuffer upgrades independently.
- `retroglyph-widgets` can bump on its own release cadence as widgets are added.

## Workspace-level configuration

Shared lint and metadata config goes in the root `Cargo.toml` to avoid duplication:

```toml
[workspace]
members = ["crates/*", "tools/cargo-bin"]
resolver = "2"

[workspace.package]
edition = "2024"
rust-version = "1.88"
license = "MIT"
repository = "https://github.com/crates-lurey-io/retroglyph"

[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
unreachable_pub = "warn"
unused_qualifications = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
nursery = { level = "deny", priority = -1 }
must_use_candidate = "deny"
missing_errors_doc = "deny"
missing_panics_doc = "deny"
module_name_repetitions = "allow"
```

Each crate inherits with `[lints] workspace = true` and `edition.workspace = true`.

## Risks

**Presenter designed against effectively one consumer.** Accepted and discussed above under
`retroglyph-window`. Mitigated by an imminent GL prototype, not eliminated. If the seam is wrong,
expect an early breaking `retroglyph-window` release, not a silent workaround.

**No facade means multi-crate Cargo.toml entries for every consumer**, including our own examples.
This is a real UX regression versus the single-crate `retroglyph = { features = [...] }` story users
have today, deferred deliberately (see "No facade, for now"). Track how much friction this actually
causes in `examples/` and `tests/` as a signal for when to revisit.

**Cross-crate doc links.** Rustdoc `[Type]` links that cross crate boundaries need full paths
(`retroglyph_core::Grid` instead of `crate::Grid`). One-time fixup during migration, ongoing
discipline afterward.

**MSRV alignment.** All crates share the same MSRV (1.88) via `workspace.package`. No crate can
independently raise its MSRV without raising the workspace MSRV. Accepted: simplicity over
flexibility.

**Dev-dependency cycles.** If `retroglyph-crossterm` wants `Headless` in its tests, it already
depends on `retroglyph-core`. No cycle.

**Feature flag matrix growth.** Even without a facade, `retroglyph-software`'s `tilesets` /
`default-font` flags and `retroglyph-window`'s wasm variant stack up. `cargo-hack` (Step 4) is the
mitigation; keep the local recipe, not just CI, current.

## Deferred: `retroglyph-app`

Unchanged from the original draft: the `App`/`Flow`/`Frame` contract and the generic `run_blocking`
driver live in `retroglyph-core`; the inverted driver lives in `retroglyph-window`. Small, dep-free
app-level utilities (e.g. `InputMap`) stay as loose modules graduating from `examples/util/`,
promoted to a published `retroglyph-app` only if a retained-mode layer materializes later.

## Windowed backend family and `retroglyph-window`

This is now a **decision**, not a future note: `retroglyph-window` is extracted as part of this
split (see above), ahead of the second windowed backend actually landing, because a GL prototype is
imminent enough to validate the seam quickly rather than guess at it in the abstract.

```text
retroglyph-core
  └── retroglyph-window     winit loop, event translation, inverted run, Presenter trait,
        │                   WindowBackend<P: Presenter>
        ├── retroglyph-software   Presenter via softbuffer
        ├── retroglyph-wgpu       Presenter via wgpu           (not yet built)
        └── retroglyph-gl         Presenter via glutin/glow    (prototype imminent)
```

`retroglyph-crossterm` stays entirely outside this family: no winit, no `Presenter`, implements
`Backend` directly, driven by core's `run_blocking`.

Open question carried forward: a glyph/tile atlas _layout_ (glyph -> sprite cell) is common to
`software-tilesets` and every GPU backend; only upload and sampling are backend-specific. Whether
that becomes a shared `retroglyph-atlas` module is a smaller question to revisit once the GL
prototype's needs are concrete, not now.

## Non-goals

- **Splitting grid/terminal into separate crates.** The internal dependency graph between color,
  style, tile, grid, event, text, layout, and terminal is dense. Splitting these would create 5+
  tiny crates with tight coupling. Not worth the friction.
- **A facade crate in this pass.** Deferred; see "No facade, for now" above.
- **Plugin/dynamic backend loading.** Backends are compile-time selections.
- **Workspace-level feature unification beyond each crate's own Cargo.toml.** No workspace-wide
  feature resolver beyond what Cargo provides.
- **Publishing to crates.io.** Deferred to ADR 017.

## References

- [ADR 001 §1: "single crate, split later"](001-architecture.md)
- [ADR 007: Software Backend](007-software-backend.md) -- winit-owned loop, source of the windowing
  layer
- [ADR 011: WASM Portability](011-wasm-portability-revised.md) -- rAF frame gating
- [ADR 013: Codecov](013-codecov.md) -- flag-per-crate model designed for this split
- [ADR 015: Cross-Backend Rendering and Loop Consistency](015-cross-backend-consistency.md) -- loop
  decomposition, `composites_layers`, windowed family (Accepted, landed first as planned)
- [ADR 016: Widget-Trait Verdict](016-widget-trait-verdict.md) -- amended by this ADR;
  `retroglyph-widgets` ships now rather than waiting for a second consumer
- [ADR 017: Release Process and Workspace Tooling](017-release-and-workspace-tooling.md) -- stub,
  publishing/changelog/release tooling deferred here
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
