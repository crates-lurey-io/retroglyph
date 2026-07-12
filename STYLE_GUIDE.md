# Style Guide

This file states retroglyph's own rules and decisions. It does not re-explain idiomatic Rust; for
that, see [External resources](#external-resources).

## Enforced automatically

Source of truth is `Cargo.toml`, not this document. If this file and `Cargo.toml` ever disagree,
`Cargo.toml` wins and this file is out of date.

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
unreachable_pub = "warn"
unused_qualifications = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
nursery = { level = "deny", priority = -1 }
must_use_candidate = "deny"
missing_errors_doc = "deny"
missing_panics_doc = "deny"
module_name_repetitions = "allow"
```

In practice `missing_docs` is enforced too: `just clippy` runs with `-D warnings`, which promotes
every warn-level lint to a hard failure. `just check` (fmt-check, clippy, compile, test, doc,
llms-check) is the actual gate; see `AGENTS.md`'s Correctness gate section for the exact commands.

No `rustfmt.toml` exists, on purpose: the project uses 100% rustfmt defaults (4-space indent, 100
col width, block indent, trailing commas on multiline lists). Don't add one to "fix" formatting
without a conscious decision to change the defaults.

## Comments and doc comments

- Comment and doc-comment prose wraps at ~100 cols, not ~80. `wrap_comments` is `false` by default,
  so rustfmt never rewraps prose for you; hand-wrapping to ~80 out of habit just leaves ragged
  lines.
- Comment the why, not the what. No banners, section dividers, or decorative comment blocks.
- Omit doc comments on functions where the name and signature already say it all. Every public item
  still needs _something_ because of `missing_docs`, but that something can be one line.
- Every fallible public function needs a `# Errors` section (clippy `missing_errors_doc`); every
  panicking public function needs a `# Panics` section (clippy `missing_panics_doc`).
- Internal-only docs (design notes, roadmap, personal `.matan/` scratch) are never cited by path,
  number, or title from doc comments, rustdoc, or anything else that ends up in published API docs.
  Those readers have no access to the file and the reference is dead weight. If a doc comment needs
  the rationale, restate the relevant part of it inline instead of pointing at an internal doc.

## Error handling

Retroglyph forbids `unsafe_code` and stays `no_std`-capable at the core, which shapes the error
story: hand-rolled error enums, not a derive-macro crate.

- **No `thiserror`, no `anyhow`, anywhere in the workspace.** This isn't an oversight; it's the
  established and going-forward convention. Public error types are hand-written `enum`s:
  `#[derive(Debug)]`, a manual `impl fmt::Display` (lowercase message, no trailing punctuation,
  matching the existing `SoftwareBackendError`/`SurfaceError`/`TilesetError` types in
  `retroglyph-software`), and a manual `impl std::error::Error`, chaining `source()` to an inner
  error when the variant wraps one. `anyhow` remains fine in non-published code
  (`retroglyph-examples` and `tools/cargo-bin`) where there's no API contract to keep stable, but is
  rejected for any published crate. `core::error::Error` exists at our MSRV (1.88, edition 2024),
  but the std-only crates still implement `std::error::Error` rather than switching, for maximum
  compatibility with downstream error-handling crates that expect the `std` trait.
- **New public error types should be `#[non_exhaustive]`.** The existing error enums predate this as
  an explicit rule and aren't marked that way yet; new ones should be, so adding a variant later
  isn't a breaking change. (This is a forward-looking addition, not a description of code that
  already exists everywhere -- flagging it as such rather than implying it's already universal.)
- **No `eprintln!` in library code.** Use the `log` crate, feature-gated. Fatal backend
  initialization errors: `log::error!` + `event_loop.exit()`, not `panic!`. `log`'s scope today is
  narrow and should stay that way: backend init/runtime failure reporting (`retroglyph-software`,
  `retroglyph-window`), not pervasive logging throughout normal operation.
- **No `.unwrap()`/`.expect()` in library code outside `#[cfg(test)]` modules**, except for a
  genuinely unreachable invariant with a clear `expect()` message explaining why the case cannot
  occur (for example, indexing into a buffer with a just-validated index). Every current use of
  either across the workspace is inside a test module; that's the bar to hold, not just a
  description of the current state.
- Most of the workspace has no fallible public API at all today: error enums exist only in
  `retroglyph-software` (font/surface/tileset loading), because that's the only crate with
  operations that can actually fail (missing font, surface creation, PNG decode). `core`,
  `terminal`, `crossterm`, `terminal-wasm`, and `window` have none. Don't introduce a
  `Result`-returning API "for consistency" if nothing in it can actually fail; an infallible
  constructor should stay infallible.

## Testing

See `docs/testing.md` for the full testing architecture (unit tests, insta snapshots, the examples
crate's three-way snapshot harness). Commands: `cargo insta test` / `cargo insta accept` for
reviewing snapshots, `cargo test --workspace --all-features` (or `just test`) for everything.

## External resources

Curated, not summarized. Each entry is a link plus one line on when it's actually worth opening;
none of these are re-explained here.

### Canonical / official

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) -- the closest thing to a
  formal spec for public API shape (naming, the C-\* checklist). Read before designing a new public
  type.
- [Official Rust Style Guide](https://doc.rust-lang.org/style-guide/) -- what rustfmt enforces by
  default. Read only if you're confused about why rustfmt formatted something a certain way.
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/) -- idioms/anti-patterns
  catalogue. Good for "is there a name for this pattern" lookups.
- [The Rust Performance Book](https://nnethercote.github.io/perf-book/) -- read before a perf pass,
  not everyday reading.

### Books / courses

- [Effective Rust](https://effective-rust.com/) -- item-based format, easy to reference by number in
  review comments.
- [Google Comprehensive Rust](https://google.github.io/comprehensive-rust/) -- teaching material,
  more useful for onboarding someone new to Rust than to this project specifically.
- [Ferrous Systems: Elements of Rust](https://github.com/ferrous-systems/elements-of-rust) --
  practical de-nesting/clarity techniques; a good code-review reference for "how do I simplify this
  match."
- [Microsoft Rust Patterns book](https://microsoft.github.io/RustTraining/rust-patterns-book/) --
  intermediate-to-advanced design pattern deep dives (type-state, newtype, macros).

### Company/team guidelines

- [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/) -- large,
  versioned guideline set with rationale per rule; useful when justifying adopting or rejecting a
  specific "must"/"should."
- [Embark Studios guidelines](https://github.com/EmbarkStudios/rust-ecosystem/blob/main/guidelines.md)
  -- minimal, short enough to skim.
- [Apollo GraphQL Rust best practices](https://github.com/apollographql/rust-best-practices) --
  strong opinions on error hierarchies; useful reading given retroglyph's own error-handling stance
  above.
- [Sentry Rust guidelines](https://develop.sentry.dev/engineering-practices/rust/) -- iterator
  design (`FusedIterator`/`DoubleEndedIterator`/`ExactSizeIterator` conventions), relevant to
  retroglyph's own grid-iteration types.
- [Linux kernel Rust coding guidelines](https://docs.kernel.org/rust/coding-guidelines.html) --
  extreme end of the spectrum (near-total panic prohibition, mandatory SAFETY comments). Useful
  contrast reading, not a rulebook this project follows, since it forbids `unsafe` entirely.

## Maintenance

This file should stay short. If a rule needs more than a paragraph and an example, either it belongs
in a crate's own module docs, or it's a sign the rule needs a `Cargo.toml` lint instead of prose.
