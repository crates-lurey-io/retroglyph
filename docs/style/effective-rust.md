# Effective Rust - Comprehensive Summary

> **35 Specific Ways to Improve Your Rust Code** by David Drysdale Source:
> [lurklurk.org/effective-rust](https://www.lurklurk.org/effective-rust/) |
> [effective-rust.com](https://www.effective-rust.com)

Modeled after _Effective C++_ and _Effective Java_. Intended as a second book for Rust programmers
who already know the basics. Covers the 2018 edition (compatible with 2021), stable toolchain,
synchronous Rust only (no async).

---

## Chapter 1: Types

### Item 1: Use the type system to express your data structures

**Core advice:** Make invalid states inexpressible in your types.

- Rust's `enum` with data fields is an algebraic data type (ADT), far more powerful than enums in
  C/Java/Go. Use it to encode state machines and invariants directly.
- Replace `bool` parameters with enums for clarity. `print_page(Sides::Both, Output::BlackAndWhite)`
  is self-documenting; `print_page(true, false)` is not. The compiler catches argument order
  mistakes with enums.
- A dead giveaway for poor type design: a comment like
  `// fg_color must be (0,0,0) if monochrome is true`. Replace with an enum: `Color::Monochrome` vs
  `Color::Foreground(RgbColor)`.
- Always use `Option<T>` for values that can be absent; never use sentinel values like -1 or
  nullptr.
- Always use `Result<T, E>` for fallible operations. It enables `?` and standard transforms.
- Example: `SchedulerState` enum with `Inert`, `Pending(HashSet<Job>)`,
  `Running(HashMap<CpuId, Vec<Job>>)` tells the whole story from the type definition alone.

### Item 2: Use the type system to express common behavior

**Core advice:** Encode behavior as traits; prefer trait bounds over concrete types.

- Methods on `self` signal intent: `&self` = read-only, `&mut self` = may modify, `self` = consumes.
- Closures: the compiler auto-implements `Fn`, `FnMut`, or `FnOnce` depending on what the closure
  captures. You cannot manually implement these traits.
- Prefer the most general `Fn*` trait that works: accept `FnOnce` for closures used once. Prefer
  `Fn*` trait bounds over bare `fn` pointers.
- Use marker traits for behaviors that can't be expressed in method signatures (e.g., a
  `StableSort: Sort` marker).
- Trait objects (`dyn Trait`) use dynamic dispatch via vtables. Not all traits are object-safe: no
  generic methods, no `Self` in argument/return types (except receiver).
- Trait bounds use static dispatch (monomorphization). The compiler checks them explicitly, unlike
  C++ templates which use implicit "duck typing".

### Item 3: Prefer `Option` and `Result` transforms over explicit `match`

**Core advice:** Use transformation methods instead of matching; use `?` to propagate errors.

- `if let Some(i) = &s.field` is cleaner than matching with an empty `None => {}` arm.
- `unwrap()` and `expect()` are deliberate choices to treat failure as fatal. Use them consciously.
- The `?` operator is syntactic sugar for matching `Err`, converting the error type via `From`, and
  returning. It keeps code focused on the happy path.
- Key transforms: `map`, `and_then`, `or_else`, `unwrap_or_default`, `ok_or`, `map_err`.
- Use `as_ref()` to convert `&Option<T>` into `Option<&T>` when you need to work with references
  without moving.
- Prefer `Result` over `Option` for errors, even when converting between them, because `Result`
  communicates failure details.

### Item 4: Prefer idiomatic `Error` types

**Core advice:** Implement `std::error::Error` for your error types; use `thiserror` for libraries
and `anyhow` for applications.

- `Error` requires `Display` + `Debug`. The `source()` method exposes nested errors.
- For simple errors, use a newtype around `String` with `Error` implemented.
- For nested errors, use an enum with variants for each sub-error type. Implement `From` for each
  sub-error so `?` auto-converts.
- `thiserror` crate: generates enum-based error types with derive macros. Doesn't leak `thiserror`
  into your public API.
- `anyhow` crate: type-erased error handling for applications. Uses `Box<dyn Error>` internally.
- Library vs application split: libraries should emit concrete error types (enum-style);
  applications should use dynamic error types (anyhow-style).

### Item 5: Understand type conversions

**Core advice:** Implement `From` for conversions; use `Into` for trait bounds; prefer `from`/`into`
over `as` casts.

- Four conversion traits: `From<T>`, `TryFrom<T>`, `Into<T>`, `TryInto<T>`. The `Try` variants
  return `Result`.
- Implement `From`, not `Into`. The standard library provides a blanket `Into` impl for anything
  with `From`. This also gives you `TryFrom` for free (with `Infallible` error type).
- Use `Into<T>` as trait bounds (accepts both direct `Into` impls and `From`-derived ones).
- The reflexive `impl<T> From<T> for T` means generic functions accepting `Into<MyType>` also accept
  `MyType` directly.
- `as` casts allow lossy conversions silently (e.g., `u32` to `u16`). Prefer `from`/`into` for
  safety. Clippy has lints for this.
- Coercions are implicit: `&mut T` to `&T`, closures to `fn` pointers, arrays to slices, concrete
  types to trait objects.

### Item 6: Embrace the newtype pattern

**Core advice:** Wrap types in single-field tuple structs to add semantic meaning and bypass the
orphan rule.

- Classic example: `PoundForceSeconds(f64)` vs `NewtonSeconds(f64)` prevents unit confusion (the
  Mars Climate Orbiter problem). Type aliases (`type PoundForceSeconds = f64`) do NOT provide this
  safety.
- The newtype pattern lets you implement foreign traits on foreign types, circumventing the orphan
  rule.
- `#[repr(transparent)]` ensures the newtype has identical memory layout to the inner type.
- Downside: trait implementations from the inner type are lost. You need `#[derive(...)]` for
  derivable traits and forwarding boilerplate for others (e.g., `Display`).

### Item 7: Use builders for complex types

**Core advice:** When structs have many fields (especially optional ones), provide a builder type.

- Rust requires all struct fields to be initialized. Without `Default`, this means verbose
  construction.
- Builder pattern: a separate struct that accumulates configuration, then emits the final object via
  `build()`.
- Consuming builder (methods take `self`, return `Self`): supports chaining but each value can only
  build one item.
- Mutable builder (methods take `&mut self`, return `&mut Self`): allows separated build stages and
  building multiple items, but chaining from `new()` is awkward.
- Builder methods can be smarter than setters: e.g., `just_seen()` sets `last_seen` to `now()`.
- Consider the `derive_builder` crate to generate builder boilerplate automatically.

### Item 8: Familiarize yourself with reference and pointer types

**Core advice:** Understand the pointer trait hierarchy and when to use each smart pointer type.

- Rust references (`&T`, `&mut T`) are never null, always valid, always aligned, and
  lifetime-checked.
- `Box<T>`: heap allocation, single owner. Implements `Deref`.
- `Deref` / `DerefMut`: enable smart pointer coercion. Only one `Target` type (associated type, not
  generic).
- `AsRef<T>` / `AsMut<T>`: explicit conversion to reference. Can have multiple target types (generic
  parameter).
- Fat pointers (16 bytes on 64-bit): slices (`&[T]` = pointer + length) and trait objects
  (`&dyn Trait` = pointer + vtable pointer).
- `Rc<T>`: shared ownership via reference counting. Not thread-safe. Combine with `RefCell<T>` for
  interior mutability.
- `Arc<T>`: thread-safe `Rc`. Combine with `Mutex<T>` for thread-safe interior mutability.
- `Cow<T>`: clone-on-write; stays borrowed until mutation is needed.
- Key guideline: `Rc<RefCell<Vec<T>>>` = shared, mutable vector. `Rc<Vec<RefCell<T>>>` = shared
  vector with individually mutable elements.

### Item 9: Consider using iterator transforms instead of explicit loops

**Core advice:** Express loops as chains of iterator transforms for clarity, concision, and
sometimes performance.

- Step-by-step transformation: explicit index loop → for-each with `.iter()` → add `.filter()` → add
  `.take()` → add `.map()` → replace loop body with `.sum()`.
- Source iterators: `.iter()` (borrows), `.into_iter()` (consumes), `.iter_mut()` (mutable borrows).
- Flow transforms: `take`, `skip`, `step_by`, `chain`, `cycle`, `rev`.
- Item transforms: `map`, `cloned`, `copied`, `enumerate`, `zip`.
- Filter transforms: `filter`, `take_while`, `skip_while`, `flatten`.
- Consumers: `sum`, `product`, `min`, `max`, `reduce`, `fold`, `find`, `any`, `all`, `collect`.
- `collect()` can build `Result<Vec<T>, E>` from `Iterator<Item=Result<T,E>>` with turbofish:
  `.collect::<Result<Vec<_>, _>>()?`. This short-circuits on the first error.
- Iterator transforms can be more efficient than explicit loops because they avoid bounds checks.
- When NOT to use: large/multifunctional loop bodies, complex error handling, or when the conversion
  feels forced.

---

## Chapter 2: Traits

### Item 10: Familiarize yourself with standard traits

**Core advice:** Know and derive the common standard traits. They encode fundamental type behaviors.

- **`Clone`**: Explicit copy via `.clone()`. Derive unless the type embodies unique access (RAII
  types, crypto keys) or contains un-Cloneable fields (`&mut T`, `MutexGuard`).
- **`Copy`**: Marker trait; bitwise copy is valid. Changes assignment from move to copy semantics.
  Don't implement if large (implicit copies become slow) or if `Clone` needed a manual impl.
- **`Default`**: Default constructor. Works with struct update syntax:
  `Color { red: 128, ..Default::default() }`.
- **`PartialEq` / `Eq`**: Equality. `Eq` adds reflexivity (`x == x`). The split exists because
  `NaN != NaN`. Implement `Eq` whenever you implement `PartialEq` unless you have float-like
  semantics. Required for `HashMap` keys.
- **`PartialOrd` / `Ord`**: Ordering. Be wary of implementing only `PartialOrd`; the lack of total
  ordering leads to surprising behavior.
- **`Hash`**: Must be consistent with `Eq`: if `x == y` then `hash(x) == hash(y)`. If you have a
  manual `Eq`, check if you need a manual `Hash`.
- **`Debug`**: For programmers (`{:?}`). Always derive unless content is sensitive.
- **`Display`**: For users (`{}`). Cannot be derived; must be manually implemented.
- Operator overload traits (`Add`, `Sub`, `Mul`, etc.): Implement a coherent set. Avoid overloading
  for unrelated types (C++ lesson).

### Item 11: Implement the `Drop` trait for RAII patterns

**Core advice:** Use `Drop` to tie resource lifetimes to value lifetimes.

- RAII: resource acquisition in constructor, release in destructor. The compiler guarantees
  destructors run at scope exit.
- `MutexGuard` is the canonical Rust RAII example: lock acquired on creation, released on drop.
- Use blocks `{ ... }` to restrict RAII scope and release resources promptly.
- `drop()` cannot be called directly; use `std::mem::drop(item)` which takes ownership and drops
  immediately.
- `Drop::drop(&mut self)` has no return type, so it can't signal failure. Provide a separate
  `release() -> Result` if cleanup can fail.
- Implement `Drop` for: OS resources (file descriptors), sync primitives (locks), raw memory (FFI).

### Item 12: Understand the trade-offs between generics and trait objects

**Core advice:** Prefer generics (static dispatch) by default; use trait objects (dynamic dispatch)
when you need heterogeneous collections or type erasure.

- Generics: monomorphized at compile time. Pros: faster (no vtable indirection), supports multiple
  trait bounds, smaller runtime overhead. Cons: larger code size, longer compile times.
- Trait objects: dynamic dispatch via vtable. Pros: smaller code, supports heterogeneous
  collections, works with dynamically loaded code. Cons: slight runtime overhead, limited to
  object-safe traits.
- Generics can conditionally add methods based on multiple trait bounds. Trait objects encode only a
  single trait's vtable.
- Object safety rules: no generic methods, no `Self` in return types (unless bounded by `Sized`).
- `impl Trait` in argument position is syntactic sugar for a generic with a trait bound.
- Trait bounds (`Shape: Draw`) mean "also-implements", not "is-a". The vtable for `Shape` includes
  `Draw`'s methods but you can't upcast `dyn Shape` to `dyn Draw` (until the trait upcasting feature
  stabilizes).

### Item 13: Use default implementations to minimize required trait methods

**Core advice:** Provide defaults for methods that can be built from primitives; keep the required
method set minimal.

- Tension: implementors want few required methods; users want a rich API. Default implementations
  resolve this.
- Example: `Iterator` has one required method (`next()`) but 50+ default-implemented methods.
- Default methods can have trait bounds that restrict when they're available (e.g., `cloned()`
  requires `Item: Clone`).
- Adding new methods with default implementations is usually backward-compatible, unless the name
  clashes with an existing method from another trait.

---

## Chapter 3: Concepts

### Item 14: Understand lifetimes

**Core advice:** Lifetimes prevent dangling references. Named lifetimes (`'a`) indicate
relationships between input and output references.

- Every reference has a lifetime. The compiler tracks these to ensure references never outlive their
  referents.
- Named items (local variables, parameters) live until moved or scope exit. Unnamed temporaries live
  until the end of the expression.
- Lifetime elision rules: (1) single input ref → output gets same lifetime, (2) multiple inputs
  without `&self` → each gets distinct lifetime, (3) `&self` method → output gets `self`'s lifetime.
- `'static`: the reference is valid for the entire program. Sources: `static` variables, `const`
  values (promoted), `Box::leak` (permanent memory leak).
- Heap lifetimes are tied to their owning stack variable. The chain of ownership always ends at
  either a local variable or a `static`.
- Data structures containing references need lifetime parameters, which propagate to all containers.
  This is infectious and awkward. Prefer data structures that own their contents.
- Anonymous lifetime `'_` marks an elided lifetime as present without naming it. Useful for
  readability.

### Item 15: Understand the borrow checker

**Core advice:** Design data structures with the borrow checker in mind. Use smart pointers for
interconnected structures.

- Borrow rules: (1) reference scope must be within the referent's lifetime, (2) either multiple `&T`
  OR one `&mut T`, never both.
- Non-lexical lifetimes (NLL): reference lifetimes end at last use, not at scope exit.
- Owner operations: updating via owner creates an ephemeral `&mut`, which conflicts with existing
  references.
- `std::mem::replace` performs swap-in-place safely. `Option::replace` is a convenience wrapper.
- Tactics for borrow checker fights: read compiler errors carefully; add/remove `{ }` blocks to
  adjust lifetimes; introduce named local variables to extend lifetimes; temporarily split complex
  chains into typed locals.
- For interconnected data structures: clone data (simple but doesn't handle mutation), use indices
  as pseudo-pointers (fragile), or use `Rc<RefCell<T>>` (most robust).
- Self-referential structs are fundamentally problematic because moves invalidate internal pointers.
  Use index-based approaches or `Pin` or crates like `ouroboros`.

### Item 16: Avoid writing `unsafe` code

**Core advice:** Don't write `unsafe`; use existing safe abstractions from std and crates.

- The standard library uses ~4500 `unsafe` blocks internally to provide safe APIs (`Rc`, `RefCell`,
  `Arc`, `Mutex`, `Pin`, atomics, `mem::replace`).
- Crate ecosystem covers common needs: `once_cell`, `rand`, `byteorder`, `cxx`.
- When forced to write `unsafe`: add safety comments, minimize `unsafe` block scope, write extensive
  tests, run Miri, think about threading.
- Consider enabling `unsafe_op_in_unsafe_fn` lint so even `unsafe fn` bodies require explicit
  `unsafe` blocks.
- Wrap `unsafe` FFI code in a safe API layer to localize risk.

### Item 17: Don't panic (about shared-state concurrency)

**Core advice:** Rust prevents data races but not deadlocks. Prefer message passing; minimize shared
state.

- Data races: Rust's borrow checker prevents them entirely (in safe code). Single writer XOR
  multiple readers is enforced at compile time.
- `Mutex<T>` wraps the protected data (unlike C++ where mutex is separate). `MutexGuard` provides
  RAII locking and acts as a proxy via `Deref`/`DerefMut`.
- Methods on `Mutex`-protected data use `&self` (not `&mut self`) because multiple threads hold
  references. Interior mutability via `Mutex`.
- `Arc<Mutex<T>>` is the standard pattern for shared mutable state across threads.
- `Send`: safe to transfer between threads. `Sync`: safe to reference from multiple threads. Both
  are auto-traits derived from constituent types.
- `Rc` is `!Send` (use `Arc`). `Cell`/`RefCell` are `!Sync` (use `Mutex`/`RwLock`).
- Deadlocks: still possible. Caused by lock inversion (A then B vs B then A). Solutions: single lock
  covering related data, minimize lock scope, avoid closures under locks, don't return `MutexGuard`
  to callers.
- Prefer channels (`std::sync::mpsc`) over shared state when possible.

### Item 18: Don't panic

**Core advice:** Return `Result` instead of panicking. Reserve `panic!` for truly unrecoverable
situations.

- `catch_unwind` is NOT a substitute for exception handling. Panics can abort (via compiler option),
  and panic-based recovery violates exception safety.
- Panics in disguise: `unwrap()`, `expect()`, `unreachable!()`, out-of-bounds indexing, division by
  zero.
- It's OK to panic in `main` (no caller to pass errors to) or when an internal invariant is violated
  (corrupted data, not invalid input).
- For public APIs: provide infallible/fallible pairs (e.g., `from_utf8_unchecked` / `from_utf8`).
  Document panic conditions in `# Panics` doc sections.
- Enforce no-panic in CI: grep for panicking methods, use the `no_panic` crate.
- Use `catch_unwind` only at FFI boundaries to prevent panics from crossing into non-Rust code.

### Item 19: Avoid reflection

**Core advice:** Rust has minimal reflection. Use traits and macros instead.

- `std::any::type_name<T>()`: returns a string name, but it's generic (compile-time, not runtime).
  Only knows about the trait object type, not the concrete underlying type.
- `TypeId`: unique identifier per type, but only for `'static` types.
- `Any` trait: provides `is::<T>()`, `downcast_ref::<T>()`, `downcast_mut::<T>()`. Requires
  explicitly constructing `&dyn Any`.
- There is no way to discover what traits a type implements at runtime, or to get a vtable for a
  different trait from a trait object.
- Trait upcasting (converting `dyn Shape` to `dyn Draw` when `Shape: Draw`) is being stabilized in
  recent Rust versions.
- Alternatives to reflection: define traits for needed behavior, use marker traits, use derive
  macros to generate code at compile time.

### Item 20: Avoid the temptation to over-optimize

**Core advice:** Optimize only when measured performance is genuinely a concern. Prefer usability.

- Data structures that own their contents are easier to use than those with borrowed references. The
  TLV example: `value: &'a [u8]` (zero-copy but lifetime-infectious) vs `value: Vec<u8>` (copies
  data but no lifetime constraints).
- Rust makes copies explicit (`.clone()`, `.to_vec()`, `Box::new()`). Visibility is not a reason to
  optimize away.
- Don't implement `Copy` if the type is large (implicit copies become slow). Do implement it if
  bitwise copy is valid and fast.
- Use `Rc<RefCell<T>>` and `Arc<Mutex<T>>` freely. They lead to simpler, more maintainable designs
  than complex lifetime webs.
- Quote from Josh Triplett: "I call .clone() when I need to, and use Arc to get local objects into
  threads more smoothly. And it feels glorious."

---

## Chapter 4: Dependencies

### Item 21: Understand what semantic versioning promises

**Core advice:** Semver is a blunt instrument. Understand its limits. Don't fear version 1.0.

- MAJOR = breaking changes, MINOR = backward-compatible additions, PATCH = backward-compatible
  fixes.
- For pre-1.0 crates, Cargo treats the first non-zero component as the major version (0.2.x to 0.3.x
  can break).
- Subtle breaking changes: adding enum variants (unless `non_exhaustive`), adding public struct
  fields, breaking object safety, adding blanket trait impls, changing license, changing default
  features, increasing MSRV.
- Fewer public items = fewer things that can break.
- When making a breaking change: (1) release a minor version with new API + `deprecated` old API,
  (2) release a major version removing the deprecated parts.
- Make breaking changes breaking: if behavior changes incompatibly, force a type change too, don't
  silently reuse the old API.
- Don't stay at 0.x forever; it reduces semver expressivity from three categories to two.

### Item 22: Minimize visibility

**Core advice:** Default to private. Make things `pub` only when necessary.

- Visibility markers: `pub`, `pub(crate)`, `pub(super)`, `pub(in path)`.
- Making an `enum` or `trait` pub automatically makes its variants/methods pub.
- Making a `struct` pub does NOT make its fields pub.
- Once something is `pub`, making it private is a breaking change. The converse (private → public)
  is only a minor version bump.
- Private internals keep your options open for future refactoring.

### Item 23: Avoid wildcard imports

**Core advice:** Don't `use somecrate::*` for crates you don't control.

- A new trait method or inherent method added in a minor version can clash with your existing method
  names, causing ambiguous method errors.
- Exception: `use super::*` in test modules, `use thing::prelude::*` for curated preludes, internal
  module re-exports.
- If you do wildcard-import, consider pinning the dependency to a precise version.

### Item 24: Re-export dependencies whose types appear in your API

**Core advice:** If your public API uses types from a dependency, `pub use` that dependency.

- Problem: your crate uses `rand` 0.7 in its API, but a user depends on `rand` 0.8. They can't pass
  their `Rng` to your function because it's a different type.
- Solution: `pub use rand;` in your crate, so users can access the exact version you use as
  `your_crate::rand`.
- Think carefully before using another crate's types in your API; a major version bump in the
  dependency forces one in your crate.

### Item 25: Manage your dependency graph

**Core advice:** Understand Cargo's resolution, use tooling, and weigh the cost of each dependency.

- Crate names are a flat namespace on crates.io (shared with feature names). Hyphens in names become
  underscores in code.
- Cargo allows multiple semver-incompatible versions of the same crate but NOT multiple
  semver-compatible versions (e.g., 1.2 and 1.3 can't coexist, but 1.x and 2.x can).
- `Cargo.lock`: commit for binaries (deterministic builds), optionally for libraries (ignored by
  consumers).
- If you check in `Cargo.lock`, set up a process for upgrades (Dependabot, `cargo update`).
- Tools: `cargo tree` (visualize deps), `cargo-deny` (license/security/duplicate checks),
  `cargo-udeps` (unused deps).
- Version specs: `"1.4.23"` (semver-compatible, minimum 1.4.23) is the Goldilocks choice. Avoid
  `"*"` wildcards and exact pins (`"=1.2.3"`).
- Every dependency has a cost: build time, binary size, and the risk of supply chain attacks.

### Item 26: Be wary of feature creep

**Core advice:** Features must be additive. Avoid feature-gating public struct fields or trait
methods.

- Features are Cargo-specific; to `rustc`, `feature` is just another `cfg` option.
- `default` features can be disabled with `default-features = false`.
- Optional dependencies (`rand = { optional = true }`) automatically become features. Feature names
  and crate names share a namespace.
- Feature unification: a crate is built with the union of all features requested across the
  dependency graph. You cannot rely on a feature being off.
- Don't feature-gate public struct fields (construction breaks when features are unified). Don't
  feature-gate trait methods (implementors can't tell if the method exists).
- N independent features = 2^N build combinations. Test them all in CI.
- Convention: `std` or `alloc` feature to enable functionality requiring those libraries;
  `#![cfg_attr(not(feature = "std"), no_std)]`.

---

## Chapter 5: Tooling

### Item 27: Document public interfaces

**Core advice:** Document the why, not the what. Don't repeat what the type signature already says.

- Use backticks for code, cross-reference with ``[`SomeThing`]`` syntax, include `# Examples` with
  compilable code (tested by `cargo test`).
- Document `# Panics` and `# Safety` sections.
- Enable `#![deny(broken_intra_doc_links)]` to catch invalid doc links. Consider
  `#![warn(missing_docs)]`.
- `crates.io` shows README.md; `docs.rs` shows `//!` comments from lib.rs.
- Bad documentation: repeating parameter types and return types in prose, describing how other code
  uses a method (gets out of sync), documenting the what instead of the why.
- Good documentation: describing preconditions, invariants, error conditions, and surprises.

### Item 28: Use macros judiciously

**Core advice:** Use macros to eliminate boilerplate and keep disparate code in sync. Prefer derive
macros.

- Declarative macros (`macro_rules!`): hygienic (can't capture local variables), but watch for
  repeated evaluation of arguments with side effects.
- Prefer macros whose expansion looks like normal Rust. Avoid hidden `return` statements or control
  flow.
- Use `format_args!` for macros that do argument formatting.
- Procedural macros: function-like (rare), attribute (wraps items), derive (most common,
  auto-generates code from struct/enum definitions). Require a separate `proc-macro` crate.
- `syn` crate parses token streams into AST nodes. `cargo-expand` shows expanded macro output.
- The http_codes! example: a single macro invocation defines an enum, group() method, text() method,
  and TryFrom<i32>, all from one source of truth.
- Downsides: reduced readability, tooling may not understand DSL syntax, `rustfmt` doesn't format
  macro bodies, potential code bloat.

### Item 29: Listen to Clippy

**Core advice:** Make your code Clippy-warning free. Read the full lint list for learning.

- Categories: correctness, idiom, concision, performance, readability.
- Use `#[allow(clippy::some_lint)]` to disable specific lints, but prefer fixing over suppressing.
- Enable Clippy in CI with `-Dwarnings` flag.
- Many Items in this book have corresponding Clippy lints (type casts, iterator transforms, trait
  consistency, panic usage, wildcard imports, etc.).
- Read the full [lint list](https://rust-lang.github.io/rust-clippy/stable/index.html) even for
  disabled lints; understanding the rationale improves your Rust.

### Item 30: Write more than unit tests

**Core advice:** Use all test types: unit, integration, doc tests, examples, benchmarks, fuzz tests.

- **Unit tests** (`#[test]` in `#[cfg(test)]` module): test internals, can access private items.
- **Integration tests** (`tests/` directory): test public API only, each file is a separate test
  binary.
- **Doc tests**: code samples in doc comments, run by `cargo test`. Keep them compiling.
- **Examples** (`examples/` directory): standalone programs showing API usage. Build with
  `cargo test --examples`. Use `Result`-returning `main()` instead of `unwrap()`.
- **Benchmarks**: `cargo bench` (nightly). Use `std::hint::black_box` to prevent over-optimization.
  Consider `criterion` crate for stable Rust.
- **Fuzz tests**: `cargo-fuzz` wraps `libFuzzer`. Essential if code handles untrusted input. Run
  continuously, not in CI.
- When fixing a bug, write a test that exhibits it first.
- Test every feature combination and every platform with distinct `cfg` code.

### Item 31: Use tools

**Core advice:** Explore and integrate the rich Rust tool ecosystem.

- Core tools: `cargo`, `rustup`, `rust-analyzer`, Rust playground, std docs.
- Build tools: `cargo fmt`, `cargo check`, `cargo clippy`, `cargo doc`, `cargo bench`,
  `cargo update`, `cargo tree`, `cargo metadata`.
- Extended ecosystem: `cargo-expand` (macro debugging), `cargo-tarpaulin` (coverage), `cargo-deny`,
  `cargo-udeps`, `cargo-fuzz`, `cargo-semver-checks`, Miri, Godbolt.
- Integrate useful tools into your editor/IDE and your CI system.

### Item 32: Set up a continuous integration (CI) system

**Core advice:** Automate everything. Don't waste humans' time.

- Minimum: build + test. Add: Clippy, `cargo doc`, `cargo fmt --check`, dependency audits.
- Use `rust-toolchain.toml` to pin toolchain version for deterministic CI.
- Test all feature combinations (2^N), test `no_std` compatibility, check MSRV.
- CI principles: zero false positives, fix flaky tests immediately, make checks easy to run locally,
  split checks by cadence (per-commit, per-merge, daily, continuous).
- For public projects: restrict CI execution for untrusted contributors, pin external script
  versions, monitor privileged integration steps.
- When you find a process problem, add a CI check to detect it before fixing it.

---

## Chapter 6: Beyond Standard Rust

### Item 33: Consider making library code `no_std` compatible

**Core advice:** Many library crates can be `no_std` with minimal changes. Add a CI check to prevent
regression.

- `core`: always available. Includes `Option`, `Result`, `Iterator`, primitive types. No heap
  allocation.
- `alloc`: requires `extern crate alloc;`. Provides `Box`, `Vec`, `String`, `Rc`, `Arc`, `BTreeMap`,
  `BTreeSet`, `format!`. Notably missing: `HashMap`/`HashSet` (need OS randomness), `Mutex` (needs
  OS primitives).
- For many libraries, making `no_std` work just means: replace `std::` with `core::` / `alloc::`,
  switch `HashMap` to `BTreeMap`, add `use` for prelude items.
- CI: cross-compile for `--target thumbv6m-none-eabi` to verify `no_std` compatibility.
- Use `std` or `alloc` feature names (additive!) to gate functionality requiring those libraries.
  NOT `no_std` features (non-additive).
- Fallible allocation: `try_reserve`, `Box::try_new` (nightly). `no_global_oom_handling` flag
  disables infallible allocation entirely (used by Linux kernel).

### Item 34: Control what crosses FFI boundaries

**Core advice:** Encapsulate `unsafe` FFI in safe wrappers. Allocate and free on the same side of
the boundary.

- `extern "C"` functions are automatically `unsafe` and `#[no_mangle]`.
- Use sized types (`uint32_t` / `u32`) instead of `int` / `c_int` at FFI boundaries.
- Use `#[repr(C)]` for structs shared across FFI.
- Use `CString` / `CStr` for C-compatible strings.
- Name mangling: C symbols are bare names; C++ and Rust mangle by default. `extern "C"` +
  `#[no_mangle]` removes mangling but also removes type safety at link time.
- Lifetime dangers: C code can hold stale pointers, cast away const, ignore Mutex protections, free
  memory Rust owns.
- RAII wrappers: implement `Drop` to free FFI-allocated resources. Check for null pointers.
- `Box::into_raw` + `Box::from_raw`: transfer heap ownership across FFI boundaries.
- Prevent `panic!` from crossing FFI boundaries (undefined behavior).
- For Rust called from C: use `#[no_mangle]` + `extern "C"`, consider name prefixes to avoid global
  namespace collisions.

### Item 35: Prefer `bindgen` to manual FFI mappings

**Core advice:** Auto-generate Rust FFI declarations from C headers with `bindgen`.

- `bindgen` parses C header files and emits corresponding Rust `extern "C"` declarations and
  `#[repr(C)]` struct definitions.
- Eliminates the risk of manual declaration mismatches (which the linker silently ignores).
- Supports allowlisting/blocklisting specific functions and types.
- Common pattern: `xyzzy-sys` crate (raw `bindgen` output, `unsafe`) + `xyzzy` crate (safe Rust
  wrapper).
- Include `bindgen` in CI: regenerate and diff against checked-in version to catch drift.
- For C++ interop, consider the `cxx` crate instead of `bindgen`. It generates both Rust and C++
  from a shared schema.

---

## Key Patterns and Recurring Themes

1. **Encode invariants in the type system.** If a state is invalid, make it unrepresentable.
2. **Prefer owned data over references** in data structures, unless performance is measured and
   critical.
3. **Use `?` and `Result`** for error propagation. Reserve `panic!` for truly unrecoverable
   situations.
4. **Smart pointers are not a last resort.** `Rc<RefCell<T>>` and `Arc<Mutex<T>>` are legitimate,
   idiomatic patterns.
5. **Automate everything.** CI, Clippy, `cargo fmt`, `bindgen`, dependency audits.
6. **Make invalid states inexpressible.** This is the single most important design principle.
