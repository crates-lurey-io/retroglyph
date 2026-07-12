# retroglyph-software

A CPU rasterization backend for [retroglyph](https://github.com/crates-lurey-io/retroglyph): renders
grid cells into a pixel buffer and blits it to a window surface via
[`softbuffer`](https://crates.io/crates/softbuffer). `SoftwareBackend` holds configuration only
(font, grid size, scale); it builds a `SoftwareRenderer`, wrapped by
[`retroglyph-window`](https://crates.io/crates/retroglyph-window) into a real windowed `Backend`.

Optional features: `default-font` (an embedded VGA 8x16 bitmap font) and `tilesets` (PNG sprite
sheet tilesets with alpha blending).

See [docs.rs](https://docs.rs/retroglyph-software) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for a real backend quick
start.
