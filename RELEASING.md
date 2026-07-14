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

**Do not put `!` on a commit for an ordinary API-signature breaking change.** `semver_check = true`
(release-plz.toml) runs `cargo-semver-checks` while computing the Release PR and independently
detects and correctly bumps a crate whose public API actually broke -- it is the real authority, not
the commit message. Verified concretely: a `#[non_exhaustive]` addition to `retroglyph-core` (a real
breaking change) computes `retroglyph-core: 0.1.0 -> 0.2.0` from `semver_check` alone, with no `!`
anywhere in the commit.

**Why this matters in a monorepo with atomic, cross-crate commits:** release-plz attributes a
commit's Conventional Commit classification (including a `!`/`BREAKING CHANGE` marker) to every
crate whose packaged files that commit touches -- by file path, not by the commit's stated
`type(scope)`. A single atomic commit that changes `crates/core/` (a real break) and also touches
`crates/widgets/` (a companion, non-breaking, mechanical fix needed only because of the core change)
will have its `!` applied to **both** crates, even though widgets' own API is untouched. This isn't
hypothetical: it happened on this exact repo (`retroglyph-widgets` was incorrectly proposed for a
`0.1.0 -> 0.2.0` bump from a `feat(core)!:` commit that happened to also touch a private function in
`crates/widgets/src/interact/pointer.rs`) and required rewriting an already-merged commit's message
to fix, since `cliff.toml`/`release-plz.toml` config cannot intervene at the granularity needed --
the misattribution happens before either file's rules are ever read. See the reserved `!` case below
for commits that unavoidably need to declare a real break; keep them scoped to the crate(s) actually
breaking, and land any companion cross-crate fix as a **separate** commit/PR when practical, since
that's the only thing that fully avoids this class of misattribution.

**Reserve `!` / `BREAKING CHANGE:` for the narrow case `cargo-semver-checks` cannot see:** a
behavioral break with unchanged public signatures (same types, same function shapes, different
runtime meaning). That's genuinely rare. Declare it with a `!` in the PR title, placed after the
scope per Conventional Commits grammar (`feat(core)!: ...`, `fix!: ...`), or a `BREAKING CHANGE:`
footer in the squash commit body. Prefer keeping that commit scoped to only the crate(s)
experiencing the break, for the reason above.

`cargo-semver-checks` runs at two points:

- At release time, inside release-plz (`semver_check = true`): the authority described above. Its
  result is shown on the Release PR.
- At PR time (`.github/workflows/check-semver.yml`), as a **non-blocking** informational comment,
  not a merge gate. A crate's `Cargo.toml` version isn't bumped until the Release PR, so a hard
  PR-time gate would fail almost every legitimate breaking PR (since `!` is now reserved for the
  rare behavioral-break case) and force a `semver-override`-style label onto the common case --
  exactly backwards. The PR-time job exists purely so a reviewer sees the finding before merge; the
  real enforcement is `semver_check` on the Release PR.

## PR labels (overrides)

| Label            | Effect                                                                                                    |
| ---------------- | --------------------------------------------------------------------------------------------------------- |
| `skip-changelog` | Keep this PR out of the generated changelog. See the note below on how this is wired.                     |
| `no-release`     | Annotation only: marks a Release PR you intend to hold. Not enforced; the real control is not merging it. |

There is deliberately no `breaking` label: a label can't drive release-plz's version bump (only a
commit's `!`/`BREAKING CHANGE:` can), so a `breaking` label without one would be a footgun that
ships a break as a patch. There is also no `semver-override` label anymore: the PR-time semver check
is non-blocking, so nothing needs overriding.

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
| `.github/workflows/check-semver.yml` | Non-blocking PR-time `cargo-semver-checks` report.         |

## Known gotcha: rewriting a commit `main` already has a Release PR built on

If you ever amend an already-merged commit on `main` (e.g. correcting a mis-declared breaking
change, as happened once on this repo), the _existing_ Release PR's branch does not get rebased
automatically. release-plz's `release-pr` job will report success and log the correct recomputed
versions, but silently leave the stale PR's branch content untouched, since it diverges from the new
`main` and a normal (non-force) update can't reconcile that. Symptom: the Release PR's diff still
shows the old, wrong numbers despite the workflow run's own logs showing the right ones.

Fix: close the stale Release PR and delete its branch; release-plz creates a fresh one (with a new
timestamped branch name) on the next push to `main`, correctly based on the new history. If no other
push is imminent, use `workflow_dispatch` on `Release-plz` (Actions tab, or
`gh workflow run release-plz.yml`) to trigger it manually rather than waiting.

Rewriting an already-merged commit on `main` at all requires temporarily disabling this repo's
branch protection ruleset (`Settings -> Rules -> Protect`, or `gh api` on
`repos/{owner}/{repo}/rulesets/{id}` with `enforcement: disabled`, then `active` again immediately
after pushing) -- it blocks force-pushes to `main` outright by design. Treat this as a rare,
deliberate, single-purpose escape hatch, not a normal workflow.

## Non-blocking follow-ups

- `license-file.workspace = true` per crate (some compliance scanners want a local `LICENSE`).
- Same-author upstream dependencies (`gem`, `grixy`, `ixy`) are pre-1.0/alpha; their churn is a
  transitive stability risk for consumers, worth tracking but not fixable from this repo.
- Revisit whether `no-release` earns its keep after a few real releases.
- Reconsider a `1.0.0` stabilization and an `-rc` prerelease channel when the API settles.
