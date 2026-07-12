# Releasing

Manual release process for the first `0.1.0` publish. From the second release onward the version
bump stays manual (a normal PR that bumps the crate versions); release-plz automates only the
publish/tag/GitHub-release half (see below).

## Versioning

**Lockstep.** All 7 publishable crates version together: `retroglyph-core`, `retroglyph-terminal`,
`retroglyph-crossterm`, `retroglyph-terminal-wasm`, `retroglyph-software`, `retroglyph-window`,
`retroglyph-widgets`. `retroglyph-examples` and `tools/cargo-bin` are `publish = false` and never
ship to crates.io.

This matches the two closest architectural precedents for this workspace, ratatui and bevy (both
researched in this project's own comparison pass before `docs/references/libs/` was retired) -- both
version their entire multi-crate workspace as one number. It also matches this workspace's actual
coupling: every crate here path-depends on `retroglyph-core`, so a change to core forces a version
bump through the whole dependency graph regardless of versioning scheme. Independent per-crate
versioning is meant for workspaces where crates genuinely release on unrelated schedules;
retroglyph's crates don't, so independent versioning would mostly produce release-plz's documented
"cascade bump" noise (a core change bumping every dependent crate's version anyway) without giving
back the benefit independent versioning is supposed to provide.

**Lockstep is a starting choice, not a permanent one.** Nothing about this repo's structure requires
it forever -- ratatui itself was a single crate before becoming a lockstep workspace, and a project
can split to independent versioning later if individual crates' release cadences genuinely diverge
(e.g. a backend crate needing frequent patches while core is stable). Revisit if that happens; don't
pre-commit to switching on any particular timeline.

### Pre-1.0 SemVer policy

While every crate's major version is `0`, Cargo's own SemVer-compatibility rules (and the wider Rust
community's convention) shift the breaking-change signal down one slot from the usual meaning:

- **MINOR** (`0.1.0` -> `0.2.0`): breaking change. This is the pre-1.0 equivalent of a major bump.
- **PATCH** (`0.1.0` -> `0.1.1`): backwards-compatible fix or addition.
- **MAJOR** stays `0` until an explicit, deliberate decision to stabilize the public API at `1.0.0`.

This is not obvious to consumers who assume "0.x means anything goes," so it's stated here
explicitly rather than left implicit. Version bumps are chosen by hand (see "Post-0.1.0" below), so
applying this remapping correctly is on whoever writes the bump PR; `cargo-semver-checks` in CI
(`.github/workflows/check-semver.yml`) is the safety net that catches an accidental breaking change
shipped as a patch.

### MSRV

Current MSRV is `1.88` (edition 2024), set via `workspace.package.rust-version` and inherited by
every crate. No formal bump policy exists yet (e.g. "MSRV bump = minor version bump", or "floats N
releases behind stable") -- that's a deliberate deferral, not an oversight. Decide a policy once
there's real pressure to move the MSRV, rather than picking one speculatively now.

## Pre-publish checklist

1. `readme` set per crate (short crate-specific `README.md`, not the shared workspace README).
2. `[package.metadata.docs.rs] all-features = true` set per crate, so docs.rs renders the full API
   surface including non-default features (`egc`, `tilesets`, `default-font`, etc.). Cargo does not
   support workspace inheritance for this table; it's duplicated per crate on purpose.
3. `CHANGELOG.md` has an "0.1.0 - Initial release" entry.
4. All 7 crate names are reserved on crates.io at `0.0.0-reserved`, confirmed via the registry API.
5. `just check` is green.
6. `cargo publish --dry-run -p <crate>` is clean for every crate, in publish order (below).

   **Caveat, verified during this checklist's execution:** `cargo publish --dry-run` resolves a
   path+version dependency (e.g. `retroglyph-terminal`'s
   `retroglyph-core = { path = "../core", version = "0.1.0" }`) against the _live registry_, not the
   local path, once the crate isn't the first in the chain. Since crates.io currently only has
   `retroglyph-core@0.0.0-reserved`, every dependent crate's dry-run fails with "candidate versions
   found which didn't match" until `retroglyph-core@0.1.0` is actually published. This means the
   full publish order can only be dry-run-verified one tier at a time, not all at once ahead of
   time: dry-run `retroglyph-core` first, publish it for real, then dry-run the next tier, and so
   on. Don't treat a downstream crate's dry-run failure as a real problem before its upstream is
   actually live.

## Publish order

```text
core  ->  terminal, window  ->  crossterm, terminal-wasm, software  ->  widgets
```

`core` has no workspace dependencies. `terminal` and `window` depend only on `core` (either order).
`crossterm` depends on `terminal` + `core`; `terminal-wasm` depends on `core`; `software` depends on
`window` + `core`. `widgets` depends only on `core`, so it can publish any time after step 1, but
publishing it last keeps the run linear and easy to re-run if something in an earlier tier needs a
fix.

```sh
cargo publish -p retroglyph-core
# wait for crates.io index propagation before the next tier
cargo publish -p retroglyph-terminal
cargo publish -p retroglyph-window
# wait for index propagation
cargo publish -p retroglyph-crossterm
cargo publish -p retroglyph-terminal-wasm
cargo publish -p retroglyph-software
# wait for index propagation
cargo publish -p retroglyph-widgets
```

Path dependencies re-resolve against the just-published version once each tier's `Cargo.lock`
regenerates, so publishing out of order (a downstream crate before its upstream dependency is live
on the index) will fail the dry run first.

## Tagging

No tags exist in this repository yet; `v0.1.0` is the first one. Tag the commit that was actually
published, after all 7 crates are live:

```sh
jj bookmark set v0.1.0 -r @-  # or the specific change that was published
jj git push --bookmark v0.1.0
```

(Lockstep versioning means one tag covers all 7 crates for this release. Post-release-plz adoption,
per-crate tags may replace this if versioning splits per crate.)

## Post-0.1.0: release-plz

[release-plz](https://release-plz.dev) automates the publish half of a release; the version bump
stays manual. Config: `release-plz.toml`, `cliff.toml`, `.github/workflows/release-plz.yml`,
`.github/workflows/check-semver.yml`. The flow:

1. Bump all 7 crate versions in a normal PR (lockstep, see above), update `CHANGELOG.md` for the new
   version (by hand, or generate the entries locally with `release-plz update` / `git-cliff`), and
   merge to `main`.
2. The `check-versions` job in `release-plz.yml` diffs each crate's `Cargo.toml` version at `HEAD`
   against its version at the commit main pointed at right before this push (`github.event.before`,
   plain git history, no tags or crates.io lookups involved). If any version changed, the `release`
   environment's required reviewer is asked to approve; pushes that don't bump versions never ping
   anyone.
3. On approval, `release-plz release` publishes the crates whose version isn't on crates.io yet,
   tags each as `<name>-v<version>`, and creates GitHub releases with the git-cliff changelog as the
   body.

release-plz's `release-pr` command (the mode that reads Conventional Commits history and auto-opens
version-bump PRs) is deliberately not used: it treats _any_ commit touching a crate's files as
releasable by default, which produced
[PR #78](https://github.com/crates-lurey-io/retroglyph/pull/78) (an unprompted v0.1.1 bump with an
empty changelog, closed as unintended). Manual bumps keep the release decision with a human.

Remaining configuration notes:

- No automated alpha/pre-release channel for now (ratatui runs a weekly automated alpha build off
  `main`; that's real CI investment worth deferring until there's actual demand for bleeding-edge
  pre-releases, not something to set up speculatively on a library's first public release).
- `semver_check` handled via a separate `cargo-semver-checks` CI job (ratatui's pattern), not
  release-plz's own built-in check, so the manual version bump and the actual API-compat check
  surface independently instead of one silently overriding the other.

The `check-versions` job needs no one-time baseline: it only ever looks at the two Cargo.toml
revisions either side of a push, so it works from the very first push after this workflow lands,
regardless of how the 0.1.0 release was tagged (or not).

### Publishing: trusted publishing, not a registry token

`release-plz-release` publishes via
[crates.io trusted publishing](https://crates.io/docs/trusted-publishing) -- no
`CARGO_REGISTRY_TOKEN` secret exists in this repo at all. release-plz implements the same OIDC token
exchange as `rust-lang/crates-io-auth-action` internally, so the workflow only needs
`id-token: write` permission on that job (already set in `.github/workflows/release-plz.yml`).

This was configured on crates.io for all 7 publishable crates (Settings -> Trusted Publishing -> Add
-> GitHub), each pointing at:

- Repository owner: `crates-lurey-io`
- Repository name: `retroglyph`
- Workflow filename: `release-plz.yml`
- Environment: `release`

Trusted publishing can't be used for a crate's _first_ publish (crates.io requires a real API token
for that) -- moot here since the 0.1.0 publish already happened manually, so every crate had an
existing version before trusted publishing was configured.

The `release` environment referenced above
(github.com/crates-lurey-io/retroglyph/settings/environments) adds a second, independent gate on top
of trusted publishing's own repo/workflow-filename check: required reviewer (`matanlurey`) and a
`main`-only branch policy, so an actual crates.io publish always needs a manual approval click, even
though the workflow itself runs unattended.

## Non-blocking follow-ups (do alongside, not before)

- `license-file.workspace = true` per crate (crates.io doesn't require a local `LICENSE` file when
  `license` is set as an SPDX string, but some compliance scanners look for one; same mechanism as
  the readme fix).
- `codecov.yml` per-crate flags (already fixed, see `codecov.yml` -- flags now match `crates/*/src/`
  instead of the pre-split single `retroglyph`/`src/` flag).
- Same-author upstream dependencies (`gem`, `grixy`, `ixy`) are pre-1.0/alpha; their version churn
  is a transitive stability risk for consumers, not fixable from this repo, just worth tracking.
