# Handle a click

`retroglyph-ui` has no retained widget tree: nothing remembers "button A is at this rectangle"
between frames. Instead,
[`Interaction`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/interact/struct.Interaction.html)
tracks pointer and keyboard state across frames, and each frame's draw call re-declares where its
widgets are by calling into it, the same immediate-mode shape as drawing itself.

## Pairing a surface with `Interaction`

[`Interaction::frame`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/interact/struct.Interaction.html#method.frame)
wraps one frame: it hands back a
[`Ui`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/ui/struct.Ui.html), which pairs that
frame's `Surface` with the `Interaction` context so a single call
([`Ui::show`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/ui/struct.Ui.html#method.show))
both hit-tests and draws an
[`InteractiveWidget`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/widget/trait.InteractiveWidget.html)
like [`Button`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/widget/struct.Button.html) from
the one `id`/`rect` a call site names:

```rust,ignore
{{#include ../../../examples/examples/10_widgets_interaction.rs:click}}
```

`id` is any `Copy + Eq` type the app defines (an enum listing every interactive widget on screen,
typically); `Interaction<Id>` uses it to track which widget is hovered, pressed, and focused across
frames, and to resolve Tab/Shift+Tab cycling between them. `ui.show` returns a
[`Response`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/interact/struct.Response.html);
`.clicked()` is `true` for exactly one frame, the one where a press-then-release (or a drag that
stayed inside the widget) completed inside `rect`, or Enter/Space activated it while focused.

Before drawing, every event for the frame needs to reach the `Interaction`: feed each one to
`ui.interaction().handle_event(event)` (see `10_widgets_interaction.rs`'s `tick` for the full
event-draining loop this snippet sits inside) so hover/press/focus state reflects that frame's input
before any widget asks `ui.show` what happened to it.

## Styling by response state

`Button::style`/`hovered_style`/`pressed_style`/`focused_style` (or `.theme()`, see
[Theme a widget](./theme-a-widget.md)) pick the four states a click can pass through; the widget
itself decides which one applies each frame from the `Response` `ui.show` resolves internally, so
the call site never branches on hover/press by hand.

## Running it

```sh
cargo run --example 10_widgets_interaction --features crossterm
cargo run --example 10_widgets_interaction --features software
cargo run --example 10_widgets_interaction  # headless fallback, prints a few frames to stdout
```

## See also

- [Draw a panel](./draw-a-panel.md), for widgets that only draw and never read `Interaction`.
- `examples/examples/09_widgets_dashboard.rs` and `examples/examples/17_theme_switch.rs` for
  `Interaction` shared across several widget kinds (`Table`, `List`, `Tabs`) in one frame.
