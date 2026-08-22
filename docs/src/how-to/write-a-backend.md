# Write a backend

The six backends this workspace ships cover terminals, wasm terminals, and CPU/GPU windowed
rendering. If none of them fit (a different terminal emulator protocol, a custom hardware display,
an existing rendering pipeline you need to plug retroglyph into), implement the trait directly.

There are two levels to implement at, depending on what you're building:

- A full `Backend` from scratch, for anything that isn't an existing `winit` window: implement
  [`Output`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Output.html),
  [`Input`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Input.html), and
  [`Cursor`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Cursor.html).
- A `Presenter`, to drop into the existing `retroglyph-window` `winit` event loop (`run_windowed`)
  and get windowing, input translation, and DPI handling for free: implement
  [`Presenter`](https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html),
  an `Output` supertrait.

## `Output`, `Input`, `Cursor`

`Backend` itself is a blanket impl with no methods of its own:

```rust,ignore
pub trait Backend: Output + Input + Cursor {}
impl<T: Output + Input + Cursor> Backend for T {}
```

so once a type implements all three facet traits, it's a `Backend`; there's nothing extra to write.

**`Output`** is the one required piece: draw cells to the display and flush them. The minimum
implementation is
[`draw_layers`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Output.html#tymethod.draw_layers),
[`flush`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Output.html#tymethod.flush),
[`size`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Output.html#tymethod.size),
and
[`clear`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Output.html#tymethod.clear).
`Terminal::present` always calls `draw_layers`, never `draw` directly: for a backend that renders
one glyph per cell (the common case, `composites_layers` left at its default `false`), `present`
pre-flattens every allocated layer into one before calling in, so layers above 0 still show up
without the backend doing any compositing itself. Only a pixel/GPU backend that needs true per-pixel
layering (transparency, sub-cell offsets bleeding between layers) needs to return `true` from
`composites_layers` and do that compositing itself.

**`Input`** needs only
[`poll_event`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Input.html#tymethod.poll_event).
If your backend never receives events from outside its own polling (reading a real terminal's event
stream, for example), that's the whole implementation:
[`push_event`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Input.html#method.push_event)
defaults to a no-op. A backend fed externally (a window event loop's callbacks, a test harness
injecting synthetic input) overrides `push_event` to queue what it's handed for `poll_event` to
return later.

**`Cursor`** is entirely optional: `impl Cursor for MyBackend {}` is a complete implementation for a
backend with no text cursor to manage (any pixel/windowed backend where the game draws its own
cursor, if it wants one at all). Override
[`set_cursor_visible`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Cursor.html#method.set_cursor_visible)
and
[`set_cursor_position`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Cursor.html#method.set_cursor_position)
for a backend that does manage one (a terminal's own cursor, via its escape sequences).

`Headless` (`crates/core/src/backend/headless.rs`) is the shortest real implementation in the
workspace and the best reference to read start to finish: it implements all three traits in well
under 300 lines with no platform dependency at all.

## `Presenter`

`Presenter` is an `Output` supertrait plus window-surface lifecycle methods, with no input methods
of its own (the `retroglyph-window` event loop owns input and forwards translated events into its
own queue). Implement:

- [`init_surface`](https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html#tymethod.init_surface):
  create your platform surface (a GPU context, a pixel buffer target) from the
  [`WindowHandle`](https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.WindowHandle.html)
  the loop hands you.
- [`resize_surface`](https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html#tymethod.resize_surface)
  and
  [`present`](https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html#tymethod.present).
- [`cell_size`](https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html#tymethod.cell_size),
  in physical pixels, so the windowing layer can convert between window size and grid dimensions.

`retroglyph-software`'s `SoftwareRenderer` is the simplest of the three shipped `Presenter`
implementations (`retroglyph-gl` and `retroglyph-wgpu` are the other two) and a reasonable starting
point to read before writing a fourth.

## See also

- [Choose a backend](./choose-a-backend.md): confirm none of the six shipped backends already fit
  before writing a new one.
