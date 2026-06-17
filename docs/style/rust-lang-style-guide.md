# Official Rust Style Guide - Comprehensive Reference

Source: <https://doc.rust-lang.org/style-guide/>

This is the official style guide maintained by the Rust style team. It defines the default
formatting that `rustfmt` enforces. Everything below uses imperative language ("must", "put",
"break") to describe the _default style_, not hard prohibitions on alternatives.

---

## 1. Formatting Rules

### Indentation and Line Width

- **Spaces only**, never tabs.
- Each indentation level is **4 spaces**. All indentation outside string literals and comments must
  be a multiple of 4.
- Maximum line width: **100 characters**.
- **Block indent over visual indent.** Block indent produces smaller diffs and avoids rightward
  drift.

```rust
// Preferred: block indent
a_function_call(
    foo,
    bar,
);

// Avoid: visual indent
a_function_call(foo,
                bar);
```

### Trailing Commas

Use a trailing comma in any comma-separated list when followed by a newline. This makes diffs
smaller and copy-paste easier.

```rust
function_call(
    argument,
    another_argument,
);
```

Single-line calls and single-line struct literals do **not** get trailing commas.

### Blank Lines

Separate items and statements by zero or one blank line (never two or more consecutive blank lines).

### Trailing Whitespace

No trailing whitespace on any line, including blank lines, comments, and string literals.

### Comments

- Prefer line comments (`//`) over block comments (`/* ... */`).
- Single space after `//`.
- Comments should be complete sentences: capital letter, period at end.
- Comment-only lines are limited to **80 characters** (including sigils, excluding indentation) or
  the max line width (100), whichever is smaller.
- Put doc comments (`///`) before attributes. Prefer `///` over `/** ... */`.
- Use inner doc comments (`//!`) only for module-level or crate-level docs.

### Attributes

- One attribute per line, indented to the item's level.
- Format attribute argument lists like function calls (block indent if multi-line).
- Single `derive` attribute per item; tools must merge multiple `#[derive(...)]` into one,
  preserving order.
- Spaces around `=` in attributes: `#[foo = 42]`.

### The "Small" Heuristic

Many rules distinguish "small" items (formatted on one line) vs. larger ones (multi-line). The guide
intentionally leaves the exact threshold to tooling. Heuristics include character count and
expression complexity.

### Sorting

Where sorting is specified, use "version sorting": numeric chunks compare by numeric value (so
`u8 < u16 < u32`), underscores sort as word separators right after space, and uppercase sorts before
lowercase.

---

## 2. Naming Conventions

| Construct                                    | Case                              |
| -------------------------------------------- | --------------------------------- |
| Types (structs, enums, traits, type aliases) | `UpperCamelCase`                  |
| Enum variants                                | `UpperCamelCase`                  |
| Struct fields                                | `snake_case`                      |
| Functions and methods                        | `snake_case`                      |
| Local variables                              | `snake_case`                      |
| Macros                                       | `snake_case`                      |
| Constants (`const`, immutable `static`)      | `SCREAMING_SNAKE_CASE`            |
| Generic type parameters                      | Single uppercase letter preferred |

When a name collides with a reserved word, use a raw identifier (`r#crate`) or a trailing underscore
(`crate_`). Do **not** misspell the word (`krate`).

Avoid `#[path]` annotations on modules where possible.

---

## 3. Code Organization

### File-Level Ordering

1. `extern crate` statements first, alphabetically sorted.
2. `use` statements, then module declarations (`mod foo;`). Imports before module declarations.
3. Version-sort each group, except `self` and `super` come before any other names.
4. Do not auto-move `#[macro_use]`-annotated module declarations (may change semantics).

### Import Grouping

- A _group_ of imports is a contiguous set of `use` lines. Blank lines or other items separate
  groups.
- Within a group, version-sort imports. Do not merge or reorder across groups.
- In a list import (`use foo::{a, b, c}`), version-sort names; `self`/`super` always first, globs
  and sub-groups always last.
- Prefer multiple single imports over a multi-line import. Tools should not split or merge imports
  by default.
- Nested imports force multi-line form. Non-nested names are packed onto as few lines as possible;
  each nested import gets its own line.

### Import Normalization (tools must do)

- `use a::self;` becomes `use a;`
- `use a::{};` is removed
- `use a::{b};` becomes `use a::b;`

### Extern Items

Always specify the ABI explicitly: `extern "C" fn foo` not `extern fn foo`.

---

## 4. Items, Types, and Modules Formatting

### Functions

Signature ordering: `[pub] [unsafe] [extern ["ABI"]] fn name(args) -> ReturnType`. People search for
`fn function_name`, so keep `fn` and the name on the same line.

If the signature does not fit one line, break after `(` and before `)`, one argument per line,
block-indented, trailing comma:

```rust
fn foo(
    arg1: i32,
    arg2: i32,
) -> i32 {
    ...
}
```

### Structs and Unions

- Name and opening brace on same line.
- Fields indented once, trailing comma on each.
- If a field type doesn't fit, pull it to the next line with double indent.
- Prefer `struct Foo;` (unit) over `struct Foo {}` or `struct Foo()` (empty).

```rust
struct Foo {
    a: A,
    long_name:
        LongType,
}
```

### Tuple Structs

Single line if possible, no trailing comma, no spaces around parens. Multi-line: block indent
fields, trailing comma.

### Enums

One variant per line, block indented. Format variants as struct/tuple struct/identifier accordingly.
Small struct variants can go on one line with spaces around braces and no trailing comma. If any
struct variant in the enum is multi-line, all struct variants use multi-line form.

### Traits and Impls

- Empty trait/impl: single line `trait Foo {}` / `impl Foo {}`.
- Non-empty: break after `{`, before `}`.
- Bounds: space after `:`, spaces around `+`. Prefer `where` clause over line-breaking bounds.
- If bounds must break, each bound on its own line, break before `+`, opening brace on its own line.
- Impl line break: break before `for`, block indent the concrete type.

### Generics

- Prefer single line. Break other parts of the item before breaking the generics clause.
- No spaces inside `<>`. Space after `>` only before a word or `{`, not before `(`.
- Trailing comma only in multi-line form.
- Spaces around `=` in associated type bounds: `<T: Example<Item = u32>>`.

### Where Clauses

- If following a closing bracket, `where` on same line with a space before it.
- Otherwise, `where` on a new line at same indent level.
- Each predicate on its own block-indented line, trailing comma (unless terminated by `;`).
- Block/assignment starts on a new line after the `where` clause.
- Short clauses: prefer inline bounds instead.

### Type Aliases

Single line if possible. If breaking, break before `=`, block indent the RHS.

### Types and Bounds Formatting

Single-line rules:

- `[T]`, `[T; expr]` - no spaces around brackets, space after `;`
- `*const T`, `*mut T` - no space after `*`
- `&T`, `&mut T`, `&'a T` - no space after `&`
- `fn(T, U) -> V` - spaces around keywords, after commas, no trailing comma
- `(A, B, C)` - spaces after commas, no trailing comma (except one-tuple)
- `Foo::Bar<T, U>` - spaces after commas, no spaces around `<>` or `::`
- `T + T` / `impl T + T` - spaces around `+`

Line breaks: break at outermost scope first. For `+`-separated bounds, break before every `+` and
block indent.

### Modules

```rust
mod foo {
}

mod foo;
```

Spaces around keywords and before `{`, no spaces around `;`.

### `macro_rules!`

Use `{}` for the macro definition body.

---

## 5. Expression and Statement Style

### Let Statements

- Space after `:`, spaces around `=`, no space before `;`.
- Single line if possible. If not, break after `=` first, then after `:` if needed.
- Multi-line expressions: if first line fits after `=`, keep it there; otherwise block indent on
  next line.

### Let-Else

- Single line only if the whole thing is short and the else block is a single expression with no
  comments.
- Never break between `else` and `{`.
- Multi-line initializer ending with closing brackets at the `let` indent level: `else {` goes on
  same line.
- Otherwise, `else` on its own line at the `let` indent level.

### Blocks

- Newline after `{` and before `}` unless the block qualifies as single-line.
- Keywords (`unsafe`, `async`) on same line as `{` with a space.
- Single-line blocks allowed when: in expression position (not statement), single expression (no
  statements), no comments. Spaces inside braces for single-line blocks.
- Empty blocks: `{}`.

### Closures

- No extra spaces before first `|`. Space between second `|` and body.
- Omit `{}` when possible. Add `{}` when there's a return type, statements, comments, or multi-line
  control flow body.

### Function Calls

- No space between name and `(`.
- Single-line: no trailing comma, no spaces inside parens.
- Multi-line: one argument per line, block indented, trailing comma.
- Nullary calls `func()` always single line.

### Method Chains

- No spaces around `.`.
- One line if small. Otherwise each `.method()` on its own line, break before `.`, after `?`. Block
  indent subsequent lines.
- If any element is multi-line, it and all following elements get their own lines.
- Prefer whole-chain multi-line over mixed single/multi-line elements.

### Combinable Expressions

Single-argument function calls where the argument is multi-line block-indented can be "combined"
onto a single logical call:

```rust
foo(bar(
    an_expr,
    another_expr,
))
```

Also applies to last-argument closures with explicit blocks when all prior args fit on the first
line.

### Control Flow

- No extraneous parens around `if`/`while` conditions (but parens for clarity in arithmetic are
  fine).
- `} else {` on one line.
- If control line breaks: break after `=` (for `let`), before `in` (for `for`), and put opening
  brace on its own line.
- Single-line `if-else` only in expression position, single `else` clause, and small.

### Match

- Block indent arms once.
- Trailing comma on arms only when not using a block body.
- Never start a pattern with `|`.
- If RHS is single expression with no comments: same line, no block. Otherwise: block.
- Break pattern before `|` (not after). If `if` guard: break before `if`, block indent, always use
  block body.

### Binary Operations

- Spaces around all binary operators including `=`, `+=`, `as`.
- Line break: block indent subsequent lines. Break _after_ assignment operators, _before_ all other
  operators.
- Prefer breaking at assignment over other operators.
- `as` casts: always spaces around `as`, break before `as`.

### Unary Operations

No space between operator and operand (`!x`, `-y`). Exception: space after `&mut`.

### Ranges

No spaces: `0..10`, `x..=y`, `..x.len()`. Use parens for compound bounds: `..(x + 1)`.

### Patterns

Format like their corresponding expressions.

### Macros in Statement Position

Use `()` or `[]` as delimiters, terminate with `;`. No spaces around name, `!`, delimiters, or `;`.

### Expression Statements

No space before `;`. Always terminate with `;` unless the expression ends with a block or is used as
a block's value. Use `;` for void-typed expressions even if the return value could be propagated.

---

## 6. Cargo.toml Conventions

- Same line width (100) and indentation (4 spaces) as Rust code.
- Blank line between sections (before the next `[header]`), not within sections.
- Version-sort keys within each section, except `[package]` which has a fixed order: `name`,
  `version`, then remaining keys alphabetically, then `description` last.
- `[package]` section goes at the top of the file.
- Bare keys (no quotes) for standard key names.
- Space around `=`.
- Arrays: single line if they fit; otherwise block indent with trailing comma.
- Tables: inline `{}` if they fit; otherwise break into `[dependencies.crate_name]` sub-section.
- `authors`: `"Full Name <email>"` format.
- `license`: valid SPDX expression (`/` accepted in place of `OR` by convention).
- `homepage`: full URL with scheme.
- `description`: wrap at 80 columns, don't start with the crate name, first sentence is a standalone
  summary.

---

## 7. Guiding Principles (Priority Order)

The style team follows these principles when deciding rules:

1. **Readability** - scan-ability, avoiding misleading formatting, accessibility (non-visual
   interfaces, plain-text contexts like error messages and diffs).
2. **Aesthetics** - sense of beauty, consistency with other languages/tools.
3. **Specifics** - VCS-friendly diffs, preventing rightward drift, minimizing vertical space.
4. **Application** - ease of manual application, ease of tool implementation, internal consistency,
   simplicity of rules.

---

## 8. Notable Opinions and Patterns

1. **Block indent is strongly preferred** over visual indent, everywhere. This is a deliberate
   choice for diff-friendliness and preventing rightward drift.

2. **Trailing commas are mandatory in multi-line lists** but forbidden in single-line lists. The
   only exception is single-element tuples `(x,)` which need the comma for disambiguation.

3. **Expression-oriented style is explicitly encouraged.** The guide recommends
   `let x = if y { 1 } else { 0 };` over mutable assignment in branches.

4. **Reserved word collisions**: prefer raw identifiers (`r#type`) or trailing underscore (`type_`)
   over misspelling. The guide explicitly calls out `krate` as wrong.

5. **80-char comment width** is narrower than the 100-char code width. Comment-only lines have a
   stricter limit to improve readability.

6. **"Small" is intentionally vague.** The guide delegates the definition of "small" (for
   single-line formatting decisions) to individual tools, recognizing no single threshold works
   everywhere.

7. **Dereferencing over referencing in expressions.** When both `*t op u` and `t op &u` work, prefer
   the dereference form.

8. **Parentheses are encouraged for clarity** in binary expressions, even when not required by
   precedence. Tools should not auto-insert or auto-remove them.

9. **Match arms never start with `|`.** The first pattern in a match arm is bare; `|` appears only
   between alternatives.

10. **Import merging/splitting is opt-in.** Tools must not merge or split imports by default; the
    programmer controls import granularity.

11. **`macro_rules!` uses `{}`**, not `()` or `[]`, for the outer definition body.

12. **Version sorting** is the default sort order for imports, keys, and other ordered lists. It
    handles numeric components naturally (`u8` before `u16`).

13. **Extern ABI is always explicit**: write `extern "C"`, never bare `extern`.

14. **Combinable expressions** allow nested multi-line calls to collapse one level of indentation,
    keeping code compact without sacrificing readability.

15. **Single `derive` attribute.** Multiple `#[derive(...)]` must be merged into one, preserving
    order of derived traits.
