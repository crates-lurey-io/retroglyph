# Apollo GraphQL: Rust Best Practices

> Source: [apollographql/rust-best-practices](https://github.com/apollographql/rust-best-practices)
> (Apache 2.0) Companion to the
> [Rust Official API Guidelines](https://rust-lang.github.io/api-guidelines/about.html) and
> [Rust Analyzer Style Guide](https://rust-analyzer.github.io/book/contributing/style.html). Born
> from Apollo's experience building the [Apollo Router](https://github.com/apollographql/router), a
> high-performance Rust graph router.

---

## 1. Error Handling (Chapter 4)

### Core rules

- **Always return `Result<T, E>` for fallible operations.** Never panic in production code.
- **Never use `unwrap()`/`expect()` outside tests.** Use `let Ok(..) = .. else { return .. }`,

  `if let`, `unwrap_or`, `unwrap_or_else`, or `unwrap_or_default` instead.

- Use `panic!` only for unrecoverable conditions. Prefer `todo!`, `unreachable!`, and

  `unimplemented!` where semantically appropriate.

### Error crate choices

| Context              | Crate       | Rationale                                                                                      |
| -------------------- | ----------- | ---------------------------------------------------------------------------------------------- |
| Libraries / crates   | `thiserror` | Structured error enums with `#[from]`, `#[error]`, integrates with `?` and `std::error::Error` |
| Binaries (top-level) | `anyhow`    | Ergonomic, context-rich error handling where precise types don't matter                        |
| Tests / helpers      | `anyhow`    | Low friction, acceptable for test code                                                         |

`anyhow::Result` erases context callers might need; never use it in library code.

### Error hierarchies

Build layered error enums with `#[from]` for nested subsystems:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Database handler error: {0}")]
    Db(#[from] DbError),
    #[error("External services error: {0}")]
    ExternalServices(#[from] ExternalHttpError),
}
```

For single-error modules, a struct error suffices:

```rust
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("Request failed with code `{code}`: {message}")]
struct HttpError { code: u16, message: String }
```

### Error propagation

- Prefer `?` over verbose `match` chains.
- Use `inspect_err` + `map_err` for logging and wrapping:

```rust
x.inspect_err(|err| tracing::error!("fn_name: {err}"))
 .map_err(|err| GeneralError::from(("fn_name", err)))?;
```

### Async errors

Ensure errors implement `Send + Sync + 'static` when used across `.await` boundaries or in Tokio
tasks. Avoid `Box<dyn std::error::Error>` in libraries.

### Testing errors

Unit tests should exercise error paths. Use `err.to_string()` for assertions when errors don't
implement `PartialEq`:

```rust
let err = divide(10., 0.0).unwrap_err();
assert_eq!(err.to_string(), "division by zero");
```

---

## 2. Coding Style and Idioms (Chapter 1)

### Borrowing vs. cloning

- **Default to borrowing (`&T`).** Clone only when you genuinely need a new owned copy.
- Accept `&str` over `String`, `&[T]` over `Vec<T>`, `&T` over `T` in function parameters.
- If a function needs ownership, make the caller pass it explicitly rather than cloning inside.
- When you must clone, defer it to the last possible moment.

Valid clone scenarios: `Arc`/`Rc` pointers, immutable snapshots, cross-thread sharing, caching, API
requirements.

### Copy trait

Derive `Copy` on types that are small (up to ~24 bytes), have all `Copy` fields, and represent plain
data (no heap allocations). Good candidates: `Point { x: f32, y: f32 }`, tag-like enums. Enum size
is based on the largest variant.

### Option/Result pattern matching

| Pattern                                  | When to use                                                                                               |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `match`                                  | Need to match inner variants of `T`/`E`, or transform type (e.g., `Result<T,E>` to `Result<Option<U>,E>`) |
| `let Ok(x) = expr else { return .. }`    | Divergent code doesn't need the error value                                                               |
| `if let Ok(x) = expr { .. } else { .. }` | Else branch needs extra computation                                                                       |
| `?`                                      | Don't care about `Err` value, just propagate                                                              |

Never use `unwrap`/`expect` outside tests. Use `.ok()`, `.ok_or()`, `.ok_or_else()` for
`Result`/`Option` conversions instead of `match`.

### Lazy evaluation

Use `_else` variants (`unwrap_or_else`, `ok_or_else`, `map_or_else`) when the fallback involves
allocation or function calls to avoid eager evaluation.

### Iterators vs. for loops

- Prefer iterators for collection transformations (`.filter`, `.map`, `.collect`).
- Prefer `for` loops for early exits (`break`, `continue`, `return`) and simple side-effect code.
- Iterators are lazy and compiled into tight loops (zero-cost abstractions).
- Prefer `.sum()` over `.fold()` for summing. Prefer `.iter()` over `.into_iter()` unless ownership

  transfer is needed.

- Avoid intermediate `.collect()` calls; pass iterators directly.

### Import ordering

```text

1. std / core / alloc
2. Enterprise crates (optional)
3. External crates
4. Workspace crates
5. super:: / crate::

```text

Automate with `rustfmt.toml`:

```toml
reorder_imports = true
imports_granularity = "Crate"
group_imports = "StdExternalCrate"
```

---

## 3. Linting (Chapter 2)

### Mandatory workflow

```sh
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Add `-W clippy::pedantic` for stricter checks. Run in CI (Apollo uses `cargo xtask lint` in the
Router).

### Key lints

| Lint                 | Category   |
| -------------------- | ---------- |
| `redundant_clone`    | perf       |
| `needless_borrow`    | style      |
| `large_enum_variant` | perf       |
| `unnecessary_wraps`  | pedantic   |
| `clone_on_copy`      | complexity |
| `needless_collect`   | nursery    |

### Suppression rules

- Never `#[allow(..)]`; use `#[expect(..)]` instead (warns when the lint no longer applies).
- Always document why a lint is suppressed.
- Avoid global overrides; suppress locally.

### Workspace lint config

```toml
[workspace.lints.clippy]
all = { level = "deny", priority = 10 }
redundant_clone = { level = "deny", priority = 9 }
pedantic = { level = "warn", priority = 3 }
```

---

## 4. Performance (Chapter 3)

### Golden rule

> Don't guess, measure.

- Always benchmark with `--release`. Debug builds are not representative.
- Use `cargo clippy -- -D clippy::perf` for automated perf suggestions.
- Profile with `cargo flamegraph` (or [samply](https://github.com/mstange/samply) on macOS).
- Use `cargo bench` for micro-benchmarks; only act on improvements >5%.

### Memory: stack vs. heap

- Keep small types (`Copy`, primitives) on the stack.
- Avoid passing types >512 bytes by value; pass by reference.
- Heap-allocate recursive data structures (`Box`).
- Avoid large stack arrays: use `vec![0; N].into_boxed_slice()` instead of `Box::new([0u8; N])`

  which temporarily allocates on the stack first.

- Use `#[inline]` only when benchmarks prove it helps.

### Cloning discipline

- Borrow by default. Clone only when ownership is truly needed.
- Use `Cow<'_, str>` for "maybe owned" data to avoid unnecessary allocations.
- Avoid `.clone()` in iterator chains; prefer `.cloned()` or `.copied()` at the end.

### Iterators

- Zero-cost abstractions: chaining `.filter()`, `.map()`, `.skip()`, `.take()` compiles away.
- Avoid intermediate collections. Pass `impl Iterator<Item = T>` instead of `Vec<T>` where possible.
- `.iter()` creates references; use `.into_iter()` only when you need to consume the collection.

---

## 5. Testing (Chapter 5)

### Naming

Use descriptive names that read like sentences. Group related tests in nested `mod` blocks:

```rust
#[cfg(test)]
mod test {
    mod process {
        #[test]
        fn returns_error_when_input_is_negative() { .. }
    }
}
```

Test output: `process::returns_error_when_input_is_negative`

### One behavior per test

Each test should verify one thing. Multiple assertions make failures harder to diagnose. Use
`rstest` for parameterized cases with descriptive names:

```rust
#[rstest]
#[case::single("a")]
#[case::first_letter("ab")]
fn accepts_strings_with_a(#[case] input: &str) {
    assert!(the_function(input).is_ok());
}
```

### Assertion style

- `assert_eq!` for value equality, `assert!(matches!(..))` for pattern matching.
- Always add formatted error messages to assertions.
- Use `pretty_assertions` for readable diffs.

### Doc tests

- Code examples in `///` doc comments run with `cargo test` (not `cargo nextest`).
- They serve as both documentation and correctness checks.
- Hide boilerplate lines with `#` prefix.

### Test pyramid

| Type              | Location                     | Access                 | Purpose                                     |
| ----------------- | ---------------------------- | ---------------------- | ------------------------------------------- |
| Unit tests        | Same module (`#[cfg(test)]`) | Private + `pub(crate)` | Edge cases, implementation details          |
| Integration tests | `tests/` directory           | Public API only        | Cross-module correctness                    |
| Doc tests         | `///` comments               | Public API             | Usage examples, kept up-to-date by compiler |

### Snapshot testing (cargo insta)

Use `insta` with YAML snapshots for complex/structural output. Rules:

- Name snapshots meaningfully.
- Keep snapshots small; don't snapshot huge objects.
- Use redactions for unstable fields (timestamps, UUIDs).
- Commit `.snap` files to version control.
- Use `assert_eq!` for primitives and simple types, not snapshots.

---

## 6. API Design: Generics and Dispatch (Chapter 6)

### Decision framework

> Static where you can, dynamic where you must.

| Aspect       | Static (`impl Trait` / `<T: Trait>`) | Dynamic (`dyn Trait`)     |
| ------------ | ------------------------------------ | ------------------------- |
| Performance  | Zero-cost (monomorphized)            | vtable indirection        |
| Compile time | Slower                               | Faster                    |
| Binary size  | Larger                               | Smaller                   |
| Flexibility  | One type at a time                   | Heterogeneous collections |

### Static dispatch guidelines

- Default to generics / `impl Trait` for performance-critical code.
- Use trait bounds to constrain generics at compile time.
- Prefer `impl Iterator<Item = U>` syntax over explicit `<T: Iterator<Item = U>>` for readability.

### Dynamic dispatch guidelines

- Use `dyn Trait` for plugin architectures, heterogeneous collections, or stable library interfaces.
- Prefer `&dyn Trait` over `Box<dyn Trait>` when ownership isn't needed.
- Use `Arc<dyn Trait + Send + Sync>` for shared access across threads.
- Box at the API boundary, not internally.
- Avoid boxing inside structs unless required (recursive types).
- Traits must be object-safe: no generic methods, no `Self: Sized`, methods use

  `&self`/`&mut self`/`self`.

---

## 7. Design Patterns: Type State (Chapter 7)

Encode states as types so illegal transitions become compile errors:

```rust
struct Disconnected;
struct Connected;

struct Client<State> {
    stream: Option<TcpStream>,
    _state: PhantomData<State>,
}

impl Client<Disconnected> {
    fn connect(addr: &str) -> io::Result<Client<Connected>> { .. }
}

impl Client<Connected> {
    fn send(&mut self, msg: &str) { .. }  // Only available when connected
}
```

### When to use

- Enforcing API constraints at compile time (builders with required fields, protocol state

  machines).

- Replacing runtime booleans/enums with type-safe code paths.
- Library/crate APIs where correctness is critical.

### When to avoid

- Trivial states better served by enums.
- When it leads to overcomplicated generic signatures.
- When runtime flexibility is required.

### Trade-offs

`PhantomData` fields are erased after compilation (zero runtime cost), but the pattern adds
verbosity and can require field duplication across state transitions.

---

## 8. Comments and Documentation (Chapter 8)

### Comments (`//`)

- Comment the **why**, not the what or how.
- Prefix with category: `// SAFETY:`, `// PERF:`, `// CONTEXT:`, `// TODO(issue #42):`.
- Link to ADRs or design docs for deeper justification.
- Never leave `// TODO:` without a linked issue.
- If a comment could be replaced by better naming or a helper function, refactor instead.
- Treat stale comments as bugs; remove or update them.

### Doc comments (`///` and `//!`)

- Document all public items: structs, enums, traits, functions, constants.
- Include `# Examples`, `# Errors`, `# Panics`, `# Safety` sections where relevant.
- Use `//!` at the top of `lib.rs` / `mod.rs` for module-level purpose and examples.
- Doc examples are tested by `cargo test`; prefer testable examples.
- Use `cargo doc --open` to verify output.

### Recommended doc lints

```rust
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
```

| Lint                     | Purpose                                                  |
| ------------------------ | -------------------------------------------------------- |
| `missing_docs`           | Warns on undocumented public items                       |
| `broken_intra_doc_links` | Catches renamed/broken doc links                         |
| `empty_docs`             | Prevents bypassing `missing_docs` with empty `///`       |
| `missing_panics_doc`     | Requires `# Panics` section if function can panic        |
| `missing_errors_doc`     | Requires `# Errors` section if function returns `Result` |
| `missing_safety_doc`     | Requires `# Safety` section for unsafe public functions  |

---

## 9. Pointers and Thread Safety (Chapter 9)

### Quick reference

| Pointer                       | Thread-safe?        | Use case                                                   |
| ----------------------------- | ------------------- | ---------------------------------------------------------- |
| `&T`                          | Yes                 | Shared read access                                         |
| `&mut T`                      | Not `Send`          | Exclusive mutation                                         |
| `Box<T>`                      | If `T: Send + Sync` | Single-owner heap allocation                               |
| `Rc<T>`                       | No                  | Multiple owners, single-threaded                           |
| `Arc<T>`                      | Yes                 | Multiple owners, multi-threaded                            |
| `RefCell<T>`                  | Not `Sync`          | Interior mutability, single-threaded (panics on violation) |
| `Cell<T>`                     | Not `Sync`          | Interior mutability for `Copy` types, single-threaded      |
| `Mutex<T>`                    | Yes                 | Shared mutable access, multi-threaded                      |
| `RwLock<T>`                   | Yes                 | Many readers OR one writer, multi-threaded                 |
| `OnceLock<T>` / `LazyLock<T>` | Yes                 | Thread-safe one-time / lazy initialization                 |
| `*const T` / `*mut T`         | No (manual)         | FFI, raw memory                                            |

### Guidelines

- Default to `&T` for shared access, `&mut T` for exclusive mutation.
- Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared mutable state across threads.
- Use `OnceLock` / `LazyLock` for static initialization instead of `lazy_static!`.
- Use `Cow<'_, T>` when a function might or might not need to own data.
- Raw pointers only for FFI; keep `unsafe` blocks minimal and document safety invariants.

---

## 10. Dependency Management

### Workspace lints

Centralize lint configuration in the workspace `Cargo.toml` rather than per-package:

```toml
[workspace.lints.clippy]
all = { level = "deny", priority = 10 }
pedantic = { level = "warn", priority = 3 }

[workspace.lints.rust]
future-incompatible = "warn"
nonstandard_style = "deny"
```

### Cargo.lock

Always pass `--locked` in CI to ensure reproducible builds. The lock file should be committed for
binaries (Apollo Router commits theirs).

### Key ecosystem crates recommended

| Crate               | Purpose                                          |
| ------------------- | ------------------------------------------------ |
| `thiserror`         | Structured error types for libraries             |
| `anyhow`            | Ergonomic errors for binaries                    |
| `insta`             | Snapshot testing                                 |
| `rstest`            | Parameterized / fixture-based tests              |
| `pretty_assertions` | Readable test diffs                              |
| `testcontainers`    | Integration tests with external services         |
| `smallvec`          | Stack-allocated small vectors with heap fallback |

### Rustfmt config

```toml
reorder_imports = true
imports_granularity = "Crate"
group_imports = "StdExternalCrate"
```

Requires nightly as of Rust 1.88: `cargo +nightly fmt`.

---

## 11. Large-Scale Project Patterns

These patterns are drawn from Apollo's Router codebase and the handbook's recurring themes:

1. **Error hierarchies per layer.** Each crate/module defines its own error enum with `thiserror`.

   Cross-layer errors use `#[from]` for automatic conversion. Binary entry points can use `anyhow`
   for top-level wrapping.

1. **Structured linting via xtask.** Apollo Router uses `cargo xtask lint` to run a standardized

   linting pipeline. This is more maintainable than Makefile targets for complex workspaces.

1. **Snapshot testing for complex output.** Use `cargo insta` with YAML snapshots for serialized

   data, generated code, and CLI output. Commit `.snap` files. Use redactions for non-deterministic
   fields.

1. **Type state pattern for protocol correctness.** Encode connection states, request validation

   stages, and builder requirements as types so invalid transitions are compile errors.

1. **Static dispatch by default, dynamic at boundaries.** Keep generics/`impl Trait` in hot paths.

   Use `dyn Trait` only at plugin interfaces and public API boundaries.

1. **Workspace-level configuration.** Centralize lints, dependencies, and rustfmt settings at the

   workspace level for consistency across dozens of crates.

1. **Design principles (from Router README).** Correctness first, then reliability, then

   performance. Follow the principle of least surprise. Test and document everything implied by the
   specification, including failure cases.

---

## Sources

- [apollographql/rust-best-practices](https://github.com/apollographql/rust-best-practices) -

  Primary source, 9 chapters + final notes

- [apollographql/router](https://github.com/apollographql/router) - Apollo Router README,

  CONTRIBUTING.md, DEVELOPMENT.md for real-world context

- [Rust Official API Guidelines](https://rust-lang.github.io/api-guidelines/about.html) - Companion

  reference cited by Apollo

- [Rust Analyzer Style Guide](https://rust-analyzer.github.io/book/contributing/style.html) -

  Companion reference cited by Apollo
