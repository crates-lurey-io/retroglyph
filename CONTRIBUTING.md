# Contributing to retroglyph

## Development

Prerequisites:

- Rust (latest stable)
- Node.js (v22.12.0 LTS via `.nvmrc`)

### Workflow

`just check` is the gate before every commit. All clippy lints (including `pedantic` and `nursery`)
are treated as errors.

| Command                | What it does                                                                     |
| ---------------------- | -------------------------------------------------------------------------------- |
| `just check`           | Full gate: fmt-check, lint, compile, test, doc                                   |
| `just clippy`          | Run clippy with `-D warnings` on all targets                                     |
| `just compile`         | `cargo check --all-features`                                                     |
| `just deny-advisories` | `cargo deny check advisories`                                                    |
| `just deny-licenses`   | `cargo deny check bans licenses sources`                                         |
| `just doc`             | Generate private rustdocs, regenerate `llms.txt` / `llms-full.txt`, open browser |
| `just fmt`             | Format Rust + Markdown/JSON/YAML files                                           |
| `just fmt-check`       | Verify formatting without modifying (for CI)                                     |
| `just lint`            | Clippy + markdownlint                                                            |
| `just test`            | Run all tests with all features                                                  |
| `just test-v`          | Run all tests with stdout visible                                                |

## Commit messages, PR titles, and labels

This repo is squash-merge only, and PR titles (not individual commits) must follow
[Conventional Commits](https://www.conventionalcommits.org): `feat(widgets): add sparkline`,
`fix(core): ...`, `docs(software): ...`. The squash-merge turns your PR title into the single commit
on `main`, which is what drives per-crate version bumps and changelogs (via
[release-plz](https://release-plz.dev)) -- see [`RELEASING.md`](RELEASING.md) for the full automated
release flow. Commits inside your branch are unconstrained; only the PR title is checked (CI:
`pr-title.yml`).

**Scope** is the crate directory under `crates/*` your change touches: `core`, `terminal`,
`crossterm`, `terminal-wasm`, `software`, `window`, `widgets`, `examples`. For changes that don't
belong to one crate, use `workspace` (tooling, CI, docs, release config) or `deps` (dependency
bumps). A scopeless title is accepted but `workspace` is preferred.

**Breaking changes:** don't add `!` for an ordinary API-breaking change (removing a public method,
changing a signature, etc.) -- `cargo-semver-checks`, run automatically by release-plz, detects
those and computes the correct version bump on its own, without any commit-message signal. Reserve
`!` (`feat(core)!: ...`) or a `BREAKING CHANGE:` footer for the rarer case where the public API's
signatures don't change but runtime behavior does, which no tool can detect automatically. If you're
not sure whether your change needs `!`, it almost certainly doesn't -- open the PR without it and
let CI's semver check tell you.

**Labels** are applied mostly automatically: `c:<crate>` (area, mirrors the Conventional Commit
scopes above) plus a handful of standalone status/categorization labels (`breaking`, `benchmark`,
`needs-triage`, `blocked`). Priority and type are tracked with GitHub's native Type/Priority issue
fields instead of labels. `.github/labels.yml` is the source of truth (synced with
`.github/scripts/sync-labels.sh`); `.github/labeler.yml` auto-applies and updates `c:` labels on PRs
from changed file paths as the PR evolves, `.github/workflows/labeler.yml`'s title-labels job
derives a fallback `c:` label from the PR title, `check-semver.yml` syncs the `breaking` label from
its own findings (see `RELEASING.md`), and `labeler.yml` marks new issues `needs-triage` unless
filed by a maintainer.

Other labels you may see or apply on a PR:

| Label            | Effect                                                                           |
| ---------------- | -------------------------------------------------------------------------------- |
| `skip-changelog` | Excludes this PR from the generated per-crate changelog (for chore/CI/typo PRs). |
| `no-release`     | Marks a maintainer's Release PR as intentionally held; purely informational.     |

## Crate layout

```text
src/
  backend/
    mod.rs          Backend trait
    headless.rs     In-memory backend (testing)
    crossterm.rs    Crossterm backend (feature-gated)
  cell.rs           Cell — a glyph + Style
  color.rs          Color enum (Default / ANSI / Indexed / RGB)
  grid.rs           Grid — 2-D cell buffer, diff iterator
  style.rs          Style
  event.rs          Event, KeyEvent, MouseEvent
  terminal.rs       Terminal<B> — stateful drawing API, double buffering
```

## Testing

### Unit and integration tests

```sh
just test          # run everything
just test-v        # with stdout (useful for snapshot review)
cargo test --lib   # unit tests only
```

Unit tests live alongside their modules. The integration suite in `tests/e2e.rs` drives
`Terminal<Headless>` through game-logic scenarios and asserts on the grid state directly.

### Snapshot tests (`insta`)

`Headless::format_view()` converts the in-memory grid to a text string where spaces are rendered as
`·`. Pair it with `insta::assert_snapshot!` for deterministic layout assertions:

```rust
use retroglyph::{Terminal, backend::Headless};

let backend = Headless::new(20, 5);
let mut term = Terminal::new(backend);
term.put(2, 2, 'X');
term.present();
insta::assert_snapshot!(term.backend().format_view());
```

To review and accept new or changed snapshots:

```sh
cargo install cargo-insta   # one-time
cargo insta test            # run tests and open the review UI
cargo insta accept          # accept all pending snapshots
```

Snapshot files live in `tests/snapshots/` and are committed to version control. A failing snapshot
test means visible output changed — review the diff before accepting.

### E2E visual snapshots (crossterm backend)

`tests/e2e_snapshots.rs` spawns the compiled `demo` binary (built with `--features crossterm`) in a
real pseudo-terminal using `portable-pty`, feeds it key input, then parses the raw ANSI byte stream
with a VT100 emulator (`vt100` crate) to reconstruct the final screen state. The screen is rendered
to SVG and snapshotted with `insta`.

```sh
# The demo binary must be built first

cargo build --example demo --features crossterm

cargo test --test e2e_snapshots --all-features
```

Two files are written to `tests/snapshots/` on each run:

| File                                | Purpose                                                 |
| ----------------------------------- | ------------------------------------------------------- |
| `e2e_snapshots__demo_snapshot.snap` | Insta snapshot (authoritative, diffed by CI)            |
| `demo.svg`                          | Rendered SVG — open directly in a browser or Quick Look |

GitHub renders `.svg` files, so PR diffs show a visual before/after when the snapshot changes.

To view the current snapshot locally:

```sh
open tests/snapshots/demo.svg
```

## Benchmarking

Performance benchmarks live per-crate, under each crate's own `crates/<name>/benches/` directory
(e.g. `crates/core/benches/grid_diff.rs`) -- not in a shared top-level crate, and not under
`examples/` (see `examples/AGENTS.md` for why perf work and the examples docs-gallery/regression-
suite are kept separate). This mirrors Cargo's own convention for a package's benches and keeps each
crate's `[[bench]]` targets next to the code they measure and its own dev-dependencies (e.g.
`criterion`, `fastrand`), rather than accumulating every crate's benchmarks -- and their combined
dev-dependency set -- in one ever-growing shared crate. Each `<name>.rs` file under a crate's
`benches/` is a [criterion](https://github.com/bheisler/criterion.rs) benchmark, `harness = false`.

| Command                                    | What it does                                                     |
| ------------------------------------------ | ---------------------------------------------------------------- |
| `just bench`                               | Run every benchmark once, locally, no comparison.                |
| `just bench -- <criterion-args>`           | Forward args to criterion, e.g. `just bench -- grid_diff/80x24`. |
| `just bench-compare`                       | Compare the current working copy against `origin/main`.          |
| `just bench-compare <git-ref>`             | Compare against any other commit/tag/branch instead.             |
| `just bench-compare -b <bench-name> <ref>` | Pick a different bench target (only `grid_diff` exists today).   |

`bench-compare` benchmarks your current working copy in place (dirty changes included) and checks
out the comparison ref into a throwaway `git worktree`, so your checkout is never touched. Both runs
share this repo's `target/` directory so criterion's `--save-baseline` data lands in one place, then
[`critcmp`](https://github.com/BurntSushi/critcmp) (resolved via `cargo bin`, like the workspace's
other pinned dev tools) prints the delta. See `tools/bench-compare.sh` (`-h` for the full
flag/example list) for the mechanics. Both `just bench` and `tools/bench-compare.sh` invoke
`cargo bench --workspace --all-features` (no `-p`), so a bench target is found regardless of which
crate owns it, as long as its name is unique across the workspace (`--all-features` matches
`just test`'s convention and avoids feature-gated crates failing to build in isolation).

Note: the comparison ref must already contain the bench target being compared -- you can't compare
against a commit that predates that crate's `benches/` directory.

### CI

`.github/workflows/bench.yml` runs the same
`cargo bench --workspace --all-features --bench grid_diff` on every push to `main` (tracked as
historical data via [Bencher](https://bencher.dev)) and, when a PR is labeled `benchmark`, compares
that PR's branch against `main`'s tracked history and fails the check on a statistically significant
regression (`--error-on-alert`). Add the `benchmark` label to a PR to trigger a check; it's removed
automatically once the workflow runs.

### Adding a new benchmark

Add a `crates/<name>/benches/<bench-name>.rs` file (see `crates/core/benches/grid_diff.rs` for the
pattern: build inputs deterministically -- fixed RNG seeds -- so `--save-baseline`/`--baseline`
comparisons are meaningful) and a matching `[[bench]]` entry plus any needed dev-dependencies (e.g.
`criterion`, `fastrand`) in that crate's `Cargo.toml`. If you want it tracked in CI alongside
`grid_diff`, add it to the `cargo bench` invocations in `bench.yml` too.

## Feature flags

| Flag                    | Default | Description                                                        |
| ----------------------- | ------- | ------------------------------------------------------------------ |
| `std`                   | on      | Enable `std`-dependent code. Disable for `no_std` builds.          |
| `crossterm`             | off     | Enable the `Crossterm` backend. Pulls in the `crossterm` crate.    |
| `software`              | off     | Software pixel backend (winit + softbuffer).                       |
| `software-tilesets`     | off     | PNG sprite sheet tilesets with alpha blending. Implies `software`. |
| `software-default-font` | off     | Include the embedded Unscii 16 bitmap font. Implies `software`.    |
| `egc`                   | on      | Extended grapheme cluster support (combining marks, wide chars).   |
