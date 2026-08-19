# Split a layout

`retroglyph-ui`'s layout engine divides one `Rect` into several, ratatui-style: no widget tree, no
retained layout state, just a function that takes an area and a list of constraints and hands back
the resulting `Rect`s for that one frame.

## `split_h` and `split_v`

[`split_h`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/layout/fn.split_h.html) divides a
`Rect` into side-by-side columns;
[`split_v`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/layout/fn.split_v.html) divides it
into stacked rows. Both take the same
[`Constraint`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/layout/enum.Constraint.html) list,
one per resulting pane, and hand back a `Vec<Rect>` in the same order:

```rust,ignore
{{#include ../../../examples/examples/19_weighted_fill.rs:layout}}
```

## Choosing a `Constraint`

- `Constraint::Fixed(n)` reserves exactly `n` columns/rows, first, regardless of the other panes.
- `Constraint::Fill(weight)` divides whatever's left over _after_ every `Fixed`/`Min`/`Max` pane is
  reserved, in proportion to each pane's own weight: `Fill(1)`, `Fill(2)`, `Fill(3)` splits the
  remainder 1:2:3, not into three equal thirds.
- `Constraint::Min(n)`/`Constraint::Max(n)` floor or cap a pane's share of the fill remainder; they
  weigh `1` in that division regardless of `n`.

`19_weighted_fill` is a complete, static reference for all four combined: equal thirds, a weighted
ratio, a `Fixed` pane plus a weighted remainder, and `Min`/`Max` mixed with `Fill`:

```sh
cargo run --example 19_weighted_fill --features crossterm
cargo run --example 19_weighted_fill --features software
cargo run --example 19_weighted_fill  # headless fallback, prints a few frames to stdout
```

## Nesting a split

Neither function is aware of the other: `split_v`'s output `Rect`s are ordinary `Rect`s, so a pane
from one call is exactly what the other call's first argument expects. `11_sokoban`'s screen layout
is a `split_v` (a one-row title bar over the rest of the screen) feeding one of its rows into a
`split_h` (the play field next to the status [`Panel`](./draw-a-panel.md)), and there's no limit to
how many levels deep that composes.

## `Layout`: spacing and `Flex` alignment

[`Layout`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/layout/struct.Layout.html) wraps
`split_h`/`split_v` with two optional builder calls, for the cases plain `split_h`/`split_v` don't
cover:

```rust,ignore
Layout::vertical([Constraint::Fixed(1), Constraint::Fill(1)])
    .spacing(1)
    .flex(Flex::Center)
    .split(area)
```

`.spacing(n)` carves a fixed-cell gap (or, via `Spacing::Overlap(n)`, a shared border) between every
adjacent pair of panes; `split_with_gaps` returns those gap `Rect`s alongside the panes for drawing
dividers into. `.flex(Flex)` controls where leftover space goes when constraints don't consume a
`Rect`'s full extent (every pane is `Fixed`, say, and they don't add up to the whole width):
[`Flex`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/layout/enum.Flex.html)'s `Start` (the
default, matching plain `split_h`/`split_v`: leftover space trails after the last pane), `Center`,
`End`, or `SpaceBetween`/`SpaceAround` to distribute gaps between panes instead.

## See also

- [Draw a panel](./draw-a-panel.md) and [Handle a click](./handle-a-click.md), for what typically
  goes inside a split pane.
- `retroglyph_core::grid::Rect` itself, if a split's ratio-based math is overkill and a layout is
  simpler to compute by hand.
