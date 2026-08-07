# AGENTS.md

`retroglyph` is a 2D pseudographic terminal library. It provides a double-buffered `Terminal<B>`
generic over a pluggable `Backend`, with styled cells, input events, and pluggable
software/crossterm/WASM backends.

Workspace: a Cargo workspace under `crates/*` (`retroglyph-core` plus backend and helper crates),
plus `examples/` and `tools/cargo-bin`. There is no single-crate `src/` root and no `retroglyph`
facade crate; consumers depend on `retroglyph-core` and whichever backend/helper crates they need.
For the crate list and feature overview see `README.md`; for a machine-readable summary of each
crate's public module and type surface see that crate's `llms.txt`, generated under
`target/doc/<crate>/llms.txt` by `just doc`.

## Correctness gate

**Run `just check` before every commit.** It's the fast local gate: clippy, tests, and docs, all
built once with `--workspace --all-features` (plus whatever's needed to power that, like building
the PTY examples ahead of the test run). It deliberately skips formatting, Markdown/prose lint, and
the `--no-default-features`/isolated-feature passes -- run `just fmt-check`, `just lint`, and
`just compile` directly for those, or rely on CI, which runs all of them on every push. All clippy
lints (including `pedantic` and `nursery`) are errors; `missing_docs` is a warning that's promoted
to a hard failure via `-D warnings` in `just clippy`.

```sh
just check           # fast gate: clippy + test + doc, --workspace --all-features only
just check-targets   # also required when touching Output/DrawCell/a backend impl (see below)
just fmt             # auto-fix Rust + Markdown/JSON formatting
just test            # cargo test --all-features
just test-v          # same but with stdout (useful for snapshot review)
just clippy          # clippy only
just compile         # cargo check --all-features
just doc              # private rustdocs + per-crate llms.txt
just docs-preview    # build docs and open in browser
```

For a quick iterative loop: `just compile` to catch type errors fastest, `just check` for the full
`--all-features` build/lint/test/doc pass, then `just fmt-check`/`just lint`/`just compile` before
pushing.

**`just check` only compiles the host target.** `retroglyph-gl` gates three test modules to other
targets (`headless.rs` to Linux, `webgl_smoke.rs` and `webgl_recovery.rs` to wasm32), and all three
drive the `Output` trait. On any other host they are invisible to the gate, so a change to the
backend draw contract can be locally green and fail CI. Run `just check-targets` as well when
touching `Output`, `DrawCell`, or a backend implementation (retroglyph#552).

**`retroglyph-wgpu`'s offscreen render tests need a GPU adapter, not a particular target.** They are
not `cfg`-gated, so `just check` runs them wherever `wgpu` finds an adapter (any developer machine)
and skips them with a message where it doesn't. That skip is silent enough to hide a real break, so
CI's `wgpu-headless` job sets `RETROGLYPH_REQUIRE_WGPU=1` to turn a missing adapter into a failure.
If you touch the wgpu draw path on a machine without an adapter, check the CI job rather than
assuming a green local run covered it.

## Key rules

- **Comment/doc-comment line width: use the full ~100 cols, not ~80.** There's no `rustfmt.toml` in
  this repo, so rustfmt's default `max_width = 100` applies, and `wrap_comments` is `false` by
  default (rustfmt never rewraps prose comments for you). Don't hand-wrap doc comments to ~80 cols
  out of habit; wrap near the real 100-col budget instead.
- **Doc comments describe the API as it is, never the change that produced it.** A reader of
  `cargo doc` has no idea what the code looked like before, so a comment written from the diff's
  point of view is noise to them and rots the moment the next change lands. Don't write "this used
  to be X", "the only behaviour available before Y existed", "kept for backwards compatibility",
  "renamed from Z", or attribute rationale ("`#[non_exhaustive]` so adding a variant isn't a
  breaking change"). State the current contract and the reason it holds, in the present tense. That
  history belongs in the commit message, the PR body, or the changelog, all of which are addressed
  to reviewers rather than to consumers. Referencing an issue for design context (`retroglyph#304`)
  is fine and already common in this repo; narrating the edit is not.
- **No `eprintln!` in library code.** Use the `log` crate (feature-gated). Fatal backend init
  errors: `log::error!` + `event_loop.exit()`, not `panic!`.
- **`unsafe_code` is forbidden** (`Cargo.toml` lint).
- **No interactive jj/git commands.** Always pass `-m` to avoid opening `$EDITOR`. Use
  `jj split [FILESETS...]` by path, never interactively.
- See `STYLE_GUIDE.md` for the full Rust API/style rules (error handling, `#[non_exhaustive]`,
  `unwrap`/`expect` policy, and the curated external-reading list).

## Testing

`cargo test --workspace --all-features` (or `just test`) runs everything. Snapshot review:
`just insta` (blesses unconditionally, no review step of its own). See `examples/AGENTS.md` for the
per-example validation checklist.

## Commit messages

Conventional Commits, scoped to the crate directory under `crates/*` a change touches:
`feat(widgets): ...`, `fix(software): ...`, `docs(core): ...`. Valid scopes: `core`, `terminal`,
`crossterm`, `terminal-wasm`, `software`, `gl`, `wgpu`, `window`, `widgets`, `examples`. For changes
that don't belong to a single crate, use a workspace-level scope: `workspace` (tooling, CI, root
docs, release config) or `deps` (dependency bumps). A scopeless title is still accepted, but prefer
`workspace` over omitting the scope.

The convention is enforced on **PR titles**, not individual commits. The repo is squash-merge only,
so the PR title becomes the single commit on `main`, and `.github/workflows/pr-title.yml`
(`amannn/action-semantic-pull-request`) validates each title against the grammar and scope list
above. Work-in-progress commits inside a branch are unconstrained. This is load-bearing:
`release-plz` (see `RELEASING.md`) reads the squashed history to compute per-crate version bumps and
changelogs, so a PR title that doesn't follow the convention won't be attributed a version bump
correctly.

PR-title CI is where enforcement lives; there's no local commit-msg hook for this.

### Breaking changes: don't reach for `!` by default

**Do not mark an ordinary API-signature breaking change with `!`.** `release-plz`'s own
`semver_check` (via `cargo-semver-checks`) independently detects and correctly per-crate-scopes that
kind of break while computing the Release PR. Verified concretely on this repo: adding
`#[non_exhaustive]` to a public enum computed the correct `0.1.0 -> 0.2.0` bump from
`cargo-semver-checks` alone, with no `!` anywhere in the commit.

**Why it matters in this monorepo:** release-plz attributes a commit's Conventional Commit
classification (including a `!`) to every crate whose packaged files that commit touches, by file
path, not by the commit's stated `type(scope)`. A single atomic commit that changes `crates/core/`
(a real break) and also touches `crates/ui/` (a companion, non-breaking, mechanical fix needed only
because of the core change) will have its `!` applied to **both** crates, even though widgets' own
API is untouched. This happened for real on this repo and required rewriting an already-merged
commit to fix (see `RELEASING.md`'s "Known gotcha" section). Since atomic, cross-crate commits are
the whole point of this being a monorepo, don't try to avoid this by splitting commits; avoid it by
not putting `!` on commits that don't need it.

**Reserve `!` / a `BREAKING CHANGE:` footer for the narrow case `cargo-semver-checks` can't see:** a
behavioral break with unchanged public signatures (same types, same function shapes, different
runtime meaning). That's genuinely rare. When you do need it, prefer keeping that commit scoped to
only the crate(s) actually experiencing the break.

### PR labels

| Label            | Effect                                                                                                      |
| ---------------- | ----------------------------------------------------------------------------------------------------------- |
| `skip-changelog` | Keep this PR out of the generated per-crate changelog (chore/CI/typo noise).                                |
| `no-release`     | Annotation only, marking a Release PR you intend to hold. Not enforced: the real control is not merging it. |

The `breaking` label (`.github/labels.yml`) is a plain categorization label synced automatically by
`check-semver.yml` from its own `cargo-semver-checks` finding; it never drives release-plz's version
bump. There is no `semver-override` label; see `RELEASING.md` for why.

## Docs

- `README.md`: project overview, features, crate list, quick start.
- `STYLE_GUIDE.md`: Rust API and code style conventions.
- `RELEASING.md`: the crates.io publish process.
- `llms.txt` / `llms-full.txt`: generated machine-readable per-crate API summaries. `just doc`
  generates a pair under `target/doc/<crate>/` for each publishable crate; the full version includes
  all public type signatures and doc comments.
- `docs/references/`: deep-dive research for topics an ADR flagged as open or deferred (future
  GPU/SDL backends, accessibility, font rendering, benchmarking, packaging/distribution). Not a
  general reference library; if a topic here is fully implemented, the code and its rustdoc are the
  reference, not this directory.
- Nested `AGENTS.md` files exist per-crate/directory for rules specific to that scope (see
  `crates/ui/AGENTS.md`, `examples/AGENTS.md`).
