# retroglyph-window

[![crates.io](https://img.shields.io/crates/v/retroglyph-window.svg)](https://crates.io/crates/retroglyph-window)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-window)](https://docs.rs/retroglyph-window)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=window)](https://app.codecov.io/gh/crates-lurey-io/retroglyph/flags)
[![license](https://img.shields.io/crates/l/retroglyph-window.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

A shared windowing layer for [retroglyph](https://github.com/crates-lurey-io/retroglyph)'s
window-based backends (software today; GL/wgpu are future candidates). `Input` and `Output` are
independent facets of `Backend`, which fits a terminal process (one type implements both) but not a
window, where an event loop owns input and a renderer owns output separately: this crate keeps that
split (`Presenter` is an `Output` supertrait; `WindowBackend` owns its own `Input` queue) and
reassembles both into one `Backend` via `winit`.

Most consumers don't depend on this crate directly; use
[`retroglyph-software`](https://crates.io/crates/retroglyph-software) instead, which depends on it.

Both graphical backends resolve every character through an ordered `FontChain` of 1-bit bitmap
fonts, so a fallback font built with `BitmapFont::with_charset` supplies coverage the primary font
has no mapping for (e.g. `unscii16` has no quadrants, sextants, or braille; the `legacy-computing`
feature below fills that gap). A chain glyph is drawn from the same 1-bit mask path as any other
glyph and takes the cell's foreground color, unlike a tileset sprite, which carries the colors it
was authored in. A single `BitmapFont` converts into a `FontChain` of one, so callers configuring
just one font never construct a chain explicitly.

## Quick start

```sh
cargo add retroglyph-window
```

## Features

<!-- gen-features:start -->
<details>

<summary>Default features: `winit`.</summary>

### `default-font`

⚪ Optional.

Embeds the Unscii 16 default font (`font::unscii16`).

Off by default so a consumer that supplies its own bitmap font pays nothing for the ~4 KB atlas; the
graphical backends' own `default-font` features forward to this one.

### `dev`

⚪ Optional.

Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build that
would otherwise compile them out (see `retroglyph_core::dev`).

Forwarded so a consumer of this crate can turn them on without adding a direct dependency on core
just to reach the flag.

### `legacy-computing`

⚪ Optional.

Embeds a generated block-elements/braille fallback font (`font::legacy_computing`): the 10 quadrant,
60 sextant, and 256 braille glyphs CP437 (and so `unscii16`) has no mapping for.

A separate opt-in from `default-font` rather than folded into it: this repertoire is a much more
niche/specialized addition (subcell image rendering, braille density tricks) than the base text
font, so a consumer that only wants CP437 text shouldn't pay for it. Computed at compile time by a
`const fn`, so this adds no font asset and no new dependency.

### `testing`

⚪ Optional.

Testing helpers for asserting glyph coverage (`testing::assert_glyphs_covered`,
`testing::uncovered_glyphs`), so a consumer can check a `FontChain` actually draws the characters it
cares about rather than silently falling back to the substituted solid block (retroglyph#1292).

### `tilesets`

⚪ Optional.

Shared PNG sprite/tileset support (`tileset` + `sprite_cache` modules, issue #366).

Both graphical backends' own `tilesets` features forward to this one.

### `winit`

🟢 Enabled by default.

The winit event loop and event translation (`run`, `translate`, `run_windowed`/`run_app`).

Renderer crates that only implement `Presenter` can disable this and depend solely on
`raw-window-handle`; loops other than winit (SDL2, tao, custom) bring their own driver against
`Presenter` + `WindowBackend`.

</details>
<!-- gen-features:end -->

A game never implements [`Presenter`] itself (that's `retroglyph-software`'s job), but a new
renderer backend does. This is the whole contract it implements, sized to fit a window from its own
cell geometry via [`WindowConfig::fit`]:

```rust
use retroglyph_core::backend::DrawCell;
use retroglyph_core::backend::Output;
use retroglyph_core::grid::Size;
use retroglyph_window::geometry::CellGeometry;
use retroglyph_window::presenter::{Presenter, WindowHandle};
use retroglyph_window::winit::WindowConfig;
use std::sync::Arc;

struct NullPresenter;

impl Output for NullPresenter {
    type Error = core::convert::Infallible;

    fn draw<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        Ok(())
    }

    fn draw_layers<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(10, 5)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resize(&mut self, _size: Size) {}
}

impl Presenter for NullPresenter {
    type SurfaceError = core::convert::Infallible;

    fn init_surface(&mut self, _window: Arc<dyn WindowHandle>) -> Result<(), Self::SurfaceError> {
        Ok(())
    }

    fn resize_surface(&mut self, _width: u32, _height: u32) {}

    fn present(&mut self) -> Result<(), Self::SurfaceError> {
        Ok(())
    }

    fn geometry(&self) -> CellGeometry {
        CellGeometry::new(8, 16, 1)
    }
}

let config = WindowConfig::fit(&NullPresenter, "demo", None, true);
assert_eq!((config.width(), config.height()), (80, 80));
```

Hand a real `Presenter` (e.g. `retroglyph-software`'s `SoftwareRenderer`) and a `config` like this
to [`run_windowed`]/[`run_app`] to actually open a window and drive the event loop.

[`Presenter`]:
  https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html
[`WindowConfig::fit`]:
  https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/run/struct.WindowConfig.html#method.fit
[`run_windowed`]:
  https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/run/fn.run_windowed.html
[`run_app`]: https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/run/fn.run_app.html

See [docs.rs](https://docs.rs/retroglyph-window) for the API.
