# Microsoft Pragmatic Rust Guidelines

Comprehensive reference extracted from <https://microsoft.github.io/rust-guidelines/>.
Last generated upstream: 2025-11-05.

These guidelines complement the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html), the [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/), and [Rust Design Patterns](https://rust-unofficial.github.io/patterns//intro.html). A guideline must positively affect safety, COGs, or maintenance; be agreed upon by experienced (3+ years) Rust developers; be comprehensible to novices (4+ weeks); and be pragmatic.

**Applicability**: "must" guidelines always hold; "should" guidelines indicate flexibility. Teams are free to adopt as they see fit. The golden rule: each item exists for a reason, and it is the spirit that counts, not the letter.

**Maturity**: Each guideline has a version number (semver-like). Version 1.0 = stable; 0.x = evolving.

---

## Table of Contents

1. [Universal](#1-universal)
2. [Library / Interoperability](#2-library--interoperability)
3. [Library / UX](#3-library--ux)
4. [Library / Resilience](#4-library--resilience)
5. [Library / Building](#5-library--building)
6. [Applications](#6-applications)
7. [FFI](#7-ffi)
8. [Safety](#8-safety)
9. [Performance](#9-performance)
10. [Documentation](#10-documentation)
11. [AI](#11-ai)
12. [Complete Lint Configuration](#12-complete-lint-configuration)
13. [Guideline ID Index](#13-guideline-id-index)

---

## 1. Universal

### M-UPSTREAM-GUIDELINES: Follow the Upstream Guidelines (v1.0)

Projects must follow existing community guidelines as a baseline:

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html)
- [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/)
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns//intro.html)
- [Rust Reference - Undefined Behavior](https://doc.rust-lang.org/reference/behavior-considered-undefined.html)

Frequently forgotten upstream items to pay special attention to:

- **C-CONV**: Ad-hoc conversions follow `as_`, `to_`, `into_` conventions.
- **C-GETTER**: Getter names follow Rust convention (no `get_` prefix).
- **C-COMMON-TRAITS**: Types eagerly implement `Copy`, `Clone`, `Eq`, `PartialEq`, `Ord`, `PartialOrd`, `Hash`, `Default`, `Debug`, `Display`.
- **C-CTOR**: Constructors are static, inherent methods. Have `Foo::new()` even if you have `Foo::default()`.
- **C-FEATURE**: Feature names are free of placeholder words.

### M-STATIC-VERIFICATION: Use Static Verification (v1.0)

Projects should use these tools, both locally and in CI:

| Tool | Purpose |
|------|---------|
| **Compiler lints** | Bug prevention, code quality |
| **Clippy lints** | Hundreds of lints for bugs and quality |
| **rustfmt** | Consistent formatting |
| **cargo-audit** | Security vulnerability scanning |
| **cargo-hack** | Validate all feature combinations |
| **cargo-udeps** | Detect unused dependencies |
| **Miri** | Validate unsafe code correctness |

See [Section 12](#12-complete-lint-configuration) for the full recommended lint configuration.

### M-LINT-OVERRIDE-EXPECT: Lint Overrides Should Use `#[expect]` (v1.0)

When overriding lints on a specific item, use `#[expect]` instead of `#[allow]`. Expected lints emit a warning if the suppressed lint was not actually encountered, preventing accumulation of stale overrides. `#[allow]` is still acceptable for generated code and macros.

Always include a `reason`:

```rust
#[expect(clippy::unused_async, reason = "API fixed, will use I/O later")]
pub async fn ping_server() {
    // Stubbed out for now
}
```

### M-PUBLIC-DEBUG: Public Types are Debug (v1.0)

All public types must implement `Debug`. Most via `#[derive(Debug)]`.

Types holding sensitive data must use a custom `Debug` impl that redacts secrets, with unit tests proving no leak:

```rust
impl Debug for UserSecret {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "UserSecret(...)")
    }
}

#[test]
fn secret_not_leaked() {
    let key = "552d3454-d0d5-445d-ab9f-ef2ae3a8896a";
    let secret = UserSecret(key.to_string());
    let rendered = format!("{:?}", secret);
    assert!(rendered.contains("UserSecret"));
    assert!(!rendered.contains(key));
}
```

### M-PUBLIC-DISPLAY: Public Types Meant to be Read are Display (v1.0)

Types expected to be read by consumers (developers or end users) must implement `Display`. This includes error types (required by `std::error::Error`) and wrappers around string-like data. Follow Rust customs for rendering newlines and escape sequences. Sensitive data redaction from M-PUBLIC-DEBUG applies analogously.

### M-SMALLER-CRATES: If in Doubt, Split the Crate (v1.0)

Err on the side of too many crates rather than too few. Benefits: dramatic compile time improvements and prevention of cyclic dependencies. If a submodule can be used independently, move it to a separate crate.

Losing access to `pub(crate)` fields is often a desirable side-effect that prompts better abstractions.

**Features vs. Crates**: Crates are for items that can be used on their own. Features unlock extra functionality that can't live alone. For umbrella crates, features may enable constituents.

### M-CONCISE-NAMES: Names are Free of Weasel Words (v1.0)

Avoid meaningless type/trait names: `Service`, `Manager`, `Factory`.

- `BookingService` -> `Bookings` or `BookingDispatcher` (be specific)
- `FooManager` -> name the specific responsibility
- `FooFactory` -> `FooBuilder` (the Rust canonical name is `Builder`)

Accepting factories as parameters is unidiomatic. Prefer `impl Fn() -> Foo` over `FooBuilder` for repeatable instantiation.

### M-REGULAR-FN: Prefer Regular over Associated Functions (v1.0)

Associated functions should primarily be for instance creation. Functionality not directly related to a type's receiver should be a regular (free) function:

```rust
// Bad: associated function with no relation to Database
impl Database {
    fn check_parameters(p: &str) {}
}

// Good: free function
fn check_parameters(p: &str) {}
```

Associated trait functions are perfectly idiomatic.

### M-PANIC-IS-STOP: Panic Means 'Stop the Program' (v1.0)

Panics are not exceptions. They signal immediate program termination. It is NOT valid to:

- Use panics to communicate errors upstream
- Use panics to handle self-inflicted error conditions
- Assume panics will be caught

Valid reasons to panic:

1. Programming error detected: `x.expect("must never happen")`
2. Const contexts: `const { foo.unwrap() }`
3. User-requested: providing an `unwrap()` method
4. Poison encountered: `lock.unwrap()` on a poisoned mutex

Remember: `panic = "abort"` in release profiles will terminate on any panic.

### M-PANIC-ON-BUG: Detected Programming Bugs are Panics, Not Errors (v1.0)

When an unrecoverable programming error is detected, libraries and applications must panic, not return an `Error`. No error type should be introduced for conditions that cannot be acted upon at runtime.

Contract violations (broken invariants) are programming errors and must panic. APIs are not expected to go out of their way to detect violations if checks would be expensive.

Aim for "correct by construction" using the type system to avoid panicking code paths.

### M-DOCUMENTED-MAGIC: All Magic Values and Behaviors are Documented (v1.0)

Hardcoded magic values must have comments explaining:

- Why this value was chosen
- Non-obvious side effects if changed
- External systems that interact with it

Prefer named constants over inline values:

```rust
/// How long we wait for the server.
///
/// Large enough to ensure the server can finish. Setting this too low
/// might abort a valid request. Based on `api.foo.com` timeout policies.
const UPSTREAM_SERVER_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 24);
```

### M-LOG-STRUCTURED: Use Structured Logging with Message Templates (v0.1)

Use structured events with named properties following the [message templates](https://messagetemplates.org/) spec.

**Key rules:**

1. **Avoid string formatting** (allocates at runtime). Use message templates that defer formatting:
   ```rust
   // Bad
   tracing::info!("file opened: {}", path);
   // Good
   event!(name: "file.open.success", Level::INFO, file.path = path.display(),
       "file opened: {{file.path}}");
   ```

2. **Name your events** with hierarchical dot-notation: `<component>.<operation>.<state>`

3. **Follow OpenTelemetry semantic conventions** for common attributes:
   - HTTP: `http.request.method`, `http.response.status_code`, `url.path`
   - File: `file.path`, `file.size`, `file.directory`
   - Database: `db.system.name`, `db.namespace`, `db.operation.name`
   - Errors: `error.type`, `error.message`

4. **Redact sensitive data**: emails, file paths revealing identity, tokens, PII. Consider the [`data_privacy`](https://crates.io/crates/data_privacy) crate.

---

## 2. Library / Interoperability

### M-TYPES-SEND: Types are Send (v1.0)

Public types should be `Send` for compatibility with Tokio and work-stealing runtimes:

- All futures (explicit or implicit) **must** be `Send`.
- Most other types **should** be `Send`.
- Types with only instantaneous use (never held across `.await`) may be `!Send`.

Assert `Send` at compile time:

```rust
const fn assert_send<T: Send>() {}
const _: () = assert_send::<MyFuture>();
```

The cost of `Send` (atomics, uncontended locks) is negligible unless accessed more frequently than every ~64 words in a hot loop.

### M-ESCAPE-HATCHES: Native Escape Hatches (v0.1)

Types wrapping native handles should provide `unsafe` escape hatches for interop:

```rust
pub unsafe fn from_native(native: HNATIVE) -> Self { Self(native) }
pub fn into_native(self) -> HNATIVE { self.0 }
pub fn to_native(&self) -> HNATIVE { self.0 }
```

Document all safety requirements on `from_native`.

### M-DONT-LEAK-TYPES: Don't Leak External Types (v0.1)

Prefer `std` types in public APIs over third-party crate types. Leaking external types creates coupling.

Heuristic:

- Avoid leaking third-party types if possible
- Sibling crates in an umbrella may freely leak each other's types
- Behind a feature flag (e.g., `serde`), leaking is acceptable
- Without a feature, only if it gives **substantial** ecosystem interoperability benefit

---

## 3. Library / UX

### M-SIMPLE-ABSTRACTIONS: Abstractions Don't Visibly Nest (v0.1)

Avoid exposing nested or complex parameterized types to users:

```rust
// Good
service: Service
// Acceptable
service: Service<Backend>
// Bad
service: Service<Backend<Store>>
```

Rule of thumb: primary service API types should not nest on their own, and if they do, only 1 level deep. Container types (`List<T>`) obviously expose parameters but should still limit nesting.

Consider: Will the type be named by users? Does it compose with non-user types? Do type parameters have complex bounds?

### M-AVOID-WRAPPERS: Avoid Smart Pointers and Wrappers in APIs (v1.0)

`Rc<T>`, `Arc<T>`, `Box<T>`, `RefCell<T>` are implementation details. Public APIs should use `&T`, `&mut T`, or `T` directly.

Acceptable when: the smart pointer is fundamental to the API's purpose, or benchmarks show significant performance improvement.

### M-DI-HIERARCHY: Prefer Types over Generics, Generics over Dyn Traits (v0.1)

Design escalation ladder for async dependencies:

1. **Concrete types** (best default)
2. **Enums** for testing/mocking (see M-MOCKABLE-SYSCALLS)
3. **Generic type parameters** with narrow traits (`StoreObject`, `LoadObject`)
4. **`dyn Trait`** only when generics cause nesting problems; wrap in a custom type

```rust
// Good: generic with narrow trait
async fn read_database(x: impl LoadObject) { ... }

// When dyn is needed, wrap it
struct DynamicDataAccess(Arc<dyn DataAccess>);
```

### M-ERRORS-CANONICAL-STRUCTS: Errors are Canonical Structs (v1.0)

Errors should be situation-specific `struct`s containing:

- A `Backtrace`
- An optional upstream error cause
- Helper methods for contextual information (e.g., `config_file()`)
- Proper `Display` and `std::error::Error` implementations

```rust
pub struct ConfigurationError {
    backtrace: Backtrace,
}
```

**Error design rules:**

- Prefer separate error types per operation domain (`DownloadError`, `VmError`) over a global enum
- If using an inner `ErrorKind` enum, keep it `pub(crate)` and expose `is_xxx()` methods
- Capture backtraces when creating errors (cheap when `RUST_BACKTRACE` is unset, ~4us when enabled)
- Consider a private `bail!()` macro for frequent error emission

### M-INIT-BUILDER: Complex Type Construction has Builders (v0.3)

Types with 4+ optional initialization permutations should provide builders. Up to 2 optional parameters can use inherent `with_*` methods.

**Builder conventions:**

- Builder for `Foo` is named `FooBuilder`
- Methods are chainable, final method is `.build()`
- Shortcut: `Foo::builder()`, not `FooBuilder::new()`
- Setters are `x()`, not `set_x()`
- Required parameters go in `builder()` args, not setter methods

```rust
impl Foo {
    pub fn builder(deps: impl Into<FooDeps>) -> FooBuilder { ... }
}
```

For multiple required params, use a deps struct with `From` impls for convenient usage:
- `Foo::builder(logger)` (single dep)
- `Foo::builder((logger, config))` (tuple)
- `Foo::builder(FooDeps { logger, config })` (explicit)

Runtime-specific builders use `builder_{runtime}(deps)` pattern:
- `Foo::builder_tokio(deps)`, `Foo::builder_smol(deps)`

### M-INIT-CASCADED: Complex Type Initialization Hierarchies are Cascaded (v1.0)

Types requiring 4+ parameters should cascade initialization via helper types:

```rust
// Bad: confusable parameters
fn new(bank_name: &str, customer_name: &str, currency_name: &str, amount: u64)

// Good: semantic grouping
fn new(account: Account, amount: Currency)
```

Also apply C-NEWTYPE from the API Guidelines.

### M-SERVICES-CLONE: Services are Clone (v1.0)

Heavyweight service types should implement shared-ownership `Clone` via the `Arc<Inner>` pattern:

```rust
struct ServiceInner { /* core logic and data */ }

#[derive(Clone)]
pub struct Service {
    inner: Arc<ServiceInner>,
}
```

`Clone` produces a lightweight handle, not a deep copy. Users can create one instance and pass it to multiple consumers.

### M-IMPL-ASREF: Accept `impl AsRef<>` Where Feasible (v1.0)

In **function** signatures (not struct fields), accept `impl AsRef<T>` for types with clear reference hierarchies:

| Instead of | Accept |
|-----------|--------|
| `&str`, `String` | `impl AsRef<str>` |
| `&Path`, `PathBuf` | `impl AsRef<Path>` |
| `&[u8]`, `Vec<u8>` | `impl AsRef<[u8]>` |

For functions that take ownership of data and are high-frequency, accepting the owned type may be better for performance.

### M-IMPL-RANGEBOUNDS: Accept `impl RangeBounds<>` Where Feasible (v1.0)

Functions accepting a range of numbers must use a `Range` type, not hand-rolled `(low, high)` parameters. Prefer `impl RangeBounds<T>` over `Range<T>` for maximum flexibility (`1..3`, `1..`, `..`).

### M-IMPL-IO: Accept `impl 'IO'` Where Feasible ('Sans IO') (v0.1)

Functions doing one-shot I/O during initialization should accept `impl Read` / `impl Write` instead of concrete types:

```rust
// Bad: requires File
fn parse_data(file: File) {}

// Good: accepts File, TcpStream, Stdin, &[u8], UnixStream, ...
fn parse_data(data: impl std::io::Read) {}
```

Sync: `std::io::Read`/`Write`. Async functions: `futures::io::AsyncRead`/`AsyncWrite`.

### M-ESSENTIAL-FN-INHERENT: Essential Functionality Should be Inherent (v1.0)

Core functionality lives as inherent methods. Trait implementations forward to inherent methods:

```rust
impl HttpClient {
    fn download_file(&self, url: impl AsRef<str>) { /* logic */ }
}

impl Download for HttpClient {
    fn download_file(&self, url: impl AsRef<str>) {
        Self::download_file(self, url) // forward
    }
}
```

This ensures discoverability without needing to know which traits to import.

---

## 4. Library / Resilience

### M-MOCKABLE-SYSCALLS: I/O and System Calls Are Mockable (v0.2)

Any type doing I/O or syscalls with side effects must be mockable. This includes: file/network access, clocks, entropy sources, seeds, and anything non-deterministic, environment-dependent, or hardware-dependent.

**Implementation pattern**: internal enum dispatching to native or mock:

```rust
enum LibraryCore {
    Native,
    #[cfg(feature = "test-util")]
    Mocked(mock::MockCtrl),
}
```

Return mock controllers via tuple: `fn new_mocked() -> (Self, MockCtrl)`.

Libraries should NOT:
- Perform ad-hoc I/O (`read("foo.txt")`)
- Rely on non-mockable syscalls
- Create their own I/O core
- Offer `MyIoLibrary::default()` constructors

### M-TEST-UTIL: Test Utilities are Feature Gated (v0.2)

Testing functionality must be behind a `test-util` feature flag:
- Mocking functionality
- Sensitive data inspection
- Safety check overrides
- Fake data generation

```rust
#[cfg(feature = "test-util")]
pub fn bypass_certificate_checks() { ... }
```

### M-STRONG-TYPES: Use the Proper Type Family (v1.0)

Use the strongest `std` type available, as early as possible:

| Don't use | Use instead |
|-----------|-------------|
| `String` / `&str` for OS paths | `PathBuf` / `Path` |

Follow `std` conventions: numeric types at public API boundaries (e.g., `window_size()`) should be regular numbers, not `Saturating<usize>` or `NonZero<usize>`.

### M-NO-GLOB-REEXPORTS: Don't Glob Re-Export Items (v1.0)

Don't `pub use foo::*` from other modules/crates. Glob exports are hard to review and risk leaking unintended types. Re-export individually:

```rust
pub use foo::{A, B, C};
```

Exception: platform-specific HAL re-exports where everything from a single platform module is forwarded.

### M-AVOID-STATICS: Avoid Statics (v1.0)

Libraries should avoid `static` and thread-local items when consistent state is required for correctness. Statics used only for performance optimization are acceptable.

**The core problem**: Rust may link multiple versions of the same crate, each with its own independent `static` instances. A `GLOBAL_COUNTER` in crate `core v0.4` and `core v0.5` are separate variables. This causes silent state duplication, correctness issues, testing interference, and contention in thread-per-core designs.

---

## 5. Library / Building

### M-OOBE: Libraries Work Out of the Box (v1.0)

Libraries must compile on all [Tier 1 platforms](https://doc.rust-lang.org/rustc/platform-support.html) with nothing beyond `cargo` and `rust`. No additional tools, environment variables, or prerequisites.

If a dependency needs external tools (e.g., `.proto` generation), run those during publishing and include the generated artifacts.

Libraries are responsible for their dependencies' build requirements.

### M-SYS-CRATES: Native `-sys` Crates Compile Without Dependencies (v0.2)

For `foo` / `foo-sys` crate pairs wrapping native libraries:

- Build `foo.lib` entirely from `build.rs` using the [`cc`](https://crates.io/crates/cc) crate
- Do NOT run Makefiles or external build scripts
- Make external tools optional (e.g., `nasm`)
- Embed upstream source code with verifiable Git URL + hash
- Pre-generate `bindgen` glue
- Support both static and dynamic linking ([`libloading`](https://crates.io/crates/libloading))

### M-FEATURES-ADDITIVE: Features are Additive (v1.0)

All library features must be additive; any combination must work:

- No `no-std` feature (use a `std` feature instead)
- Adding feature `foo` must not disable or modify any public item
- Features must not rely on manual co-enablement
- Features must not skip-enable children's features

---

## 6. Applications

### M-MIMALLOC-APP: Use Mimalloc for Apps (v0.1)

Applications should set [mimalloc](https://crates.io/crates/mimalloc) as their global allocator. Up to 25% benchmark improvement on allocating hot paths.

```rust
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
```

### M-APP-ERROR: Applications may use Anyhow or Derivatives (v0.1)

Applications (and app-internal crates) may use [anyhow](https://github.com/dtolnay/anyhow) or [eyre](https://github.com/eyre-rs/eyre) instead of custom error types. Pick one and use it consistently across all application-level code. Libraries must still follow M-ERRORS-CANONICAL-STRUCTS.

---

## 7. FFI

### M-ISOLATE-DLL-STATE: Isolate DLL State Between FFI Libraries (v0.1)

When loading multiple Rust-based DLLs within one application, only share "portable" state. A type is portable if it is `#[repr(C)]` AND:

- Has no interaction with any `static` or thread local
- Has no interaction with any `TypeId`
- Contains no value, pointer, or reference to non-portable data

Each DLL is a separate compilation artifact with its own:
- `static` and thread-local variables
- Type layouts for `#[repr(Rust)]` types
- Unique type IDs

**Dangerous to share across DLLs:**
- Any allocated instance (`String`, `Vec<u8>`, `Box<Foo>`)
- Any library relying on statics (`tokio`, `log`)
- Any struct not `#[repr(C)]`
- Any data structure relying on consistent `TypeId`

Even calling methods on a type from another DLL executes code from the *calling* DLL, using statics from the calling DLL, on data from the other DLL.

---

## 8. Safety

### M-UNSAFE: Unsafe Needs Reason, Should be Avoided (v0.2)

The only valid reasons for `unsafe`:

1. **Novel abstractions**: new smart pointers, allocators
2. **Performance**: e.g., `.get_unchecked()` after benchmarking
3. **FFI and platform calls**: calling into C or the kernel

**Never use ad-hoc `unsafe` to:**
- Shorten a safe program (e.g., `transmute` for enum casts)
- Bypass `Send` bounds (`unsafe impl Send`)
- Bypass lifetime requirements via `transmute`

**Requirements by category:**

| Category | Must verify no alternative | Must be minimal/testable | Must handle adversarial code | Must have safety comment | Must pass Miri | Must follow unsafe code guidelines |
|----------|:---:|:---:|:---:|:---:|:---:|:---:|
| Novel abstractions | Yes | Yes | Yes | Yes | Yes | Yes |
| Performance | - | - | - | Yes | Yes | Yes |
| FFI | - | - | - | - | - | Yes |

**Adversarial code hardening** (for novel abstractions):
- Must become invalid (poisoned) if a closure panics
- Must assume any safe trait is misbehaving (`Deref`, `Clone`, `Drop`)

### M-UNSAFE-IMPLIES-UB: Unsafe Implies Undefined Behavior (v1.0)

`unsafe` may only be applied to functions and traits if misuse implies the risk of undefined behavior. It must NOT mark functions that are merely "dangerous" for other reasons:

```rust
unsafe fn print_string(x: *const String) { }  // Valid: UB risk
unsafe fn delete_database() { }                // Invalid: dangerous but no UB
```

### M-UNSOUND: All Code Must be Sound (v1.0)

**No exceptions.** Unsound code is never acceptable.

A function is unsound if it appears safe (not marked `unsafe`) but any calling mode could cause undefined behavior, even in "remote, theoretical" scenarios with "weird code."

Soundness boundaries equal module boundaries: safe functions within the same module may rely on invariants guaranteed elsewhere in that module.

If you cannot safely encapsulate something, expose `unsafe` functions and document proper behavior.

---

## 9. Performance

### M-THROUGHPUT: Optimize for Throughput, Avoid Empty Cycles (v0.1)

Key metric: items per CPU cycle.

**Do:**
- Partition reasonable chunks of work ahead of time
- Let threads/tasks work independently on their slice
- Sleep or yield when no work is present
- Design and use batched APIs
- Yield within long items or between batch chunks
- Exploit CPU caches, temporal and spatial locality

**Don't:**
- Hot spin for individual items
- Process individual items when batching is possible
- Do work stealing to balance individual items

Shared state only when sharing cost < re-computation cost.

### M-HOTPATH: Identify, Profile, Optimize the Hot Path Early (v0.1)

For performance/COGS-relevant crates:

1. Identify hot paths early
2. Create benchmarks with [criterion](https://crates.io/crates/criterion) or [divan](https://crates.io/crates/divan)
3. Regularly run a profiler (CPU and allocation insights)
4. Document the most performance-sensitive areas

Enable debug symbols for benchmarks:
```toml
[profile.bench]
debug = 1
```

**Common perf issues seen (~15-50% gains when fixed):**
- Frequent re-allocations (cloned, growing, `format!`-assembled strings)
- Short-lived allocations (use bump allocators)
- Memory copy from cloning Strings and collections
- Repeated re-hashing of equal data structures
- Using Rust's default hasher where collision resistance isn't needed

### M-YIELD-POINTS: Long-Running Tasks Should Have Yield Points (v0.2)

Futures with long CPU operations without I/O must cooperatively yield:

```rust
async fn process_items(zip_file: File) {
    let items = zip_file.read().await;
    for i in items {
        decompress(i);
        yield_now().await;
    }
}
```

**Target**: 10-100us of CPU-bound work between yield points (keeps switching overhead < 1%).

For unpredictable operation durations, use runtime APIs like `has_budget_remaining()`.

---

## 10. Documentation

### M-FIRST-DOC-SENTENCE: First Sentence is One Line; Approx. 15 Words (v1.0)

The first sentence of a doc comment becomes the summary shown in module listings. Keep it to ~15 words max so it fits on one line in rendered docs, avoiding widows and unpleasant reading flow.

### M-MODULE-DOCS: Has Module Documentation (v1.1)

Any public library module must have `//!` module documentation. The first sentence follows M-FIRST-DOC-SENTENCE. The rest should cover:

- What the module contains
- When to use it (and when not to)
- Examples
- Subsystem specifications
- Observable side effects and guarantees
- Relevant implementation details (e.g., system APIs used)

Great examples: `std::fmt`, `std::pin`, `std::option`.

### M-CANONICAL-DOCS: Documentation Has Canonical Sections (v1.0)

Public library items must contain these sections (when applicable):

```rust
/// Summary sentence < 15 words.
///
/// Extended documentation in free form.
///
/// # Examples
/// Directly usable code examples.
///
/// # Errors
/// List known error conditions (if fn returns Result).
///
/// # Panics
/// List when panics may occur.
///
/// # Safety
/// List all conditions a caller must uphold (if unsafe).
///
/// # Abort
/// List when process abort may happen.
pub fn foo() {}
```

Do NOT create parameter tables. Instead, explain parameters in plain text:

```rust
/// Copies a file from `src` to `dst`.
fn copy(src: File, dst: File) {}
```

### M-DOC-INLINE: Mark `pub use` Items with `#[doc(inline)]` (v1.0)

Re-exported items should be annotated with `#[doc(inline)]` so they appear organically in docs rather than in an opaque re-export block:

```rust
#[doc(inline)]
pub use foo::Foo;
```

Exception: `std` or third-party types should always be re-exported without inlining to make it clear they are external.

---

## 11. AI

### M-DESIGN-FOR-AI: Design with AI Use in Mind (v0.1)

Rust's strong type system counterbalances AI agents' lack of genuine understanding. Making APIs easier for humans also makes them easier for AI.

**Key guidelines for AI-friendly code:**

1. **Idiomatic patterns**: Follow Rust API Guidelines and Library/UX guidelines so AI can pattern-match against the majority of Rust code.
2. **Thorough docs**: Include docs for all modules and public items. Assume solid-but-not-expert Rust knowledge.
3. **Thorough examples**: Documentation should have directly usable examples; the repository should have more elaborate ones.
4. **Strong types**: Avoid primitive obsession. Use strong types with strict, well-documented semantics (C-NEWTYPE).
5. **Testable APIs**: Design APIs that allow customers to test their usage in unit tests. Introduce mocks, fakes, or cargo features as needed.
6. **Test coverage**: Good coverage over observable behavior enables agents to refactor in a mostly hands-off mode.

---

## 12. Complete Lint Configuration

### Compiler Lints (`[lints.rust]`)

```toml
[lints.rust]
ambiguous_negative_literals = "warn"
missing_debug_implementations = "warn"
redundant_imports = "warn"
redundant_lifetimes = "warn"
trivial_numeric_casts = "warn"
unsafe_op_in_unsafe_fn = "warn"
unused_lifetimes = "warn"
```

### Clippy Lints (`[lints.clippy]`)

```toml
[lints.clippy]
# Enable all major lint categories
cargo = { level = "warn", priority = -1 }
complexity = { level = "warn", priority = -1 }
correctness = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
perf = { level = "warn", priority = -1 }
style = { level = "warn", priority = -1 }
suspicious = { level = "warn", priority = -1 }
# nursery = { level = "warn", priority = -1 }  # optional, more false positives

# Restriction lints for consistency, quality, and brevity
allow_attributes_without_reason = "warn"
as_pointer_underscore = "warn"
assertions_on_result_states = "warn"
clone_on_ref_ptr = "warn"
deref_by_slicing = "warn"
disallowed_script_idents = "warn"
empty_drop = "warn"
empty_enum_variants_with_brackets = "warn"
empty_structs_with_brackets = "warn"
fn_to_numeric_cast_any = "warn"
if_then_some_else_none = "warn"
map_err_ignore = "warn"
redundant_type_annotations = "warn"
renamed_function_params = "warn"
semicolon_outside_block = "warn"
string_to_string = "warn"
undocumented_unsafe_blocks = "warn"
unnecessary_safety_comment = "warn"
unnecessary_safety_doc = "warn"
unneeded_field_pattern = "warn"
unused_result_ok = "warn"

# Prevent issues with structured logging
literal_string_with_formatting_args = "allow"

# Define custom opt-outs below as needed
```

---

## 13. Guideline ID Index

Complete list of all M-* guidelines with their section, maturity, and summary.

| ID | Section | Version | Summary |
|----|---------|---------|---------|
| M-UPSTREAM-GUIDELINES | Universal | 1.0 | Follow Rust API Guidelines, Style Guide, Design Patterns |
| M-STATIC-VERIFICATION | Universal | 1.0 | Use compiler lints, clippy, rustfmt, cargo-audit, cargo-hack, cargo-udeps, Miri |
| M-LINT-OVERRIDE-EXPECT | Universal | 1.0 | Use `#[expect]` instead of `#[allow]` for lint overrides |
| M-PUBLIC-DEBUG | Universal | 1.0 | All public types implement Debug; redact secrets |
| M-PUBLIC-DISPLAY | Universal | 1.0 | Public types meant to be read implement Display |
| M-SMALLER-CRATES | Universal | 1.0 | Split crates aggressively for compile times and modularity |
| M-CONCISE-NAMES | Universal | 1.0 | No weasel words (Service, Manager, Factory) in type names |
| M-REGULAR-FN | Universal | 1.0 | Prefer free functions over unrelated associated functions |
| M-PANIC-IS-STOP | Universal | 1.0 | Panics mean stop; not for error communication |
| M-PANIC-ON-BUG | Universal | 1.0 | Programming bugs panic, not errors |
| M-DOCUMENTED-MAGIC | Universal | 1.0 | All magic values documented with rationale |
| M-LOG-STRUCTURED | Universal | 0.1 | Structured logging with message templates and OTel conventions |
| M-TYPES-SEND | Library/Interop | 1.0 | Public types (especially futures) are Send |
| M-ESCAPE-HATCHES | Library/Interop | 0.1 | Native handle types provide unsafe escape hatches |
| M-DONT-LEAK-TYPES | Library/Interop | 0.1 | Prefer std types in public APIs; minimize third-party leakage |
| M-SIMPLE-ABSTRACTIONS | Library/UX | 0.1 | Avoid visible type parameter nesting in service types |
| M-AVOID-WRAPPERS | Library/UX | 1.0 | No smart pointers (Arc, Rc, Box, RefCell) in public APIs |
| M-DI-HIERARCHY | Library/UX | 0.1 | Concrete types > generics > dyn Trait |
| M-ERRORS-CANONICAL-STRUCTS | Library/UX | 1.0 | Errors are structs with Backtrace, cause, and helper methods |
| M-INIT-BUILDER | Library/UX | 0.3 | Builders for 4+ optional init permutations |
| M-INIT-CASCADED | Library/UX | 1.0 | Cascade initialization via semantic helper types for 4+ params |
| M-SERVICES-CLONE | Library/UX | 1.0 | Service types implement Clone via Arc\<Inner\> |
| M-IMPL-ASREF | Library/UX | 1.0 | Accept impl AsRef\<T\> for str, Path, [u8] in functions |
| M-IMPL-RANGEBOUNDS | Library/UX | 1.0 | Accept impl RangeBounds\<T\> instead of (low, high) pairs |
| M-IMPL-IO | Library/UX | 0.1 | Accept impl Read/Write for sans-IO composability |
| M-ESSENTIAL-FN-INHERENT | Library/UX | 1.0 | Core functionality as inherent methods; traits forward to them |
| M-MOCKABLE-SYSCALLS | Library/Resilience | 0.2 | I/O and syscalls mockable via internal enum dispatch |
| M-TEST-UTIL | Library/Resilience | 0.2 | Test utilities behind `test-util` feature flag |
| M-STRONG-TYPES | Library/Resilience | 1.0 | Use strongest std type (PathBuf not String for paths) |
| M-NO-GLOB-REEXPORTS | Library/Resilience | 1.0 | No `pub use foo::*`; re-export individually |
| M-AVOID-STATICS | Library/Resilience | 1.0 | Avoid statics when consistent state matters for correctness |
| M-OOBE | Library/Building | 1.0 | Libraries build on Tier 1 platforms with only cargo + rust |
| M-SYS-CRATES | Library/Building | 0.2 | -sys crates build via cc, embed sources, no external deps |
| M-FEATURES-ADDITIVE | Library/Building | 1.0 | All features additive; no mutually exclusive features |
| M-MIMALLOC-APP | Applications | 0.1 | Use mimalloc as global allocator for apps |
| M-APP-ERROR | Applications | 0.1 | Apps may use anyhow/eyre; libraries must not |
| M-ISOLATE-DLL-STATE | FFI | 0.1 | Only share portable (#[repr(C)], no statics/TypeId) data across DLLs |
| M-UNSAFE | Safety | 0.2 | Unsafe needs valid reason; must be tested, commented, Miri-verified |
| M-UNSAFE-IMPLIES-UB | Safety | 1.0 | unsafe marker only for UB risk, not general danger |
| M-UNSOUND | Safety | 1.0 | All code must be sound; no exceptions ever |
| M-THROUGHPUT | Performance | 0.1 | Optimize items/cycle; batch work; avoid empty cycles |
| M-HOTPATH | Performance | 0.1 | Identify hot paths early; benchmark with criterion/divan; profile regularly |
| M-YIELD-POINTS | Performance | 0.2 | Yield every 10-100us of CPU-bound async work |
| M-FIRST-DOC-SENTENCE | Documentation | 1.0 | First doc sentence: one line, ~15 words max |
| M-MODULE-DOCS | Documentation | 1.1 | All public modules have //! docs |
| M-CANONICAL-DOCS | Documentation | 1.0 | Use canonical sections: Examples, Errors, Panics, Safety, Abort |
| M-DOC-INLINE | Documentation | 1.0 | Use #[doc(inline)] on pub use re-exports |
| M-DESIGN-FOR-AI | AI | 0.1 | Idiomatic patterns, thorough docs/examples, strong types, testable APIs |

---

*Source: [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/) (2025-11-05)*
*GitHub: [microsoft/rust-guidelines](https://github.com/microsoft/rust-guidelines)*