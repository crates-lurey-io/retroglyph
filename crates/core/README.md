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

## Features

Default features: `std`, `egc`, `color-space`.

### `color-space`

🟢 Enabled by default.

Gates perceptual color-space math: `gem/libm`, `alpha-blend` (the W3C separable blend modes on
`BlendMode`), and its own `libm`. gem's pixel-format layer (`rgb`/`gray`/`alpha`/`channel`) is
always available regardless of this feature.

### `dev`

⚪ Optional.

Forces `BuildMode::Dev` on in a build that would otherwise resolve to `Release`.

Can be used so an optimized build still reports development diagnostics (see the `dev` module).

### `egc`

🟢 Enabled by default.

Enables grapheme-cluster-aware text handling for EGC-correct cell diffing and layout.

### `serde`

⚪ Optional.

Adds `Serialize`/`Deserialize` impls for `Color`, `Style`, `Size`, and other structs.

### `std`

🟢 Enabled by default.

Enables `gem/std` and `alpha-blend?/std`.

Disabling this feature (`--no-default-features`) builds this crate `no_std`.

### `testing`

⚪ Optional.

Enables `::testing`'s `TestHarness`, which drives an `App` against `Headless` for tests.

Includes synthetic input queuing and frame-settling helpers.
