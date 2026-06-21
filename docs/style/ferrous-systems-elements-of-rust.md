# Ferrous Systems: Elements of Rust

Source: [github.com/ferrous-systems/elements-of-rust](https://github.com/ferrous-systems/elements-of-rust)

A collection of intermediate Rust techniques curated by Ferrous Systems (a major Rust consultancy, co-founded by members of the Rust core team). The repository focuses on practical software engineering patterns for expressing intent clearly in Rust. It is organized into four themes: cleanup, blocks for clarity, lockdown, and avoiding limitations.

---

## 1. Combating Rightward Pressure (Cleanup)

After wrestling with the compiler, code tends to accumulate nested combinators and match statements. Most of the art of clean Rust is de-nesting.

### 1.1 Basics of De-nesting

- **Use `?`** to flatten error handling. Avoid converting errors into a single top-level enum unless those errors genuinely belong together. Keep separate concerns in separate error types.
- **Split long combinator chains.** When a chain grows beyond one line, assign intermediate steps to named variables. Multi-line combinator chains can often be rewritten as for-loops for readability.
- **Pattern match on the full type** instead of nesting match statements. If your outer match just destructures to feed an inner match, combine them.
- **Use `if let`** when you only care about one arm. Replace a match with a single interesting pattern + wildcard with `if let Pattern(thing) = value { ... }`, optionally adding `else` for the wildcard case.
- **Run `cargo clippy`.** It catches many of these issues automatically.

### 1.2 Tuple Matching

Nested matches on two `Option`s or `Result`s can be flattened by matching on a tuple:

```rust
// Before: nested matches (4 levels of indentation)
let c = match a {
    Some(a) => match b {
        Some(b) => whatever,
        None => other_thing,
    },
    None => match b {
        Some(b) => another_thing,
        None => a_fourth_thing,
    },
};

// After: flat tuple match
let c = match (a, b) {
    (Some(a), Some(b)) => whatever,
    (Some(a), None)    => other_thing,
    (None,    Some(b)) => another_thing,
    (None,    None)    => a_fourth_thing,
};
```

**Boolean decision tables.** Matching on tuples of booleans encodes decision logic cleanly. This pattern is used in Cargo itself for `cargo new`:

```rust
let kind = match (args.is_present("bin"), args.is_present("lib")) {
    (true, true)  => failure::bail!("can't specify both lib and binary outputs"),
    (false, true) => NewProjectKind::Lib,
    (_, false)    => NewProjectKind::Bin, // default to bin
};
```

---

## 2. Iteration Patterns

### 2.1 `collect` on `Result` Iterators

`collect()` can short-circuit on an iterator of `Result<T, E>`, returning either a collection of all `Ok` values or the first `Err`. This works because `Result` implements `FromIterator<Result<A, E>>`.

```rust
let results = [Ok(1), Err("nope"), Ok(3), Err("bad")];
let result: Result<Vec<_>, &str> = results.iter().cloned().collect();
assert_eq!(Err("nope"), result);

let results = [Ok(1), Ok(3)];
let result: Result<Vec<_>, &str> = results.iter().cloned().collect();
assert_eq!(Ok(vec![1, 3]), result);
```

The key insight: the returned `Result` has a success type that is any collection implementing `FromIterator<A>` (where `A` is the `Ok` type), and the error type `E` passes through. On the first `Err`, iteration stops.

### 2.2 Reverse Iterator Ranges

`for item in 50..0` iterates zero times (the range is empty). Use `.rev()`:

```rust
for item in (0..50).rev() {}   // 49 down to 0
for item in (0..=50).rev() {}  // 50 down to 0 (RangeInclusive)
for item in (1..=50).rev() {}  // 50 down to 1
```

Under the hood, `0..50` creates a `Range` and `0..=50` creates a `RangeInclusive`. Half-open ranges (`..50`, `0..`, `..`) are also available.

### 2.3 Empty and Singular Iterators

Use `std::iter::empty()` and `std::iter::once(item)` instead of allocating:

```rust
// Instead of:
vec![].into_iter()       // allocates a Vec for nothing
vec![my_item].into_iter() // allocates a Vec for one item

// Use:
std::iter::empty()
std::iter::once(my_item)
```

### 2.4 Enum Variants and Tuple Structs as Constructor Functions

Tuple variants and tuple structs are functions from their fields to an instance. This lets you use them with `.map()`:

```rust
enum E { A(u64) }
struct B(u64);

let v: Vec<E>          = (0..50).map(E::A).collect();
let v: Vec<Option<u64>> = (0..50).map(Some).collect();
let v: Vec<B>          = (0..50).map(B).collect();
```

The compiler error message hints at this: `expected enum E, found fn(u64) -> E {E::A}`. The "function" it mentions is the constructor.

---

## 3. Blocks for Clarity (Closure Capture)

Blocks are expressions. Anywhere a closure is accepted, you can use a block that evaluates to a closure. This is particularly useful for `Arc::clone` scoping before `thread::spawn`.

### 3.1 The Problem

Cloning `Arc` for multiple threads leads to ugly numbered variable names:

```rust
// Before: inventing config1, config2, etc.
fn spawn_threads(config: Arc<Config>) {
    let config1 = Arc::clone(&config);
    thread::spawn(move || do_x(config1));

    let config2 = Arc::clone(&config);
    thread::spawn(move || do_y(config2));
}
```

### 3.2 The Pattern

Use a block expression that clones into a shadowed local, then returns a `move` closure:

```rust
// After: each block is self-contained
fn spawn_threads(config: Arc<Config>) {
    thread::spawn({
        let config = Arc::clone(&config);
        move || do_x(config)
    });

    thread::spawn({
        let config = Arc::clone(&config);
        move || do_y(config)
    });
}
```

Each block creates a scoped `config` that the `move` closure takes ownership of. The outer `config` remains available for subsequent blocks.

### 3.3 Why This Works (Precise Capture Clauses)

This pattern is documented in depth by Niko Matsakis in [Rust pattern: Precise closure capture clauses](http://smallcultfollowing.com/babysteps/blog/2018/04/24/rust-pattern-precise-closure-capture-clauses/). Closures capture entire local variables, not sub-fields. When a closure uses `self.input`, it captures all of `self`, potentially conflicting with other borrows of `self`. The fix: bind the needed field to a local variable before the closure, so the closure captures only that local.

```rust
// Closure captures all of `self`, conflicts with `self.output.extend()`
self.output.extend(values.iter().map(|v| self.input.get(v).cloned().unwrap_or(0)));

// Fix: bind field to local, closure captures only `input`
let input = &self.input;
self.output.extend(values.iter().map(|v| input.get(v).cloned().unwrap_or(0)));

// Generalized block form:
self.output.extend(values.iter().map({
    let input = &self.input;
    move |v| input.get(v).cloned().unwrap_or(0)
}));
```

The `let` statements effectively serve as C++-style "capture clauses", declaring precisely what the closure borrows and how. This pattern was seen in [salsa](https://github.com/salsa-rs/salsa).

Note: Rust RFC #2229 (now implemented in Edition 2021+) changed closures to capture individual fields rather than whole variables, partially addressing this issue. The block pattern remains useful for `Arc::clone` scenarios and other cases where you want explicit control.

---

## 4. Lockdown Patterns

Patterns for preventing undesirable usage at compile time.

### 4.1 Empty Enums as Never Types

An enum with no variants can never be instantiated:

```rust
enum Never {}

// There is no variant to construct:
// let never = Never:: ... nothing to complete to
```

Use cases:
- **Represent impossibility.** Where a type is structurally required but should never exist at runtime.
- **Infallible `Result`.** When a function returns `Result<T, Never>`, the `Err` case is statically impossible. The caller can safely unwrap without runtime cost.
- **Embedded main.** Functions that never return (infinite loops, `process::exit`) can return `!` (the never type). Empty enums are the stable equivalent for use in type parameters.

The `!` never type is being stabilized in std. `!` is the type of expressions like `return`, `break`, `continue`, `panic!()`, and infinite `loop`s.

### 4.2 Deactivating Mutability (Read-Only Newtypes)

A newtype that implements `Deref` but not `DerefMut`, with a private inner field, makes "finalized" objects immutable even in owned mutable bindings:

```rust
mod config {
    #[derive(Clone, Debug, PartialOrd, Ord, Eq, PartialEq)]
    pub struct Immutable<T>(T);  // private field!

    impl<T> Copy for Immutable<T> where T: Copy {}

    impl<T> std::ops::Deref for Immutable<T> {
        type Target = T;
        fn deref(&self) -> &T { &self.0 }
    }
    // Deliberately no DerefMut impl

    #[derive(Default)]
    pub struct Config {
        pub a: usize,
        pub b: String,
    }

    impl Config {
        pub fn build(self) -> Immutable<Config> {
            Immutable(self)
        }
    }
}
```

After calling `.build()`, the returned `Immutable<Config>`:
- Can be read via `Deref` (e.g., `finalized.a` works).
- Cannot be mutated: `finalized.a = 666` fails (no `DerefMut`).
- Cannot be unwrapped: `finalized.0.a = 666` fails (field is private).
- Can be freely copied/cloned and passed around, but all copies remain read-only.

This enforces a builder pattern where configuration is mutable during construction and frozen after finalization, with the guarantee enforced by the type system rather than runtime checks.

---

## 5. Avoiding Limitations

### 5.1 Shared Reference Swap Trick (`Cell::take`)

`Cell<T>` is usually associated with `Copy` types (via `Cell::get`), but `Cell::replace` and `Cell::take` (where `T: Default`) enable mutation through `&self` for non-Copy types. This is "Jones's trick" (`std::mem::replace` semantics through a shared reference).

Use case: implementing `fmt::Display` for an iterator, which requires consuming the iterator but `Display::fmt` only receives `&self`:

```rust
use std::{cell::Cell, fmt};

fn display_iter<I>(xs: I) -> impl fmt::Display
where
    I: Iterator,
    I::Item: fmt::Display,
{
    struct IterFmt<I>(Cell<Option<I>>);

    impl<I> fmt::Display for IterFmt<I>
    where
        I: Iterator,
        I::Item: fmt::Display,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let xs = self.0.take().unwrap(); // take ownership through &self
            let mut first = true;
            for item in xs {
                if !first { f.write_str(", ")? }
                first = false;
                fmt::Display::fmt(&item, f)?
            }
            Ok(())
        }
    }

    IterFmt(Cell::new(Some(xs)))
}
```

Pattern origin: [rustc's own HTML formatter](https://github.com/rust-lang/rust/blob/6b5f9b2e973e438fc1726a2d164d046acd80b170/src/librustdoc/html/format.rs#L1061).

### 5.2 Using Sets as Maps

When you want to store structs keyed by a field, `HashMap<String, Person>` forces you to clone the key. You can't borrow the key from the value because there's no lifetime to express that relationship. The trick: use `HashSet` with `Borrow`:

```rust
use std::{borrow::Borrow, collections::HashSet, hash::{Hash, Hasher}};

struct Person {
    name: String,
    age: u32,
}

impl Borrow<str> for Person {
    fn borrow(&self) -> &str { &self.name }
}

impl PartialEq for Person {
    fn eq(&self, other: &Person) -> bool { self.name == other.name }
}

impl Eq for Person {}

impl Hash for Person {
    fn hash<H: Hasher>(&self, hasher: &mut H) { self.name.hash(hasher) }
}

// Now you can look up by &str:
fn get_by_name<'p>(persons: &'p HashSet<Person>, name: &str) -> Option<&'p Person> {
    persons.get(name)  // Borrow<str> enables this
}
```

The mechanism: Rust's `HashSet::get` accepts any `Q` where `T: Borrow<Q>`. By implementing `Borrow<str> for Person`, you tell the set to look up entries by their name field. `Eq` and `Hash` must be consistent with `Borrow` (compare/hash only by `name`).

**Caveat:** Because `Eq` and `Hash` now ignore `age`, two `Person`s with the same name but different ages are considered equal. If this is undesirable for the rest of your application, wrap in a newtype: `struct PersonByName(Person)` and implement `Borrow` on the wrapper.

### 5.3 `Box<FnOnce>` Workaround (Obsolete)

**Obsolete since Rust 1.35.** `Box<dyn FnOnce>` now works directly. Previously, calling a `Box<FnOnce>` on stable required a `FnBox` trait workaround (seen in Cargo's source). The pattern used `self: Box<Self>` as a receiver, which was already stable and object-safe. Included for historical reference only.

---

## 6. Ergonomics (Compiler Workflow)

### 6.1 Type Unification and Error Messages

The compiler infers types bidirectionally: from argument types downward and from the return type upward. A single gap in the chain produces cascading errors for every ambiguous type. The practical advice: **start with the first error and work down.** Most of the errors after the first are noise caused by a single missing type.

### 6.2 Write-Compile-Fix Loop

Install `cargo-watch` to automatically recompile on save:

```bash
cargo install cargo-watch
cargo watch -s 'clear; cargo check --tests --color=always 2>&1 | head -40'
```

This shows only the first screenful of errors, auto-refreshing on each save. Reduces context-switching fatigue significantly.

### 6.3 `sccache` for Build Caching

`sccache` (by Mozilla) caches compilation artifacts across projects and `cargo clean` cycles:

```bash
cargo install sccache
export RUSTC_WRAPPER=sccache
```

Handles feature flags, versions, etc. correctly. Supports remote backends (S3, Redis, memcached) for CI.

### 6.4 Editor Integration

Use editor plugins that jump to compiler errors directly:
- **Vim:** `vim.rust` + Syntastic
- **Emacs:** `flycheck-rust`
- Modern setups: rust-analyzer LSP (not mentioned in the original, but now the standard approach)

---

## Summary of Patterns

| Category | Pattern | Key Idea |
|---|---|---|
| Cleanup | `?` operator | Flatten error handling without nesting |
| Cleanup | Tuple matching | Match `(a, b)` instead of nesting `match a { match b }` |
| Cleanup | `if let` | Replace single-arm match + wildcard |
| Cleanup | Boolean decision tables | `match (bool, bool)` for flag combinations |
| Iteration | `collect` on `Result` | `Iterator<Item=Result<T,E>>` -> `Result<Vec<T>, E>` |
| Iteration | `.rev()` on ranges | `(0..n).rev()` for descending iteration |
| Iteration | `iter::empty` / `once` | Zero-alloc empty or single-element iterators |
| Iteration | Variants as functions | `(0..n).map(Some)` or `.map(E::A)` |
| Blocks | Closure capture scoping | Block with `let clone = Arc::clone(&x); move \|\| ...` |
| Blocks | Precise capture clauses | Bind sub-fields to locals to narrow closure captures |
| Lockdown | Empty enum (Never) | Type that cannot be instantiated |
| Lockdown | Read-only newtype | `Deref` without `DerefMut`, private inner field |
| Limitations | `Cell::take` swap trick | Mutate through `&self` for non-Copy types |
| Limitations | Sets as Maps | `HashSet` + `Borrow<K>` to avoid cloning keys |

---

## Sources

- **Kept:**
  - [Ferrous Systems: Elements of Rust](https://github.com/ferrous-systems/elements-of-rust) - primary source, the full repository README
  - [Niko Matsakis: Precise closure capture clauses](http://smallcultfollowing.com/babysteps/blog/2018/04/24/rust-pattern-precise-closure-capture-clauses/) - detailed explanation of the block-for-closure pattern, referenced directly by the repo
  - [std::iter::Iterator::collect docs](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.collect) - explains `FromIterator` for `Result`
  - [std::primitive::never docs](https://doc.rust-lang.org/std/primitive.never.html) - never type stabilization status
  - [salsa cancellation test](https://github.com/salsa-rs/salsa/blob/3dc4539c7c34cb12b5d4d1bb0706324cfcaaa7ae/tests/parallel/cancellation.rs#L42-L53) - real-world usage of the closure block pattern
  - [rustc HTML formatter](https://github.com/rust-lang/rust/blob/6b5f9b2e973e438fc1726a2d164d046acd80b170/src/librustdoc/html/format.rs#L1061) - origin of the Cell::take display trick

- **Dropped:**
  - Packt tutorial on Cell/RefCell - generic tutorial content, no additional insight
  - StackOverflow HashSet+Borrow questions - derivative of the same pattern, no new information
  - RFC 2229 tracking issue - now shipped in Edition 2021, historical only
  - `Box<FnOnce>` workaround - obsolete since Rust 1.35