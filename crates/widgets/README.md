# retroglyph-widgets

[![crates.io](https://img.shields.io/crates/v/retroglyph-widgets.svg)](https://crates.io/crates/retroglyph-widgets)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-widgets)](https://docs.rs/retroglyph-widgets)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=widgets)](https://app.codecov.io/gh/crates-lurey-io/retroglyph/flags)
[![license](https://img.shields.io/crates/l/retroglyph-widgets.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

Immediate-mode drawing helpers for [retroglyph](https://github.com/crates-lurey-io/retroglyph): box
borders, filled panels, gauges, tables, lists, tab strips, buttons, sparklines, and a small
constraint-based layout splitter (`split_h`/`split_v` with ratatui-style
`Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints, plus const-generic `split_h_n`/`split_v_n`
siblings that return `[Rect; N]` instead of allocating a `Vec`), plus hover/click/drag/focus
interaction tracking. Every widget (`Panel`, `Gauge`, `Table`, `Sparkline`, `BoxBorder`, `List`,
`Tabs`, `Button`, `Scrollbar`, `ProgressBar`, `Modal`, `StatBar`, `Meter`, `Log`, `TextInput`, ...)
is a builder struct that draws itself into a `Surface` (an area-relative view over a `Grid`) via
`Widget`/`StatefulWidget` and retains no state of its own; state that outlives one render call (a
selection index, a scroll offset, a text field's value and cursor) lives in
`ListState`/`TextInputState` instead. A handful of things that are genuinely just functions
(`fill_rect`, `thumb_geometry`/`offset_for_pos`, `truncate`/`truncate_owned`) stay free functions
rather than pretending to be widgets. Depends only on
[`retroglyph-core`](https://crates.io/crates/retroglyph-core), so games that draw manually never
pull it in.

Alongside the widgets is the constraint-based `Rect` splitter above, with `Flex` alignment
(`Start`/`End`/`Center`/`SpaceBetween`/`SpaceAround`), similar to [ratatui](https://ratatui.rs)'s
layout system. `Fill(weight)` claims a share of the leftover space proportional to `weight` relative
to the other `Fill`/`Min`/`Max` panes in the same split (`Fill(1)` reproduces plain equal
distribution).

Three more independent layers build on top:

- `Widget`/`StatefulWidget` traits let callers box or store heterogeneous widgets, e.g. a
  `Vec<Box<dyn Widget>>` of panes to render each frame, backed by `ListState` for selection and
  scroll position. `AnimatedWidget`, a sibling of `StatefulWidget`, is for state that evolves with
  wall-clock time instead (`ScrollState`'s momentum/rubber-band physics, a `Tween`-driven
  transition), taking a `Frame` (the same one `App::update` already receives) alongside the state,
  so advancing and drawing happen in one call instead of two independently ordered ones. `Scrollbar`
  implements it directly, ticking `ScrollState` before drawing the thumb at the result.
  `ScrollState::apply` feeds a frame's resolved `Response::scroll_delta` straight into the wheel
  impulse, so a scrollable widget doesn't have to re-derive wheel handling from raw mouse events
  (see `13_combat_log`, which wires wheel scrolling into its `Log`/`Scrollbar` pair this way).
- `BoxStyle`, a Lip-Gloss-style box model (padding, border, margin) rendered into a standalone
  `Grid`. `Paragraph` (behind the `egc` feature) word-wraps text via `retroglyph-core`'s
  `TextLayout` and implements a `Measure` trait so a caller can size a pane to fit before rendering.
- `join_h`/`join_v` to compose several `Grid`s (e.g. `BoxStyle::render` output) into one;
  `retroglyph-core`'s `Surface::blit` stamps the result onto a surface.
- `Theme` (`Theme::DARK`/`Theme::LIGHT`, or a caller-built palette): named color roles (`border`,
  `accent`, `hover_bg`, ...) that every widget with a style knob can pick up via a `.theme(Theme)`
  builder method, optionally: a manual `.border_style(...)`/etc. call after `.theme(...)` still
  wins, and nothing requires a `Theme` at all.

See the `09_widgets_dashboard` and `15_outpost_dashboard` examples for all of the above wired
together in one UI, `17_theme_switch` for `Theme::DARK`/`Theme::LIGHT` switched live at runtime by a
keypress, or `19_weighted_fill` for `Fill(weight)`'s proportional splits.

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

<!-- gen-features:start -->

Default features: `std`.

### `dev`

⚪ Optional.

Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build that
would otherwise compile them out (see `retroglyph_core::dev`).

### `egc`

⚪ Optional.

Forwards to `retroglyph-core`'s `egc` feature.

Upgrades `Paragraph`'s word-wrap (always available) to grapheme-cluster-aware correctness.

### `libm`

⚪ Optional.

Uses `retroglyph-core`'s `libm` feature (the `no_std` float backend: scrollbar geometry,
gauge/sparkline/bar percentage rounding, scroll momentum decay) instead of `std`'s own float
intrinsics. See `std` below; a build needs exactly one of the two.

### `libm-arch`

⚪ Optional.

Alias for `libm`, matching `retroglyph-core`'s own `libm-arch` feature name.

### `serde`

⚪ Optional.

Adds `Serialize`/`Deserialize` impls for `Theme` and `Density`, forwarding to `retroglyph-core`'s
`serde` feature.

`Theme` round-trips through `Color`'s own `serde` impl.

### `std`

🟢 Enabled by default.

Enables `retroglyph-core/std`, whose float intrinsics back this crate's own float use (see `libm`
above for the `no_std` alternative).

Disabling this feature (`--no-default-features`) builds this crate `no_std` (requires an allocator
and one of `std`/`libm`; see the crate-level `compile_error!` in `src/lib.rs`).

<!-- gen-features:end -->

See [docs.rs](https://docs.rs/retroglyph-widgets) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list.
