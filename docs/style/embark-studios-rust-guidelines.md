# Research: Embark Studios Rust Guidelines & Ecosystem

## Summary

Embark Studios (Stockholm, founded 2018 by ex-DICE/EA veterans including Patrick Soderlund) adopted
Rust as their primary language from day one for game development, ultimately shipping THE FINALS
(2023). They published a concise set of development guidelines (rustfmt on save, parking_lot over
std::sync, Clippy + Rust 2018 idiom warnings in every crate), maintained a comprehensive standard
lint configuration (v6, ~70 Clippy lints), and released 35+ open-source Rust crates spanning GPU
shaders, physics, profiling, dependency management, rendering, and cloud infrastructure. Their CI
template (used across all repos) enforces rustfmt, Clippy, cargo-deny, cross-platform testing, and
MSRV checks via GitHub Actions. Embark still operates (owned by Nexon, ~287 employees as of 2024),
but their Rust open-source activity has significantly wound down, with rust-gpu transitioning to
community ownership.

## Findings

### 1. Core Development Guidelines

The guidelines live at
[`rust-ecosystem/guidelines.md`](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)
and are deliberately minimal: three numbered rules plus editor setup.

1. **Guideline 001: Run rustfmt on save.** All repos include `.vscode/settings.json` with

   `"editor.formatOnSave": true`. CI verifies formatting with `cargo fmt -- --check --color always`.
   The motivation: "We want to keep a consistent code base where we don't have to argue about
   style." No custom rustfmt configuration is mentioned; they use the default style.
   [Source](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)

1. **Guideline 002: Prefer `parking_lot` over `std::sync`.** They mandate

   `parking_lot::{Mutex, RwLock}` instead of `std::sync::{Mutex, RwLock}`. Rationale: smaller,
   faster, and avoids poisoning errors (no `.unwrap()` on `.lock()`). They noted a wish for
   parking_lot primitives to eventually land in std.
   [Source](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)

1. **Guideline 003: Opt-in for Clippy and Rust 2018 style warnings.** Every crate must have

   `#![warn(clippy::all)]` and `#![warn(rust_2018_idioms)]` at the top of `lib.rs` or `main.rs`. CI
   verifies clippy produces no warnings. They tracked a desire for workspace-level lint
   configuration in [issue #22](https://github.com/EmbarkStudios/rust-ecosystem/issues/22).
   [Source](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)

1. **VS Code as primary IDE.** Recommended extensions: rust-analyzer (sponsored by Embark), Crates

   (for Cargo.toml dependency management), C/C++ (Windows debugging), Native Debug (Linux/Mac via
   GDB/LLDB), plus GitLens, Better TOML, shader language support, and TODO Highlight.
   [Source](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)

### 2. Standard Lints Configuration (v6)

Embark maintained a versioned, opinionated lint set beyond the basic guidelines, tracked in
[issue #59](https://github.com/EmbarkStudios/rust-ecosystem/issues/59). The "v6 for Rust 1.55+"
configuration was distributed in two formats:

**Format A: `lints.rs`** (copy-pasted into every crate's `lib.rs`/`main.rs`):

```rust
#![deny(unsafe_code)]
#![warn(
    clippy::all,
    clippy::await_holding_lock,
    clippy::char_lit_as_u8,
    clippy::checked_conversions,
    clippy::dbg_macro,
    clippy::debug_assert_with_mut_call,
    clippy::doc_markdown,
    clippy::empty_enum,
    clippy::enum_glob_use,
    clippy::exit,
    clippy::expl_impl_clone_on_copy,
    clippy::explicit_deref_methods,
    clippy::explicit_into_iter_loop,
    clippy::fallible_impl_from,
    clippy::filter_map_next,
    clippy::flat_map_option,
    clippy::float_cmp_const,
    clippy::fn_params_excessive_bools,
    clippy::from_iter_instead_of_collect,
    clippy::if_let_mutex,
    clippy::implicit_clone,
    clippy::imprecise_flops,
    clippy::inefficient_to_string,
    clippy::invalid_upcast_comparisons,
    clippy::large_digit_groups,
    clippy::large_stack_arrays,
    clippy::large_types_passed_by_value,
    clippy::let_unit_value,
    clippy::linkedlist,
    clippy::lossy_float_literal,
    clippy::macro_use_imports,
    clippy::manual_ok_or,
    clippy::map_err_ignore,
    clippy::map_flatten,
    clippy::map_unwrap_or,
    clippy::match_on_vec_items,
    clippy::match_same_arms,
    clippy::match_wild_err_arm,
    clippy::match_wildcard_for_single_variants,
    clippy::mem_forget,
    clippy::mismatched_target_os,
    clippy::missing_enforced_import_renames,
    clippy::mut_mut,
    clippy::mutex_integer,
    clippy::needless_borrow,
    clippy::needless_continue,
    clippy::needless_for_each,
    clippy::option_option,
    clippy::path_buf_push_overwrite,
    clippy::ptr_as_ptr,
    clippy::rc_mutex,
    clippy::ref_option_ref,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::same_functions_in_if_condition,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::string_add_assign,
    clippy::string_add,
    clippy::string_lit_as_bytes,
    clippy::string_to_string,
    clippy::todo,
    clippy::trait_duplication_in_bounds,
    clippy::unimplemented,
    clippy::unnested_or_patterns,
    clippy::unused_self,
    clippy::useless_transmute,
    clippy::verbose_file_reads,
    clippy::zero_sized_map_values,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms
)]
```

**Format B: `.cargo/config.toml`** (workspace-wide, preferred for large repos): The same lints
expressed as `-W` rustflags under `[target.'cfg(all())']`. This was tested as a workaround for the
lack of workspace-level lint configuration (Cargo didn't support `[lints]` at the time). One of
their large repositories had 80+ crates; per-crate copy-paste was error-prone.
[Source](https://github.com/EmbarkStudios/rust-ecosystem/pull/68)

**Key lint policy decisions:**- `unsafe_code` is**denied** (not just warned), meaning unsafe code requires explicit
  `#[allow(unsafe_code)]` opt-in.

- `clippy::dbg_macro`, `clippy::todo`, `clippy::unimplemented` are warned against, catching debug

  leftovers.

- `clippy::exit` is warned, preventing unclean process exits.
- `clippy::mem_forget` is warned, important for game engines with manual resource management.
- String manipulation lints (`string_add`, `string_add_assign`, `string_to_string`,

  `string_lit_as_bytes`) push toward idiomatic Rust string handling.

- Match-related lints (`match_same_arms`, `match_wildcard_for_single_variants`, etc.) enforce

  exhaustive pattern matching.

- Individual crates can override with `#![allow(...)]` after the standard block.

### Lints they wanted but couldn't use (blocked on Clippy bugs)

- `clippy::use_self` (too many false positives)
- `clippy::unnecessary_wraps`
- `clippy::option_if_let_else` (multiple suggestion bugs)
- `clippy::wildcard_enum_match_arm` (broken with non-exhaustive enums)
- `clippy::print_stdout` / `clippy::print_stderr` (ineffective in non-top-level modules)

[Source: lints.rs](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/lints.rs),
[Source: lints.toml](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/lints.toml),
[Source: Issue #59](https://github.com/EmbarkStudios/rust-ecosystem/issues/59)

### 3. CI Configuration Pattern

Their canonical CI pattern (from cargo-about's `rust-ci.yml`, representative of all Embark repos):

```yaml
name: CI
on:
  push:
    branches: [main]
    tags: ['*']
  pull_request:

concurrency:
  group: ${{ github.workflow }}-${{ github.head_ref || github.run_id }}
  cancel-in-progress: true

jobs:
  lint:
    runs-on: ubuntu-22.04
    steps:

      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt -- --check --color always
      - run: cargo clippy --all-targets -- -D warnings

  test:
    strategy:
      matrix:
        os: [ubuntu-24.04, macos-14, windows-2022]
    runs-on: ${{ matrix.os }}
    steps:

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --release

  msrv-check:
    runs-on: ubuntu-24.04
    steps:

      - uses: dtolnay/rust-toolchain@master

        with:
          toolchain: '1.88.0' # pinned MSRV

      - run: cargo check --all-targets

  deny-check:
    runs-on: ubuntu-22.04
    steps:

      - uses: EmbarkStudios/cargo-deny-action@v2

```

**Key CI patterns:**-**Concurrency control**: `cancel-in-progress: true` prevents queue buildup.

- **dtolnay/rust-toolchain** over the deprecated actions-rs/toolchain.
- **Swatinem/rust-cache** for build caching across runs.
- **cargo-deny-action**: Their own GitHub Action for license/advisory/ban checking.
- **Cross-platform matrix**: Ubuntu, macOS (ARM), Windows.
- **MSRV check**: Separate job with a pinned older toolchain.
- **Release builds for tests**: `cargo test --release` catches release-only issues.
- **Separate lint and test jobs**: Linting is fast on a single OS; tests fan out across platforms.
- **Tag-triggered releases**: Build matrix producing binaries for x86_64/aarch64 on Linux (musl),

  Windows, and macOS, packaged via shell script and published as GitHub Releases.

[Source](https://github.com/EmbarkStudios/cargo-about/blob/master/.github/workflows/rust-ci.yml)

### 4. Open-Source Crate Ecosystem (35+ projects)

Grouped by category:

**GPU & Rendering:**-**rust-gpu** (8.3k+ stars): Compiles Rust to SPIR-V for Vulkan GPU shaders. Used
  `rustc_codegen_spirv` as a backend. Now transitioned to community ownership at
  [Rust-GPU org](https://github.com/Rust-GPU/rust-gpu). Their most ambitious project.

- **kajiya** (5k+ stars): Experimental real-time global illumination renderer using Rust + Vulkan.

  ReSTIR-based, fully dynamic GI without precomputed light transport. Personal project of Tomasz
  Stachowiak that became part of Embark's production pipeline.

- **ash-molten**: Statically linked MoltenVK for Vulkan on macOS via the `ash` crate.
- **spirv-tools-rs**: Rust wrapper for SPIR-V Tools.
- **fsr-rs**: Rust bindings for AMD FidelityFX Super Resolution.

**Dependency & License Management:**-**cargo-deny** (~4M total downloads, ~940k in 90 days): Checks dependency graphs for license
  compliance, security advisories, banned crates, and duplicate versions. Widely adopted beyond
  Embark. Has a companion GitHub Action (`cargo-deny-action`).

- **cargo-about**: Generates license listings for all dependencies using customizable Handlebars

  templates.

- **krates**: Builds crate dependency graphs from cargo metadata.
- **cfg-expr**: Parser/evaluator for Rust `cfg()` expressions.
- **spdx**: SPDX license expression parser.

**Game Engine Infrastructure:**-**physx-rs** (PhysX bindings): Rust wrapper over NVIDIA PhysX. Described by Tomasz Stachowiak as
  "an unholy fusion of Rust and C++."

- **puffin**: Instrumentation-based CPU profiler for Rust. Designed for game loops.
- **superluminal-perf-rs**: Integration with Superluminal Performance profiler.
- **poll-promise**: Promise type for games and immediate-mode GUIs (no async runtime needed).
- **mirror-mirror**: Reflection library for Rust.
- **presser**: Safe low-level data copies into raw buffers without UB.
- **rpmalloc-rs**: Cross-platform global allocator using rpmalloc.
- **tiny-bench**: Minimal benchmarking library.
- **cervo**: ML inference middleware for games (ONNX-based).

**Cloud & Networking:**-**tame-gcs**, **tame-oauth**, **tame-oidc**: Sans-io approach to Google Cloud Storage, OAuth, and
  OIDC.

- **discord-sdk**: Open implementation of the Discord Game SDK.
- **rymder**: Unofficial Agones (game server orchestration) client.
- **cloud-dns**: Google Cloud DNS client.
- **tryhard**: Retry library for futures.
- **gsutil**: Partial gsutil replacement.

**Build & CI:**-**cargo-fetcher**: `cargo fetch` alternative optimized for CI/clean environments.

- **cargo-deny-action**: GitHub Action wrapping cargo-deny.
- **buildkite-jobify**: Kubekite replacement in Rust.
- **octobors**: GitHub Action for PR automerging.
- **relnotes**: Automatic GitHub release note generation.

**Crash Handling & Tracing:**-**crash-handling**: Collection of crates for catching/handling crashes.

- **tracing-logfmt**: logfmt formatter for tracing-subscriber.
- **tracing-ext-ffi-subscriber**: FFI bridge for passing tracing spans to external profilers.

**Texture & Content:**-**texture-synthesis**: Example-based texture synthesis, one of their earliest open-source
  projects.

[Source: embark.dev](https://embark.dev/),
[Source: rust-ecosystem README](https://github.com/EmbarkStudios/rust-ecosystem)

### 5. Rust for Game Development Experience

Key insights from blog posts and talks (2019-2022):

- **Rust as primary language from founding.** "Embark has used Rust as our primary programming

  language since the day we started the studio." (2019). They shipped THE FINALS (a major
  free-to-play shooter, 14M+ copies) using Rust on a custom engine.

- **Benefits they highlighted:**
  - "Fearlessly refactor and change the code, without common lifetime/ownership, memory safety, or

    race condition problems."

  - "Teamwork is much better when you can trust other people not to introduce subtle threading and

    ownership issues." (Tomasz Stachowiak)

  - "It just made me enjoy programming again" (Jake Shadle).
  - "Rust fixes all of [C++'s limitations] and even takes over many cases where I previously would

    have used Python." (Tomasz Stachowiak)

- **Production renderer built in Rust.** Their creative platform renderer used Rust on both CPU and

  GPU (via rust-gpu). They ran with "vertex colors and blob shadows for almost two years" before
  building proper rendering tech, demonstrating Rust's viability for rapid engine prototyping.

- **Shader unit testing.** Because rust-gpu shaders are regular Rust code, they could unit-test GPU

  shaders on the CPU as part of standard CI. This was a unique advantage over HLSL/GLSL workflows.

- **Code sharing between CPU and GPU.** Functions and structs shared in a type-safe manner between

  CPU and GPU code via regular Rust modules. Expensive shader computations that turned out to be
  constant could be moved to CPU without rewriting.

- **Daily tooling:** cargo, rustfmt, clippy, VS Code with rust-analyzer (which Embark sponsored

  financially).

- **Pain points acknowledged:**
  - Compile times (wanted D-like speed).
  - Dependency management complexities vs. C++.
  - Missing stable features: specialization, generic associated types (at the time).
  - Wanted more crates to reach 1.0.

[Source: "Inside Rust at Embark" (2019)](https://dev.to/embark/inside-rust-at-embark-230o),
[Source: "Homegrown Rendering with Rust" (2022)](https://medium.com/@h3r2tic/homegrown-rendering-with-rust-1e39068e56a7)

### 6. Blog Posts and Talks

- **"Inside Rust at Embark"** (Dec 2019, Medium/dev.to): Interview with Jake Shadle and Tomasz

  Stachowiak about daily Rust workflows, favorite crates, and vision for Rust in game dev.

- **"Homegrown Rendering with Rust"** (2022, Medium): Technical deep-dive into their Rust+Vulkan

  rendering pipeline, kajiya, production renderer, and rust-gpu shader usage.

- **"Texture Synthesis and Remixing from a Single Example"** (Medium): Anastasia Opara on their

  texture-synthesis crate.

- **Stockholm Rust Meetup** (Nov 2019): Jake Shadle on "Rust, Open Source, Game Dev" and Tomasz

  Stachowiak on "An unholy fusion of Rust and C++ in physx-rs" (available on
  [YouTube](https://www.youtube.com/c/EmbarkStudiosAB)).

- **RustFest 2019 interview** (Rustacean Station podcast): Jake Shadle on "Rust for AAA Game

  Development."

- **Rust GPU announcement** (2020): Blog post introducing the rust-gpu project and their vision for

  Rust as a GPU programming language.

- **80.lv feature**: "Embark Studios' Open-Source Tools Make Game Development Easier" covering their

  tool ecosystem.

### 7. Studio Status and Legacy

- **Founded** November 2018 by ex-DICE/EA veterans (Patrick Soderlund, Johan Andersson, Magnus

  Nordin, Rob Runesson, Stefan Strandberg, Jenny Huldschiner).

- **Acquired by Nexon** (Japanese gaming conglomerate). THE FINALS shipped December 2023, reaching

  14M+ players.

- **Still operating** as of 2024 with ~287 employees (not shut down, contrary to some reports). ARC

  Raiders is in continued development with ~360 staff.

- **Open-source wind-down:** Rust-gpu transitioned to community ownership in 2024. The

  rust-ecosystem repo's last push was January 2025. Most open-source crates are in maintenance mode.
  cargo-deny and cargo-about remain actively maintained by Jake Shadle.

- **Key contributors:** Jake Shadle (cargo-deny, cargo-about, cfg-expr, krates, spdx,

  crash-handling), Tomasz Stachowiak (kajiya, physx-rs, rendering), Ashley Hauck and eddyb (rust-gpu
  compiler), Maik Klein (ash Vulkan bindings), Emil Ernerfeldt (puffin, egui contributor), Gray
  Olson (presser, spirv-tools), Johan Andersson (CTO, overall direction).

### 8. Areas of Interest Tracked

Embark tracked Rust ecosystem gaps via GitHub issues in the rust-ecosystem repo:

- Distributed systems (async, tokio, tonic)
- Game engine systems (multiplayer, rendering, physics, audio)
- Developer experience (fast iteration, monorepos, distributed builds, debugging, profiling)
- WebAssembly and WASI
- Machine learning (efficient inference, training environments)
- High-performance runtime (CPU job scheduling, code generation)
- Console and mobile platform support (PlayStation, Xbox, Android)
- Rust on GPU

[Source: rust-ecosystem README](https://github.com/EmbarkStudios/rust-ecosystem)

## Key Takeaways for Rust Style Guides

1. **Keep guidelines minimal and actionable.** Three numbered rules is better than a 50-page style

   guide. Link to the Rust Book for everything else.

1. **Standardize lints across the workspace.** Use `.cargo/config.toml` rustflags or (now in modern

   Rust) `[workspace.lints]` in `Cargo.toml` to configure lints once for all crates.

1. **Deny unsafe_code by default.** Require explicit opt-in per module/function.
1. **Ban debug leftovers.** Warn on `dbg_macro`, `todo`, `unimplemented`.3. **Prefer better ecosystem crates.** parking_lot over std::sync was a real productivity win (no

   lock poisoning).

1. **Enforce in CI, not just docs.** rustfmt check, Clippy with `-D warnings`, cargo-deny for

   dependencies.

1. **Cross-platform CI matrix.** Test on all target platforms; don't assume Linux-only.
1. **MSRV checking.** Separate CI job with a pinned older toolchain.
## Sources

- Kept:

  [rust-ecosystem/guidelines.md](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)
  -- primary source for the three guidelines

- Kept:

  [rust-ecosystem/lints.rs](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/lints.rs) --
  complete standard lint set (v6)

- Kept:

  [rust-ecosystem/lints.toml](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/lints.toml)
  -- workspace-level lint config via rustflags

- Kept:

  [Issue #59: Embark's standard lints](https://github.com/EmbarkStudios/rust-ecosystem/issues/59) --
  lint policy discussion, blocked lints, rationale

- Kept:

  [PR #68: Add lints as .cargo/config.toml](https://github.com/EmbarkStudios/rust-ecosystem/pull/68)
  -- workspace-level lint workaround

- Kept: [Issue #79: Clippy wants](https://github.com/EmbarkStudios/rust-ecosystem/issues/79) --

  wishlist for Clippy improvements

- Kept:

  [cargo-about rust-ci.yml](https://github.com/EmbarkStudios/cargo-about/blob/master/.github/workflows/rust-ci.yml)
  -- canonical CI configuration

- Kept: [Inside Rust at Embark (dev.to)](https://dev.to/embark/inside-rust-at-embark-230o) --

  first-hand developer experience

- Kept:

  [Homegrown Rendering with Rust (Medium)](https://medium.com/@h3r2tic/homegrown-rendering-with-rust-1e39068e56a7)
  -- rendering + rust-gpu production use

- Kept: [embark.dev](https://embark.dev/) -- complete project listing
- Kept: [Rust GPU Transition Announcement](https://rust-gpu.github.io/blog/transition-announcement/)

  -- community ownership context

- Kept: [rust-ecosystem README](https://github.com/EmbarkStudios/rust-ecosystem) -- full crate

  listing, areas of interest

- Dropped: Allabolag/Merinfo financial records -- Swedish corporate data, not relevant to Rust

  practices

- Dropped: Revelio Labs employee count -- workforce analytics, tangential
- Dropped: Nexon press releases about THE FINALS -- game marketing, not Rust-specific

## Gaps

1. **Internal Cargo.toml structure.** Embark's internal repos are not public, so workspace

   Cargo.toml configuration (features, profiles, workspace dependencies) is unknown beyond what
   cargo-deny and cargo-about show.

1. **rustfmt.toml customizations.** The guidelines say "use rustfmt" but never mention any custom

   rustfmt.toml settings. It's unclear whether they used pure defaults or had internal overrides.

1. **Post-2022 guideline evolution.** The guidelines.md hasn't been updated since the v6 lint set.

   Modern Rust features (workspace lints in Cargo.toml, edition 2024) may have been adopted
   internally without public documentation.

1. **Build performance optimizations.** Tomasz mentioned using "artisanal hand-crafted RUSTFLAGS in

   conjunction with LLD" for build speed, but the specific configuration isn't public.

1. **Console platform (PlayStation, Xbox) Rust specifics.** Tracked as an interest area but no

   public details about cross-compilation or platform-specific patterns.

1. **Error handling conventions.** The guidelines don't specify an error handling strategy (anyhow

   vs thiserror, custom error types, etc.).
