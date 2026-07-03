# ADR 017: Release Process and Workspace Tooling

**Status:** Draft (stub -- most decisions intentionally deferred) **Date:** 2026-07-02 **Relates
to:** [ADR 014: Workspace Split](014-workspace-split.md)

## Context

ADR 014 splits `retroglyph` into a multi-crate workspace. That raises questions this ADR is meant to
eventually answer: which tool automates the multi-crate crates.io publish, how changelogs get
generated, and what additional workspace-hygiene tooling (semver checking, feature powerset
checking, dependency auditing, unused-dep detection) is worth running.

**None of that is urgent yet.** Publishing to crates.io is explicitly out of scope for ADR 014's
implementation -- the workspace can exist, build, and be tested entirely from git first. This ADR
exists so the research done while discussing ADR 014 isn't lost, and so there's a place to land the
actual decisions when a first publish is scheduled.

## Decided now

**Adopt Conventional Commits** (`feat:`, `fix:`, `chore:`, `BREAKING CHANGE:` footers, etc.) as a
repo-wide commit message convention, effective immediately, independent of when publishing starts.

Rationale for deciding this now rather than deferring with everything else: it's a low-cost habit
change, not a tooling investment, and both of the changelog/release tool candidates below
(git-cliff, release-plz) infer versions and changelogs from commit history. Adopting the convention
late means a chunk of history a changelog tool can't parse; adopting it now costs nothing and keeps
the option open.

Enforcement: via an `hk` pre-commit/pre-push hook (see `hk.pkl`), not CI-only, matching the
project's general "local dev story first" preference. Exact lint rule (commitlint-style regex vs
something looser) is implementation detail, not an ADR-level decision.

## Deferred: everything else

The following are open questions, with the current leading candidates recorded for when this ADR is
revisited (not decided):

### Release automation tool

Candidates researched: [`release-plz`](https://github.com/release-plz/release-plz) (PR-based,
automates changelog generation via git-cliff + version bumps + publish ordering from Conventional
Commits history) vs [`cargo-release`](https://github.com/crate-ci/cargo-release) (CLI-driven,
explicit per-release control, native `--workspace`/`--exclude`/`--package` support, no built-in
changelog generation). Note: `release-plz` has documented workspace-specific rough edges around
dependency-version-propagation edge cases worth reading before adopting, not just taking on faith.

**Not decided.** Revisit once a first crates.io publish is actually being scheduled.

### Changelog generation

[`git-cliff`](https://git-cliff.org) is the standard choice here, either standalone or via
`release-plz`'s built-in integration (it uses git-cliff internally). Leaning toward _not_ running it
standalone -- only via whichever release tool is chosen above, to avoid maintaining two configs for
the same thing.

**Not decided.** Depends on the release automation tool choice.

### Additional workspace tooling (researched, not yet adopted)

| Tool                                                                       | What it does                                                                                                                     | Why it's relevant here                                                                                                                                                                                                                          |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks) | Compares a crate's public API against the last published version; `deny`-level lints hard-fail CI on accidental breaking changes | Once there are 4-5 independently-versioned published crates, an accidental breaking change in a "patch" release is easy to miss without this                                                                                                    |
| [`cargo-hack`](https://github.com/taiki-e/cargo-hack)                      | Exercises feature powersets / `--each-feature` builds                                                                            | Already pulled forward into ADR 014 Step 4 (needed at split time, not publish time) for `retroglyph-software`'s `tilesets`/`default-font` flags and `retroglyph-window`'s wasm variant. Re-listed here for completeness of the tooling picture. |
| [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny)                | License / advisory / duplicate-dependency linting across the workspace                                                           | Useful once winit/softbuffer/image/wgpu pull in a much larger dependency tree than today's single crate                                                                                                                                         |
| [`cargo-udeps`](https://github.com/est31/cargo-udeps)                      | Detects unused dependencies per-crate                                                                                            | Splitting one Cargo.toml into five makes it easy to leave stale deps behind mid-migration                                                                                                                                                       |

**Not decided** which of these get adopted, or when. `cargo-hack` is the only one with an ADR 014
dependency (feature matrix correctness at split time); the rest are purely publish-adjacent hygiene
and can wait.

## Non-goals (for now)

- Actually publishing any crate to crates.io.
- Configuring or running any of the tools listed above.
- Picking a release cadence or versioning policy beyond what ADR 014 already states (all crates
  start at `0.2.0` together, then version independently).

## References

- [ADR 014: Workspace Split](014-workspace-split.md) -- the split this release process will
  eventually publish
- [ADR 013: Codecov](013-codecov.md) -- flag-per-crate coverage model, same "one config per crate"
  shape as the tooling question here
- [release-plz](https://github.com/release-plz/release-plz)
- [cargo-release](https://github.com/crate-ci/cargo-release)
- [git-cliff](https://git-cliff.org)
- [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks)
- [cargo-hack](https://github.com/taiki-e/cargo-hack)
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
- [cargo-udeps](https://github.com/est31/cargo-udeps)
