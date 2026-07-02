# ADR 014: Workspace Split

**Status:** Draft  
**Date:** 2026-06-21  
**Supersedes:** [ADR 001 §1: "single crate, split later"](001-architecture.md)

## Context

ADR 001 chose a single-crate structure with the expectation that it would split "when the API
stabilizes and compile times or dependency isolation warrant it." That time is approaching:

1. **Dependency bloat.** Enabling `software-tilesets` pulls in winit, softbuffer, image, and
   alpha-blend. A user who only wants crossterm pays for none of that today (feature-gated), but
   downstream crates that depend on `retroglyph` with default features still download and compile
   the optional dep metadata. Separate crates make the dependency tree explicit at the Cargo.toml
   level.

2. **Compile-time isolation.** The software backend is 2,863 lines across 6 files, nearly as large
   as the rest of the library combined (3,799 lines). Touching `grid.rs` recompiles everything. In a
   workspace, `retroglyph-software` only recompiles when its own sources or `retroglyph-core`
   change.

3. **Independent versioning.** Backend crates can release breaking changes (e.g., upgrading winit
   0.30 to 0.31) without bumping the core crate. Users on crossterm are unaffected.

4. **Clearer ownership boundaries.** The `Backend` trait, core types, and each backend have distinct
   stability profiles. Core types are nearly stable; the software backend is still evolving
   (tilesets, sprite cache, windowed trait). Separate crates make this visible.

The module structure already anticipates the split (ADR 001's stated goal). The internal dependency
graph is clean enough that the split is mechanical.

## Internal dependency graph (current)

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

Everything above the `backend::` line forms the core. Each backend depends on the core but not on
other backends. This is the natural crate boundary.

## Decision

Split into a Cargo workspace with five published crates (`retroglyph-core`, `retroglyph-crossterm`,
`retroglyph-software`, `retroglyph-widgets`, and the `retroglyph` facade) and one internal tools
crate. Further backend crates (`retroglyph-window`, `retroglyph-wgpu`, `retroglyph-gl`) join later;
see the windowed-family note below.

### Workspace layout

```text
Cargo.toml              (workspace root, no [package])
crates/
  core/                 retroglyph           (the facade crate)
    src/
      lib.rs            re-exports from retroglyph-core + backends
  retroglyph-core/      retroglyph-core
    src/
      lib.rs
      color.rs
      style.rs
      tile.rs
      grid.rs
      event.rs
      text.rs
      layout.rs
      terminal.rs
      app.rs            App/Flow/Frame, step, run_blocking (std) -- ADR 015
      frame_clock.rs    FrameClock accumulator (no_std)
      backend/
        mod.rs          Backend trait + Headless
        headless.rs
  crossterm/            retroglyph-crossterm
    src/
      lib.rs            Crossterm backend
  software/             retroglyph-software
    src/
      lib.rs
      window.rs         winit loop + event translation + inverted run (liftable)
      bitmap_font.rs
      config.rs
      windowed.rs
      sprite_cache.rs
      tileset.rs
  widgets/             retroglyph-widgets
    src/
      lib.rs           Widget trait; panel, gauge, list, tabs, sparkline; camera/viewport
  (future) window/     retroglyph-window  -- shared winit/window layer, extracted
                       from software when the 2nd windowed backend (wgpu/gl) lands
tools/
  cargo-bin/            (existing, stays)
examples/               (workspace root, depends on retroglyph facade)
tests/                  (workspace root integration tests)
```

### Crate responsibilities

#### `retroglyph-core`

The `no_std`-compatible foundation. Contains:

- `Color`, `AnsiColor`, `Style`, `CellModifier`
- `Tile`, `TileFlags`
- `Grid`, `Pos`, `Rect`, `Size`
- `Event`, `KeyEvent`, `MouseEvent`, `KeyCode`, `KeyModifiers`
- `Span`, `Line`, `TextLayout`, `HAlign`, `VAlign`, `TextMetrics`
- `Terminal<B>`
- `Backend` trait + `Headless` backend
- `App`, `Flow`, `Frame` loop contract; `step` + `run_blocking` generic driver (`std`); `FrameClock`
  accumulator (see ADR 015 Decision 2)

Dependencies: `ixy`, `grixy`, `unicode-width`, `bitflags`, and optionally `unicode-segmentation`
(behind `egc` feature). No platform-specific deps.

The loop _contract_ lives here because `App` is the dual of `Backend` (update contract vs output
contract), and the generic blocking driver is dep-free. The _inverted_ driver does not live here; it
belongs to the windowing layer.

Feature flags:

- `std` (default) -- enables std-dependent code
- `egc` (default) -- extended grapheme cluster support

#### `retroglyph-crossterm`

Crossterm backend. Depends on `retroglyph-core` and `crossterm`. Always requires `std`.

Exposes `Crossterm` and the `From`/`TryFrom` conversions for events and colors.

No feature flags of its own (crossterm's features are re-exported if needed).

#### `retroglyph-software`

Software rendering backend. Depends on `retroglyph-core`, `winit`, `softbuffer`, `log`. Always
requires `std`.

Exposes `SoftwareBackend`, `SoftwareBackendBuilder`, `SoftwareRenderer`, `WindowedBackend`,
`BitmapFont`, and the sprite/tileset types.

Feature flags:

- `tilesets` -- PNG sprite sheet support (adds `image`, `alpha-blend`)
- `default-font` -- embedded VGA 8x16 bitmap font

The winit event loop, winit-event → `retroglyph::Event` translation, and the inverted `run(app)`
driver (ADR 015 Decision 2, piece 3) live in a self-contained `window.rs` module, kept liftable so
it can be extracted to `retroglyph-window` when the first GPU backend lands (see the windowed-family
note below).

#### `retroglyph-widgets`

Immediate-mode drawing helpers over a `Rect`. Depends on `retroglyph-core` only. Contains the
`Widget` trait plus panel/border, gauge/progress bar, list/menu, tabs, sparkline, and a
camera/viewport for rendering a world `Grid` larger than the screen. Graduates
`examples/util/draw.rs`. Optional: games that draw manually pay nothing. Separated because it
imposes a rendering model and churns as widgets are added.

#### `retroglyph` (facade)

The user-facing crate. Re-exports everything from `retroglyph-core` unconditionally, and from
backend crates behind feature flags. Users write `retroglyph = { features = ["crossterm"] }` in
their Cargo.toml, same as today.

```rust
// retroglyph/src/lib.rs
pub use retroglyph_core::*;

#[cfg(feature = "crossterm")]
pub use retroglyph_crossterm as crossterm_backend;
#[cfg(feature = "crossterm")]
pub use retroglyph_crossterm::Crossterm;

#[cfg(feature = "software")]
pub use retroglyph_software as software_backend;
#[cfg(feature = "software")]
pub use retroglyph_software::{SoftwareBackend, SoftwareBackendBuilder, SoftwareRenderer};

#[cfg(feature = "widgets")]
pub use retroglyph_widgets as widgets;

// Feature-selected loop entry, replacing the old `rg_run!` macro (ADR 015).
// Terminal backends resolve to core's generic driver; windowed backends resolve
// to the windowing layer's inverted driver.
#[cfg(all(feature = "crossterm", not(feature = "software")))]
pub use retroglyph_crossterm::run;
#[cfg(feature = "software")]
pub use retroglyph_software::run;
```

Feature flags on the facade map to optional dependencies:

```toml
[features]
default = ["std", "egc"]
std = ["retroglyph-core/std"]
egc = ["retroglyph-core/egc"]
crossterm = ["dep:retroglyph-crossterm"]
software = ["dep:retroglyph-software"]
software-tilesets = ["software", "retroglyph-software/tilesets"]
software-default-font = ["software", "retroglyph-software/default-font"]
widgets = ["dep:retroglyph-widgets"]
```

This preserves the existing user-facing API. `use retroglyph::Terminal` and
`use retroglyph::Crossterm` work exactly as before.

### What moves where

| Current path                | Destination crate      | Notes                                |
| --------------------------- | ---------------------- | ------------------------------------ |
| `src/color.rs`              | `retroglyph-core`      |                                      |
| `src/style.rs`              | `retroglyph-core`      |                                      |
| `src/tile.rs`               | `retroglyph-core`      |                                      |
| `src/grid.rs`               | `retroglyph-core`      |                                      |
| `src/event.rs`              | `retroglyph-core`      |                                      |
| `src/text.rs`               | `retroglyph-core`      |                                      |
| `src/layout.rs`             | `retroglyph-core`      | Depends on `Terminal`, stays with it |
| `src/terminal.rs`           | `retroglyph-core`      |                                      |
| `src/backend/mod.rs`        | `retroglyph-core`      | `Backend` trait only                 |
| `src/backend/headless.rs`   | `retroglyph-core`      | Zero external deps, used in tests    |
| `src/backend/crossterm.rs`  | `retroglyph-crossterm` |                                      |
| `src/backend/software/*.rs` | `retroglyph-software`  |                                      |

### Headless stays in core

`Headless` has no external dependencies and is the primary testing backend. Every crate that tests
`Terminal<B>` needs it. Putting it in core avoids a circular dependency (backend crate -> core for
types, core -> backend crate for test backend) and means users get snapshot testing support without
adding a dev-dependency.

## Migration plan

**Gated on [ADR 015](015-cross-backend-consistency.md).** ADR 015 lands first: it is the
API-stabilizing work ADR 001 §1 named as the precondition for this split. Once `Tile`'s public
shape, the `Backend::composites_layers` capability, and core's loop surface
(`App`/`Flow`/`Frame`/`run_blocking`/`FrameClock`) are settled and the examples are ported off
`rg_run!`, the split below is mechanical: no public API changes, no new abstractions.

### Step 1: Create workspace skeleton

- Move `[package]` out of the root `Cargo.toml` into `crates/core/Cargo.toml`.
- Root `Cargo.toml` becomes workspace-only with `[workspace]` members list.
- Create `crates/retroglyph-core/`, `crates/crossterm/`, `crates/software/`, `crates/core/`.
- Move source files per the table above.
- Replace `use crate::` with `use retroglyph_core::` in backend crates.

### Step 2: Wire up the facade

- `crates/core/Cargo.toml` depends on `retroglyph-core` unconditionally and backend crates as
  optional deps behind feature flags.
- `crates/core/src/lib.rs` re-exports everything.
- Verify `cargo build --all-features` and `cargo build` (default features only) both compile.

### Step 3: Move examples and tests

- Examples stay at workspace root, depend on the `retroglyph` facade crate.
- Integration tests (`tests/e2e.rs`, `tests/software_renderer.rs`) depend on specific backend crates
  as dev-dependencies.
- Unit tests within each module move with their source files.

### Step 4: Update CI and tooling

- `just check` runs `cargo check --workspace --all-features`.
- `just test` runs `cargo test --workspace --all-features`.
- `just clippy` runs against the workspace.
- Codecov flags (ADR 013) map one flag per crate.
- `cargo-llvm-cov --workspace` generates a single report; `codecov.yml` path filters split it.

### Step 5: Publish

Publishing order matters because crates.io resolves dependencies at publish time:

1. `retroglyph-core` (no in-workspace deps)
2. `retroglyph-crossterm` (depends on `retroglyph-core`)
3. `retroglyph-software` (depends on `retroglyph-core`)
4. `retroglyph` facade (depends on all three)

Use `cargo-release` or `release-plz` to automate this sequence.

## Versioning strategy

All four crates start at `0.2.0` (the split itself is the breaking change from the single-crate
`0.1.x`).

After the initial release, crates version independently:

- `retroglyph-core` bumps affect all downstream crates (they depend on it).
- `retroglyph-crossterm` can bump for crossterm upgrades without touching core or software.
- `retroglyph-software` can bump for winit/softbuffer upgrades independently.
- The `retroglyph` facade bumps when it changes its re-export surface or when a backend crate has a
  major version bump that changes the facade's feature flag behavior.

The facade's Cargo.toml uses version ranges to stay compatible:

```toml
[dependencies]
retroglyph-core = "0.2"
retroglyph-crossterm = { version = "0.2", optional = true }
retroglyph-software = { version = "0.2", optional = true }
```

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

**Re-export churn.** If `retroglyph-core` adds a public type, the facade must re-export it. Glob
re-exports (`pub use retroglyph_core::*`) handle this automatically for the core crate. Backend
crates need explicit re-exports since they are behind feature flags.

**Cross-crate doc links.** Rustdoc `[`Type`]` links that cross crate boundaries need full paths
(`retroglyph_core::Grid` instead of `crate::Grid`). This is a one-time fixup during migration.

**Dev-dependency cycles.** If `retroglyph-crossterm` wants to use `Headless` in its tests, it adds
`retroglyph-core` as a regular dependency (which it already has) and uses `Headless` from there. No
cycle.

**MSRV alignment.** All crates share the same MSRV (1.88) via `workspace.package`. A backend crate
cannot independently raise its MSRV without raising the workspace MSRV.

## Deferred: `retroglyph-app`

An earlier sketch put the game-loop scaffolding in a dedicated `retroglyph-app` crate. After
decomposing the loop (ADR 015 Decision 2), almost nothing is left for it to own: the
`App`/`Flow`/`Frame` contract and the generic `run_blocking` driver live in `retroglyph-core`, and
the inverted driver lives in the windowing layer. The only genuinely app-level utilities that remain
are small and dep-free (for example `InputMap`, a rebindable key→action map graduating from
`examples/util/action.rs`). Those go in the facade behind features rather than a separate published
crate.

Promote to `retroglyph-app` later only if a retained-mode layer materializes: a screen/state stack,
focus and event routing over a retained widget tree, or an ECS bridge. Because the utilities already
sit behind a facade feature, moving them into a crate the facade re-exports keeps the public path
(for example `retroglyph::InputMap`) stable. Same "split when it warrants it" logic as ADR 001.

## Future: the windowed backend family and `retroglyph-window`

Planned GPU backends (`wgpu`, `opengl`, `webgl2`) join `retroglyph-software` as a **windowed
family** sharing a large, backend-agnostic surface:

- winit window + event loop (and the wasm `requestAnimationFrame` variant),
- winit-event → `retroglyph::Event` translation (keys, mouse, modifiers, resize, scale factor,
  close),
- the inverted `run(app)` driver (ADR 015 Decision 2, piece 3),
- cursor, DPI, and resize handling.

What differs per backend is only rasterization: softbuffer CPU blit vs a GPU glyph atlas with
instanced quads. `Backend::composites_layers()` (ADR 015 Decision 1) returns `true` for the entire
family; only character-cell crossterm returns `false`.

Rather than reimplement windowing in each GPU crate, extract the shared surface into
`retroglyph-window` (depends on `retroglyph-core` + `winit`) exposing a renderer seam (a
`Presenter`/surface trait) that each renderer implements:

```text
retroglyph-core
  └── retroglyph-window     winit loop, event translation, inverted run, Presenter trait
        ├── retroglyph-software   Presenter via softbuffer
        ├── retroglyph-wgpu       Presenter via wgpu
        └── retroglyph-gl         Presenter via glutin/glow (opengl, webgl2)
```

`retroglyph-crossterm` stays entirely outside this family: no winit, no `Presenter`, driven by
core's `run_blocking`.

Timing and open questions:

- **Do not extract now.** With one windowed backend the seam has a single consumer. wgpu (async
  device/queue init), GL (context-current-on-thread), and softbuffer (neither) have materially
  different surface lifecycles; the `Presenter` trait must be designed against at least two of them
  or it fits none. Keep the windowing code as a liftable `window.rs` inside `retroglyph-software`
  until then.
- **Trait-fusion caveat.** Today's `Backend` fuses input (`poll_event`, `push_event`) with output
  (`draw_layers`, `flush`, `size`, `resize`, cursor). In the windowed family, input is owned by the
  shared window and output by the per-renderer `Presenter`. Separating those halves is best designed
  when `retroglyph-window` is extracted, not now.
- **Shared atlas.** A glyph/tile atlas _layout_ (glyph → sprite cell) is common to
  `software-tilesets` and every GPU backend; only upload and sampling are backend-specific. Whether
  that becomes a shared `retroglyph-atlas` module is a smaller, separate question to revisit with
  the first GPU backend.

## Non-goals

- **Splitting grid/terminal into separate crates.** The internal dependency graph between color,
  style, tile, grid, event, text, layout, and terminal is dense. Splitting these would create 5+
  tiny crates with tight coupling. Not worth the friction.
- **Plugin/dynamic backend loading.** Backends are compile-time selections. Dynamic dispatch adds
  complexity for no clear user benefit in this domain.
- **Workspace-level feature unification beyond the facade.** Each backend crate declares its own
  features. The facade maps them, but there is no workspace-wide feature resolver beyond what Cargo
  provides.

## References

- [ADR 001 §1: "single crate, split later"](001-architecture.md)
- [ADR 007: Software Backend](007-software-backend.md) -- winit-owned loop, source of the windowing
  layer
- [ADR 011: WASM Portability](011-wasm-portability-revised.md) -- rAF frame gating
- [ADR 013: Codecov](013-codecov.md) -- flag-per-crate model designed for this split
- [ADR 015: Cross-Backend Rendering and Loop Consistency](015-cross-backend-consistency.md) -- loop
  decomposition, `composites_layers`, windowed family
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [cargo-release](https://github.com/crate-ci/cargo-release) -- multi-crate publish ordering
- [release-plz](https://github.com/MarcoIeni/release-plz) -- alternative with changelog generation
