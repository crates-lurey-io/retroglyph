# Use a tileset

The three windowed backends (`software`, `gl`, `wgpu`) can composite PNG sprite sheets over or
instead of the bitmap font, via each crate's `tilesets` feature. `crossterm` and `terminal-wasm`
have no pixels to sprite, so the same draw call that places a sprite on a windowed backend falls
back to plain text glyphs there automatically, with no capability check or `cfg` on your part.

## Loading a sheet

A tileset is a sprite sheet PNG sliced into equally sized tiles, each mapped to a glyph via a
[`Codepage`](https://docs.rs/retroglyph-window/latest/retroglyph_window/tileset/enum.Codepage.html).
Build one with
[`TilesetOptions::builder`](https://docs.rs/retroglyph-window/latest/retroglyph_window/tileset/struct.TilesetOptions.html#method.builder)
and register it on the backend's builder before opening the window:

```rust,ignore
{{#include ../../../examples/examples/07_sprites_tileset.rs:tilesets}}
```

`Codepage::Custom` maps tiles to specific characters, in sheet order (here, `#`/`.`/`@`/`$` for a
four-tile sheet). `Codepage::Cp437` (the default) and `Codepage::Unicode { start }` are the other
two options, for sheets authored against one of those existing layouts instead.

Every graphical backend takes a `TilesetOptions` the same way, via
[`PresenterBuilder::tileset`](https://docs.rs/retroglyph-window/latest/retroglyph_window/trait.PresenterBuilder.html#tymethod.tileset)
(or, when writing code generic over the three backends, `retroglyph_window::PresenterBuilder`'s
`configure`-style hook, as `examples/examples/07_sprites_tileset.rs`'s own `configure` function does
to register the same tilesets across all three builder types).

## Drawing a sprite

Draw a single-cell sprite with the glyph it's mapped to, exactly like drawing any other cell:
`surface.put((x, y), '#', Style::default())` picks up the wall sprite from the sheet above instead
of the font glyph, on every backend that has one.

A sprite larger than one cell (a chest, a portrait, a boss) uses
[`Surface::put_span`](https://docs.rs/retroglyph-core/latest/retroglyph_core/surface/struct.Surface.html#method.put_span)
instead: it declares a footprint of `(width, height)` cells, anchored at one glyph, with the rest of
the footprint carrying the sprite's text fallback (what a cell backend actually prints, one
character per covered cell). One call, no capability check: a pixel backend blits one sprite across
the whole footprint, a cell backend prints the fallback glyphs. `Grid::span_owner` resolves any cell
inside a multi-cell sprite back to its anchor in O(1), which is what you want for hit-testing (e.g.
"did the player step on any part of the chest") instead of hand-rolled rectangle math.

## Recoloring a sprite per cell

[`Surface::with_tint`](https://docs.rs/retroglyph-core/latest/retroglyph_core/surface/struct.Surface.html#method.with_tint)
recolors a sprite for one draw call, without touching the underlying tileset. Use
[`Tint::Multiply`](https://docs.rs/retroglyph-core/latest/retroglyph_core/color/enum.Tint.html#variant.Multiply)
to darken toward black (a torchlight falloff around the player, for example) or
[`Tint::Mix`](https://docs.rs/retroglyph-core/latest/retroglyph_core/color/enum.Tint.html#variant.Mix)
to blend toward another color (highlighting an interactable object). Tints only affect sprites; a
cell backend, with no sprite pixels to recolor, renders unaffected.

`examples/examples/07_sprites_tileset.rs` is a complete, runnable reference for all of the above,
including multi-cell spans and tinting:

```sh
cargo run --example 07_sprites_tileset --features software
cargo run --example 07_sprites_tileset --features gl
cargo run --example 07_sprites_tileset --features wgpu
```

## See also

- [Choose a backend](./choose-a-backend.md) for which crate to add `tilesets` to.
