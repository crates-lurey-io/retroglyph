# Draw a panel

[`Panel`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/struct.Panel.html), from
`retroglyph-ui`, is a bordered, optionally titled, filled rectangle: the box every other widget in
this crate typically sits inside. It's a plain
[`Widget`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/trait.Widget.html) like the rest of
the crate: build it, then render it into a
[`Surface`](https://docs.rs/retroglyph-core/latest/retroglyph_core/surface/struct.Surface.html)-shaped
area, no `Backend` type parameter involved.

## Rendering into a sub-area

`Surface::scope` narrows a surface to one rectangle so a widget draws relative to that rectangle's
own `(0, 0)` instead of the whole screen's. `11_sokoban`'s status pane is exactly this: a `Panel`
with a title, filled and bordered into the right-hand column reserved for it:

```rust,ignore
{{#include ../../../examples/examples/11_sokoban.rs:panel}}
```

`status_area` is one of the `Rect`s a `split_h` call produced earlier in the same function; see
[Split a layout](./split-a-layout.md) for where that comes from. Content drawn after the `Panel`
call (the move counter, the key legend) is offset from `status_area`'s own top-left, one cell in
from the border it just drew, matching the interior inset `Panel` reserves for its box outline.

## Styling and theming

`Panel::border_style`/`fill_style` set the outline and background directly; `.title()` (and
`.add_title()` for more than one, or a title on the bottom edge) adds a label into the top border.
For an app with more than one widget, prefer `.theme()` over hand-picking
`border_style`/`fill_style` so every widget's panel matches the same palette; see
[Theme a widget](./theme-a-widget.md).

## See also

- [Split a layout](./split-a-layout.md), for the `Rect` a `Panel` renders into.
- [Handle a click](./handle-a-click.md), for widgets that read pointer/keyboard input instead of
  only drawing.
- `examples/examples/11_sokoban.rs`, `examples/examples/09_widgets_dashboard.rs`, and
  `examples/examples/17_theme_switch.rs` for complete panels in context.
