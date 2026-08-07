# 2. Input

Chapter 1's `@` sat still. This chapter gives it a position and moves it with the arrow keys --
still no map, just the four edges of the grid to bump into.

## State

`Hello`'s empty struct becomes a position:

```rust,ignore
{{#include ../../../examples/tutorial/02_input.rs:state}}
```

## Reading input and moving

Every backend's `Terminal::drain_events` yields the same `Event` enum, so the input loop looks
identical whether it's driven by real key presses, a synthetic test event, or (in chapter 6) a
browser's keyboard events forwarded over WASM. `try_move` clamps the new position to the grid so `@`
can't walk off the edge of the screen:

```rust,ignore
{{#include ../../../examples/tutorial/02_input.rs:movement}}
```

## Running it

```sh
cargo run --example 02_input --features crossterm
cargo run --example 02_input --features software
cargo run --example 02_input  # headless fallback, prints a few frames to stdout
```

Arrow keys move `@`. `q` or `Escape` quits. Chapter 3 replaces the screen-edge clamp with a real map
and wall collision.
