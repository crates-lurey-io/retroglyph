# Handle resize

A backend's display can resize out from under you: a user resizes their terminal (`crossterm`) or
drags a window's edge (`software`/`gl`/`wgpu`). retroglyph surfaces this as an ordinary event rather
than something you poll for, and one call resizes the grid to match.

## Reacting to `Event::Resize`

Drain events every tick, same as any other input, and look for
[`Event::Resize`](https://docs.rs/retroglyph-core/latest/retroglyph_core/event/enum.Event.html#variant.Resize).
It carries the new size in cells, already converted from whatever the backend measured (a real
terminal's reported columns/rows, or a window's physical pixel size divided by the font's cell
size):

```rust,ignore
{{#include ../../../examples/examples/14_resize.rs:handle_events}}
```

Apply it with
[`Terminal::resize`](https://docs.rs/retroglyph-core/latest/retroglyph_core/terminal/struct.Terminal.html#method.resize),
not `Backend::resize` directly: `Terminal::resize` resizes the grid _and_ calls the backend's own
`resize` to keep
[`Backend::size`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Output.html#method.size)
in sync. Calling `Backend::resize` on its own only updates what the backend reports; it does not
touch the grid, so your next draw call would still be working against the old dimensions.

## Draw at `term.area()`, not a hardcoded size

The part that actually needs a resize handler to matter: never assume a fixed grid size. Read
[`Terminal::area`](https://docs.rs/retroglyph-core/latest/retroglyph_core/terminal/struct.Terminal.html#method.area)
fresh every frame and lay out relative to it, so whatever size the resize handler applies is
immediately reflected in the next draw:

```rust,ignore
let area = term.area();
if area.width() == 0 || area.height() == 0 {
    return;
}
```

## Redraw the whole area on resize, not just what changed

`Terminal::present` only sends a backend the cells that changed since the last frame, and
`Terminal::resize` preserves overlapping content across the resize rather than clearing it. That
combination means a cell your draw code never explicitly touches keeps showing whatever was there
before, indefinitely, on any backend that doesn't clear its own surface on resize. Shrinking a
window and then growing it back leaves stale glyphs sitting in what's now the middle of the frame if
your draw code only paints an outline.

The fix isn't a `Terminal` method: it's that a resize-aware draw function fills its _entire_ current
area every frame (background first, then whatever's drawn on top), so every cell's on-screen content
is explained by that frame's own draw call. See `examples/examples/14_resize.rs` for a complete
example built around exactly this rule, runnable as:

```sh
cargo run --example 14_resize --features crossterm
cargo run --example 14_resize --features software
```

## See also

- [Choose a backend](./choose-a-backend.md), if you're deciding between a terminal and a windowed
  backend up front.
