# retroglyph-core

[![crates.io](https://img.shields.io/crates/v/retroglyph-core.svg)](https://crates.io/crates/retroglyph-core)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-core)](https://docs.rs/retroglyph-core)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=core)](https://app.codecov.io/gh/crates-lurey-io/retroglyph/flags)
[![license](https://img.shields.io/crates/l/retroglyph-core.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

The `no_std`-compatible foundation of [retroglyph](https://github.com/crates-lurey-io/retroglyph):
grid, tile, style, color, text, terminal, and event types, plus the `Backend` trait and a
dependency-free `Headless` test backend. Platform backends
([`retroglyph-crossterm`](https://crates.io/crates/retroglyph-crossterm),
[`retroglyph-software`](https://crates.io/crates/retroglyph-software)) and drawing helpers
([`retroglyph-widgets`](https://crates.io/crates/retroglyph-widgets)) are separate crates that
depend on this one.

## Quick start

```sh
cargo add retroglyph-core
```

```rust
# fn main() -> Result<(), core::convert::Infallible> {
use retroglyph_core::{Terminal, Color, Style, backend::Headless};

let mut term = Terminal::new(Headless::new(80, 24));
term.draw(|s| s.put((5, 5), '@', Style::new().fg(Color::GREEN)))?;
# Ok(())
# }
```

`Headless` never touches a real terminal or window, so this runs anywhere: including this README's
own doctest (see `src/lib.rs`'s `#[cfg(doctest)]` include). For a real backend, add
[`retroglyph-crossterm`](https://crates.io/crates/retroglyph-crossterm) or
[`retroglyph-software`](https://crates.io/crates/retroglyph-software) and see the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme)'s quick start.

See [docs.rs](https://docs.rs/retroglyph-core) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list and a
real backend quick start.

## Grid, drawing, and double buffering

`Grid` holds up to 256 layers, each cell carrying a glyph, foreground/background color, and sub-cell
pixel offsets; layer 0 is always allocated, layers 1+ are allocated on first write, so a
single-layer game pays zero overhead. See the
[`grid`](https://docs.rs/retroglyph-core/latest/retroglyph_core/grid/index.html) module docs for the
full layering/compositing model.

Draw through `Surface` (handed out by `Terminal::draw`/`Terminal::surface`): place characters with
`put()`, print strings with `print()`, or style a whole run at once with `with_style()`. See the
[`surface`](https://docs.rs/retroglyph-core/latest/retroglyph_core/surface/index.html) module docs.
`Terminal::present()` diffs the current frame against the previous one and forwards only the changed
cells to the backend; pixel backends request full frames instead, since sub-cell offsets can leave
orphaned pixels behind otherwise. `Terminal::retain_layer()` skips both that diff _and_ the app's
own redraw for one layer for the next frame, for content (e.g. a scrolled map) that's known
unchanged.

This crate is `no_std`-compatible: disable the `std` feature (requires an allocator). Useful for
embedded or kernel-space roguelikes; see the `std` feature below.

## Features

<!-- gen-features:start -->

Default features: `blend-modes`, `egc`, `indexed-quant`, `std`.

### `__math`

⚪ Optional.

Internal: exposes this crate's `math` shim (std-or-libm float dispatch) to `retroglyph-widgets` as
`#[doc(hidden)] pub mod math`, so both crates share one dispatch point instead of each vendoring
their own. No stability guarantee; not meant to be enabled by anyone but `retroglyph-widgets`.
Deliberately does not imply `float`: a lone `--features __math` build has no `math` module at all,
so this feature stays meaningful (and compiles) on its own rather than silently pulling in a backend
nobody asked for.

### `blend-modes`

🟢 Enabled by default.

Gates the four W3C separable `BlendMode` variants (`Screen`/`Dodge`/`Burn`/`Overlay`/ `Multiply`)
and pulls in the optional `alpha-blend` dependency. Needs a float backend (`std` or `libm`) for its
per-channel blend math, so this also turns on `float`.

`BlendMode::Linear` and `Grid::blit_alpha` are always available regardless of this feature: `Linear`
only needs `gem::Mix`, not `alpha-blend`.

### `dev`

⚪ Optional.

Forces `BuildMode::Dev` on in a build that would otherwise resolve to `Release`.

Can be used so an optimized build still reports development diagnostics (see the `dev` module).

### `egc`

🟢 Enabled by default.

Enables grapheme-cluster-aware text handling (via `unicode-segmentation`) for EGC-correct cell
diffing and layout.

### `float`

⚪ Optional.

Internal marker: some float backend (`std` or `libm`) is available. Never enable this directly; it's
turned on by `std` and `libm` below and gates `animate` and this crate's own `math` shim. Not meant
to be named by consumers, but not name-mangled either -- it's a plain capability flag, not
`__`-prefixed like `__math`, since `cargo`'s feature docs (and `cargo tree`) are clearer when every
feature that participates in resolution has an ordinary name.

### `indexed-quant`

🟢 Enabled by default.

Gates perceptual (Oklab) RGB → Indexed/ANSI quantization (`gem/space`) and `Color`'s `gem`-space
conversions (`to_srgb`/`from_srgb`/`lerp`/`from_hex`).

Without it, `Color::to_indexed`/ `Color::to_ansi` fall back to euclidean RGB cube-mapping instead of
failing to compile.

This is a capability flag, not a backend: it only turns on `gem`'s `space` module, and needs `std`
or `libm` (below) enabled separately to actually supply that module's float math -- see the
`compile_error!` in `src/lib.rs` for what happens if neither is on.

### `libm`

⚪ Optional.

Uses `libm`'s software float implementation (`roundf`/`fmaf`/`sinf`/`cosf`/`powf`) for `animate`'s
easing curves and `blend-modes`' separable channel math, via this crate's own `math` shim -- the
`no_std` side of that split. Turns on `float`. See `std` above for the alternative that prefers the
platform's own float intrinsics when available.

### `libm-arch`

⚪ Optional.

Alias for `libm`, matching `gem`'s and `alpha-blend`'s own `libm-arch` feature name so a reader
following their docs finds the name they expect. Already implied by `libm` above (which always
requests the `arch`-intrinsified `libm` dependency; see the `[dependencies.libm]` comment below), so
this exists purely for discoverability and never needs to be enabled on its own.

### `serde`

⚪ Optional.

Adds `Serialize`/`Deserialize` impls for `Color`, `Style`, `Size`, and (via `ixy`) `Pos`/`Rect`, so
a config file can round-trip a saved camera position, window geometry, or theme color.

`Color` serializes through its `Display`/`FromStr` round trip (e.g. `"bright-red"`, `"#ff8000"`)
rather than a derived structural form, so hand-edited TOML/JSON stays legible.

### `std`

🟢 Enabled by default.

Enables `gem/std` and `alpha-blend?/std`, and uses `std`'s float intrinsics (via this crate's `math`
shim) instead of `libm`'s software implementation for `animate` and `blend-modes`. Turns on `float`.

Disabling this feature (`--no-default-features`) builds this crate `no_std`.

### `testing`

⚪ Optional.

Enables `testing`'s `TestHarness`, which drives an `App` against `Headless` for tests, with
synthetic input queuing and frame-settling helpers.

Test-only surface, `no_std` + `alloc` compatible, off by default so it never ships in a release
build by accident.

<!-- gen-features:end -->
