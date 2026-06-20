# Rust Design Patterns

Comprehensive reference derived from the
[Rust Design Patterns](https://rust-unofficial.github.io/patterns/) book (rust-unofficial). Covers
all idioms, design patterns, anti-patterns, and functional patterns catalogued in the book.

Rust is not object-oriented, and the combination of functional elements, a strong type system, and
the borrow checker makes its design patterns distinct from traditional OO languages.

---

## 1. Idioms

Idioms are community-agreed coding guidelines. Break them only with good reason.

### 1.1 Use Borrowed Types for Arguments

**Principle**: prefer `&str` over `&String`, `&[T]` over `&Vec<T>`, `&T` over `&Box<T>` in function
parameters.

Using the borrowed type avoids unnecessary layers of indirection and allows the function to accept
more input types via deref coercion. A function taking `&str` accepts both `&String` (coerced) and
string literals (`&'static str`), while `&String` only accepts owned strings.

```rust
// Good: accepts &String, &str, string literals, slices from split(), etc.
fn three_vowels(word: &str) -> bool { /* ... */ }

// Bad: only accepts &String
fn three_vowels(word: &String) -> bool { /* ... */ }
```

Similarly, `&[T]` accepts `&Vec<T>`, arrays, and arbitrary slices.

[Source](https://rust-unofficial.github.io/patterns/idioms/coercion-arguments.html)

### 1.2 Concatenating Strings with `format!`

Prefer `format!("Hello {name}!")` over manual `push_str`/`+` chains for readability. `format!` is
the most succinct and readable way to combine strings, especially when mixing literal and
non-literal parts.

Trade-off: `format!` is not the most efficient approach. A series of `push` operations on a
pre-allocated mutable `String` is faster for hot paths.

```rust
fn say_hello(name: &str) -> String {
    format!("Hello {name}!")
}
```

[Source](https://rust-unofficial.github.io/patterns/idioms/concat-format.html)

### 1.3 Constructors

Rust has no language-level constructors. The convention is an associated function called `new`:

```rust
pub struct Second { value: u64 }

impl Second {
    pub fn new(value: u64) -> Self {
        Self { value }
    }
}
```

Types should implement both `Default` and `new` when a no-argument constructor makes sense. `new` is
the idiomatic convention users expect; `Default` enables generic usage (e.g.,
`unwrap_or_default()`).

[Source](https://rust-unofficial.github.io/patterns/idioms/ctor.html)

### 1.4 The `Default` Trait

`Default` provides a generic "zero value" constructor. Unlike `new`, it can be derived automatically
when all fields implement `Default`, and it enables struct update syntax:

```rust
#[derive(Default, Debug, PartialEq)]
struct MyConfiguration {
    output: Option<PathBuf>,      // defaults to None
    search_path: Vec<PathBuf>,    // defaults to empty vec
    timeout: Duration,            // defaults to zero
    check: bool,                  // defaults to false
}

// Partial initialization with defaults
let conf = MyConfiguration {
    check: true,
    ..Default::default()
};
```

The more types implement `Default`, the more useful it becomes across generic APIs.

[Source](https://rust-unofficial.github.io/patterns/idioms/default.html)

### 1.5 Collections Are Smart Pointers (Deref Idiom)

Use the `Deref` trait to treat collections like smart pointers, offering both owning and borrowed
views of data. `Vec<T>` implements `Deref<Target=[T]>`, and `String` implements `Deref<Target=str>`.

This means most methods can be implemented only on the borrowed view (the slice), and they become
implicitly available on the owning type. Clients choose between borrowing or taking ownership.

This is distinct from the _Deref polymorphism anti-pattern_ (section 3.3): here, `Deref` converts
from an owning container to its borrowed view, not between unrelated types.

[Source](https://rust-unofficial.github.io/patterns/idioms/deref.html)

### 1.6 Finalisation in Destructors

Rust has no `finally` blocks. Instead, use an object's `Drop` implementation to run cleanup code on
exit, regardless of how a function returns (early return, `?` operator, panic unwinding):

```rust
struct Foo;
impl Drop for Foo {
    fn drop(&mut self) {
        println!("exit");
    }
}

fn bar() -> Result<(), ()> {
    let _exit = Foo;  // destructor runs on any exit path
    baz()?;
    Ok(())
}
```

Caveats:

- Destructors are not guaranteed to run (infinite loops, double panics, process abort).
- The finalizer variable name must start with `_` (not bare `_`, which drops immediately).
- The finalizer must be a value or uniquely owned pointer, not a shared pointer like `Rc`.

[Source](https://rust-unofficial.github.io/patterns/idioms/dtor-finally.html)

### 1.7 `mem::{take, replace}` for Owned Values in Changed Enums

When you have a `&mut MyEnum` and need to move owned data between variants, use `mem::take`
(replaces with `Default`) or `mem::replace` (replaces with a specified value) to avoid cloning:

```rust
use std::mem;

enum MyEnum {
    A { name: String, x: u8 },
    B { name: String },
}

fn a_to_b(e: &mut MyEnum) {
    if let MyEnum::A { name, x: 0 } = e {
        *e = MyEnum::B {
            name: mem::take(name), // moves name out, leaves empty String
        }
    }
}
```

This avoids the "clone to satisfy the borrow checker" anti-pattern. `mem::take` works when the type
implements `Default`. For `Option`, use the built-in `.take()` method instead.

[Source](https://rust-unofficial.github.io/patterns/idioms/mem-replace.html)

### 1.8 On-Stack Dynamic Dispatch

Use `&mut dyn Trait` references to achieve dynamic dispatch without heap allocation:

```rust
use std::io;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = "-";
    let readable: &mut dyn io::Read = if arg == "-" {
        &mut io::stdin()
    } else {
        &mut fs::File::open(arg)?
    };
    // Read from `readable` here
    Ok(())
}
```

Since Rust 1.79.0, the compiler automatically extends temporary lifetimes within `&` or `&mut`,
making this pattern clean without deferred initialization tricks. The alternative `Box<dyn Trait>`
works but requires a heap allocation.

[Source](https://rust-unofficial.github.io/patterns/idioms/on-stack-dyn-dispatch.html)

### 1.9 FFI Error Handling

Three strategies for exposing Rust errors through FFI:

1. **Flat enums** - convert to integer codes directly

   (`enum DatabaseError { IsReadOnly = 1, IOError = 2, ... }`).

1. **Structured enums** - convert to integer code plus a string error message accessible via a

   separate C function.

1. **Custom error types** - create `#[repr(C)]` mirror structs that are transparent to C callers.

[Source](https://rust-unofficial.github.io/patterns/idioms/ffi/errors.html)

### 1.10 FFI: Accepting Strings

When receiving C strings, keep them borrowed rather than copying. Use `CStr::from_ptr()` to convert
to `&CStr`, then `.to_str()` to get `&str`. This is zero-cost and minimizes unsafe code:

```rust
pub unsafe extern "C" fn mylib_log(msg: *const libc::c_char, level: libc::c_int) {
    let msg_str: &str = match std::ffi::CStr::from_ptr(msg).to_str() {
        Ok(s) => s,
        Err(e) => { crate::log_error("FFI string conversion failed"); return; }
    };
    crate::log(msg_str, level);
}
```

Avoid manual `strlen` + `copy_nonoverlapping` approaches, which are verbose and prone to off-by-one
bugs with null terminators.

[Source](https://rust-unofficial.github.io/patterns/idioms/ffi/accepting-strings.html)

### 1.11 FFI: Passing Strings

When sending strings to C functions:

1. Maximize the lifetime of owned `CString` values (don't create temporaries).
2. Minimize `unsafe` blocks.
3. Use `Vec` instead of `CString` if the C code may modify the string.
4. Don't transfer ownership unless the foreign API requires it.

Common mistake: creating a temporary `CString` inside an `unsafe` block, which produces a dangling
pointer because the `CString` is dropped at the end of the expression.

```rust
// WRONG: dangling pointer!
unsafe { seterr(CString::new(err)?.as_ptr()); }

// RIGHT: CString lives long enough
let c_err = CString::new(err)?;
unsafe { seterr(c_err.as_ptr()); }
```

[Source](https://rust-unofficial.github.io/patterns/idioms/ffi/passing-strings.html)

### 1.12 Iterating over an `Option`

`Option` implements `IntoIterator`, yielding zero or one elements. This enables composing it with
iterator adapters:

```rust
let turing = Some("Turing");
let mut logicians = vec!["Curry", "Kleene", "Markov"];

logicians.extend(turing);                              // append if Some
logicians.iter().chain(turing.iter());                 // chain into iteration
```

For always-`Some` values, prefer `std::iter::once()` for clarity. `Iterator::filter_map` is the
specialized version of `map` for `Option`-returning functions.

[Source](https://rust-unofficial.github.io/patterns/idioms/option-iter.html)

### 1.13 Pass Variables to Closures

Use a scoped block to prepare variables for a `move` closure, keeping clone/borrow operations
co-located with the closure definition:

```rust
let closure = {
    let num2 = num2.clone();    // cloned
    let num3 = num3.as_ref();   // borrowed
    move || {
        *num1 + *num2 + *num3;  // num1 is moved
    }
};
```

This is preferable to creating `_cloned` variables in the outer scope because the intent is clearer
and the cloned values are dropped immediately if not consumed.

[Source](https://rust-unofficial.github.io/patterns/idioms/pass-var-to-closure.html)

### 1.14 Privacy for Extensibility (`#[non_exhaustive]`)

Use `#[non_exhaustive]` on structs and enums to allow adding fields/variants without breaking
downstream code across crate boundaries:

```rust
#[non_exhaustive]
pub struct S {
    pub foo: i32,
}

#[non_exhaustive]
pub enum AdmitMoreVariants {
    VariantA,
    VariantB,
}
```

This prevents external code from constructing the struct directly or exhaustively matching the enum.
Within the same crate, the attribute has no effect; use a private field (`_b: ()`) instead.

Use deliberately: incrementing the major version when adding fields is often a better option.
`#[non_exhaustive]` makes downstream code less ergonomic (wildcard arms, no direct construction).

[Source](https://rust-unofficial.github.io/patterns/idioms/priv-extend.html)

### 1.15 Easy Doc Initialization

When a struct requires complex setup in doc examples, wrap the example in a helper function to avoid
repeating boilerplate:

````rust
/// Sends a request over the connection.
///
/// # Example
/// ```
/// # fn call_send(connection: Connection, request: Request) {
/// let response = connection.send_request(request);
/// assert!(response.is_ok());
/// # }
/// ```
fn send_request(&self, request: Request) -> Result<Status, SendErr> { /* ... */ }
````

The hidden function avoids `no_run` annotations while keeping examples concise. The code compiles
but assertions inside the function are not executed during `cargo test`.

[Source](https://rust-unofficial.github.io/patterns/idioms/rustdoc-init.html)

### 1.16 Temporary Mutability

When data must be mutable during setup but immutable afterward, make the intent explicit via
rebinding or a nested block:

```rust
// Nested block approach
let data = {
    let mut data = get_vec();
    data.sort();
    data
};
// `data` is now immutable

// Rebinding approach
let mut data = get_vec();
data.sort();
let data = data;
// `data` is now immutable
```

The compiler then enforces that no accidental mutation occurs after initialization.

[Source](https://rust-unofficial.github.io/patterns/idioms/temporary-mutability.html)

### 1.17 Return Consumed Argument on Error

When a fallible function consumes (moves) an argument, return it inside the error type so the caller
can retry or recover without cloning:

```rust
pub fn send(value: String) -> Result<(), SendError> { /* ... */ }
pub struct SendError(String);  // wraps the original value

// Caller can recover the value on failure:
value = match send(value) {
    Ok(()) => break,
    Err(SendError(v)) => v,  // get it back
};
```

The standard library uses this pattern in `String::from_utf8`, where `FromUtf8Error` lets you
recover the original `Vec<u8>` via `into_bytes()`.

[Source](https://rust-unofficial.github.io/patterns/idioms/return-consumed-arg-on-error.html)

---

## 2. Design Patterns

Reusable solutions to common problems. Rust's unique features (traits, borrow checker, ownership)
mean many traditional OO patterns are either unnecessary or take a different form.

### 2.1 Behavioural Patterns

#### 2.1.1 Command

Encapsulate actions as objects that can be executed, stored, queued, and undone. Three Rust
approaches:

**Trait objects** (best for complex commands with state):

```rust
pub trait Migration {
    fn execute(&self) -> &str;
    fn rollback(&self) -> &str;
}

struct Schema {
    commands: Vec<Box<dyn Migration>>,
}
```

**Function pointers** (lightweight, static dispatch):

```rust
type FnPtr = fn() -> String;
struct Command { execute: FnPtr, rollback: FnPtr }
```

**`Fn` trait objects** (closures, most flexible):

```rust
type Migration<'a> = Box<dyn Fn() -> &'a str>;
```

Trade-off: function pointers give static dispatch (faster), trait objects give dynamic dispatch
(more flexible). Use trait objects when commands are complex structs; use closures for simple,
inline actions.

[Source](https://rust-unofficial.github.io/patterns/patterns/behavioural/command.html)

#### 2.1.2 Interpreter

Express recurring problem instances in a simple language and solve them with an interpreter. In
Rust, this maps naturally to recursive descent parsers and, at a higher level, to `macro_rules!`:

```rust
macro_rules! norm {
    ($($element:expr),*) => {{
        let mut n = 0.0;
        $( n += ($element as f64) * ($element as f64); )*
        n.sqrt()
    }};
}

assert_eq!(5f64, norm!(3.0, 4.0));
```

The pattern is not just about formal grammars; it is about expressing problem instances in a
domain-specific way and implementing interpreters for them.

[Source](https://rust-unofficial.github.io/patterns/patterns/behavioural/interpreter.html)

#### 2.1.3 Newtype

Wrap a type in a single-field tuple struct to create a distinct type with custom behavior:

```rust
struct Password(String);

impl Display for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "****************")
    }
}
```

Use cases:

- **Type safety**: `Miles(f64)` vs `Kilometres(f64)` are distinct types.
- **Custom trait implementations**: override `Display`, `Serialize`, etc.
- **Restricting functionality**: expose only a subset of the inner type's API.
- **Hiding implementation**: `pub struct Foo(Bar<T1, T2>)` hides `Bar`, `T1`, `T2`.

Properties: zero-cost abstraction (no runtime overhead), not type-compatible with the wrapped type
(unlike `type` aliases). Downside: significant boilerplate for pass-through methods and trait impls.
The `derive_more` crate helps.

[Source](https://rust-unofficial.github.io/patterns/patterns/behavioural/newtype.html)

#### 2.1.4 RAII Guards

Resource Acquisition Is Initialization. Create guard objects that acquire a resource in the
constructor and release it in the destructor, using the type system to mediate all access:

```rust
struct MutexGuard<'a, T: 'a> {
    data: &'a T,
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) { /* unlock mutex */ }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T { self.data }
}
```

The borrow checker ensures references obtained through the guard cannot outlive the guard itself,
statically preventing use-after-free. The `Deref` implementation makes the guard ergonomic to use
(behaves like a pointer to the protected data).

Standard library examples: `MutexGuard`, `RwLockReadGuard`, `RefMut`.

[Source](https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html)

#### 2.1.5 Strategy (aka Policy)

Separate algorithm implementations from the algorithm's skeleton using traits or closures.

**Trait-based** (static dispatch, extensible):

```rust
trait Formatter {
    fn format(&self, data: &Data, buf: &mut String);
}

struct Report;
impl Report {
    fn generate<T: Formatter>(g: T, s: &mut String) { /* ... */ }
}
```

**Closure-based** (lightweight, inline):

```rust
struct Adder;
impl Adder {
    pub fn add<F: Fn(u8, u8) -> u8>(x: u8, y: u8, f: F) -> u8 { f(x, y) }
}

let arith_adder = |x, y| x + y;
assert_eq!(9, Adder::add(4, 5, arith_adder));
```

Serde is a canonical real-world example: format-specific serialization strategies implement
`Serialize`/`Deserialize` traits, allowing `serde_json` and `serde_cbor` to be swapped freely.

In Rust, the strategy pattern is often "invisible" because traits are the natural way to express
polymorphic behavior.

[Source](https://rust-unofficial.github.io/patterns/patterns/behavioural/strategy.html)

#### 2.1.6 Visitor

Encapsulate algorithms that operate over heterogeneous collections (e.g., AST nodes) without
modifying the data:

```rust
mod visit {
    pub trait Visitor<T> {
        fn visit_name(&mut self, n: &Name) -> T;
        fn visit_stmt(&mut self, s: &Stmt) -> T;
        fn visit_expr(&mut self, e: &Expr) -> T;
    }
}
```

Idiomatic Rust provides `walk_*` functions for shared traversal logic (instead of `accept` methods
common in OO languages). The visitor can be stateful, communicating information between nodes.
Multiple visitors (interpreter, type checker, optimizer) can be written over the same data without
modifying it.

Related: the Fold pattern (section 2.2.2) is similar but produces a new version of the data
structure.

[Source](https://rust-unofficial.github.io/patterns/patterns/behavioural/visitor.html)

### 2.2 Creational Patterns

#### 2.2.1 Builder

Construct complex objects step by step. Especially useful in Rust because the language lacks
function overloading and default parameter values:

```rust
pub struct Foo { bar: String }

impl Foo {
    pub fn builder() -> FooBuilder { FooBuilder::default() }
}

#[derive(Default)]
pub struct FooBuilder { bar: String }

impl FooBuilder {
    pub fn name(mut self, bar: String) -> FooBuilder {
        self.bar = bar;
        self
    }
    pub fn build(self) -> Foo {
        Foo { bar: self.bar }
    }
}

// Usage: Foo::builder().name("Y".into()).build()
```

The builder can take and return by value (chaining) or by mutable reference (reusable template).
`std::process::Command` is a real-world builder for `Child` processes. The `derive_builder` crate
automates the boilerplate.

[Source](https://rust-unofficial.github.io/patterns/patterns/creational/builder.html)

#### 2.2.2 Fold

Run an algorithm over each node in a data structure to produce a new data structure of the same (or
similar) shape. Common in compilers (AST transformations):

```rust
pub trait Folder {
    fn fold_name(&mut self, n: Box<Name>) -> Box<Name> { n }
    fn fold_stmt(&mut self, s: Box<Stmt>) -> Box<Stmt> {
        match *s {
            Stmt::Expr(e) => Box::new(Stmt::Expr(self.fold_expr(e))),
            Stmt::Let(n, e) => Box::new(Stmt::Let(self.fold_name(n), self.fold_expr(e))),
        }
    }
    // ...
}

// Concrete folder: rename everything to "foo"
struct Renamer;
impl Folder for Renamer {
    fn fold_name(&mut self, _n: Box<Name>) -> Box<Name> {
        Box::new(Name { value: "foo".to_owned() })
    }
}
```

Like the visitor, fold separates traversal from operation. Unlike the visitor, fold consumes the old
data and produces new data (functional style). Trade-offs exist between `Box` (exclusive ownership,
efficient reuse of unchanged nodes), borrowed references (preserves original, requires cloning), and
`Rc` (best of both, less ergonomic).

[Source](https://rust-unofficial.github.io/patterns/patterns/creational/fold.html)

### 2.3 Structural Patterns

#### 2.3.1 Compose Structs (Struct Decomposition)

When a large struct causes borrow checker issues (can't borrow one field mutably while another is
borrowed immutably), decompose it into smaller structs:

```rust
// Problem: can't borrow connection_string mutably while passing &db to print_database
struct Database {
    connection_string: String,
    timeout: u32,
    pool_size: u32,
}

// Solution: decompose into smaller types
struct ConnectionString(String);
struct Timeout(u32);
struct PoolSize(u32);

struct Database {
    connection_string: ConnectionString,
    timeout: Timeout,
    pool_size: PoolSize,
}
```

The borrow checker can borrow struct fields independently (`a.b` and `a.c` are distinct), so after
decomposition, functions can take individual fields rather than the whole struct.

[Source](https://rust-unofficial.github.io/patterns/patterns/structural/compose-structs.html)

#### 2.3.2 Prefer Small Crates

Cargo and crates.io make fine-grained dependencies practical. Small crates are:

- Easier to understand, encouraging modular code.
- Reusable across projects (e.g., `url` crate originated from Servo).
- Parallelizable in compilation (Rust's compilation unit is the crate).

Downsides: dependency hell (conflicting versions), uncurated ecosystem (malicious/poorly written
crates), less optimization without LTO.

[Source](https://rust-unofficial.github.io/patterns/patterns/structural/small-crates.html)

#### 2.3.3 Contain Unsafety in Small Modules

Isolate `unsafe` code in the smallest possible module that upholds the needed invariants, then
expose a safe interface:

- The inner module handles raw operations with `unsafe`.
- The outer module provides ergonomic, safe wrappers.
- Users may optionally call `unsafe` functions directly for performance.

Example: `String` wraps `Vec<u8>` with the UTF-8 invariant. Safe methods enforce the invariant;
`from_utf8_unchecked` is the opt-in unsafe escape hatch.

[Source](https://rust-unofficial.github.io/patterns/patterns/structural/unsafe-mods.html)

#### 2.3.4 Custom Traits to Avoid Complex Type Bounds

When trait bounds become unwieldy (especially with `Fn` traits and associated output types),
introduce a custom trait with a blanket implementation:

```rust
// Before: verbose, with explicit type parameter T
struct Value<G: FnMut() -> Result<T, Error>, S: Fn(&T) -> Status, T: Display> { /* ... */ }

// After: clean, T is an associated type
trait Getter {
    type Output: Display;
    fn get_value(&mut self) -> Result<Self::Output, Error>;
}

impl<F: FnMut() -> Result<T, Error>, T: Display> Getter for F {
    type Output = T;
    fn get_value(&mut self) -> Result<Self::Output, Error> { self() }
}

struct Value<G: Getter, S: Fn(&G::Output) -> Status> { /* ... */ }
```

This eliminates a type parameter, makes bounds more expressive, and opens opportunities for
additional methods and specialized implementations on the new trait.

_Added 2025-12-14._

[Source](https://rust-unofficial.github.io/patterns/patterns/structural/trait-for-bounds.html)

### 2.4 FFI Patterns

#### 2.4.1 Object-Based APIs

When designing FFI APIs, follow these principles:

1. **Encapsulated types** are owned by Rust, managed by the user, and opaque (user gets a pointer,

   never sees the layout).

1. **Transactional types** are owned by the user and transparent (`#[repr(C)]` structs).
1. All behavior is functions acting on encapsulated types.3. Behavior is grouped by provenance/lifetime, not structure.

Key insight: consolidate the iterator's lifetime with its parent object. The POSIX DBM API does this
with `dbm_firstkey(DBM*)` / `dbm_nextkey(DBM*)` instead of exposing a separate iterator type,
avoiding use-after-free footguns in C.

[Source](https://rust-unofficial.github.io/patterns/patterns/ffi/export.html)

#### 2.4.2 Type Consolidation into Wrappers

Wrap multiple related Rust types (e.g., a collection and its iterator) into a single opaque
"wrapper" struct for FFI export. This avoids exposing lifetime relationships that C cannot express:

```rust
struct MySetWrapper {
    myset: MySet,
    iter_next: usize,
}

impl MySetWrapper {
    pub fn first_key(&mut self) -> Option<&Key> {
        self.iter_next = 0;
        self.next_key()
    }
    pub fn next_key(&mut self) -> Option<&Key> {
        self.myset.keys().nth(self.iter_next).map(|k| { self.iter_next += 1; k })
    }
}
```

Attempting to expose Rust iterators directly through FFI creates aliasing violations (mutable
reference to the collection + shared reference in the iterator = undefined behavior). The wrapper
avoids this entirely.

[Source](https://rust-unofficial.github.io/patterns/patterns/ffi/wrappers.html)

---

## 3. Anti-Patterns

Solutions that cause more problems than they solve.

### 3.1 Clone to Satisfy the Borrow Checker

**The problem**: Using `.clone()` to make borrow checker errors disappear, without understanding the
consequences.

```rust
let mut x = 5;
let y = &mut (x.clone());  // clones just to avoid borrow conflict
println!("{x}");
*y += 1;                    // mutation is lost, y points to the clone
```

Cloning creates a completely independent copy. Changes to the clone are not synchronized with the
original. This is often a sign that the code's ownership model is wrong.

**When cloning is acceptable**:

- The developer is still learning ownership.
- Performance/memory constraints are not critical (prototypes, hackathons).
- The borrow checker issue is genuinely complex and readability matters more.

**Alternatives**: `mem::take`/`mem::replace` (section 1.7), restructuring code to avoid overlapping
borrows, using `Rc<T>`/`Arc<T>` for shared ownership. Always run `cargo clippy` to detect
unnecessary clones.

[Source](https://rust-unofficial.github.io/patterns/anti_patterns/borrow_clone.html)

### 3.2 `#![deny(warnings)]`

**The problem**: Adding `#![deny(warnings)]` to crate roots opts out of Rust's stability guarantees.
New compiler versions may introduce new warnings (e.g., for newly deprecated APIs or newly detected
issues), causing previously working builds to fail.

This also breaks compatibility with additional lint tools like `clippy` unless the annotation is
removed.

**Alternatives**:

1. **CI-only**: `RUSTFLAGS="-D warnings" cargo build` in CI, not in source code.
2. **Named lints**: `#![deny(dead_code, unused, ...)]` with specific lint names instead of the

   blanket `warnings`. Avoid including `deprecated` in the list.

[Source](https://rust-unofficial.github.io/patterns/anti_patterns/deny-warnings.html)

### 3.3 Deref Polymorphism

**The problem**: Misusing `Deref` to emulate OO inheritance between unrelated structs:

```rust
struct Foo {}
impl Foo { fn m(&self) { /* ... */ } }

struct Bar { f: Foo }
impl Deref for Bar {
    type Target = Foo;
    fn deref(&self) -> &Foo { &self.f }
}

// Now bar.m() works via auto-deref, mimicking inheritance
```

This is an anti-pattern because:

- It's surprising; `Deref` is designed for smart pointers, not type conversion.
- It doesn't create a subtyping relationship; traits on `Foo` are not automatically on `Bar`.
- It interacts badly with generic bounds and trait resolution.
- It only supports single "inheritance" with no notion of interfaces or privacy.
- `self` refers to the "parent" type, not the "child", differing from OO semantics.

**Alternatives**: implement traits, write explicit delegation methods, or use delegation crates like
`delegate` or `ambassador`.

[Source](https://rust-unofficial.github.io/patterns/anti_patterns/deref.html)

---

## 4. Functional Patterns

Rust is imperative but incorporates many functional programming paradigms.

### 4.1 Programming Paradigms

Imperative code describes _how_ to do something (step-by-step mutations). Declarative/functional
code describes _what_ to do (composing functions):

```rust
// Imperative
let mut sum = 0;
for i in 1..11 { sum += i; }

// Declarative
let sum = (1..11).fold(0, |a, b| a + b);
```

Both are valid Rust. The functional style is often more concise and easier to reason about, since it
avoids mutable state.

[Source](https://rust-unofficial.github.io/patterns/functional/paradigms.html)

### 4.2 Generics as Type Classes

Rust's generics create what functional languages call "type class constraints." Each different type
parameter creates a genuinely different type (monomorphization), unlike C++ templates which are
syntactic code duplication.

This enables powerful compile-time API design. Protocol-specific behavior can be encoded in the type
system:

```rust
struct FileDownloadRequest<P: ProtoKind> {
    file_name: PathBuf,
    protocol: P,
}

// Methods available only for NFS requests
impl FileDownloadRequest<Nfs> {
    fn mount_point(&self) -> &Path {
        self.protocol.mount_point()
    }
}

// Calling mount_point() on a Bootp request is a compile-time error
```

This pattern (also called "type state") is used extensively in:

- `embedded-hal` (pin modes with compile-time verified configurations).
- `hyper` (different connector types expose different methods).
- The standard library (`Vec<u8>` has methods other `Vec<T>` types don't).

[Source](https://rust-unofficial.github.io/patterns/functional/generics-type-classes.html)

### 4.3 Functional Optics (Lenses and Prisms)

Optics are a functional programming concept for composable data access patterns. In Rust, they
appear most prominently in Serde's design.

**The Iso**: a pair of functions that convert between two types (serialize/deserialize).

**The Poly Iso**: a generic Iso, like `FromStr` and `ToString` which work across many types.

**The Prism**: adds a second generic parameter (format), enabling composition of multiple
serialization formats with multiple data types. This is what Serde achieves:

1. Types implement `Serialize`/`Deserialize` (the "top layer").
2. A `Visitor` bridges between the data model and the type's structure (usually derive-macro

   generated).

3. Format-specific `Serializer`/`Deserializer` implementations handle the "bottom layer" (JSON,
   CBOR, etc.).

Each layer composes independently: any type works with any format, via the visitor as an
intermediary. This is why Serde's API involves multiple levels of associated types and generic
parameters; it achieves a Prism through indirection and type erasure.

[Source](https://rust-unofficial.github.io/patterns/functional/optics.html)

---

## 5. Quick Reference Table

| Category         | Pattern                       | Key Idea                                   |
| ---------------- | ----------------------------- | ------------------------------------------ |
| **Idiom**        | Borrowed types for args       | `&str` not `&String`                       |
| **Idiom**        | `format!` concatenation       | Readable string building                   |
| **Idiom**        | Constructor (`new`)           | Associated function convention             |
| **Idiom**        | `Default` trait               | Generic zero-value construction            |
| **Idiom**        | Collections as smart pointers | `Deref` for owning/borrowed views          |
| **Idiom**        | Finalisation in destructors   | `Drop` as `finally`                        |
| **Idiom**        | `mem::take`/`replace`         | Move owned data without cloning            |
| **Idiom**        | On-stack dynamic dispatch     | `&mut dyn Trait` without heap              |
| **Idiom**        | FFI errors                    | Flat enums, structured enums, `#[repr(C)]` |
| **Idiom**        | FFI accepting strings         | `CStr::from_ptr` for zero-cost             |
| **Idiom**        | FFI passing strings           | `CString` lifetime management              |
| **Idiom**        | Iterating over `Option`       | `Option` as zero/one-element iterator      |
| **Idiom**        | Pass vars to closure          | Scoped block for clone/borrow prep         |
| **Idiom**        | Privacy for extensibility     | `#[non_exhaustive]`                        |
| **Idiom**        | Easy doc init                 | Hidden helper functions in examples        |
| **Idiom**        | Temporary mutability          | Rebind `let mut` as `let`                  |
| **Idiom**        | Return consumed arg on error  | Include the value in the error type        |
| **Pattern**      | Command                       | Actions as objects (trait/fn/closure)      |
| **Pattern**      | Interpreter                   | DSLs, recursive descent, `macro_rules!`    |
| **Pattern**      | Newtype                       | Tuple struct wrapper for type safety       |
| **Pattern**      | RAII Guards                   | Resource lifecycle via `Drop` + `Deref`    |
| **Pattern**      | Strategy                      | Trait/closure-based algorithm selection    |
| **Pattern**      | Visitor                       | Algorithms over heterogeneous data         |
| **Pattern**      | Builder                       | Step-by-step complex object construction   |
| **Pattern**      | Fold                          | Transform data structures node by node     |
| **Pattern**      | Compose structs               | Decompose for independent borrowing        |
| **Pattern**      | Small crates                  | Fine-grained, reusable dependencies        |
| **Pattern**      | Contain unsafe                | Minimal unsafe modules, safe wrappers      |
| **Pattern**      | Custom traits for bounds      | Simplify complex `Fn` trait bounds         |
| **Pattern**      | Object-based FFI API          | Opaque types, consolidated lifetimes       |
| **Pattern**      | FFI type consolidation        | Wrapper structs to hide lifetime relations |
| **Anti-pattern** | Clone for borrow checker      | Hides ownership bugs, wastes memory        |
| **Anti-pattern** | `#![deny(warnings)]`          | Breaks builds on compiler upgrades         |
| **Anti-pattern** | Deref polymorphism            | Misusing `Deref` for inheritance           |
| **Functional**   | Paradigms                     | Imperative vs declarative style            |
| **Functional**   | Generics as type classes      | Monomorphization, type-state pattern       |
| **Functional**   | Optics                        | Isos, Poly Isos, Prisms (Serde design)     |
