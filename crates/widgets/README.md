# retroglyph-widgets

[![crates.io](https://img.shields.io/crates/v/retroglyph-widgets.svg)](https://crates.io/crates/retroglyph-widgets)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-widgets)](https://docs.rs/retroglyph-widgets)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=widgets)](https://app.codecov.io/gh/crates-lurey-io/retroglyph/flags)
[![license](https://img.shields.io/crates/l/retroglyph-widgets.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

Immediate-mode drawing helpers for [retroglyph](https://github.com/crates-lurey-io/retroglyph): box
borders, filled panels, gauges, tables, lists, tab strips, buttons, sparklines, and a small
constraint-based layout splitter (`split_h`/`split_v` with ratatui-style
`Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints), plus hover/click/drag/focus interaction tracking.
Every widget is a builder struct that draws itself into a `Surface` (an area-relative view over a
`Grid`) and retains no state of its own; depends only on
[`retroglyph-core`](https://crates.io/crates/retroglyph-core), so games that draw manually never
pull it in.

`Theme` (`Theme::DARK`/`Theme::LIGHT`, or a caller-built palette) is a set of named color roles --
every widget with a style knob has a matching `.theme(Theme)` builder method that maps those roles
onto it, optionally: nothing requires a `Theme` at all, and a manual `.border_style(...)`/etc. call
after `.theme(...)` still wins.

## Quick start

```sh
cargo add retroglyph-core retroglyph-widgets
```

```rust
use retroglyph_core::{Grid, Rect};
use retroglyph_widgets::{Gauge, Surface, Widget};

let area = Rect::new(0, 0, 20, 1);
let mut grid = Grid::new(20, 1);
Gauge::new("HP", 0.75).render(&mut Surface::new(&mut grid, area, 0));
```

A clickable widget (`Button`, `Scrollbar`, `List`, `Tabs`, or any `InteractiveWidget`) is shown
through `Ui`, which pairs one frame's `Surface` with an `Interaction<Id>` so a call site names an
area and an id once and gets both hit-testing and drawing from it:

```rust
use retroglyph_core::{Grid, Rect, Surface};
use retroglyph_widgets::{Button, Interaction};

#[derive(Clone, Copy, PartialEq, Eq)]
enum WidgetId {
    Save,
}

let mut grid = Grid::new(20, 1);
let mut interaction = Interaction::<WidgetId>::new();
let clicked = interaction.frame(&mut Surface::new(&mut grid, Rect::new(0, 0, 20, 1), 0), |ui| {
    ui.show(Rect::new(0, 0, 10, 1), WidgetId::Save, &Button::new("Save")).clicked()
});
assert!(!clicked); // nothing clicked yet: no input was fed in
```

A control that exists but can't be used right now ("Save" with no game loaded, say) disables a whole
`Ui` subtree via `Ui::enabled`, not a per-widget flag: the returned `Response` still reports
`hovered`, so a call site can explain why, but never an activation:

```rust
use retroglyph_core::{Grid, Rect, Surface};
use retroglyph_widgets::{Button, Interaction};

#[derive(Clone, Copy, PartialEq, Eq)]
enum WidgetId {
    Save,
}

let has_unsaved_changes = false;
let mut grid = Grid::new(20, 1);
let mut interaction = Interaction::<WidgetId>::new();
interaction.frame(&mut Surface::new(&mut grid, Rect::new(0, 0, 20, 1), 0), |ui| {
    let mut ui = ui.enabled(has_unsaved_changes);
    let response = ui.show(Rect::new(0, 0, 10, 1), WidgetId::Save, &Button::new("Save"));
    if response.hovered() && response.disabled() {
        // draw a tooltip: "Nothing to save"
    }
});
```

## Features

### `dev`

⚪ Optional. Forwards `retroglyph-core`'s `dev` feature, forcing development diagnostics on in a
build that would otherwise compile them out.

### `egc`

⚪ Optional. Forwards to `retroglyph-core`'s `egc` feature; upgrades `Paragraph`'s word-wrap (always
available) to grapheme-cluster-aware correctness.

### `serde`

⚪ Optional. `Serialize`/`Deserialize` impls for `Theme` and `Density`, forwarding to
`retroglyph-core`'s `serde` feature (`Theme` round-trips through `Color`'s own `serde` impl).

See [docs.rs](https://docs.rs/retroglyph-widgets) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list.
