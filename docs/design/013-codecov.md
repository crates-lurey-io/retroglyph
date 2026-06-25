# ADR 013: Code Coverage with Codecov

**Status:** Accepted  
**Date:** 2026-06-21  
**Parent:** [ADR 004: Testing Strategy](004-testing-strategy.md)

## Context

retroglyph has no code coverage tracking. Tests exist (unit tests, insta snapshots, PTY-based E2E
snapshots), but there is no visibility into what percentage of the codebase they exercise. Adding
coverage reporting would:

1. Surface blind spots in test coverage, especially in backend-specific code paths.
2. Catch regressions where new code lands without tests.
3. Provide a PR-level diff coverage gate so reviewers see exactly which new lines are untested.

The repo is currently a single crate with a `[workspace]` that only includes `tools/cargo-bin`. A
future monorepo split (e.g., `retroglyph-core`, `retroglyph-crossterm`, `retroglyph-software`) is
likely. The coverage setup should handle that transition without a redesign.

## Decision

Use **cargo-llvm-cov** for instrumentation and **Codecov** for reporting, with a `codecov.yml` that
uses flags and components so the monorepo transition is a config change, not a pipeline rewrite.

### Why cargo-llvm-cov over tarpaulin

| Criterion         | cargo-llvm-cov                               | tarpaulin                           |
| ----------------- | -------------------------------------------- | ----------------------------------- |
| Instrumentation   | LLVM source-based (`-C instrument-coverage`) | ptrace-based (Linux only)           |
| Accuracy          | Region-level, matches rustc's own view       | Line-level, known edge-case gaps    |
| Platform          | Linux, macOS, Windows                        | Linux only (ptrace)                 |
| Workspace support | Native (`--workspace`)                       | Native, but ptrace has crate quirks |
| Output formats    | lcov, codecov JSON, html, text               | lcov, html, json, xml               |
| Maintenance       | taiki-e (also maintains install-action)      | Single maintainer, less frequent    |
| CI speed          | ~same for small projects                     | ~same for small projects            |

cargo-llvm-cov wins on accuracy, cross-platform support, and alignment with the Rust toolchain.
tarpaulin's ptrace approach would also block the software backend's winit tests if they ever run on
CI.

### Why Codecov over Coveralls

Codecov has first-class support for flags, components, and PR comment annotations. Its `codecov.yml`
component model maps cleanly to Cargo workspace members. Coveralls works fine for single crates but
has weaker monorepo tooling.

## Design

### Phase 1: Single-crate coverage (now)

Add a `coverage` job to `.github/workflows/ci.yml` that runs after `test`:

```yaml
coverage:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        components: llvm-tools-preview
    - uses: taiki-e/install-action@v2
      with:
        tool: cargo-llvm-cov
    - uses: Swatinem/rust-cache@v2
    - name: Generate coverage
      run: cargo llvm-cov --all-features --codecov --output-path codecov.json
    - uses: codecov/codecov-action@v5
      with:
        files: codecov.json
        flags: retroglyph
        token: ${{ secrets.CODECOV_TOKEN }}
        fail_ci_if_error: true
```

Key points:

- `--all-features` ensures crossterm, software, tilesets, and egc code paths are all instrumented.
- `--codecov` outputs Codecov's native JSON format (more accurate than lcov for region coverage).
- The `flags: retroglyph` tag labels this report. When the monorepo split happens, each crate gets
  its own flag without breaking historical data.
- `fail_ci_if_error: true` catches upload failures (expired tokens, API outages) rather than
  silently skipping.

### Phase 2: Monorepo-ready codecov.yml

Add a `codecov.yml` at the repo root from day one, even though there is only one flag today. This
avoids a disruptive config migration later.

```yaml
codecov:
  require_ci_to_pass: true

coverage:
  status:
    project:
      default:
        target: auto
        threshold: 2%
    patch:
      default:
        target: 80%

flags:
  retroglyph:
    paths:
      - src/
    carryforward: true

comment:
  layout: 'condensed_header, condensed_files, condensed_footer'
  behavior: default
  require_changes: true
```

When the workspace splits into multiple crates:

```yaml
# Future: one flag per crate, each with its own path filter.
flags:
  retroglyph-core:
    paths:
      - crates/core/src/
    carryforward: true
  retroglyph-crossterm:
    paths:
      - crates/crossterm/src/
    carryforward: true
  retroglyph-software:
    paths:
      - crates/software/src/
    carryforward: true

component_management:
  individual_components:
    - component_id: core
      name: Core
      paths:
        - crates/core/src/**
    - component_id: crossterm
      name: Crossterm Backend
      paths:
        - crates/crossterm/src/**
    - component_id: software
      name: Software Backend
      paths:
        - crates/software/src/**
```

The CI job would change to run `cargo llvm-cov` per workspace member with separate `--flag` uploads,
or use `--workspace` with a single report and let Codecov's path-based flags split it.

### Coverage targets

- **Project target: `auto`** -- Codecov tracks the rolling baseline and fails only if coverage drops
  relative to the default branch. No fixed floor to maintain.
- **Patch target: `80%`** -- new code in a PR should be at least 80% covered. This is intentionally
  not 100%; backend integration code (winit event loop, crossterm raw mode) is hard to cover in CI.
- **Threshold: `2%`** -- allows minor project-level fluctuations (e.g., adding a new module with
  tests that slightly shift the ratio).

### Excluded paths

cargo-llvm-cov respects `#[cfg(not(coverage_nightly))]` and `#[coverage(off)]` attributes. For
retroglyph, the following should be excluded from coverage (they cannot be meaningfully tested in
CI):

- `src/backend/software/mod.rs` -- `ApplicationHandler` impl and winit event loop glue.
- Example binaries (`examples/`).
- The `tools/cargo-bin` workspace member.

Exclusions go in `.cargo/llvm-cov-ignore` or via `#[coverage(off)]` on specific functions once
stabilized.

### Justfile integration

Add a local coverage target for developer use:

```just
coverage:
    cargo llvm-cov --all-features --html --open
```

This generates an HTML report and opens it in the browser, matching the existing `docs-preview`
pattern.

## Files to create or modify

| File                       | Change                                  |
| -------------------------- | --------------------------------------- |
| `codecov.yml` (new)        | Coverage targets, flags, comment layout |
| `.github/workflows/ci.yml` | Add `coverage` job after `test`         |
| `Justfile`                 | Add `coverage` recipe                   |
| `README.md`                | Add Codecov badge                       |

## Non-goals

- **Branch coverage** -- LLVM source-based coverage tracks regions, not branches. Region coverage is
  strictly more informative. Branch coverage can be added later via `--show-branch-summary` if
  needed.
- **Coverage-gated merges** -- the `patch` status is informational. Blocking merges on coverage
  percentage is too blunt for a library with platform-specific backends.
- **Tarpaulin fallback** -- no reason to maintain two coverage tools. If cargo-llvm-cov has a gap,
  fix the gap rather than adding a parallel pipeline.
- **Coverage for WASM target** -- cargo-llvm-cov does not support `wasm32-unknown-unknown`.
  WASM-specific tests would need a separate approach (wasm-bindgen-test with manual instrumentation)
  if coverage is ever needed there.

## References

- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) -- CI examples, output format docs
- [Codecov flags](https://docs.codecov.com/docs/flags) -- per-component coverage in monorepos
- [Codecov components](https://docs.codecov.com/docs/components) -- path-based grouping
- [Rust Project Primer: Coverage](https://www.rustprojectprimer.com/measure/coverage.html) --
  cargo-llvm-cov vs tarpaulin comparison
