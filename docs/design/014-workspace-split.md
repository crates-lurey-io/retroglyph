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

Split into a Cargo workspace with four published crates and one internal tools crate.

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
      backend/
        mod.rs          Backend trait + Headless
        headless.rs
  crossterm/            retroglyph-crossterm
    src/
      lib.rs            Crossterm backend
  software/             retroglyph-software
    src/
      lib.rs
      bitmap_font.rs
      config.rs
      windowed.rs
      sprite_cache.rs
      tileset.rs
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

Dependencies: `ixy`, `grixy`, `unicode-width`, `bitflags`, and optionally `unicode-segmentation`
(behind `egc` feature). No platform-specific deps.

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

The split is mechanical. No public API changes, no new abstractions.

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
- [ADR 013: Codecov](013-codecov.md) -- flag-per-crate model designed for this split
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [cargo-release](https://github.com/crate-ci/cargo-release) -- multi-crate publish ordering
- [release-plz](https://github.com/MarcoIeni/release-plz) -- alternative with changelog generation
