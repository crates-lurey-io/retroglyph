# Releasing

Releases are automated with [release-plz](https://release-plz.dev). Day to day you never bump a
version, edit a changelog, or push a tag by hand. You review PRs and, when you want to ship, merge a
single machine-generated "Release PR." Everything downstream (crates.io publish, git tags, GitHub
releases) happens in CI.

## The short version

1. Land normal PRs on `main`. Each PR's **title** is a Conventional Commit; the squash-merge turns
   that title into `main`'s commit message.
2. release-plz keeps a standing **Release PR** open, continuously recomputing per-crate version
   bumps and changelog entries from the conventional history.
3. When you want to release, **merge the Release PR**. That merge is the only approval.
4. CI publishes every crate whose version isn't yet on crates.io, then creates that crate's git tag
   and GitHub release.

If a push to `main` has nothing releasable, there's no Release PR and nothing happens.

## Versioning

**Per-crate, independent versions.** Each publishable crate (`retroglyph-core`,
`retroglyph-terminal`, `retroglyph-crossterm`, `retroglyph-terminal-wasm`, `retroglyph-software`,
`retroglyph-window`, `retroglyph-widgets`) carries and bumps its own version. release-plz bumps only
the crates that actually changed. `retroglyph-examples` and `tools/cargo-bin` are `publish = false`
and never ship.

**Cascade is expected, not lockstep.** Every crate path-depends on `retroglyph-core`, so a bump to
`core` updates each dependent's `retroglyph-core = { version = ... }` requirement, which bumps those
dependents too. This looks like lockstep for any change that touches `core`, but it isn't: a change
isolated to a leaf crate (say `retroglyph-widgets`, which only depends on `core`) bumps that crate
alone. The independent-versioning benefit is real only for those leaf-local changes; core-touching
changes legitimately move most of the workspace.

This reverses the earlier lockstep decision. Lockstep was chosen for the `0.1.0` hand-publish
because one version number is simpler to reason about; automation removes that simplicity cost
(release-plz tracks all seven independently for free) while avoiding phantom bumps and empty "no
notable changes" changelog entries on crates that didn't change.

### Pre-1.0 SemVer policy

While a crate's major version is `0`, Cargo's compatibility rules shift the breaking-change signal
down one slot:

- **MINOR** (`0.1.0` -> `0.2.0`): breaking change. The pre-1.0 equivalent of a major bump.
- **PATCH** (`0.1.0` -> `0.1.1`): backwards-compatible fix or addition.
- **MAJOR** stays `0` until a deliberate decision to stabilize at `1.0.0`.

release-plz applies this remapping when it computes the Release PR: while a crate is pre-1.0 a
breaking change (`!` in the title, or a `BREAKING CHANGE:` footer) bumps the minor, and any
non-breaking `feat:`/`fix:` bumps the patch. Verify this holds on the first real Release PR and tune
release-plz's bump config if a plain `feat:` ever proposes a `0.x -> 0.(x+1)` minor bump; the whole
policy below depends on breaking being the only thing that reaches minor pre-1.0.

### No prerelease channel (pre-1.0)

There is no `-dev`/`-alpha`/`-beta` channel right now. The Release PR's unreleased changelog section
is the staging area for the next version; consumers who want bleeding-edge code use a git
dependency. crates.io has no dist-tag mechanism, and cargo never auto-selects a SemVer prerelease
unless a consumer opts in explicitly, so a prerelease channel would be machinery without a current
consumer. Post-1.0 the plan is to add `-rc` releases on crates.io for public previews; that's
deferred until 1.0 is actually on the horizon.

### MSRV

Current MSRV is `1.88` (edition 2024), set via `workspace.package.rust-version` and inherited by
every crate. No formal bump policy exists yet; decide one when there's real pressure to move the
MSRV rather than picking a policy speculatively. When it does move, treat an MSRV bump as at least a
minor (pre-1.0 breaking) bump.

## Conventional Commits: PR titles, not commits

Conventional Commits are enforced on **PR titles only**, not on individual commits. Because the repo
is squash-merge only, the PR title becomes the single commit on `main`, so the history release-plz
and git-cliff read stays fully conventional while your work-in-progress commits stay unconstrained.

- CI (`.github/workflows/pr-title.yml`) validates each PR title against the Conventional Commit
  grammar and the allowed scope list: the crate scopes (`core`, `terminal`, `crossterm`,
  `terminal-wasm`, `software`, `window`, `widgets`, `examples`) plus workspace-level `workspace`
  (tooling, CI, root docs, release config) and `deps` (dependency bumps). A scopeless title is also
  accepted.
- Repository settings allow **squash merge only** (rebase and merge-commit disabled), and the squash
  commit message is set to default to the PR title. Without this, PR-title enforcement buys nothing.

## Declaring breaking changes

Declare a breaking change with a `!` in the PR title, placed after the scope per Conventional
Commits grammar: `feat(core)!: ...`, `fix!: ...` (or a `BREAKING CHANGE:` footer in the squash
commit body). The squash-merge turns that title into the commit release-plz reads, so the `!` is
what makes release-plz pick the breaking (pre-1.0 minor) bump. A GitHub label cannot do this:
release-plz reads commit messages, not labels, so a break declared only by a label would still ship
as a patch. Declare behavioral breaks (same signatures, changed runtime behavior) with `!` too.

`cargo-semver-checks` runs at two points, in two different jobs:

- At release time, inside release-plz (`semver_check = true`): it compares each crate's API against
  its last crates.io release and forces the proposed bump to be large enough for the changes it
  finds. This is the real backstop against an undeclared API break shipping as a patch; its result
  is shown on the Release PR.
- At PR time (`.github/workflows/check-semver.yml`) as a fast reviewer signal. A crate's
  `Cargo.toml` version isn't bumped until the Release PR, so a raw semver check fails _any_ breaking
  PR regardless of intent. This job is therefore skipped when the break is already declared (`!` in
  the title) or explicitly allowed (`semver-override` label), and gates only _undeclared_ API
  breaks. A failure means: add the `!`, or apply `semver-override`.

The PR-time gate keys off the title `!` and the label, not the commit body, so if you ever declare a
break with a `BREAKING CHANGE:` footer alone (no title `!`), apply `semver-override` as well to keep
that gate green. For the normal squash-title workflow the title `!` is both the release-plz bump
signal and the gate-skip signal, so there's nothing extra to do.

## PR labels (overrides)

| Label             | Effect                                                                                                                                 |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `skip-changelog`  | Keep this PR out of the generated changelog. See the note below on how this is wired.                                                  |
| `semver-override` | Allow an intentional or false-positive `cargo-semver-checks` finding; skips the PR-time semver gate. Does not change the version bump. |
| `no-release`      | Annotation only: marks a Release PR you intend to hold. Not enforced; the real control is not merging it.                              |

There is deliberately no `breaking` label: a label can't drive release-plz's version bump (only the
`!` in the commit can), so a `breaking` label without a `!` would be a footgun that ships a break as
a patch. Declare all breaks with `!` in the title instead.

Note on `skip-changelog`: git-cliff builds the changelog from commit messages, not GitHub labels, so
label-based exclusion relies on release-plz's GitHub integration attaching PR metadata to each
commit. `cliff.toml` also skips any commit whose body carries a `changelog: ignore` footer, which is
the guaranteed fallback if the label ever doesn't take. Confirm the label works on its first use.

## The Release PR

release-plz's `release-pr` command runs on every push to `main`
(`.github/workflows/release-plz.yml`) and maintains one open Release PR. For each crate with
unreleased changes it:

- picks the version bump from that crate's conventional commits (the `!`/`BREAKING CHANGE` signal),
- cascades bumps to dependents through the `core` dependency graph,
- writes the crate's `CHANGELOG.md` section (git-cliff, Keep a Changelog format, per crate),
- updates `Cargo.toml` and `Cargo.lock`.

Crates with no changes since their last release are absent from the PR. The PR stays open and
updates itself as more PRs land, until you merge it.

This is a deliberate reversal of the previous "manual bump, release-plz publishes only" setup. That
setup existed because an earlier `release-pr` config opened an unwanted bump PR
([#78](https://github.com/crates-lurey-io/retroglyph/pull/78)). The fix is not to disable
`release-pr` but to treat its PR as the intended release control: a standing proposal you merge on
your schedule.

## Publishing (on Release-PR merge)

Merging the Release PR pushes the version bumps to `main`, which triggers release-plz's `release`
command (`.github/workflows/release-plz.yml`). For each crate whose `Cargo.toml` version is ahead of
crates.io it:

- publishes to crates.io in dependency order,
- creates a per-crate git tag `retroglyph-<crate>-v<version>`,
- creates a per-crate GitHub release with that crate's git-cliff changelog as the body.

You never create or push a tag by hand.

### Publish order

release-plz computes and follows this order automatically; it's documented here for when a manual
re-run is needed:

```text
core  ->  terminal, window, widgets, terminal-wasm  ->  crossterm, software
```

`core` has no workspace dependencies. `terminal`, `window`, `widgets`, and `terminal-wasm` depend
only on `core`. `crossterm` depends on `terminal` + `core`; `software` depends on `window` + `core`.

### Approval gate

The single human gate is **merging the Release PR**. The `release` GitHub environment
(github.com/crates-lurey-io/retroglyph/settings/environments) keeps its `main`-only branch policy
but no longer requires a separate reviewer; merging the PR already was the deliberate action, and a
second approval click on the publish job is redundant for this project. The branch policy stays as a
guard against the workflow ever running from a non-`main` ref.

### Trusted publishing

Publishing uses [crates.io trusted publishing](https://crates.io/docs/trusted-publishing); there is
no `CARGO_REGISTRY_TOKEN` secret in this repo. release-plz performs the OIDC token exchange itself,
so the publish job only needs `id-token: write`. Trusted publishing is configured per crate on
crates.io (Settings -> Trusted Publishing -> GitHub) for all seven publishable crates, each pointing
at:

- Repository owner: `crates-lurey-io`
- Repository name: `retroglyph`
- Workflow filename: `release-plz.yml`
- Environment: `release`

Trusted publishing can't perform a crate's _first_ publish (crates.io requires a real token for
that); moot here since every crate was hand-published at `0.1.0`.

## First-release checklist (already satisfied, kept for reference)

The `0.1.0` release was hand-published. These invariants must stay true for automation to work:

1. `readme` set per crate (short crate-specific `README.md`, not the shared workspace README).
2. `[package.metadata.docs.rs] all-features = true` per crate.
3. Each crate's name reserved and its `0.1.0` live on crates.io.
4. `just check` green.

## Files

| File                                 | Role                                                       |
| ------------------------------------ | ---------------------------------------------------------- |
| `release-plz.toml`                   | release-plz config: per-crate changelogs, semver, bumping. |
| `cliff.toml`                         | git-cliff changelog template and commit grouping.          |
| `crates/*/CHANGELOG.md`              | Per-crate changelog, maintained by release-plz.            |
| `.github/workflows/release-plz.yml`  | `release-pr` + `release` jobs.                             |
| `.github/workflows/pr-title.yml`     | Conventional Commit PR-title enforcement.                  |
| `.github/workflows/check-semver.yml` | PR-time `cargo-semver-checks` gate for undeclared breaks.  |

## Non-blocking follow-ups

- `license-file.workspace = true` per crate (some compliance scanners want a local `LICENSE`).
- Same-author upstream dependencies (`gem`, `grixy`, `ixy`) are pre-1.0/alpha; their churn is a
  transitive stability risk for consumers, worth tracking but not fixable from this repo.
- Revisit whether `no-release` earns its keep after a few real releases.
- Reconsider a `1.0.0` stabilization and an `-rc` prerelease channel when the API settles.
