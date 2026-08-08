# Theme a widget

Every widget in `retroglyph-ui` picks its colors from whichever style knobs you set on it directly
(`Button::style`/`hovered_style`, `Panel::border_style`/`fill_style`, and so on), independent of any
other widget on screen. That's fine for one widget; for an app with several, `.theme()` replaces the
per-widget, per-state style calls with one shared palette.

## `Theme`

[`Theme`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/struct.Theme.html) is a plain struct of
named color roles (`fg`, `panel_bg`, `hover_bg`, `press_bg`, `accent`, and the rest); build one, or
start from
[`Theme::DARK`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/struct.Theme.html#associatedconstant.DARK)
or
[`Theme::LIGHT`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/struct.Theme.html#associatedconstant.LIGHT).
Every widget that draws (`Panel`, `Tabs`, `List`, `Button`, `ProgressBar`, and the rest of the
[`widget`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/widget/index.html) module) has a
`.theme(theme)` builder method that derives every style it needs from that one `Theme` instead of
the widget's own per-state defaults:

```rust,ignore
{{#include ../../../examples/examples/17_theme_switch.rs:theme-widgets}}
```

`Theme` carries no reference to "the active theme": nothing here is global or thread-local. Each
draw call is handed whichever `Theme` value the app currently considers active, picked however the
app likes (a config setting, a `t` keypress, matching the terminal's own light/dark preference), and
every widget re-derives its colors from it fresh every frame.

## Switching at runtime

Because `.theme()` takes a plain value with no persistent state of its own, switching themes is just
picking a different `Theme` before the next frame's draw calls, including from inside a widget the
theme itself affects, like the toggle button below:

```rust,ignore
{{#include ../../../examples/examples/17_theme_switch.rs:theme-button}}
```

## Running it

```sh
cargo run --example 17_theme_switch --features crossterm
cargo run --example 17_theme_switch --features software
cargo run --example 17_theme_switch  # headless fallback, prints a few frames to stdout
```

## See also

- [Draw a panel](./draw-a-panel.md) and [Handle a click](./handle-a-click.md), for widgets typically
  themed together.
- `examples/examples/09_widgets_dashboard.rs`/`examples/examples/10_widgets_interaction.rs` for the
  hand-threaded `theme.*` style calls `.theme()` replaces.
