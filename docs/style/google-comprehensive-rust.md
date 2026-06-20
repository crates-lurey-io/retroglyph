# Google Comprehensive Rust Course

Source: [google.github.io/comprehensive-rust](https://google.github.io/comprehensive-rust/) Repo:
[github.com/google/comprehensive-rust](https://github.com/google/comprehensive-rust) (33k+ stars)
Developed by Google's Android team. Used internally at Google to teach Rust to experienced engineers
(typically C++/Java background). Free, open-source (CC-BY-4.0 for content, Apache-2.0 for code).

---

## 1. Course Structure

The course is **instructor-led, not a self-study book**. It consists of slides with speaker notes,
live-editable code playgrounds, and exercises. Approximately 5 hours of teaching per day.

### Rust Fundamentals (4 days)

Progression is deliberate: familiar concepts first, Rust-unique concepts later.

### Day 1 (Morning ~2h10m, Afternoon ~2h45m)

- Welcome, Hello World, What is Rust?, Benefits of Rust, Playground
- Types and Values: variables, values, arithmetic, type inference
- Control Flow Basics: blocks/scopes, if/match expressions, loops (for, loop),

  break/continue/labels, functions, macros

- Tuples and Arrays: arrays, tuples, array iteration, patterns and destructuring
- References: shared references, exclusive references, slices, strings, reference validity
- User-Defined Types: named structs, tuple structs, enums, type aliases, const, static

### Day 2 (Morning ~2h50m, Afternoon ~2h50m)

- Pattern Matching: irrefutable patterns, matching values, destructuring structs/enums, `if let`,

  `while let`, `let else`

- Methods and Traits: methods, traits, implementing traits, supertraits, associated types, deriving
- Generics: generic functions, trait bounds, generic data types, generic traits, `impl Trait`,

  `dyn Trait`

- Closures: syntax, capturing, closure traits (Fn/FnMut/FnOnce)
- Standard Library Types: Option, Result, String, Vec, HashMap
- Standard Library Traits: Comparisons, Operators, From/Into, Casting, Read/Write, Default

### Day 3 (Morning ~2h20m, Afternoon ~2h30m)

- Memory Management: review of program memory (stack/heap), approaches to memory management (manual,

  GC, RAII/ownership), ownership, move semantics, Clone, Copy types, Drop

- Smart Pointers: `Box<T>`, `Rc`, owned trait objects
- Borrowing: borrowing a value, borrow checking, borrow errors, interior mutability (Cell, RefCell)
- Lifetimes: borrowing and functions, returning borrows, multiple borrows, borrow both/one, lifetime

  elision, lifetimes in data structures

### Day 4 (Morning ~2h50m, Afternoon ~2h20m)

- Iterators: motivation, Iterator trait, helper methods, collect, IntoIterator
- Modules: modules, filesystem hierarchy, visibility, encapsulation, use/super/self
- Testing: unit tests, integration tests, doc tests, compiler lints and Clippy
- Error Handling: panics, Result, try operator, try conversions, Error trait, thiserror, anyhow
- Unsafe Rust: unsafe intro, raw pointers, mutable static variables, unions, unsafe functions

  (Rust + extern), unsafe traits

### Deep Dives (post-fundamentals)

### Rust in Android (half day)

- Build rules (binary, library) using Android.bp
- AIDL (Android Interface Definition Language): full tutorial building a Birthday Service
  - Interface definition, service API/bindings, server, deploy, client, API changes
  - AIDL types: primitives, arrays, sending objects, parcelables, file descriptors
- Testing on Android: GoogleTest, mocking
- Logging
- Interoperability with C/C++ and Java

### Rust in Chromium (half day)

- Setup and gn build system integration
- Bringing in third-party crates
- C++ interoperability via CXX
- CXX error handling (cannot use exceptions in Chromium, so Result<T,E> is handled via

  out-parameters or bool returns)

- Practical exercise: calling Rust from Chromium C++

### Bare-Metal Rust (full day)

- `no_std` Rust: what it means, what you lose
- Microcontrollers (BBC micro:bit v2, nRF52833):
  - PACs, HALs, board support crates
  - GPIO, I2C, interrupts
  - Embedded HAL traits
- Application processors:
  - Bootloader/kernel code
  - MMIO, volatile access
  - UEFI, PSCI, device trees
- Useful crates for bare-metal development

### Concurrency in Rust (full day)

- Morning (~3h20m): Threads (plain + scoped), Channels (senders/receivers, unbounded, bounded), Send

  and Sync (marker traits), Shared State (Arc, Mutex)

- Afternoon (~3h30m): Async Basics (async/await, futures, state machine transformation, runtimes,

  tasks), Channels and Control Flow, Pitfalls (blocking the executor, Pin), Exercises

### Idiomatic Rust (2 days, under active development)

- Foundations of API Design (~3h15m)
- Leveraging the Type System (~7h30m)
- Polymorphism (~3h05m)

### Unsafe Deep Dive (2 days, work in progress)

- Mental model of unsafe Rust
- Reading and writing unsafe code and documentation
- Code review for unsafe Rust
- FFI basics and strategies
- Building data structures the borrow checker normally rejects

---

## 2. Key Teaching Principles and Recommendations

These come from the course's
[STYLE.md](https://github.com/google/comprehensive-rust/blob/main/STYLE.md) and are distinct from
most Rust resources:

### Pedagogical Philosophy

1. **"No Magic" Rule**: Never tell students to accept syntax or behavior that will be explained

   later. For everything on slides, provide a working mental model. This is a core design principle
   not common in other Rust courses.

1. **Spiral Approach**: Introduce concepts with simplified mental models first, revisit with more

   detail later. Example: `println!` is introduced as a macro early (explaining the `!` syntax), but
   format strings and `Debug`/`Display` traits are covered later once traits are taught.

1. **Build on Foundation**: Connect new Rust concepts to what learners know from prior language

   experience or earlier course material. The course assumes C++/Java/Python background and makes
   explicit comparisons.

1. **Compiler Errors as Teaching Tools**: Instructors are expected to run and modify code live,

   using compiler errors to demonstrate concepts. Students are encouraged to interrupt with
   questions.

1. **Familiar First, Unique Later**: Days 1-2 cover concepts with parallels in other languages. Days

   3-4 introduce Rust-unique concepts (ownership, borrowing, lifetimes, unsafe).

### Slide Design Rules

- One core idea per slide
- Meaningful examples (avoid Foo/Bar/Baz, use real-world domain names)
- All code blocks are live, editable playgrounds
- Limited vertical space (slides, not pages; no scrolling)
- Speaker notes provide teaching prompts, not verbatim scripts
- Use bullet points for instructor notes, not narrative paragraphs

### Non-Goals

The course explicitly does **not** cover:

- Macro development (defers to The Rust Book and Rust by Example)
- Being a complete reference (it's a foundation for continued learning on the job)

---

## 3. Error Handling Patterns

The course teaches error handling progressively across ~55 minutes on Day 4.

### Panics

- Panics are for **unrecoverable and unexpected errors** (bugs in the program).
- Runtime failures like bounds checks, failed assertions trigger panics.
- Panics unwind the stack, dropping values as if functions returned.
- `catch_unwind` exists but should not be used to implement exceptions. Useful only for servers that

  need to keep running if a single request crashes.

- Does not work if `panic = 'abort'` in Cargo.toml.
- Prefer non-panicking APIs (e.g., `Vec::get` over indexing) if crashing is unacceptable.

### Result and the ? Operator

- `Result<T, E>` is the primary error handling mechanism.
- You cannot access success or error values without pattern matching, so you can never forget to

  handle an error (unlike C errno or Go error values).

- `unwrap()` exists for quick-and-dirty code, but you can always see in source code where proper

  handling is skipped.

- The `?` operator desugars to `match expr { Ok(v) => v, Err(e) => return Err(From::from(e)) }`,

  with the `From::from` enabling automatic error type conversion.

- `Result::map_err` is a common alternative to `From` implementations for one-off conversions.
- `Option::ok_or` converts Option to Result; `Result::ok` converts Result to Option. Cannot mix `?`

  on Option and Result in the same function.

- `main` can return `Result<(), E>` if `E: Debug`.

### Error Type Design (the progression)

1. **Custom enum errors**: Define error enums, implement `Display`, `Error`, and `From<SourceError>`

   manually.

1. **`Box<dyn Error>`**: Quick and dirty, avoids boilerplate. Not good for public library APIs. Good

   for programs that just display error messages.

1. **`thiserror`**: Derive macros for `Display`, `Error`, and `From`. Recommended for **libraries**

   that need structured error types with minimal boilerplate.

1. **`anyhow`**: Wraps `Box<dyn Error>` with context support. Recommended for **applications**, not

   library public APIs. Provides `.context()` and `.with_context()` for adding semantic traces.
   Supports downcasting.

### Idiomatic Rust Deep Dive on Errors (advanced)

- Distinguish error purpose: **recovery vs. reporting**.
- Determine error scope.
- Capture additional context as errors flow upward across scope boundaries.
- Use the `Error` trait's chain to track full error history.
- Distinguish fatal from recoverable: `Result<Result<T, RecoverableError>, FatalError>`.

---

## 4. Unsafe Rust Guidance

### Fundamentals Module (~1h15m on Day 4)

Core framing: Rust has two parts: Safe Rust (no UB possible) and Unsafe Rust (UB possible if
preconditions violated).

### Key principles taught

- "Unsafe Rust does not mean the code is incorrect. It means developers have turned off some

  compiler safety features and have to write correct code themselves."

- Unsafe code should be **small, isolated, and carefully documented**.
- Unsafe code should be **wrapped in a safe abstraction layer**.

### Five capabilities granted by `unsafe`

1. Dereference raw pointers
2. Access or modify mutable static variables
3. Access `union` fields
4. Call `unsafe` functions (including `extern` functions)
5. Implement `unsafe` traits

### Safety documentation requirements

- Every `unsafe fn` must have a `# Safety` section documenting preconditions.
- Every `unsafe` block must have a `// SAFETY:` comment explaining why the code is actually safe.
- Missing safety comments indicate unsound code.

### Specific guidance on raw pointers

- Creating raw pointers is safe; dereferencing is unsafe.
- Common pitfall example: `slice::from_raw_parts` takes element count, not byte count.

### Unions

- Rarely needed in Rust (enums are the superior alternative).
- Occasionally needed for C library interop.
- For byte reinterpretation, prefer `std::mem::transmute` or the `zerocopy` crate.

### Mutable statics

- Accessing mutable statics is unsafe because of potential data races.
- Use `Mutex` or atomics instead when possible.

**Rust 2024 edition change:** Unsafe operations inside unsafe functions now require explicit
`unsafe` blocks. In older editions, use `#[deny(unsafe_op_in_unsafe_fn)]`.

### Unsafe Deep Dive (2-day course, WIP)

Goals:

- Establish a shared vocabulary for talking about safety
- Mental model of how memory works in unsafe contexts
- Common unsafe patterns
- Code review skills: self-review easy cases, detect difficult cases
- FFI design and implementation

---

## 5. FFI Patterns

### Fundamentals: FFI Wrapper Exercise

The Day 4 exercise has students write a safe Rust wrapper around a C API, practicing the
`extern "C"` declaration pattern.

### `unsafe extern` Blocks (Rust 1.82+)

```rust
unsafe extern "C" {
    safe fn abs(input: i32) -> i32;  // No safety requirements
    unsafe fn strlen(s: *const c_char) -> usize;  // Has preconditions
}
```

- Functions in `extern` blocks must be explicitly marked `safe` or `unsafe`.
- No compiler verification that Rust signature matches C definition.
- The `"C"` is the ABI specification; other ABIs available.

### Language Differences Table (Rust vs C)

| Concern | Rust                                  | C                                                   |
| ------- | ------------------------------------- | --------------------------------------------------- |
| Errors  | `Result<T, E>`, `Option<T>`           | Magic return values, out-parameters, global `errno` |
| Strings | `&str`/`String` (UTF-8, length-known) | Null-terminated `char*`                             |
| Null    | No null pointers in safe Rust         | `NULL` everywhere                                   |
| Memory  | Ownership + borrowing                 | Manual malloc/free                                  |

### Interop Strategies (from Unsafe Deep Dive)

1. **FFI through C ABI** (most common): C is a "lossy codec." Complicated code on both sides, but

   workable.

1. **High-fidelity interop** (experimental): `crubit` (Google) provides glue code for compatible

   types across Rust/C++. `Zngur` imports C++ objects as Rust trait objects via dynamic dispatch.

1. **Distributed systems (RPC)**: Significant serialization overhead. Not good for zero-cost

   environments.

1. **Custom ABI (e.g., WASM)**: Requires a runtime or significant implementation effort.

### Chromium-specific FFI

- Uses CXX for C++ interop.
- CXX's `Result<T, E>` relies on C++ exceptions, which Chromium bans.
- Alternatives: return success values via out-parameters, use `bool` return for fallible operations,

  or return `Result` where `T` is passed via out-parameter.

- PNG decoder example demonstrates handling complex return types across FFI boundary.

### Android-specific FFI

- AIDL (Android Interface Definition Language) for Rust services.
- Interop with C, C++, and Java.
- Build system integration via `Android.bp` files.

---

## 6. Concurrency and Async Patterns

### Classical Concurrency (Morning, ~3h20m)

### Threads

- Plain threads via `std::thread::spawn` (requires `'static` data or `Arc`)
- Scoped threads via `std::thread::scope` (can borrow from parent stack)

### Channels

- `mpsc::channel()` for unbounded (unlimited buffer)
- `mpsc::sync_channel(n)` for bounded (backpressure when buffer full)
- Sender can be cloned (multiple producers), receiver cannot

### Send and Sync (marker traits)

- `Send`: safe to transfer ownership to another thread
- `Sync`: safe to share references between threads (`T: Sync` iff `&T: Send`)
- Auto-implemented based on field types
- `Rc` is neither Send nor Sync; `Arc` is both
- `MutexGuard` is Sync but not Send (on some platforms)

### Shared State

- `Arc<T>` for shared ownership across threads
- `Mutex<T>` wraps data (not code sections, unlike C++/Java)
- Mutex returns a `MutexGuard` via RAII; lock is released on drop
- Rust's type system prevents accessing data without holding the lock

**Key teaching point:** "The same tools that help with 'concurrent' access in a single thread (e.g.,
a called function that might mutate an argument) save us from multi-threading issues." The aliasing
rule (`&` XOR `&mut`) is the same mechanism that prevents data races.

### Async/Await (Afternoon, ~3h30m)

### Core model

- `async fn` returns `impl Future`, does not execute until polled
- `.await` suspends current async function until the future completes (cooperative, non-blocking)
- `async` blocks are like closures that return futures
- Futures are **inert**: they do nothing unless polled by an executor (unlike JS Promises)
- `main` cannot be async without a runtime macro (e.g., `#[tokio::main]`)

### State Machine Transformation

- The compiler transforms async functions into hidden enum types implementing `Future`.
- Each `.await` point becomes a state variant containing all live local variables and the awaited

  sub-future.

- Deeply nested async call stacks produce large compiler-generated Future types.
- Recursive async functions must box the recursive future: `Box::pin(recursive_call()).await`

### Runtimes

- Rust has no built-in runtime. Options: Tokio (most popular, rich ecosystem), smol (lightweight)
- Runtime = reactor (handles I/O events) + executor (polls futures)
- Larger apps (e.g., Fuchsia) often have custom runtimes

**Tasks:**

- `tokio::spawn` creates a new task (lightweight thread equivalent)
- Tasks have a single top-level future
- Concurrency within a task by polling multiple child futures

**Pitfalls covered:**-**Blocking the executor**: Don't call blocking I/O or `thread::sleep` in async code. Use
  `tokio::task::spawn_blocking` for CPU-heavy or blocking work.

- **Pin**: Ensures futures aren't moved in memory after being polled, so internal references remain

  valid across `.await` points.

- **Cancellation safety**: Dropping a future cancels it. Code must be written to handle partial

  execution.

---

## 7. Testing Advice

Testing is covered in ~45 minutes on Day 4.

### Unit Tests

- Use `#[test]` attribute, `#[cfg(test)]` for conditional compilation.
- Put tests in a nested `tests` module inside the same file.
- Unit tests can access private helpers (unlike integration tests).
- Use `assert_eq!`, `assert!`, `assert_ne!`.

### Integration Tests

- Place in `tests/` directory as separate `.rs` files.
- Only access public API of the crate.
- Each file in `tests/` is compiled as a separate crate.

### Doc Tests

- Code blocks in `///` doc comments are compiled and run by `cargo test`.
- Lines prefixed with `#` are hidden from rendered docs but still compiled.
- Serves dual purpose: documentation stays accurate, examples stay working.

### Compiler Lints and Clippy

- Clippy provides extensive lints organized into groups.
- Can enable/deny specific lints per-project or per-function.
- `cargo fix` can auto-apply suggestions.
- Lints are continuously expanded; check Clippy documentation regularly.

### Android-specific Testing

- GoogleTest integration for Rust on Android.
- Mocking support for AIDL service testing.

---

## 8. Ownership and Borrowing Pedagogy

This is the most distinctive aspect of the course's teaching approach. Ownership is deliberately
delayed until Day 3, after students are comfortable with basic syntax and type system.

### Deliberate Ordering

1. **Day 1**: References are introduced as a concept (shared/exclusive), but without ownership

   terminology. Students learn `&T` and `&mut T` as "shared" and "exclusive" references. Reference
   validity (dangling references) is mentioned but not deeply explored.

1. **Day 3 Morning**: Memory management foundation is built from scratch:
   - Review of program memory (stack vs. heap)
   - Survey of approaches: manual (C), garbage collection (Java/Python/Go), Rust's ownership model
   - Ownership rule: every value has exactly one owner; data is freed when owner goes out of scope
   - Move semantics as the default (opposite of C++ which copies by default)
   - Explicit `clone()` required for copies
   - `Copy` trait for types that can be implicitly copied (simple values)
   - `Drop` trait for custom cleanup

1. **Day 3 Afternoon**: Borrowing revisited with full depth:
   - Borrow checking rules: (a) references cannot outlive the value, (b) aliasing rule: multiple `&`

     OR one `&mut`, never both

   - Non-lexical lifetimes explained
   - Borrow errors with concrete examples
   - Interior mutability (Cell, RefCell) as the escape hatch

1. **Day 3 Afternoon**: Lifetimes:
   - Borrowing across function boundaries
   - Returning borrows (requires lifetime annotations)
   - Multiple borrows with different lifetimes
   - Lifetime elision rules
   - Lifetimes in data structures

### Key Teaching Comparisons

- **Move semantics vs. C++**: "Rust makes it harder than C++ to inadvertently create copies by

  making move semantics the default, and by forcing programmers to make clones explicit." C++ copies
  by default with `=`; Rust moves.

- **After move**: In Rust, a moved-from value is completely inaccessible (compile error). In C++,

  `std::move` leaves the value in a "valid but unspecified state" and the programmer can keep using
  it.

- **GC analogy**: "Students familiar with garbage collection implementations will know that a

  garbage collector starts with a set of 'roots' to find all reachable memory. Rust's 'single owner'
  principle is a similar idea."

- **Concurrency connection**: The exclusive reference constraint is explicitly connected to thread

  safety. "Rust uses the exclusive reference constraint to ensure that data races do not occur in
  multi-threaded code."

### Smart Pointer Progression

- `Box<T>`: heap allocation with single owner
- `Rc<T>`: reference counting for shared ownership (single-threaded)
- `Arc<T>`: atomic reference counting for shared ownership (multi-threaded, taught in concurrency)

---

## 9. Unique Insights Not Found in Other Guides

### Course Design Principles (from STYLE.md)

1. **"No Magic" Rule**: Unique to this course. Most Rust tutorials say "we'll explain this later"

   frequently. This course demands a working mental model for everything shown, even if simplified.

1. **Spiral Approach**: Concepts are revisited at increasing depth. References are introduced Day 1,

   ownership Day 3, lifetimes Day 3 afternoon. This is the opposite of the Rust Book's linear
   treatment.

1. **Target Audience Calibration**: Designed for engineers with 2-3 years of C/C++/Java/Python

   experience. Does NOT assume familiarity with functional programming, Swift, or Kotlin. This
   shapes the entire pedagogical approach.

1. **Instructor-Led, Not Self-Study**: All code is live and editable. Instructors are expected to

   break code live to demonstrate compiler errors. Speaker notes contain teaching prompts ("Ask:
   What happens if...?", "Demo: Comment out X to show...").

1. **Time-Bounded Fundamentals**: The 4-day schedule is "completely full, leaving no time slack for

   new topics." Any proposal to add content must include a plan to remove something. This discipline
   is unusual.

### Idiomatic Rust Deep Dive Patterns

These patterns go beyond what most Rust resources cover:

1. **"Parse, Don't Validate"**: Use newtypes to ensure data is validated at construction time. Once

   you have a `ValidatedEmail`, you know it's valid; no need to re-check.

1. **Extension Traits**: Alternative to newtype pattern when you just want additional behavior on

   existing types.

1. **RAII Scope Guards and Drop Bombs**: Using `Drop` not just for cleanup, but to trigger actions

   or enforce invariants. A "drop bomb" panics if dropped without being explicitly defused.

1. **Token Types**: Force users to prove they've performed a specific action by requiring a token

   type as a function parameter. The token can only be obtained by performing the action.

1. **Typestate Pattern**: Encode state machines in the type system. Different states are different

   types, so invalid transitions are compile errors.

1. **Using Borrow Checker for Non-Memory Invariants**: The borrow checker can enforce invariants

   unrelated to memory. Examples: `OwnedFd`/`BorrowedFd` in std, branded types from academic
   research.

1. **Owned/View Type Pairs**: `String`/`&str`, `PathBuf`/`Path`, etc. Don't hide ownership

   requirements. Learn to love `Cow<'_, T>`.

1. **Tree-Structured Ownership**: Structure ownership hierarchies as trees. For circular

   dependencies, use reference counting or indices instead of references.

1. **Fatal vs. Recoverable Errors**: The nested `Result<Result<T, Recoverable>, Fatal>` pattern.

### Chromium-Specific Insights

- CXX's `Result<T,E>` cannot be used in Chromium because it relies on C++ exceptions, which Chromium

  bans. The course teaches workarounds specific to large C++ codebases that disable exceptions.

### Android-Specific Insights

- Full AIDL integration tutorial is unique; no other Rust course covers Android IPC in Rust.
- pKVM firmware is written in bare-metal Rust, demonstrating Rust in Android's security-critical

  firmware.

- DNS over HTTP3 on Android is implemented in Rust.

### Bare-Metal Insights

- Covers both microcontrollers AND application processors (most embedded Rust resources focus on one

  or the other).

- Teaches the embedded HAL trait ecosystem and how PACs/HALs/BSPs layer.
- MMIO and volatile access patterns.

### Concurrency Insight

- Futures are "inert": they do nothing unless polled. This differs from JS Promises and is a common

  source of confusion. The course explicitly addresses this.

- The state machine transformation is shown with a full desugared example, giving students a

  concrete mental model of what the compiler generates.

- Recursive async functions need `Box::pin` due to the recursive Future type, analogous to recursive

  data types needing `Box`.

### Unsafe Conventions

- The course teaches the `// SAFETY:` comment convention as mandatory, not optional.
- Functions that can cause UB but aren't marked `unsafe` are called "unsound." If you use unsafe as

  an optimization, add a benchmark to prove the gain.

- The `unsafe extern` block syntax (Rust 1.82+) with explicit `safe`/`unsafe` marking per function

  is covered, reflecting modern Rust.

---

## 10. Recommended Crates

The course recommends specific crates at various points:

| Crate       | Purpose                                       | Context                  |
| ----------- | --------------------------------------------- | ------------------------ |
| `thiserror` | Derive macros for error types                 | Libraries                |
| `anyhow`    | Ergonomic error handling with context         | Applications             |
| `zerocopy`  | Safe byte reinterpretation                    | Alternative to transmute |
| `tokio`     | Async runtime                                 | Concurrency deep dive    |
| `smol`      | Lightweight async runtime                     | Mentioned as alternative |
| `hyper`     | HTTP library                                  | Tokio ecosystem          |
| `tonic`     | gRPC library                                  | Tokio ecosystem          |
| `cxx`       | C++ interop                                   | Chromium deep dive       |
| `crubit`    | High-fidelity Rust/C++ interop (experimental) | FFI strategies           |

---

## 11. Exercises and Their Design

Exercises are short (10-15 minutes), focused on the immediately preceding material, and have clear
instructions. Notable exercises:

- **Fibonacci** (Day 1): Basic types and loops
- **Collatz Sequence** (Day 1): Control flow
- **Nested Arrays** (Day 1): Array/tuple manipulation
- **Geometry** (Day 1): References and slices
- **Elevator Events** (Day 1): Enums and structs
- **Expression Evaluation** (Day 2): Pattern matching on recursive enum (AST evaluation)
- **Generic Logger** (Day 2): Traits and generics
- **Counter** (Day 2): HashMap usage
- **ROT13** (Day 2): Implementing Read/Write traits
- **Builder Type** (Day 3): Ownership and move semantics
- **Binary Tree** (Day 3): Box and recursive data structures
- **Wizard's Inventory** (Day 3): Borrowing rules
- **Protobuf Parsing** (Day 3): Lifetimes in data structures
- **Iterator Method Chaining** (Day 4): Iterator combinators
- **GUI Library Modules** (Day 4): Module system and visibility
- **Luhn Algorithm** (Day 4): Testing (write tests for a validator)
- **Rewriting with Result** (Day 4): Convert panicking code to Result
- **FFI Wrapper** (Day 4): Write safe wrapper around C API

---

## Sources

- [Course main page](https://google.github.io/comprehensive-rust/)
- [Course structure](https://google.github.io/comprehensive-rust/running-the-course/course-structure.html)
- [STYLE.md (design principles)](https://github.com/google/comprehensive-rust/blob/main/STYLE.md)
- [Error handling section](https://google.github.io/comprehensive-rust/error-handling.html) and

  sub-pages

- [Unsafe Rust section](https://google.github.io/comprehensive-rust/unsafe-rust.html) and sub-pages
- [Concurrency section](https://google.github.io/comprehensive-rust/concurrency/welcome.html) and

  sub-pages

- [Idiomatic Rust deep dive](https://google.github.io/comprehensive-rust/idiomatic/welcome.html)
- [Unsafe deep dive](https://google.github.io/comprehensive-rust/unsafe-deep-dive/welcome.html)
- [FFI strategies](https://google.github.io/comprehensive-rust/unsafe-deep-dive/ffi/strategies.html)
- [Memory management](https://google.github.io/comprehensive-rust/memory-management.html) and

  sub-pages

- [Borrowing](https://google.github.io/comprehensive-rust/borrowing.html) and sub-pages
- [Testing](https://google.github.io/comprehensive-rust/testing.html) and sub-pages
- [Google Security Blog: Scaling Rust Adoption Through Training](https://security.googleblog.com/2023/09/scaling-rust-adoption-through-training.html)
