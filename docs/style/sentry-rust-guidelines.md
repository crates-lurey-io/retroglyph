# Sentry Rust Development Guidelines

> Source:
> [develop.sentry.dev/engineering-practices/rust/](https://develop.sentry.dev/engineering-practices/rust/)
> Retrieved: 2026-06-15

## 1. Iterator Design Patterns

### Explicit Iterator Types

Prefer **explicit, named iterator types** over `impl Iterator` in stable, public interfaces of
published crates. This allows the type to be used in associated types, globals, and other positions
where `impl Trait` is not allowed.

The type name should end with `Iter` per naming convention.

### Additional Trait Implementations

Every custom iterator should also implement:

| Trait                 | When                                           |
| --------------------- | ---------------------------------------------- |
| `FusedIterator`       | Always, unless there is a strong reason not to |
| `DoubleEndedIterator` | If reverse iteration is possible               |
| `ExactSizeIterator`   | If the size is known beforehand                |

### Boxed Iterator Escape Hatch

If writing a custom iterator is exceptionally hard, use a **private newtype** around a boxed
iterator:

```rust
pub struct FooIter(Box<dyn Iterator<Item = Foo>>);

impl Iterator for FooIter {
    type Item = Foo;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}
```

## 2. Async Patterns in Traits

Native async in traits (stabilized in Rust 1.75) is the preferred approach. The returned future must
be `Send` to support multi-threaded runtimes:

```rust
pub trait Database {
    fn get_user(&self) -> impl Future<Output = User> + Send;
}

impl Database for MyDatabase {
    async fn get_user(&self) -> User {
        todo!()
    }
}
```

Fall back to the [`async-trait`](https://docs.rs/async-trait/) crate only when:

- Dynamic dispatch is needed
- You must support Rust versions older than 1.75

## 3. Error Handling Rules

### Never Use `.unwrap()`

Almost always avoid `.unwrap()`. Even if code is currently guaranteed not to panic (because some
precondition holds), it may be refactored or reused in a context where the precondition no longer
applies.

Instead, use `match`, `if let`, `?`, or other fallible patterns and propagate errors through
function signatures.

### Safe Slice Access

Slice syntax (`&foo[a..b]`) panics if indices are out of bounds or out of order. When dealing with
untrusted input, use `.get(a..b)` with `if let Some(...) =`:

```rust
// Bad: panics on invalid input
let data = &foo[a..b];

// Good: handles invalid indices gracefully
if let Some(data) = foo.get(a..b) {
    // ...
}
```

## 4. Arithmetic Safety

Arithmetic under/overflows panic in debug builds and silently wrap in release builds, potentially
causing incorrect results or panics elsewhere (e.g., in subsequent slice operations).

Always use checked arithmetic:

| Method                                   | Behavior                          |
| ---------------------------------------- | --------------------------------- |
| `checked_sub`, `checked_add`, etc.       | Returns `None` on overflow        |
| `saturating_sub`, `saturating_add`, etc. | Clamps to `MIN`/`MAX` of the type |

Prefer `saturating_*` when clamping is acceptable; use `checked_*` when overflow should be an
explicit error.

## 5. Struct Visibility Rules

### Default to Fully Private Fields

All struct fields (including tuple-struct fields) should be private by default.

**Only two exceptions:**1.**Newtypes** (1-tuple structs) where direct access to the inner type is desired, to annotate
   semantics of a type where accessing the inner type is required.

1. **Plain data types with very stable signatures**, such as schema definitions (e.g., Sentry

   `Event` protocol).

### No Mixed Visibility

Mixed visibility is **never allowed**, not even between `private` and `pub(crate)` or `pub(super)`.
All fields in a struct must have the same visibility level.

Instead, provide accessors:

| Pattern                                        | Use                                    |
| ---------------------------------------------- | -------------------------------------- |
| `foo()`, `foo_mut()`, `set_foo()`              | Standard getters/setters               |
| `pub(crate)` / `pub(super)` accessor functions | Corner cases for crate-internal access |
| `Default` implementation + builder             | Incremental construction               |

### Naming Conventions for Accessors

Sentry follows Rust naming conventions strictly:

- Getters: `foo()` (not `get_foo()`)
- Setters: `set_foo()`
- Mutable access: `foo_mut()`
- Conversions: `as_foo()` (cheap ref), `to_foo()` (expensive copy), `into_foo()` (ownership

  transfer)

## 6. Import Ordering and File Component Ordering

### Import Order

Imports must be declared before any other code, preceded only by module doc comments and module
attributes (`#![...]`). Group imports by origin, separated by empty lines:

1. **Rust standard library** (`std`, `core`, `alloc`)
2. **External & workspace dependencies** (including `pub use`)
3. **Crate modules** (`self`, `super`, `crate`)

Equivalent `rustfmt` configuration (requires nightly):

```toml
imports_granularity = "Module"
group_imports = "StdExternalCrate"  # nightly only
```

### Example

```rust
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{self, Seek, Write};

use fnv::{FnvHashMap, FnvHashSet};
use num::FromPrimitive;
use symbolic_common::{Arch, DebugId, Language};
use symbolic_debuginfo::{DebugSession, Function, LineInfo};

use crate::error::{SymCacheError, SymCacheErrorKind, ValueKind};
use crate::format;

pub use gimli::RunTimeEndian as Endian;
```

### File Component Order

Within a file, components should appear in this order:

1. Module-level documentation
2. Imports
3. Public re-exports
4. Modules and public modules
5. Constants
6. Error types
7. All other functions and structs
8. Unit tests

Place more significant items first. For example, a type and its `fn iter()` method come before the
corresponding iterator struct definition.

## 7. `impl` Block Ordering

### Block Order Around a Struct/Enum

Keep struct/enum and its `impl` blocks consecutive, in this order:

1. `struct` or `enum` definition
2. Inherent `impl` block
3. `impl` block with further constraints (generic bounds)
4. Trait implementations of `std` traits
5. Trait implementations of other (external) traits
6. Trait implementations of own (crate-local) traits

### Inside an `impl` Block

Order items within an `impl` block as follows (public before private):

1. Associated constants
2. Associated non-instance functions (no `self`)
3. Constructors
4. Getters / setters
5. Everything else

### Constructor Convention

Structs generally have `fn new(...) -> Self`, which **never returns `Result`**. If construction can
fail, use a different name (e.g., `from_parts`, `try_new`, `parse`).

## 8. Test Naming Conventions

### Function Naming

Test function names should contain: **function name + simple condition**. Keep names concise; avoid
filler words like "should" or "that".

```text
tests::parse_empty
tests::parse_null
```text

### Unit Test Placement

Place unit tests in a `tests` submodule with `#[cfg(test)]`:

```rust
fn foo() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foo_works() { .. }
}
```

This ensures imports and helper functions for tests are only compiled when testing.

### Integration Tests

Integration tests go into the `tests/` folder, organized by functionality. They should use only the
public interface. See
[Matklad's guidance on structuring integration tests](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html#Rules-of-Thumb).

For libraries, provide examples in `examples/`.

## 9. Documentation Standards

### Doc Comment Conventions

Follow RFC 505 and RFC 1574. Key rules:

- **Single-line short summary**: written in American English, third-person voice ("Returns the user"

  not "Return the user")

- **Default headers** where applicable; avoid custom sections outside module-level docs
- **Cross-link** between types and methods where possible, especially within the crate
- **Write doc tests**: for crate-public utilities and SDKs, at least one doctest for the critical

  path; doctests take precedence over equivalent unit tests since they both test and document the
  API

### Formatting Doc Comments

Optionally configure rustfmt to format code in doc comments:

```toml
format_code_in_doc_comments = true
```

### Crate-Level Lints

Every crate must enable these warnings in its top-level file (after the doc block, before imports):

```rust
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
```

`unsafe_code` is intentionally not denied, since `unsafe` blocks already stand out in code review
and there are legitimate uses.

## 10. Snapshot Testing with Insta

Sentry uses the [`insta`](https://github.com/mitsuhiko/insta) snapshot testing library. Key
commands:

```bash
# Install/upgrade cargo-insta

cargo install cargo-insta

# Review snapshot diffs interactively

cargo insta review --all

# Run all tests, skipping failures (for batch review)

cargo insta test --review

# Reject all pending diffs

cargo insta reject --all
```

By default, when running tests with `cargo test`, insta compares snapshots and pretty-prints diffs
to stdout. `cargo-insta` provides the interactive review workflow.

## 11. Semver Conventions

### Post-1.0 (Standard Semver)

| Change type      | Version bump |
| ---------------- | ------------ |
| Breaking changes | Major (X)    |
| New features     | Minor (Y)    |
| Bugfixes only    | Patch (Z)    |

### Pre-1.0 (Adapted for Cargo)

Before 1.0, the semver spec says "anything goes," but Cargo's caret requirements (`^0.Y.Z`) enforce
strict rules. Sentry adopts:

| Change type      | Version bump |
| ---------------- | ------------ |
| Breaking changes | Minor (Y)    |
| New features     | Patch (Z)    |
| Bugfixes only    | Patch (Z)    |

This ensures `cargo update` behaves predictably for downstream consumers.

## 12. Additional Engineering Practices

### Linting

Use `clippy` with the `clippy::all` preset. Invocation in CI:

```bash
cargo +stable clippy --all-features --all --tests --examples -- -D clippy::all
```

### Formatting

Use stable `rustfmt`:

```bash
cargo +stable fmt --all -- --check  # CI check
cargo +stable fmt --all             # Auto-format
```

### Rust Analyzer Settings

Recommended VS Code / rust-analyzer configuration:

```json
{
  "rust-analyzer.cargo.features": "all",
  "editor.inlayHints.enabled": "offUnlessPressed",
  "rust-analyzer.imports.granularity.group": "module",
  "rust-analyzer.imports.prefix": "crate"
}
```

### Makefile Convention

Sentry Rust projects use Makefiles to collect standard actions:

| Target              | Action                                                                           |
| ------------------- | -------------------------------------------------------------------------------- |
| `make check`        | `style` + `lint`                                                                 |
| `make test`         | `test-default` + `test-all`                                                      |
| `make test-default` | `cargo test --all`                                                               |
| `make test-all`     | `cargo test --all --all-features`                                                |
| `make style`        | `cargo +stable fmt --all -- --check`                                             |
| `make lint`         | `cargo +stable clippy --all-features --all --tests --examples -- -D clippy::all` |
| `make format`       | `cargo +stable fmt --all`                                                        |
| `make doc`          | `cargo doc --workspace --all-features --no-deps`                                 |

### Naming Conventions

Sentry strictly follows
[Rust Naming Conventions](https://doc.rust-lang.org/1.0.0/style/style/naming/README.html):

- **Avoid redundant prefixes**: don't repeat the module name in item names
- **Getters**: `foo()` not `get_foo()`
- **Conversions**: `as_foo` (cheap borrow), `to_foo` (expensive), `into_foo` (ownership)
- **Iterator methods**: `iter()`, `iter_mut()`, `into_iter()`
- **Constructors**: `new()` returns `Self` (never `Result`)

### Recommended Resources

Sentry points developers to these references:

- [A half-hour to learn Rust](https://fasterthanli.me/articles/a-half-hour-to-learn-rust) (syntax

  intro)

- [The Rust Programming Language](https://doc.rust-lang.org/book/) (comprehensive)
- [The Async Book](https://rust-lang.github.io/async-book/) (async/await)
- [The Little Book of Rust Macros](https://danielkeep.github.io/tlborm/book/index.html) (macros)
- [Rust Nomicon](https://doc.rust-lang.org/nomicon/) (unsafe)
- [Rust for Rustaceans](https://rust-for-rustaceans.com/) (idiomatic patterns)
- [Rust Atomics and Locks](https://marabos.nl/atomics/) (concurrency)
