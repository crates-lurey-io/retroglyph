# Rust API Guidelines - Comprehensive Reference

> Source: <https://rust-lang.github.io/api-guidelines/>
>
> Authored largely by the Rust library team, based on experience building the standard library and
> the broader ecosystem. These are recommendations, not mandates; crates that follow them integrate
> better with the ecosystem.

---

## Table of Contents

1. [Naming](#1-naming)
2. [Interoperability](#2-interoperability)
3. [Macros](#3-macros)
4. [Documentation](#4-documentation)
5. [Predictability](#5-predictability)
6. [Flexibility](#6-flexibility)
7. [Type Safety](#7-type-safety)
8. [Dependability](#8-dependability)
9. [Debuggability](#9-debuggability)
10. [Future Proofing](#10-future-proofing)
11. [Necessities](#11-necessities)

---

## 1. Naming

### C-CASE: Casing conforms to RFC 430

Follow standard Rust casing conventions (per
[RFC 430](https://github.com/rust-lang/rfcs/blob/master/text/0430-finalizing-naming-conventions.md)):

| Item                    | Convention                                                  |
| ----------------------- | ----------------------------------------------------------- |
| Modules                 | `snake_case`                                                |
| Types                   | `UpperCamelCase`                                            |
| Traits                  | `UpperCamelCase`                                            |
| Enum variants           | `UpperCamelCase`                                            |
| Functions / Methods     | `snake_case`                                                |
| Macros                  | `snake_case!`                                               |
| Local variables         | `snake_case`                                                |
| Statics / Constants     | `SCREAMING_SNAKE_CASE`                                      |
| Type parameters         | Concise `UpperCamelCase`, usually single letter: `T`        |
| Lifetimes               | Short lowercase, usually single letter: `'a`, `'de`, `'src` |
| General constructors    | `new` or `with_more_details`                                |
| Conversion constructors | `from_some_other_type`                                      |

Additional rules:

- Acronyms count as one word in `UpperCamelCase`: `Uuid` not `UUID`, `Stdin` not `StdIn`.
- In `snake_case`, acronyms are lowered: `is_xid_start`.
- A "word" in snake_case should never be a single letter unless it is the last word: `btree_map` not

  `b_tree_map`, but `PI_2` is fine.

- Crate names should not use `-rs` or `-rust` as suffix or prefix.

### C-CONV: Ad-hoc conversions follow `as_`/`to_`/`into_` conventions

| Prefix  | Cost      | Ownership                                                                 |
| ------- | --------- | ------------------------------------------------------------------------- |
| `as_`   | Free      | borrowed -> borrowed                                                      |
| `to_`   | Expensive | borrowed -> borrowed, borrowed -> owned (non-Copy), owned -> owned (Copy) |
| `into_` | Variable  | owned -> owned (non-Copy)                                                 |

- `as_` and `into_` typically decrease abstraction (expose underlying representation).
- `to_` typically stays at the same abstraction level but does work.
- Wrappers should expose the inner value via `into_inner()`.
- If `mut` is part of the return type, place it as it appears in the type: `as_mut_slice()` not

  `as_slice_mut()`.

**Examples:** `str::as_bytes()` (free borrow), `str::to_lowercase()` (allocating),
`String::into_bytes()` (ownership transfer).

### C-GETTER: Getter names follow Rust convention

- Do **not** use the `get_` prefix for getters. Use `first()` not `get_first()`.
- `get` is reserved for when there is a single obvious thing being gotten (e.g., `Cell::get`).
- Mutable getters: `first_mut()` not `get_first_mut()`.
- For runtime-validated access, provide `_unchecked` variants:

  ```rust
  fn get(&self, index: K) -> Option<&V>;
  fn get_mut(&mut self, index: K) -> Option<&mut V>;
  unsafe fn get_unchecked(&self, index: K) -> &V;
  unsafe fn get_unchecked_mut(&mut self, index: K) -> &mut V;
  ```

### C-ITER: Iterator methods follow `iter`, `iter_mut`, `into_iter`

For a container with elements of type `U`:

```rust
fn iter(&self) -> Iter         // Iterator<Item = &U>
fn iter_mut(&mut self) -> IterMut  // Iterator<Item = &mut U>
fn into_iter(self) -> IntoIter    // Iterator<Item = U>
```

This applies to conceptually homogeneous collections. Non-homogeneous types (like `str`) should use
domain-specific names (`bytes()`, `chars()`).

### C-ITER-TY: Iterator type names match the methods that produce them

`into_iter()` returns `IntoIter`, `iter()` returns `Iter`, `keys()` returns `Keys`, etc. These type
names read best when qualified by their module: `vec::IntoIter`.

### C-FEATURE: Feature names are free of placeholder words

- Name features directly: `std`, not `use-std` or `with-std`.
- This aligns with implicit features Cargo infers for optional dependencies.
- Features must be additive; names like `no-abc` are practically never correct.

Canonical `no_std` pattern:

```toml
[features]
default = ["std"]
std = []
```

### C-WORD-ORDER: Names use a consistent word order

Follow verb-object-error ordering for error types: `ParseAddrError` not `AddrParseError`. The
specific order matters less than consistency within the crate and with the standard library.

**Std examples:** `JoinPathsError`, `ParseBoolError`, `ParseIntError`, `RecvTimeoutError`,
`StripPrefixError`.

---

## 2. Interoperability

### C-COMMON-TRAITS: Types eagerly implement common traits

Due to the orphan rule, downstream crates cannot add trait impls for upstream types. Eagerly
implement all applicable common traits:

`Copy`, `Clone`, `Eq`, `PartialEq`, `Ord`, `PartialOrd`, `Hash`, `Debug`, `Display`, `Default`

It is expected for types to implement both `Default` and an empty `new()` constructor. They should
have the same behavior.

### C-CONV-TRAITS: Conversions use the standard traits `From`, `AsRef`, `AsMut`

Implement these where applicable:

- `From` / `TryFrom`
- `AsRef` / `AsMut`

**Never** implement `Into` or `TryInto` directly; they have blanket impls from `From`/`TryFrom`.

### C-COLLECT: Collections implement `FromIterator` and `Extend`

These enable `Iterator::collect`, `Iterator::partition`, and `Iterator::unzip`.

### C-SERDE: Data structures implement Serde's `Serialize`, `Deserialize`

Types that act as data structures should implement Serde traits. Gate behind a `"serde"` feature if
Serde isn't otherwise needed:

```toml
[dependencies]
serde = { version = "1.0", optional = true, features = ["derive"] }
```

```rust
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct T { /* ... */ }
```

Name the feature `"serde"`, not `"serde_impls"` or similar.

### C-SEND-SYNC: Types are `Send` and `Sync` where possible

These are auto-implemented by the compiler. For types with raw pointers, be vigilant and add
compile-time assertions:

```rust
#[test]
fn test_send() {
    fn assert_send<T: Send>() {}
    assert_send::<MyType>();
}
```

### C-GOOD-ERR: Error types are meaningful and well-behaved

- Always implement `std::error::Error`.
- Error types must be `Send + Sync` (required for threading and `io::Error::new`).
- Prefer `Error + Send + Sync + 'static` for trait objects (enables `downcast_ref`).
- **Never** use `()` as an error type. Define a meaningful type, even if it's a unit struct.
- Error `Display` messages should be lowercase, no trailing punctuation, concise.
- Do not implement `Error::description()` (deprecated; use `Display` instead).

### C-NUM-FMT: Binary number types provide `Hex`, `Octal`, `Binary` formatting

Implement `UpperHex`, `LowerHex`, `Octal`, `Binary` for types where bitwise operations (`|`, `&`)
make sense, especially bitflag types. Quantity types like `Nanoseconds(u64)` generally do not need
these.

### C-RW-VALUE: Generic reader/writer functions take `R: Read` and `W: Write` by value

Because `&mut R` implements `Read` (and `&mut W` implements `Write`), taking by value is strictly
more flexible. Document that callers can pass `&mut f` if they need to retain the reader/writer.

---

## 3. Macros

### C-EVOCATIVE: Input syntax is evocative of the output

Mirror existing Rust syntax in macro inputs. If a macro declares a struct, use the `struct` keyword
in the input. Use semicolons for constant-like declarations, not commas. The goal is that reading
the macro invocation gives a good intuition for what code is produced.

### C-MACRO-ATTR: Macros compose well with attributes

Macros producing multiple items should let users attach attributes (like `#[cfg(...)]`) to
individual items. Macros producing structs/enums should support `#[derive(...)]` on the output.

### C-ANYWHERE: Item macros work anywhere that items are allowed

Test macro invocations in both module scope and function scope. Beware that `super::` inside a
macro-generated module behaves differently in these contexts.

### C-MACRO-VIS: Item macros support visibility specifiers

Follow Rust conventions: private by default, `pub` when specified. The macro should accept and
forward `pub` (or `pub(crate)`, etc.) to generated items.

### C-MACRO-TY: Type fragments are flexible

A `$t:ty` fragment should work with primitives (`u8`), relative paths (`m::Data`), absolute paths
(`::base::Data`), upward paths (`super::Data`), and generics (`Vec<String>`). Test all of these.

---

## 4. Documentation

### C-CRATE-DOC: Crate level docs are thorough and include examples

Per [RFC 1687](https://github.com/rust-lang/rfcs/pull/1687). The top-level `//!` docs should explain
what the crate does, when to use it, and include representative examples.

### C-EXAMPLE: All items have a rustdoc example

Every public module, trait, struct, enum, function, method, macro, and type definition should have
an example. The example should show _why_ someone would use the item, not just _how_ to call it.
Linking to an example on a related item is acceptable.

### C-QUESTION-MARK: Examples use `?`, not `try!`, not `unwrap`

Example code gets copied verbatim. Structure fallible examples with hidden boilerplate:

````rust
/// ```rust
/// # use std::error::Error;
/// #
/// # fn main() -> Result<(), Box<dyn Error>> {
/// your;
/// example?;
/// code;
/// #     Ok(())
/// # }
/// ```
````

### C-FAILURE: Function docs include error, panic, and safety considerations

- **`# Errors`** section: document conditions under which the function returns an error.
- **`# Panics`** section: document conditions under which the function panics.
- **`# Safety`** section (unsafe functions): document all invariants the caller must uphold.

### C-LINK: Prose contains hyperlinks to relevant things

Use markdown links and rustdoc intra-doc links. Link to related types, methods, modules. Per
[RFC 1574](https://github.com/rust-lang/rfcs/blob/master/text/1574-more-api-documentation-conventions.md):
"Link all the things."

### C-METADATA: Cargo.toml includes all common metadata

Required in `[package]`: `authors`, `description`, `license`, `repository`, `keywords`,
`categories`.

Optional: `documentation` (only if not on docs.rs), `homepage` (only if there is a dedicated site
distinct from the repo and docs).

### C-RELNOTES: Release notes document all significant changes

- Include release notes in crate-level docs or link to them.
- Breaking changes (per

  [RFC 1105](https://github.com/rust-lang/rfcs/blob/master/text/1105-api-evolution.md)) must be
  clearly identified.

- Tag every published release in version control. Prefer annotated tags.

### C-HIDDEN: Rustdoc does not show unhelpful implementation details

- Use `#[doc(hidden)]` to hide implementation-detail impls (e.g., `From<PrivateError>`).
- Use `pub(crate)` to keep items usable within the crate but out of the public API.
- Show everything users need, nothing they don't.

---

## 5. Predictability

### C-SMART-PTR: Smart pointers do not add inherent methods

Define associated functions (taking `self` as a named parameter, not method receiver) to avoid
confusion with methods on the inner type accessed through `Deref`. Example: `Box::into_raw(b)` not
`b.into_raw()`.

### C-CONV-SPECIFIC: Conversions live on the most specific type involved

Place conversions on the more specific type. `str` (more specific than `&[u8]`) provides both
`as_bytes()` and `from_utf8()`. When in doubt, prefer `to_`/`as_`/`into_` over `from_` because they
chain more ergonomically.

### C-METHOD: Functions with a clear receiver are methods

Use `impl Foo { pub fn frob(&self, w: Widget) }` not `pub fn frob(foo: &Foo, w: Widget)`. Methods
provide autoborrowing, discoverability via rustdoc, and `self` notation.

### C-NO-OUT: Functions do not take out-parameters

Return compound types (tuples, structs) instead of mutating out-parameters. Exception: functions
meant to modify caller-owned data like `read(&mut self, buf: &mut [u8])`.

### C-OVERLOAD: Operator overloads are unsurprising

Only implement `Mul`, `Add`, etc. for operations that semantically resemble multiplication,
addition, etc. Preserve expected mathematical properties (associativity, commutativity where
applicable).

### C-DEREF: Only smart pointers implement `Deref` and `DerefMut`

`Deref` is used implicitly by the compiler for method resolution. It is designed for smart pointers
only. Standard examples: `Box<T>`, `String` (deref to `str`), `Rc<T>`, `Arc<T>`, `Cow<'a, T>`.

### C-CTOR: Constructors are static, inherent methods

- Primary constructor: `new()` (may or may not take arguments).
- I/O types may use domain names: `File::open()`, `TcpStream::connect()`.
- Secondary constructors: suffix `_with_foo` (e.g., `open_with_offset()`). For many options, use the

  builder pattern (C-BUILDER).

- Conversion constructors: `from_*` prefix. Use `from_` (not `From` trait) when the conversion is

  unsafe, needs extra args, or the source type alone is insufficient to determine encoding.

- `Default` and `new()` should have the same behavior when both exist.

---

## 6. Flexibility

### C-INTERMEDIATE: Functions expose intermediate results to avoid duplicate work

Return rich result types that include useful byproduct data. Examples:

- `Vec::binary_search` returns the insertion index on failure, not just `None`.
- `String::from_utf8` returns the valid-up-to byte offset and the original bytes on error.
- `HashMap::insert` returns the previous value if any.

### C-CALLER-CONTROL: Caller decides where to copy and place data

- If a function needs ownership, take ownership (don't borrow and clone internally).
- If a function only needs to read, take a borrow (don't take ownership and drop).
- Don't use `Copy` as a bound to signal cheap copies.

### C-GENERIC: Functions minimize assumptions about parameters by using generics

Prefer `fn foo<I: IntoIterator<Item = i64>>(iter: I)` over `fn foo(c: &[i64])` when you only need
iteration. Generics provide reusability, static dispatch, inline layout, inference, and precise
types.

Trade-offs: increased code size from monomorphization, homogeneous types only (vs. trait objects),
more verbose signatures.

### C-OBJECT: Traits are object-safe if they may be useful as a trait object

If a trait might be used as `dyn Trait`, keep it object-safe. Use `where Self: Sized` to exclude
generic methods from the trait object while keeping the trait usable as an object:

```rust
trait MyTrait {
    fn object_safe(&self, i: i32);
    fn not_object_safe<T>(&self, t: T) where Self: Sized;
}
```

---

## 7. Type Safety

### C-NEWTYPE: Newtypes provide static distinctions

Wrap primitive types to give them domain meaning: `struct Miles(pub f64)` vs
`struct Kilometers(pub f64)`. Prevents confusing one for the other at compile time.

### C-CUSTOM-TYPE: Arguments convey meaning through types, not `bool` or `Option`

Use `Widget::new(Small, Round)` not `Widget::new(true, false)`. Custom enum types are
self-documenting and extensible (e.g., add `ExtraLarge` later).

### C-BITFLAG: Types for a set of flags are `bitflags`, not enums

Use the [`bitflags`](https://github.com/bitflags/bitflags) crate for combinable flag sets. Enums
represent exactly-one-of semantics; bitflags represent any-combination-of.

### C-BUILDER: Builders enable construction of complex values

For types with many inputs, optional config, or compound data:

1. Introduce a separate builder type.
2. Builder constructor takes only required data.
3. Configuration methods return `self` for chaining.
4. Terminal methods (`.build()`, `.spawn()`, etc.) produce the final value.

**Non-consuming builders (preferred):** terminal methods take `&self`, config methods take/return
`&mut self`. Supports both one-liners and complex multi-step configuration.

**Consuming builders:** terminal methods take `self`. Config methods should also take/return owned
`self` (not `&mut self`) to keep one-liners working; complex config requires re-assignment at each
step.

---

## 8. Dependability

### C-VALIDATE: Functions validate their arguments

Rust does **not** follow the robustness principle. Enforce input validity via (in order of
preference):

1. **Static enforcement**: use newtypes/wrapper types that rule out bad inputs at compile time

   (e.g., `Ascii` instead of `u8`).

1. **Dynamic enforcement**: validate at runtime, returning `Result`/`Option` or panicking.
1. **`debug_assert!`**: dynamic checks that can be disabled in release builds.3. **Opt-out**
   (`_unchecked` variants or `raw` submodules): for performance-critical paths where the

   caller guarantees validity.

### C-DTOR-FAIL: Destructors never fail

A failing destructor during a panic causes an abort. Provide a separate `close()` method that
returns `Result`. The `Drop` impl should do best-effort teardown and ignore/log errors.

### C-DTOR-BLOCK: Destructors that may block have alternatives

Don't do blocking I/O in `Drop`. Provide a separate method for explicit, potentially-blocking
teardown.

---

## 9. Debuggability

### C-DEBUG: All public types implement `Debug`

Exceptions are rare. This is essential for debugging and for use with `{:?}` formatting.

### C-DEBUG-NONEMPTY: `Debug` representation is never empty

Even conceptually empty values must produce non-empty debug output: `""` for empty strings, `[]` for
empty vectors, etc.

---

## 10. Future Proofing

### C-SEALED: Sealed traits protect against downstream implementations

Use a private supertrait to prevent external implementations:

```rust
pub trait TheTrait: private::Sealed {
    fn method(&self);
}

mod private {
    pub trait Sealed {}
    impl Sealed for usize {}
}

impl TheTrait for usize { /* ... */ }
```

This allows adding methods to the trait in non-breaking releases. Document that the trait is sealed.
Note: removing public methods or changing their signatures is still breaking.

### C-STRUCT-PRIVATE: Structs have private fields

Public fields pin representation and prevent validation/invariant enforcement. Prefer getter/setter
methods unless the struct is purely passive data (C-style).

### C-NEWTYPE-HIDE: Newtypes encapsulate implementation details

Use newtypes (or `impl Trait`) to hide complex return types like `Enumerate<Skip<I>>`. This lets the
representation change without breaking downstream code.

`impl Trait` is more concise but less expressive (harder to also impl `Debug`, `Clone`, etc. on the
return type).

### C-STRUCT-BOUNDS: Data structures do not duplicate derived trait bounds

Do not put derivable trait bounds on struct definitions:

```rust
// Good:
#[derive(Clone, Debug, PartialEq)]
struct Good<T> { /* ... */ }

// Bad: adding a derived trait later becomes a breaking change
#[derive(Clone, Debug, PartialEq)]
struct Bad<T: Clone + Debug + PartialEq> { /* ... */ }
```

Traits that should **never** appear as bounds on data structures: `Clone`, `PartialEq`,
`PartialOrd`, `Debug`, `Display`, `Default`, `Error`, `Serialize`, `Deserialize`,
`DeserializeOwned`.

Exceptions: bounds referencing associated types, `?Sized`, and bounds required by `Drop` impls.

---

## 11. Necessities

### C-STABLE: Public dependencies of a stable crate are stable

A crate at >=1.0.0 cannot expose types from pre-1.0 dependencies in its public API. Public
dependencies can sneak in through trait impls (e.g., `impl From<other_crate::Error>`), not just
function signatures.

### C-PERMISSIVE: Crate and its dependencies have a permissive license

The Rust project uses dual MIT/Apache-2.0 licensing. Recommended for maximum ecosystem
compatibility:

```toml
[package]
license = "MIT OR Apache-2.0"
```

Include `LICENSE-APACHE` and `LICENSE-MIT` files. Apache-only is not recommended because it imposes
restrictions beyond MIT that can prevent use in some scenarios. A permissively-licensed crate should
generally only depend on permissively-licensed crates.

---

## Quick Reference Checklist

| #   | ID               | Guideline                                                  | Section         |
| --- | ---------------- | ---------------------------------------------------------- | --------------- |
| 1   | C-CASE           | Casing conforms to RFC 430                                 | Naming          |
| 2   | C-CONV           | Ad-hoc conversions follow `as_`/`to_`/`into_`              | Naming          |
| 3   | C-GETTER         | Getter names follow Rust convention (no `get_` prefix)     | Naming          |
| 4   | C-ITER           | Collection iterators: `iter`, `iter_mut`, `into_iter`      | Naming          |
| 5   | C-ITER-TY        | Iterator type names match producing methods                | Naming          |
| 6   | C-FEATURE        | Feature names free of placeholder words                    | Naming          |
| 7   | C-WORD-ORDER     | Consistent word order in names                             | Naming          |
| 8   | C-COMMON-TRAITS  | Eagerly implement common traits                            | Interop         |
| 9   | C-CONV-TRAITS    | Use `From`, `AsRef`, `AsMut` (never `Into`)                | Interop         |
| 10  | C-COLLECT        | Collections implement `FromIterator`/`Extend`              | Interop         |
| 11  | C-SERDE          | Data structures implement Serde traits                     | Interop         |
| 12  | C-SEND-SYNC      | Types are `Send`/`Sync` where possible                     | Interop         |
| 13  | C-GOOD-ERR       | Error types are meaningful and well-behaved                | Interop         |
| 14  | C-NUM-FMT        | Binary types provide Hex/Octal/Binary formatting           | Interop         |
| 15  | C-RW-VALUE       | Reader/writer functions take `R: Read`/`W: Write` by value | Interop         |
| 16  | C-EVOCATIVE      | Macro input syntax evocative of output                     | Macros          |
| 17  | C-MACRO-ATTR     | Macros compose well with attributes                        | Macros          |
| 18  | C-ANYWHERE       | Item macros work anywhere items are allowed                | Macros          |
| 19  | C-MACRO-VIS      | Item macros support visibility specifiers                  | Macros          |
| 20  | C-MACRO-TY       | Type fragments are flexible                                | Macros          |
| 21  | C-CRATE-DOC      | Crate-level docs are thorough with examples                | Docs            |
| 22  | C-EXAMPLE        | All items have a rustdoc example                           | Docs            |
| 23  | C-QUESTION-MARK  | Examples use `?`, not `try!`, not `unwrap`                 | Docs            |
| 24  | C-FAILURE        | Docs include error/panic/safety sections                   | Docs            |
| 25  | C-LINK           | Prose contains hyperlinks to relevant things               | Docs            |
| 26  | C-METADATA       | Cargo.toml includes all common metadata                    | Docs            |
| 27  | C-RELNOTES       | Release notes document all significant changes             | Docs            |
| 28  | C-HIDDEN         | Rustdoc hides unhelpful implementation details             | Docs            |
| 29  | C-SMART-PTR      | Smart pointers don't add inherent methods                  | Predictability  |
| 30  | C-CONV-SPECIFIC  | Conversions live on the most specific type                 | Predictability  |
| 31  | C-METHOD         | Functions with a clear receiver are methods                | Predictability  |
| 32  | C-NO-OUT         | Functions don't take out-parameters                        | Predictability  |
| 33  | C-OVERLOAD       | Operator overloads are unsurprising                        | Predictability  |
| 34  | C-DEREF          | Only smart pointers implement `Deref`/`DerefMut`           | Predictability  |
| 35  | C-CTOR           | Constructors are static, inherent methods                  | Predictability  |
| 36  | C-INTERMEDIATE   | Expose intermediate results to avoid duplicate work        | Flexibility     |
| 37  | C-CALLER-CONTROL | Caller decides where to copy/place data                    | Flexibility     |
| 38  | C-GENERIC        | Minimize parameter assumptions with generics               | Flexibility     |
| 39  | C-OBJECT         | Traits are object-safe if useful as trait objects          | Flexibility     |
| 40  | C-NEWTYPE        | Newtypes provide static distinctions                       | Type Safety     |
| 41  | C-CUSTOM-TYPE    | Arguments convey meaning through types, not `bool`         | Type Safety     |
| 42  | C-BITFLAG        | Flag sets use `bitflags`, not enums                        | Type Safety     |
| 43  | C-BUILDER        | Builders enable complex value construction                 | Type Safety     |
| 44  | C-VALIDATE       | Functions validate their arguments                         | Dependability   |
| 45  | C-DTOR-FAIL      | Destructors never fail                                     | Dependability   |
| 46  | C-DTOR-BLOCK     | Blocking destructors have alternatives                     | Dependability   |
| 47  | C-DEBUG          | All public types implement `Debug`                         | Debuggability   |
| 48  | C-DEBUG-NONEMPTY | `Debug` representation is never empty                      | Debuggability   |
| 49  | C-SEALED         | Sealed traits protect against downstream impls             | Future Proofing |
| 50  | C-STRUCT-PRIVATE | Structs have private fields                                | Future Proofing |
| 51  | C-NEWTYPE-HIDE   | Newtypes encapsulate implementation details                | Future Proofing |
| 52  | C-STRUCT-BOUNDS  | Data structures don't duplicate derived trait bounds       | Future Proofing |
| 53  | C-STABLE         | Public deps of a stable crate are stable                   | Necessities     |
| 54  | C-PERMISSIVE     | Crate and deps have permissive license                     | Necessities     |
