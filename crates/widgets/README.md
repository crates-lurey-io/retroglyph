# retroglyph-widgets

[![crates.io](https://img.shields.io/crates/v/retroglyph-widgets.svg)](https://crates.io/crates/retroglyph-widgets)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-widgets)](https://docs.rs/retroglyph-widgets)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=widgets)](https://codecov.io/gh/crates-lurey-io/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph-widgets.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

Immediate-mode drawing helpers for [retroglyph](https://github.com/crates-lurey-io/retroglyph): box
borders, filled panels, gauges, tables, sparklines, and a small constraint-based layout splitter
(`split_h`/`split_v` with ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints), plus
hover/click/drag/focus interaction tracking. Every widget is a builder struct that draws itself into
a `Terminal` and retains no state of its own; depends only on
[`retroglyph-core`](https://crates.io/crates/retroglyph-core), so games that draw manually never
pull it in.

## Quick start

```toml
[dependencies]
retroglyph-core = "0.1"
retroglyph-widgets = "0.1"
```

```rust
use retroglyph_core::{Rect, Terminal, backend::Headless};
use retroglyph_widgets::{Gauge, Widget};

let mut term = Terminal::new(Headless::new(20, 1));
Gauge::new("HP", 0.75).render(Rect::new(0, 0, 20, 1), &mut term);
term.present().unwrap();
```

See [docs.rs](https://docs.rs/retroglyph-widgets) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list.
