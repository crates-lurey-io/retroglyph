# retroglyph-widgets

Immediate-mode drawing helpers for [retroglyph](https://github.com/crates-lurey-io/retroglyph): box
borders, filled panels, gauges, tables, sparklines, and a small constraint-based layout splitter
(`split_h`/`split_v` with ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints), plus
hover/click/drag/focus interaction tracking. Every widget is a builder struct that draws itself into
a `Terminal` and retains no state of its own; depends only on
[`retroglyph-core`](https://crates.io/crates/retroglyph-core), so games that draw manually never
pull it in.

See [docs.rs](https://docs.rs/retroglyph-widgets) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list.
