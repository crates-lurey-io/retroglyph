# 3. A map

Chapter 2 clamped `@` to the screen edge. This chapter replaces that with an actual level: a walled
room built once from a plain string, with movement blocked by the walls in it instead of by the edge
of the grid.

## Building the level

`Grid::from_charmap` turns a multi-line string into a `Grid`, calling a closure once per character
to decide that character's tile. It's the same helper the `11_sokoban` gallery example (a complete
small game) builds its level from:

```rust,ignore
{{#include ../../../examples/tutorial/03_a_map.rs:map}}
```

## Movement and collision

`try_move` no longer clamps to the screen; it asks `is_wall` whether the destination cell is
walkable before committing to it. Out-of-bounds counts as a wall too, so a level doesn't need a
border check of its own as long as it's, well, walled in:

```rust,ignore
{{#include ../../../examples/tutorial/03_a_map.rs:movement}}
```

Drawing composes the two pieces from the last two chapters: `Surface::blit` stamps the whole level
grid onto the screen in one call, then `@` is drawn on top of it the same way it always has been.

## Running it

```sh
cargo run --example 03_a_map --features crossterm
cargo run --example 03_a_map --features software
cargo run --example 03_a_map  # headless fallback, prints a few frames to stdout
```

Arrow keys move `@`, blocked by the walls (`#`). `q` or `Escape` quits.

This is the minimum viable tutorial: the how-to section covers what comes next task by task,
including drawing a status bar or log with `retroglyph-ui` widgets (see
[Draw a panel](../how-to/draw-a-panel.md) and [Handle a click](../how-to/handle-a-click.md)).
