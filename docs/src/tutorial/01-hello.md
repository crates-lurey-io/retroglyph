# 1. Hello

This tutorial builds one small program across six chapters: a `@` that starts on an empty screen,
learns to move, gets a map to walk around, then a camera, a UI, and finally ships to a browser. Each
chapter's finished program lives in `examples/tutorial/`, compiled and run headless by CI, so every
code block below is pulled straight out of a file that actually builds -- there is nothing here that
can quietly drift out of sync with the library. See the [API documentation](../../crates/index.html)
for the full reference on anything named below.

## The `Example` trait

Every runnable example in this workspace (tutorial included) implements one trait:

```rust,ignore
pub trait Example: Default + Sized + 'static {
    const NAME: &'static str;
    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool;
}
```

`tick` runs once per frame: read input, update state, draw, and return `false` to quit. It's generic
over `Backend`, so the exact same `tick` runs against a real terminal (`crossterm`), a window
(`software`/`gl`/`wgpu`), or -- as this book's tests do -- a headless backend with no display at
all. Chapter 6 leans on that directly; for now it just means nothing here is backend-specific code
to unlearn later.

## State

Chapter 1's state is empty: the `@` never moves yet, so there's nothing to remember between frames.

```rust,ignore
{{#include ../../../examples/tutorial/01_hello.rs:state}}
```

## Drawing

`Terminal::surface` hands out a `Surface`, the one drawing primitive in the library. `Surface::put`
places a single styled character at a cell:

```rust,ignore
{{#include ../../../examples/tutorial/01_hello.rs:draw}}
```

## Running it

```sh
cargo run --example 01_hello --features crossterm  # a real terminal
cargo run --example 01_hello --features software   # a window
cargo run --example 01_hello                        # headless, prints a few frames to stdout
```

`q`, `Escape`, or the window's close button quits. Nothing else happens yet -- that's chapter 2.
