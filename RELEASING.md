# Releasing

Manual release process for the first `0.1.0` publish. Automation (release-plz, see below) takes over
from the second release onward.

## Versioning

All 7 publishable crates version in lockstep: `retroglyph-core`, `retroglyph-terminal`,
`retroglyph-crossterm`, `retroglyph-terminal-wasm`, `retroglyph-software`, `retroglyph-window`,
`retroglyph-widgets`. `retroglyph-examples` and `tools/cargo-bin` are `publish = false` and never
ship to crates.io.

First real publish is `0.1.0` (bumped straight from the current `0.1.0-alpha`, no further alpha/rc
cycle). Every crate is currently pinned 1:1 in version, so lockstep is the natural starting point;
whether to split to independent per-crate versioning is a decision for whenever release-plz is
adopted (see below), not for this first release.

## Pre-publish checklist

1. `readme` set per crate (short crate-specific `README.md`, not the shared workspace README).
2. `[package.metadata.docs.rs] all-features = true` set per crate, so docs.rs renders the full API
   surface including non-default features (`egc`, `tilesets`, `default-font`, etc.). Cargo does not
   support workspace inheritance for this table; it's duplicated per crate on purpose.
3. `CHANGELOG.md` has an "0.1.0 - Initial release" entry.
4. `retroglyph-terminal` and `retroglyph-terminal-wasm` are reserved on crates.io. (The other five
   names -- `retroglyph`, `-core`, `-crossterm`, `-software`, `-window`, `-widgets` -- are already
   parked at `0.0.0-reserved`; these two were not, as of the last check.)
5. `just check` is green.
6. `cargo publish --dry-run -p <crate>` is clean for every crate, in publish order (below).

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

## Post-0.1.0: adopt release-plz

[release-plz](https://release-plz.dev) becomes the tool of record for changelog generation, version
bumps, and publishing from the second release onward. It reads Conventional Commits history per
crate (`fix(core): ...` -> patch, `feat(widgets): ...` -> minor, `!`/`BREAKING CHANGE:` -> major,
see `AGENTS.md`'s commit-scope convention), opens a release PR with the bumps and changelog already
computed, generates changelogs via `git-cliff`, and can gate on `cargo-semver-checks` to catch
accidental breaking changes before they're tagged. It understands Cargo workspaces natively and can
version/publish/changelog each crate independently based on which paths a commit touched, so
`retroglyph-core`, `retroglyph-widgets`, etc. can diverge in version and release cadence after 0.1.0
instead of staying forced-lockstep.

It needs an existing tagged baseline to diff against, which `v0.1.0` (above) provides. Configuring
and wiring the GitHub Action is a follow-up task, not part of the manual 0.1.0 publish itself.

## Non-blocking follow-ups (do alongside, not before)

- `license-file.workspace = true` per crate (crates.io doesn't require a local `LICENSE` file when
  `license` is set as an SPDX string, but some compliance scanners look for one; same mechanism as
  the readme fix).
- `codecov.yml` per-crate flags (already fixed, see `codecov.yml` -- flags now match `crates/*/src/`
  instead of the pre-split single `retroglyph`/`src/` flag).
- Same-author upstream dependencies (`gem`, `grixy`, `ixy`) are pre-1.0/alpha; their version churn
  is a transitive stability risk for consumers, not fixable from this repo, just worth tracking.
