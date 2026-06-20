# Microsoft Rust Patterns & Engineering How-Tos

> Source:
> [microsoft.github.io/RustTraining/rust-patterns-book](https://microsoft.github.io/RustTraining/rust-patterns-book/)
>
> Author: Principal Firmware Architect in Microsoft SCHIE (Silicon and Cloud Hardware Infrastructure
> Engineering). Started with Rust in 2017 at AWS EC2. Industry veteran in security, firmware, OS,
> hypervisors, CPU/platform architecture, and C++ systems.
>
> Audience: Developers past "The Rust Programming Language" who want real codebase design patterns.
> Not a language tutorial.

---

## Book Structure

16 chapters across three parts, plus appendices. Estimated 30-45 hours for thorough study.
Difficulty tags: green (fundamentals), yellow (intermediate), red (advanced).

| Part                          | Chapters | Topics                                                                    |
| ----------------------------- | -------- | ------------------------------------------------------------------------- |
| **I: Type-Level Patterns**    | 1-4      | Generics, Traits, Newtype/Type-State, PhantomData                         |
| **II: Concurrency & Runtime** | 5-9      | Channels, Concurrency, Closures, Functional vs Imperative, Smart Pointers |
| **III: Systems & Production** | 10-16    | Error Handling, Serialization, Unsafe, Macros, Testing, API Design, Async |
| **Appendices**                | 18-19    | Reference Card, Capstone Project (type-safe task scheduler)               |

---

## Part I: Type-Level Patterns

### Ch 1. Generics (Fundamentals)

**Monomorphization** generates a specialized copy of each generic function per concrete type. Zero
runtime cost, identical to hand-written specialized code. LLVM can inline, vectorize, and specialize
independently.

Key distinction from C++: Rust checks bounds at _definition_, not instantiation. `T: PartialOrd` is
verified when you define the function, so errors are caught early with clear messages.

**Code bloat mitigation**:

1. **Outline pattern**: extract the non-generic core into a separate function that exists only once

   in the binary.

1. **Use `dyn Trait` for cold paths** (error handling, logging, configuration) where vtable cost is

   negligible.

**Decision framework** -- generics vs enums vs trait objects:

| Approach                               | Dispatch               | Extensible?     | Overhead                       |
| -------------------------------------- | ---------------------- | --------------- | ------------------------------ |
| Generics (`impl Trait` / `<T: Trait>`) | Static (monomorphized) | Yes (open set)  | Zero                           |
| Enum                                   | Match arm              | No (closed set) | Zero                           |
| Trait object (`dyn Trait`)             | Dynamic (vtable)       | Yes (open set)  | Vtable pointer + indirect call |

Rule of thumb: generics for hot paths where inlining matters; `dyn Trait` for cold paths.

**Const generics** (Rust 1.51+): parameterize types over constant values. Example:
`Matrix<const ROWS: usize, const COLS: usize>` with compile-time dimensional correctness.

**`const fn`**: Rust's `constexpr`. Make constructors and utility functions `const fn` whenever
possible. Eliminates `lazy_static!` for compile-time-computable values. Ideal for register
definitions, bitmask construction, and threshold tables.

### Ch 2. Traits In Depth (Intermediate)

**Associated types vs generic parameters**: Use associated types when there's exactly one natural
output per implementing type (`Iterator::Item`). Use generic parameters when a type can implement
the trait for many types (`From<T>`, `AsRef<T>`).

**GATs** (Rust 1.65+): associated types with generic parameters. Enables lending iterators where
returned references are tied to the iterator, not the collection.

**Trait object safety rules** -- a trait is object-safe only if:

1. No `Self: Sized` bound on the trait
2. No generic type parameters on methods
3. No `Self` in return position (except via `Box<Self>`)
4. No associated functions (methods must have `&self`)

Workaround: `where Self: Sized` excludes a method from the vtable.

**Vtables and fat pointers**: `&dyn Trait` is two machine words (data_ptr + vtable_ptr). Each vtable
call costs one pointer indirection. Rust stores the vtable in the fat pointer, not inside the
object, so plain `Circle` on the stack carries no vtable overhead.

**Extension traits**: define a new trait with a blanket impl to add methods to types you don't own.
Standard naming: `FooExt` suffix. Used pervasively: `itertools::Itertools`, `futures::StreamExt`,
`tokio::io::AsyncReadExt`.

**Enum dispatch**: for a closed set of types, replace `dyn Trait` with an enum whose variants hold
concrete types. Eliminates vtable indirection and heap allocation. Branch prediction is ~0.3ns vs
~2ns vtable indirection. Use `enum_dispatch` crate for 10+ variants.

**Capability mixins**: traits with associated types + default methods + blanket impls compose
behavior without traditional OOP inheritance.

### Ch 3. Newtype and Type-State Patterns (Intermediate)

**Newtype**: single-field tuple struct wrapping an existing type. Zero runtime overhead. Catches
parameter-swapping bugs at compile time.

**`Deref` for newtypes -- decision matrix**:

- If the newtype exists to _add type safety_ or _restrict the API_: don't implement `Deref`.
- If it exists to _add capabilities_ while keeping the inner type's full surface (smart pointer):

  `Deref` is appropriate.

- `DerefMut` doubles the risk by allowing callers to bypass validation.
- Prefer explicit delegation (`as_str()`, `len()`) over blanket `Deref`.

**Type-state pattern**: uses the type system to enforce operation ordering. Each transition
_consumes_ `self` and returns a new type. Invalid states are literally unrepresentable. Zero runtime
cost via `PhantomData`.

Example: `Connection<Disconnected>` -> `Connection<Connected>` -> `Connection<Authenticated>`.
Calling `request()` on a non-authenticated connection is a compile error.

**Builder with type states**: enforces required fields at compile time. `ServerConfig<NeedsName>`
only has `.name()`, which returns `ServerConfig<NeedsPort>`, which has `.port()`, which returns
`ServerConfig<Ready>` with `.build()`.

**Config trait pattern** -- taming generic parameter explosion: Bundle associated types into a
single trait. `DiagController<Cfg: BoardConfig>` has one generic parameter forever, regardless of
how many component types it contains. Adding a new bus means adding one associated type to
`BoardConfig` and one field. No downstream signature changes. Used by Substrate/Polkadot's frame
system (20+ associated types through one `Config` trait).

**Dual-axis typestate**: `Handle<Vendor, State>` where available methods depend on both vendor trait
bound and state marker trait. `impl` blocks are gated on both axes.

### Ch 4. PhantomData (Advanced)

Three jobs:

1. **Lifetime binding**: `PhantomData<&'a T>` -- struct is treated as borrowing `'a`
2. **Ownership simulation**: `PhantomData<T>` -- drop check assumes struct owns a `T`
3. **Variance control**: `PhantomData<fn(T)>` -- makes struct contravariant over `T`

**Lifetime branding**: prevent mixing values from different sessions/contexts using unique opaque
lifetimes.

**Unit-of-measure pattern**: `Quantity<Meters>` and `Quantity<Seconds>` are distinct types. Adding
meters to seconds is a compile error. Zero runtime cost since `PhantomData<Unit>` is zero-sized.

**Variance cheat sheet**:

| PhantomData type          | Variance over T | Use when                       |
| ------------------------- | --------------- | ------------------------------ |
| `PhantomData<T>`          | Covariant       | You logically own a T          |
| `PhantomData<&'a T>`      | Covariant       | You borrow a T                 |
| `PhantomData<&'a mut T>`  | Invariant       | You mutably borrow T           |
| `PhantomData<fn(T)>`      | Contravariant   | T appears in argument position |
| `PhantomData<fn(T) -> T>` | Invariant       | T in both positions            |

Start with `PhantomData<&'a T>` (covariant). Switch to invariant only if handing out mutable access.

---

## Part II: Concurrency & Runtime

### Ch 5. Channels and Message Passing (Fundamentals)

**`std::sync::mpsc`**: multi-producer, single-consumer. Unbounded by default.
`mpsc::sync_channel(N)` for bounded with backpressure.

**`crossbeam-channel`**: production workhorse. Faster than std, supports multi-consumer (MPMC).
`select!` for multi-source message handling (like Go's `select`).

**Bounded vs unbounded**:

| Type                    | Behavior when full    | Use case                                   |
| ----------------------- | --------------------- | ------------------------------------------ |
| Unbounded               | Grows heap (OOM risk) | Only when producer is slower than consumer |
| Bounded                 | `send()` blocks       | Production default                         |
| Rendezvous (bounded(0)) | Direct handoff        | Synchronization                            |

Rule: always use bounded channels in production.

**Actor pattern**: channels serialize access to mutable state, no mutexes needed. Actor struct owns
state + receives commands via channel. Handle struct is cheap to clone, Send + Sync. Use actors when
state has complex invariants or operations take a long time.

### Ch 6. Concurrency vs Parallelism (Intermediate)

**Scoped threads** (Rust 1.63+): threads can borrow from parent scope. No more `Arc::clone()` for
sharing. The compiler proves all threads finish before data goes out of scope.

**rayon**: `par_iter()` parallelizes collection processing. Just change `.iter()` to `.par_iter()`.

| Use                 | When                                            |
| ------------------- | ----------------------------------------------- |
| `rayon::par_iter()` | Processing collections in parallel              |
| `thread::spawn`     | Long-running background tasks                   |
| `thread::scope`     | Short-lived parallel tasks borrowing local data |
| `async` + `tokio`   | I/O-bound concurrency                           |

**Shared state primitives**:

| Primitive   | Use case                | Cost                     |
| ----------- | ----------------------- | ------------------------ |
| `Mutex<T>`  | Short critical sections | Lock + unlock            |
| `RwLock<T>` | Read-heavy, rare writes | Reader-writer lock       |
| `AtomicU64` | Counters, flags         | Hardware CAS (lock-free) |

**Lazy initialization** (replaces `lazy_static!`):

- `OnceLock<T>` (Rust 1.70): init depends on runtime args
- `LazyLock<T>` (Rust 1.80): init is self-contained

**Lock-free patterns**: use proven crates (`crossbeam`, `arc-swap`, `dashmap`) over hand-rolled.
`Mutex`/`RwLock` first, atomics only if profiling shows lock contention.

### Ch 7. Closures and Higher-Order Functions (Fundamentals)

**Closure trait hierarchy**: `Fn` (borrows immutably) : `FnMut` (borrows mutably) : `FnOnce`
(consumes). Every `Fn` is also `FnMut` and `FnOnce`.

API design rule: accept `FnMut` by default (most flexible). Only require `Fn` for concurrent
calling. Only `FnOnce` if called exactly once.

**The `with` pattern** (bracketed resource access): lend a resource through a closure to guarantee
setup/teardown. The caller never touches lifecycle management. The borrow checker prevents the
resource handle from escaping. Examples: `std::thread::scope`, GPIO pin direction management.

### Ch 8. Functional vs Imperative (Intermediate)

Core principle: functional shines for _data pipelines_ (filter/map/collect). Imperative shines for
_state transitions with side effects_.

**Option/Result combinators** replace most `if let`/`match` boilerplate:

- `opt.map(f)` instead of `match opt { Some(x) => Some(f(x)), None => None }`
- `opt.unwrap_or_else(|| default())` instead of `if let Some(x) = opt { x } else { default() }`
- `bool::then_some(x)` instead of `if cond { Some(x) } else { None }`

**When loops win**: building multiple outputs simultaneously, state machines with I/O, in-place
mutation.

**Performance**: iterator chains compile to the _same machine code_ as hand-written loops. LLVM
inlines closure calls and eliminates adapter structs. The one exception: intermediate `.collect()`
allocations.

**Break chains at ~4 adapters** with named intermediates. Over-functionalizing is as bad as
under-functionalizing.

**Scoped mutability**: confine mutation to a block, bind result immutably.
`let data = { let mut buf = Vec::new(); /* mutate */ buf };`

### Ch 9. Smart Pointers and Interior Mutability (Intermediate)

| Pointer      | Owner Count | Thread-Safe      | Use When                                            |
| ------------ | ----------- | ---------------- | --------------------------------------------------- |
| `Box<T>`     | 1           | Yes (if T: Send) | Heap alloc, trait objects, recursive types          |
| `Rc<T>`      | N           | No               | Shared ownership, single thread                     |
| `Arc<T>`     | N           | Yes              | Shared ownership across threads                     |
| `Cell<T>`    | -           | No               | Interior mutability for Copy types                  |
| `RefCell<T>` | -           | No               | Interior mutability, any type (panics on violation) |
| `Cow<'_, T>` | 0 or 1      | Yes              | Avoid allocation when data often unchanged          |

**Weak references** break `Rc`/`Arc` reference cycles. Use `Rc`/`Arc` for ownership edges, `Weak`
for back-references.

**Pin**: prevents a value from being moved in memory. Essential for self-referential types and
`Future`s. Use `pin-project` crate for safe pin projections instead of manual unsafe.

**Drop ordering**: fields drop in declaration order (top to bottom). Locals drop in reverse
declaration order. This matters for resource management: if a struct has `Sender` and `JoinHandle`,
put `Sender` above `JoinHandle` so the channel closes first.

**`ManuallyDrop<T>`**: suppresses automatic drop. Use in unsafe abstractions for fine-grained
lifecycle control.

---

## Part III: Systems & Production

### Ch 10. Error Handling Patterns (Fundamentals)

**`thiserror` vs `anyhow`**:

|             | `thiserror`                        | `anyhow`                 |
| ----------- | ---------------------------------- | ------------------------ |
| Use in      | Libraries, shared crates           | Applications, binaries   |
| Error types | Concrete enums (callers can match) | `anyhow::Error` (opaque) |

**`#[from]`** auto-generates `From` impls for error conversion. `?` desugars to `From::from()` +
early return.

**`.context()`** / `.with_context()` adds human-readable wrappers without losing the original error.
Produces chained "Caused by:" output.

**Panics vs errors**: `Result<T, E>` for expected failures. `panic!()` for programming bugs.
`process::abort()` for unrecoverable state. `catch_unwind` for FFI boundaries.

### Ch 11. Serialization, Zero-Copy, and Binary Data (Intermediate)

**serde**: derive `Serialize`/`Deserialize` once, works with every format (JSON, TOML, bincode,
MessagePack, etc.).

Key attributes: `rename_all`, `deny_unknown_fields`, `default`, `skip`, `skip_serializing_if`,
`flatten`, `with`, `alias`.

**Enum representations**: externally tagged (default), internally tagged (`tag = "type"` --
recommended for JSON APIs), adjacently tagged, untagged.

**Zero-copy deserialization**: `&'a str` fields borrow directly from input buffer. Zero allocation.
Use `Cow<'a, str>` for best of both (borrow when possible, allocate when escapes need unescaping).

**Binary data**: `#[repr(C)]` for predictable memory layout. `zerocopy`/`bytemuck` for safe
zero-copy transmutation. `bytes::Bytes` for reference-counted buffers with zero-copy splitting (used
by tokio, hyper, tonic).

Format selection:

- Config files humans edit: TOML or JSON
- Rust-to-Rust IPC: bincode
- Cross-language binary: MessagePack or CBOR
- Embedded / `no_std`: postcard

### Ch 12. Unsafe Rust (Advanced)

**Five superpowers**: dereference raw pointers, call unsafe functions, access mutable statics,
implement unsafe traits, access union fields.

**Three rules of sound unsafe code**:

1. Document invariants with `// SAFETY:` comments
2. Encapsulate behind safe APIs
3. Minimize unsafe scope

**MaybeUninit**: `[const { MaybeUninit::uninit() }; N]` (Rust 1.79+) replaces the old `assume_init`
anti-pattern for arrays.

**FFI patterns**: `extern "C"`, `CStr`/`CString`, `#[no_mangle]`, `#[repr(C)]`.

**Custom allocators**:

| Pattern                | Crate         | Use case                                              |
| ---------------------- | ------------- | ----------------------------------------------------- |
| Arena (bump allocator) | `bumpalo`     | Request/frame-scoped, ~2ns alloc, O(1) bulk free      |
| Typed arena            | `typed-arena` | Same type, lifetime-scoped refs                       |
| Slab                   | `slab`        | Fixed-size object pools, O(1) alloc/free, index-based |
| Fixed arena (no_std)   | Custom        | Bare-metal, stack-backed                              |

### Ch 13. Macros (Intermediate)

**`macro_rules!`**: pattern matching on syntax, expands at compile time. Fragment types: `expr`,
`ty`, `ident`, `pat`, `stmt`, `tt`, `literal`.

When to use macros: variadic arguments, DSLs, conditional code generation, DRY test generation. When
NOT to: a function or generic would work; used only once or twice.

**Procedural macros**: three types -- derive, attribute, function-like. Built with `syn` (parse) +
`quote` (generate) + `proc-macro2` (bridge).

**Hygiene**: `$crate` ensures correct resolution regardless of how users import your crate. Always
use `$crate::` in `#[macro_export]` macros.

**`tt` munching**: recursive macros process input one token at a time.

### Ch 14. Testing and Benchmarking (Fundamentals)

**Three test tiers**: unit tests (same file, `#[cfg(test)]`), integration tests (`tests/` directory,
public API only), doc tests (`///` comments, compiled and run).

Tests can return `Result<(), E>` -- `?` works inside tests.

**Test fixtures**: helper functions for shared setup. RAII cleanup via `Drop` (e.g., `TempDir`).

**Property-based testing** (`proptest`): generates hundreds of random inputs, shrinks failures to
minimal reproducing case. Test _properties_ that should always hold (e.g., "reversing twice is
identity").

**Benchmarking** (`criterion`): statistically rigorous benchmarks with HTML reports. Place in
`benches/`, set `harness = false`.

**Mocking via traits**: define behavior as a trait, inject mock implementations in tests. No
framework needed. The Config trait pattern (Ch 3) lets you swap entire hardware layers with
`TestBoard` vs `ProductionBoard`.

### Ch 15. Crate Architecture and API Design (Intermediate)

#### Module Layout

````text
my_crate/
├── src/
│   ├── lib.rs          # Re-exports and public API
│   ├── config.rs
│   ├── parser/
│   │   ├── mod.rs
│   │   ├── lexer.rs
│   │   └── ast.rs
│   ├── error.rs
│   └── utils.rs        # pub(crate)
├── tests/
├── benches/
└── examples/
```rust

Re-export what users need at the crate root. Users write `use my_crate::Config`, not
`use my_crate::config::Config`.

**Visibility modifiers**: `pub`, `pub(crate)`, `pub(super)`, `pub(in path)`, private (default).

#### Public API Design Checklist

1. Accept references, return owned: `fn process(input: &str) -> String`
2. Use `impl Trait` for parameters: `fn read(r: impl Read)`
3. Return `Result`, not `panic!`
4. Implement standard traits: `Debug`, `Display`, `Clone`, `Default`, `From`/`Into`
5. Make invalid states unrepresentable (type states, newtypes)
6. Builder pattern for complex configuration
7. Seal traits you don't want users to implement
8. Mark types/functions `#[must_use]`

#### `#[must_use]` Guidance

Apply to any type where ignoring the return value is almost certainly a bug:

```rust
#[must_use = "dropping the guard immediately releases the lock"]
pub struct LockGuard<'a, T> { /* ... */ }

#[must_use]
pub fn validate(input: &str) -> Result<ValidInput, ValidationError> { /* ... */ }
````

#### `#[non_exhaustive]`

Mark public enums and structs so adding variants/fields is not a breaking change. Downstream must
use wildcard arm in match, cannot construct with struct literal syntax.

#### Ergonomic Parameter Patterns

**Decision tree**:

````rust
Do you need ownership of the data inside the function?
├── YES → impl Into<T>
│         "Give me anything that can become a T"
└── NO  → Do you only need to read it?
     ├── YES → impl AsRef<T> or &T
     │         "Give me anything I can borrow as a &T"
     └── MAYBE (might need to modify sometimes?)
          └── Cow<'_, T>
              "Borrow if possible, clone only when you must"
```rust

| Pattern             | Ownership | Allocation       | When to use                            |
| ------------------- | --------- | ---------------- | -------------------------------------- |
| `&str`              | Borrowed  | Never            | Simple string params                   |
| `impl AsRef<str>`   | Borrowed  | Never            | Accept String, &str -- read only       |
| `impl Into<String>` | Owned     | On conversion    | Accept &str, String -- will store      |
| `Cow<'_, str>`      | Either    | Only if modified | Processing that usually doesn't modify |

**`Borrow<T>` vs `AsRef<T>`**: `Borrow` additionally guarantees `Eq`/`Ord`/`Hash` consistency. Use
`Borrow` for lookup keys (HashMap); use `AsRef` for general "give me a reference."

#### Parse Don't Validate

Principle: don't check data then pass around the raw form. Parse it into a type that can only exist
if valid. `TryFrom` is the standard tool.

```rust
pub struct Port(u16);

impl TryFrom<u16> for Port {
    type Error = PortError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 0 { Err(PortError::Zero) }
        else { Ok(Port(value)) }
    }
}

// Downstream never re-validates:
fn start_server(port: Port) { /* Port is guaranteed valid */ }
````

Implement `FromStr` for types commonly parsed from text (CLI args, config files). Works with
`.parse()` and `clap`.

**`TryFrom` chaining**: parse raw config into `ValidConfig` where every field is a validated
newtype. Parse once at the boundary, use validated types everywhere inside.

| Approach                      | Compiler enforces validity? | Re-validation needed?   |
| ----------------------------- | --------------------------- | ----------------------- |
| Runtime checks (if/assert)    | No                          | Every function boundary |
| Validated newtype + `TryFrom` | Yes                         | Never                   |

#### Feature Flags and Conditional Compilation

```toml
[features]
default = ["json"]
json = ["dep:serde_json"]    # dep: syntax (Rust 1.60+)
xml = ["dep:quick-xml"]
full = ["json", "xml"]
```

Best practices:

- Keep `default` features minimal
- Use `dep:` syntax for optional dependencies (avoids implicit features)
- Document features in README
- `compile_error!` if no required feature is enabled

**`cfg_attr`**: conditionally apply attributes. E.g.,
`#[cfg_attr(feature = "serde", derive(Serialize))]`.

**Compile-time env vars**: `env!("CARGO_PKG_VERSION")` for guaranteed vars, `option_env!("GIT_SHA")`
for optional. Set custom vars from `build.rs`.

#### Workspace Organization

```toml
[workspace]
members = ["core", "parser", "server", "client", "cli"]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

Benefits: single `Cargo.lock`, shared build cache, `cargo test --workspace`, clean dependency
boundaries.

**`.cargo/config.toml`**: project-level Cargo configuration. Aliases
(`xt = "test --workspace --release"`), default target, custom runners for cross-compilation,
environment variables.

#### Supply Chain Security

`cargo audit` for known CVEs. `cargo deny` for licenses, bans, advisories, sources. Configure in
`deny.toml`.

### Ch 16. Async/Await Essentials (Advanced)

**Key concepts**:

1. A `Future` is a lazy state machine. Calling `async fn` returns a `Future` that must be polled.
2. You need a runtime (`tokio`, `async-std`, `smol`) to poll futures.
3. `async fn` desugars into a state machine implementing `Future`.

**Common pitfalls**:

| Pitfall                                            | Fix                                         |
| -------------------------------------------------- | ------------------------------------------- |
| Blocking in async (`std::thread::sleep`)           | `tokio::task::spawn_blocking`               |
| `Send` bound errors (Rc, MutexGuard across .await) | Drop non-Send values before .await, use Arc |
| Future not polled                                  | Always .await or tokio::spawn               |
| Holding `std::sync::MutexGuard` across .await      | Use `tokio::sync::Mutex`                    |
| Accidental sequential execution                    | Use `tokio::join!` or `tokio::spawn`        |

**`join!` vs `try_join!` vs `select!`**: join waits for all, try_join short-circuits on first Err,
select returns on first completion.

---

## Appendix: Reference Card

### Pattern Decision Guide

````text
Need type safety for primitives?           → Newtype (Ch3)
Need compile-time state enforcement?       → Type-state (Ch3)
Need a "tag" with no runtime data?         → PhantomData (Ch4)
Need to handle "one of N types"?
  ├─ Known closed set                      → Enum
  ├─ Open set, hot path                    → Generics
  ├─ Open set, cold path                   → dyn Trait
  └─ Completely unknown types              → Any + TypeId (Ch2)
Need shared state across threads?
  ├─ Simple counter/flag                   → Atomics
  ├─ Short critical section                → Mutex
  ├─ Read-heavy                            → RwLock
  ├─ Lazy one-time init                    → OnceLock / LazyLock (Ch6)
  └─ Complex state                         → Actor + Channels
Need error handling?
  ├─ Library                               → thiserror
  └─ Application                           → anyhow
```text

### Trait Bounds Cheat Sheet

| Bound               | Meaning                              |
| ------------------- | ------------------------------------ |
| `T: Clone`          | Can be duplicated                    |
| `T: Send`           | Can be moved to another thread       |
| `T: Sync`           | `&T` can be shared between threads   |
| `T: 'static`        | Contains no non-static references    |
| `T: Sized`          | Size known at compile time (default) |
| `T: ?Sized`         | Size may not be known                |
| `T: Unpin`          | Safe to move after pinning           |
| `T: Into<U>`        | Can be converted to U                |
| `T: AsRef<U>`       | Can be borrowed as &U                |
| `F: Fn(A) -> B`     | Callable, borrows immutably          |
| `F: FnMut(A) -> B`  | Callable, may mutate state           |
| `F: FnOnce(A) -> B` | Callable once, may consume state     |

### Lifetime Elision Rules

1. Each reference parameter gets its own lifetime
2. If exactly one input lifetime, it's used for all outputs
3. If one parameter is `&self`/`&mut self`, its lifetime is used for outputs

### Visibility Quick Reference

```rust
pub           → visible everywhere
pub(crate)    → visible within the crate
pub(super)    → visible to parent module
pub(in path)  → visible within a specific path
(nothing)     → private to current module + children
```rust

---

## Key Themes Across the Book

1. **Make invalid states unrepresentable**: newtypes, type-state, `TryFrom`, `#[non_exhaustive]`,

   sealed traits. The type system is your first line of defense.

1. **Parse at the boundary, use validated types inside**: raw data enters, gets parsed via

   `TryFrom`/`FromStr` into validated newtypes. From that point the type system guarantees validity.
   No re-validation downstream.

1. **Accept the most general type, return the most specific**: `impl Into<String>` for owned params,

   `impl AsRef<str>` for borrowed, `Cow` for maybe-modified. Return concrete types.

1. **Zero-cost abstractions are real**: iterator chains compile to the same machine code as loops.

   Newtypes and PhantomData are zero-sized. Type-state markers vanish at runtime.

1. **Config traits tame complexity**: one generic parameter on a struct, regardless of how many

   component types it manages. Adding a component means one associated type, one field.

1. **Prefer proven crates over hand-rolled**: `crossbeam` over custom channels, `parking_lot` over

   custom locks, `pin-project` over manual pin projections, `bumpalo` over custom arenas.

1. **Firmware/hardware domain examples throughout**: IPMI addresses, GPIO pins, SPI/I2C/I3C buses,

   PCIe capability headers, sensor readings, BMC commands. The patterns are grounded in real systems
   engineering.
````
