# retroglyph-window

[![crates.io](https://img.shields.io/crates/v/retroglyph-window.svg)](https://crates.io/crates/retroglyph-window)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-window)](https://docs.rs/retroglyph-window)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=window)](https://codecov.io/gh/crates-lurey-io/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph-window.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

A shared windowing layer for [retroglyph](https://github.com/crates-lurey-io/retroglyph)'s
window-based backends (software today; GL/wgpu are future candidates). `Backend` fuses input and
output, which fits a terminal process but not a window, where an event loop owns input and a
renderer owns output separately -- this crate splits the two apart and reassembles them into one
`Backend` via `winit`.

Most consumers don't depend on this crate directly; use
[`retroglyph-software`](https://crates.io/crates/retroglyph-software) instead, which depends on it.

## Quick start

```toml
[dependencies]
retroglyph-window = "0.1"
```

A game never implements [`Presenter`] itself -- that's `retroglyph-software`'s job -- but a new
renderer backend does. This is the whole contract it implements, sized to fit a window from its own
cell geometry via [`WindowConfig::fit`]:

```rust
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_window::winit::WindowConfig;
use retroglyph_window::{Presenter, WindowHandle};
use std::sync::Arc;

struct NullPresenter;

impl Presenter for NullPresenter {
    type Error = core::convert::Infallible;
    type SurfaceError = core::convert::Infallible;

    fn draw<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        Ok(())
    }

    fn draw_layers<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Size {
        Size { width: 10, height: 5 }
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resize(&mut self, _size: Size) {}

    fn init_surface(&mut self, _window: Arc<dyn WindowHandle>) -> Result<(), Self::SurfaceError> {
        Ok(())
    }

    fn resize_surface(&mut self, _width: u32, _height: u32) {}

    fn present(&mut self) -> Result<(), Self::SurfaceError> {
        Ok(())
    }

    fn cell_size(&self) -> (u32, u32) {
        (8, 16)
    }
}

let config = WindowConfig::fit(&NullPresenter, "demo", None);
assert_eq!((config.width(), config.height()), (80, 80));
```

Hand a real `Presenter` (e.g. `retroglyph-software`'s `SoftwareRenderer`) and a `config` like this
to [`run_windowed`]/[`run_app`] to actually open a window and drive the event loop.

[`Presenter`]: https://docs.rs/retroglyph-window/latest/retroglyph_window/trait.Presenter.html
[`WindowConfig::fit`]:
  https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/struct.WindowConfig.html#method.fit
[`run_windowed`]:
  https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/fn.run_windowed.html
[`run_app`]: https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/fn.run_app.html

See [docs.rs](https://docs.rs/retroglyph-window) for the API.
