# Multi-Crate Rust Workspace Architecture

Research for structuring a terminal/grid rendering library (BearLibTerminal-like) as a multi-crate
workspace. Findings drawn from ratatui v0.30, wgpu v29, and bevy's workspace.

---

## 1. Workspace Layout Patterns

Three proven patterns from production Rust projects:

### Ratatui (flat namespace)

Ratatui v0.30 split from a monolithic crate into a flat workspace. All crates live at the repo root
with a shared `ratatui-` prefix.
[Source](https://github.com/ratatui/ratatui/blob/main/ARCHITECTURE.md)

````rust
ratatui/
├── Cargo.toml          # workspace root
├── ratatui/            # facade crate (re-exports everything)
├── ratatui-core/       # Buffer, Style, Color, Layout, Widget trait
├── ratatui-widgets/    # Paragraph, List, Table, Chart, etc.
├── ratatui-crossterm/  # crossterm backend
├── ratatui-termion/    # termion backend
├── ratatui-termina/    # termina backend
├── ratatui-termwiz/    # termwiz backend
├── ratatui-macros/     # declarative macros
└── xtask/              # build automation
```text

Root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["ratatui", "ratatui-*", "xtask"]
default-members = [
  "ratatui",
  "ratatui-core",
  "ratatui-crossterm",
  "ratatui-macros",
  "ratatui-termina",
  "ratatui-termwiz",
  "ratatui-widgets",
]
````

### wgpu (flat namespace, layered)

wgpu uses a flat layout with 29 workspace members. Crates are layered: `wgpu-types` -> `wgpu-hal` ->
`wgpu-core` -> `wgpu`. Naga (shader compiler) lives alongside as a sibling crate.
[Source](https://deepwiki.com/gfx-rs/wgpu/6.4-build-system-and-workspace)

````text
wgpu/
├── Cargo.toml       # workspace root
├── wgpu/            # public API facade
├── wgpu-types/      # shared type definitions
├── wgpu-core/       # core implementation
├── wgpu-hal/        # hardware abstraction layer (backends)
├── naga/            # shader compiler (its own sub-ecosystem)
├── naga-cli/
└── xtask/
```rust

### Bevy (crates/ directory)

Bevy puts all crates under `crates/` with a `bevy_` prefix. Uses a two-crate facade pattern: `bevy`
(root) -> `bevy_internal` (aggregator) -> individual `bevy_*` crates.
[Source](https://deepwiki.com/bevyengine/bevy/1.2-crate-organization)

```text
bevy/
├── Cargo.toml              # workspace root + facade
├── crates/
│   ├── bevy_internal/      # aggregator crate
│   ├── bevy_ecs/
│   ├── bevy_app/
│   ├── bevy_render/
│   ├── bevy_math/
│   └── ...60+ crates
├── examples/
├── benches/
└── tools/
```rust

**Recommendation for a grid library**: Use the flat namespace (ratatui pattern). The `crates/`
directory pattern only pays off at 15+ crates. For a library with 5-8 crates, flat is simpler and
keeps paths short.

---

## 2. Core Types Crate Design

The core crate should contain only types and traits that downstream crates (widgets, backends,
algorithm crates) need to compile against. It is the stability anchor of the workspace.

### What belongs in core

Based on ratatui-core and wgpu-types:

| Type            | Purpose                                                |
| --------------- | ------------------------------------------------------ |
| `Cell`          | Single grid cell (glyph + style)                       |
| `Buffer`        | 2D grid of cells (the render target)                   |
| `Color`         | Color representation (Named, Rgb, Indexed)             |
| `Style`         | Foreground, background, modifiers (bold, italic, etc.) |
| `Rect`          | Position + size rectangle                              |
| `Position`      | x, y coordinate                                        |
| `Size`          | width, height                                          |
| `Widget` trait  | The core rendering trait                               |
| `Backend` trait | Terminal backend abstraction                           |
| Text types      | `Span`, `Line`, `Text` for styled text                 |
| Layout          | Constraint solver, Direction, Alignment                |

Ratatui-core's actual contents: widget traits, text types, buffer, layout, style, color, and symbol
collections. [Source](https://github.com/ratatui/ratatui/blob/main/ARCHITECTURE.md)

### What does NOT belong in core

- Widget implementations (those go in a widgets crate)
- Backend implementations (those go in backend crates)
- Algorithms (FOV, pathfinding go in algorithm crates)
- Macros (separate crate for proc/declarative macros)

### Example core Cargo.toml

```toml
[package]
name = "rg-core"
version = "0.1.0"
description = "Core types and traits for the rg terminal grid library."
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
std = ["serde?/std"]
serde = ["dep:serde"]

[dependencies]
bitflags = { workspace = true }
serde = { workspace = true, optional = true }

[dev-dependencies]
pretty_assertions = { workspace = true }

[lints]
workspace = true
````

Key design principles from ratatui-core:

- Default to `no_std` compatible (no `std` feature in defaults)
- `serde` support is opt-in
- Use `workspace = true` for all shared dependencies
- Keep dependency count minimal; this crate compiles the most often

---

## 3. Backend Trait Crate

The backend trait defines the interface all terminal backends implement. In ratatui this trait lives
inside `ratatui-core`, not in a separate crate. This is the right call for most projects: separating
the trait into its own crate adds a dependency hop without meaningful benefit.

### Backend trait location: in core

```rust
// In rg-core/src/backend.rs
pub trait Backend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>;

    fn hide_cursor(&mut self) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
    fn get_cursor_position(&mut self) -> io::Result<Position>;
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()>;
    fn clear(&mut self) -> io::Result<()>;
    fn size(&self) -> io::Result<Size>;
    fn flush(&mut self) -> io::Result<()>;
}
```

Each backend crate depends only on `rg-core` and its terminal library:

````rust
rg-core (defines Backend trait + Cell, Buffer, Style, etc.)
  ^           ^            ^
  |           |            |
rg-crossterm  rg-termion   rg-wgpu
```text

### TestBackend

Include a `TestBackend` in core (behind no feature gate). Every project needs it for testing widget
rendering without a real terminal.

---

## 4. Individual Backend Crates: Features vs Separate Crates

**Use separate crates, not features.** This is the clear consensus from ratatui, wgpu, and bevy.

### Why separate crates win

1. **Compile time**: Users only compile the backend they use. Feature flags cause conditional

   compilation but still pull in all backend source.

1. **Dependency isolation**: Each backend pulls in a different terminal library (crossterm, termion,

   etc.). Separate crates prevent unused transitive dependencies.

1. **Independent versioning**: Backend crates can release independently when the underlying terminal

   library updates. Ratatui-crossterm supports both crossterm 0.28 and 0.29 via internal feature
   flags. [Source](https://github.com/ratatui/ratatui/blob/main/ratatui-crossterm/Cargo.toml)

1. **Platform exclusion**: termion is Unix-only. With separate crates, it simply isn't a

   default-member on Windows. With features you need `cfg` guards everywhere.

### When features make sense inside a backend crate

Within a single backend crate, features are fine for:

- Version selection (`crossterm_0_28` vs `crossterm_0_29`)
- Optional capabilities (`underline-color`, `scrolling-regions`)
- Unstable APIs (`unstable-backend-writer`)

### Concrete structure

```toml
# rg-crossterm/Cargo.toml

[package]
name = "rg-crossterm"

[dependencies]
rg-core = { workspace = true }
crossterm = { workspace = true }

# rg-wgpu/Cargo.toml (GPU-rendered backend)

[package]
name = "rg-wgpu"

[dependencies]
rg-core = { workspace = true }
wgpu = { workspace = true }
````

---

## 5. Algorithm Crates (FOV, Pathfinding)

Algorithms should be separate, optional crates. They depend on core types (`Rect`, `Position`) but
not on backends or widgets.

````text
rg-fov/          # Field of vision algorithms (shadowcasting, etc.)
rg-pathfinding/  # A*, Dijkstra, BFS over grids
rg-noise/        # Noise generation for procedural content
```rust

### Why separate crates, not features

- Algorithms have their own dependency trees (potentially `num-traits`, etc.)
- Users building a roguelike want FOV; users building a dashboard don't
- Independent release cadence; algorithm improvements don't force core bumps
- Each algorithm crate depends only on `rg-core` for grid types

### Example

```toml
# rg-fov/Cargo.toml

[package]
name = "rg-fov"
version = "0.1.0"

[dependencies]
rg-core = { workspace = true }

[features]
default = []
serde = ["rg-core/serde"]
````

The facade crate can optionally re-export these:

```toml
# rg/Cargo.toml (facade)

[features]
fov = ["dep:rg-fov"]
pathfinding = ["dep:rg-pathfinding"]
```

---

## 6. Dependency Management

### workspace.dependencies

All dependency versions are declared once in the root `Cargo.toml`. Member crates reference them
with `workspace = true`. This is the universal pattern across ratatui, wgpu, and bevy.
[Source](https://github.com/ratatui/ratatui/commit/a07f5bec)

```toml
# Root Cargo.toml

[workspace.dependencies]
# Internal crates (with path + version for publishing)

rg-core = { path = "rg-core", version = "0.1.0" }
rg-crossterm = { path = "rg-crossterm", version = "0.1.0", optional = true }
rg-widgets = { path = "rg-widgets", version = "0.1.0" }
rg-fov = { path = "rg-fov", version = "0.1.0", optional = true }

# External dependencies

bitflags = "2.12"
crossterm = "0.29"
serde = { version = "1", default-features = false, features = ["derive"] }
pretty_assertions = "1"
```

```toml
# In any member crate

[dependencies]
rg-core = { workspace = true }
bitflags = { workspace = true }
serde = { workspace = true, optional = true }
```

### Version pinning strategy

Ratatui's approach: specify semver-compatible versions (e.g., `"0.29"` not `"0.29.3"`) to avoid
frequent Cargo.toml churn. The `Cargo.lock` pins exact versions. This communicates "we work with any
0.29.x". [Source](https://github.com/ratatui/ratatui/commit/a07f5bec)

### workspace.package for shared metadata

```toml
[workspace.package]
edition = "2024"
rust-version = "1.85.0"
license = "MIT OR Apache-2.0"
repository = "https://github.com/you/rg"
```

### workspace.lints

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
cast_possible_truncation = "allow"
module_name_repetitions = "allow"
must_use_candidate = "allow"
```

Member crates opt in:

```toml
[lints]
workspace = true
```

### cargo-deny for license/security auditing

Both ratatui and wgpu use `cargo-deny` in CI. wgpu's `.deny.toml` bans duplicate crate versions
(with explicit exceptions) and restricts to permissive licenses only.

---

## 7. Feature Flag Design

### Additive-only rule

From the Cargo reference: "Features should be additive. Enabling a feature should not disable
functionality." [Source](https://doc.rust-lang.org/cargo/reference/features.html)

Concrete implications:

- Use `std` (enables stdlib) not `no_std` (disables stdlib)
- Use `serde` (enables serialization) not `no-serde`
- Never use mutually exclusive features; split into separate crates instead

### Feature categories

For a grid rendering library:

```toml
[features]
# Defaults: the most common setup

default = ["std", "crossterm", "widgets", "macros"]

# Capability features (additive)

std = ["rg-core/std", "rg-widgets?/std"]
serde = ["dep:serde", "rg-core/serde", "rg-widgets?/serde"]
macros = ["dep:rg-macros"]

# Backend selection (each is an optional dep)

crossterm = ["dep:rg-crossterm", "std"]
termion = ["dep:rg-termion", "std"]

# Widget groups

widgets = ["dep:rg-widgets"]
widget-calendar = ["rg-widgets?/calendar"]

# Algorithm modules

fov = ["dep:rg-fov"]
pathfinding = ["dep:rg-pathfinding"]

# Unstable (prefixed for clarity)

unstable = ["unstable-widget-ref"]
unstable-widget-ref = []
```

### Use `dep:` syntax to hide internal optional deps

```toml
[dependencies]
rg-crossterm = { workspace = true, optional = true }

[features]
# Using dep: prevents an implicit "rg-crossterm" feature from leaking

crossterm = ["dep:rg-crossterm"]
```

### Feature forwarding with `?` syntax

Forward features to optional dependencies only when they're already enabled:

```toml
serde = ["dep:serde", "rg-core/serde", "rg-crossterm?/serde"]
#                                       ^ only if crossterm is enabled

```

---

## 8. Re-export / Facade Crate Pattern

The facade crate is the main crate users `cargo add`. It re-exports everything from the sub-crates
so users don't need to know about the internal structure.

### Ratatui's approach

The `ratatui` crate depends on all sub-crates and re-exports their public APIs. Applications use
`use ratatui::*`, while widget library authors depend on `ratatui-core` directly for stability.
[Source](https://docs.rs/ratatui/latest/x86_64-pc-windows-msvc/ratatui/)

### Bevy's two-layer approach

`bevy` -> `bevy_internal` -> individual crates. The internal crate does the aggregation and aliasing
(`pub use bevy_ecs as ecs`). The outer `bevy` crate is a thin wrapper. This is overkill for a
smaller project.

### Concrete re-export pattern

```rust
// rg/src/lib.rs (facade crate)

// Always available
pub use rg_core::*;

// Conditionally re-export backends
#[cfg(feature = "crossterm")]
pub use rg_crossterm::{self, CrosstermBackend};

#[cfg(feature = "termion")]
pub use rg_termion::{self, TermionBackend};

// Conditionally re-export widgets
#[cfg(feature = "widgets")]
pub use rg_widgets as widgets;

// Conditionally re-export algorithms
#[cfg(feature = "fov")]
pub use rg_fov as fov;

#[cfg(feature = "pathfinding")]
pub use rg_pathfinding as pathfinding;

// Macros
#[cfg(feature = "macros")]
pub use rg_macros::*;
```

### User perspective

```toml
# Application developer: just use the facade

[dependencies]
rg = { version = "0.1", features = ["crossterm"] }

# Widget library author: depend on core for stability

[dependencies]
rg-core = "0.1"

# Game developer wanting algorithms

[dependencies]
rg = { version = "0.1", features = ["crossterm", "fov", "pathfinding"] }
```

---

## 9. MSRV Policy

### Strategies from production projects

| Project | MSRV      | Policy                                                    |
| ------- | --------- | --------------------------------------------------------- |
| ratatui | 1.88.0    | N-2 releases; uniform across all crates                   |
| wgpu    | 1.87-1.93 | Tiered: public crates at 1.87 (Firefox), internal at 1.93 |
| bevy    | ~latest-2 | Tracks recent stable                                      |

### Recommendations

- **Start with a uniform MSRV** across all workspace crates. Tiered MSRVs (like wgpu) add complexity

  only justified by downstream consumers with hard constraints (Firefox).

- **Declare in workspace.package**:

  ```toml
  [workspace.package]
  rust-version = "1.85.0"
  ```

- **Test MSRV in CI** using `cargo hack --rust-version` or an explicit matrix entry. Ratatui's CI

  matrix includes both MSRV and stable.

- **Bump MSRV in minor releases**, not patches. Document the policy in README/CONTRIBUTING.

- **N-2 policy**: support current stable minus 2. This balances access to new language features

  against downstream compatibility.

---

## 10. CI Configuration for Workspace Testing

Ratatui's CI is the gold standard for workspace testing.
[Source](https://github.com/ratatui/ratatui/blob/main/.github/workflows/ci.yml)

### Job structure

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

concurrency:
  group: ${{ github.workflow }}-${{ github.head_ref || github.run_id }}
  cancel-in-progress: true

jobs:
  # Fast feedback jobs first
  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly

        with:
          components: rustfmt

      - run: cargo fmt --all --check

  clippy:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        toolchain: [stable, beta]
    continue-on-error: ${{ matrix.toolchain == 'beta' }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master

        with:
          toolchain: ${{ matrix.toolchain }}
          components: clippy

      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings

  # Cross-platform check with MSRV
  check:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        toolchain: ['1.85.0', 'stable']
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master

        with:
          toolchain: ${{ matrix.toolchain }}

      - uses: taiki-e/install-action@cargo-hack
      - uses: Swatinem/rust-cache@v2
      - run: cargo hack check --workspace --each-feature --no-dev-deps

  # Feature combination testing
  test-features:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-hack
      - uses: Swatinem/rust-cache@v2

      # depth=2 limits powerset explosion while catching pairwise conflicts

      - run: cargo hack test --workspace --feature-powerset --depth 2

  # Test each backend on each platform
  test-backends:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        backend: [crossterm, termion, wgpu]
        exclude:
          - os: windows-latest

            backend: termion
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test -p rg-${{ matrix.backend }}

  # no_std build verification
  build-no-std:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

        with:
          targets: x86_64-unknown-none

      - run: cargo build --target x86_64-unknown-none -p rg-core
      - run: cargo build --target x86_64-unknown-none -p rg-widgets

  # Doc tests
  test-docs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-hack
      - run: cargo hack test --workspace --doc --each-feature

  # License and security audit
  cargo-deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2

        with:
          arguments: --all-features --exclude-unpublished
          command: check advisories bans licenses sources

  # Gate for merge
  required:
    runs-on: ubuntu-latest
    needs: [fmt, clippy, check, test-features, test-backends, build-no-std, test-docs, cargo-deny]
    if: always()
    steps:
      - name: All jobs passed

        run: |
          echo '${{ toJson(needs) }}'
          test "${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}" = "false"
```

### Key CI tools

| Tool                  | Purpose                                                                                               |
| --------------------- | ----------------------------------------------------------------------------------------------------- |
| `cargo-hack`          | Test each feature individually (`--each-feature`) or in combinations (`--feature-powerset --depth 2`) |
| `cargo-deny`          | License compliance, security advisories, duplicate crate bans                                         |
| `cargo-machete`       | Detect unused dependencies                                                                            |
| `cargo-llvm-cov`      | Coverage reports                                                                                      |
| `Swatinem/rust-cache` | Cache `target/` between CI runs                                                                       |

### cargo-hack usage patterns

```bash
# Check that each feature compiles alone (catches missing dep declarations)

cargo hack check --workspace --each-feature --no-dev-deps

# Test pairwise feature combinations (catches conflicts without exponential blowup)

cargo hack test --workspace --feature-powerset --depth 2

# Verify MSRV compiles

cargo hack check --workspace --rust-version
```

---

## Complete Workspace Cargo.toml Example

```toml
[workspace]
resolver = "2"
members = ["rg", "rg-*", "xtask"]
default-members = [
  "rg",
  "rg-core",
  "rg-widgets",
  "rg-crossterm",
  "rg-macros",
]

[workspace.package]
edition = "2024"
rust-version = "1.85.0"
license = "MIT OR Apache-2.0"
repository = "https://github.com/you/rg"
keywords = ["terminal", "grid", "roguelike", "tui"]
categories = ["command-line-interface", "game-development"]

[workspace.dependencies]
# Internal

rg = { path = "rg", version = "0.1.0" }
rg-core = { path = "rg-core", version = "0.1.0" }
rg-widgets = { path = "rg-widgets", version = "0.1.0", default-features = false }
rg-crossterm = { path = "rg-crossterm", version = "0.1.0", optional = true }
rg-termion = { path = "rg-termion", version = "0.1.0", optional = true }
rg-fov = { path = "rg-fov", version = "0.1.0", optional = true }
rg-pathfinding = { path = "rg-pathfinding", version = "0.1.0", optional = true }
rg-macros = { path = "rg-macros", version = "0.1.0", optional = true }

# External

bitflags = "2.12"
crossterm = "0.29"
serde = { version = "1", default-features = false, features = ["derive"] }
termion = "4"

# Dev/test

pretty_assertions = "1"
criterion = { version = "0.5", features = ["html_reports"] }
rstest = "0.26"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
cast_possible_truncation = "allow"
module_name_repetitions = "allow"
must_use_candidate = "allow"

[profile.bench]
codegen-units = 1
lto = true
```

---

## Sources

- **Kept**:
  - [ratatui ARCHITECTURE.md](https://github.com/ratatui/ratatui/blob/main/ARCHITECTURE.md) -

    definitive reference for the workspace split pattern

  - [ratatui root Cargo.toml](https://github.com/ratatui/ratatui/blob/main/Cargo.toml) -

    workspace.dependencies, workspace.lints, workspace.package

  - [ratatui-core Cargo.toml](https://github.com/ratatui/ratatui/blob/main/ratatui-core/Cargo.toml) -

    core crate feature design, no_std support

  - [ratatui facade Cargo.toml](https://github.com/ratatui/ratatui/blob/main/ratatui/Cargo.toml) -

    re-export pattern, feature forwarding with `?` syntax

  - [ratatui-crossterm Cargo.toml](https://github.com/ratatui/ratatui/blob/main/ratatui-crossterm/Cargo.toml) -

    backend crate with version selection features

  - [ratatui CI workflow](https://github.com/ratatui/ratatui/blob/main/.github/workflows/ci.yml) -

    comprehensive CI with cargo-hack, cargo-deny, MSRV matrix

  - [wgpu workspace (DeepWiki)](https://deepwiki.com/gfx-rs/wgpu/6.4-build-system-and-workspace) -

    tiered MSRV, feature flag layering, deny.toml

  - [bevy crate organization (DeepWiki)](https://deepwiki.com/bevyengine/bevy/1.2-crate-organization) -

    two-layer facade, feature hierarchy

  - [Cargo Features reference](https://doc.rust-lang.org/cargo/reference/features.html) -

    additive-only rule, dep: syntax, ? syntax

  - [cargo-hack README](https://github.com/taiki-e/cargo-hack) - --each-feature, --feature-powerset,

    --depth, --rust-version

- **Dropped**:
  - forky workspace (mrchantey) - small/hobby project, no novel patterns beyond what ratatui covers
  - ratatui Issue #1388 - superseded by ARCHITECTURE.md which documents the final design
  - Various GitHub nav chrome from Cargo.toml fetch - not content

## Gaps

- **Publishing order**: When publishing to crates.io, workspace crates must be published in

  dependency order (core first, facade last). Tools like `cargo-release` or `cargo-workspaces`
  handle this but weren't researched in depth.

- **Independent vs synchronized versioning**: All three studied projects use synchronized versions.

  Independent versioning (where core is 0.3 while widgets is 0.7) is viable but adds coordination
  overhead. Worth revisiting once the API stabilizes.

- **Benchmarking across crates**: How to structure `criterion` benchmarks that span multiple

  workspace crates (e.g., benchmarking widget rendering through a backend). Ratatui puts benchmarks
  in the facade crate.
