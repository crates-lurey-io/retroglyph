//! [`WindowBackend`]: the generic [`Backend`](retroglyph_core::Backend)
//! implementation for windowed presenters.

use crate::presenter::Presenter;
use retroglyph_core::backend::Backend;
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use std::collections::VecDeque;
use std::time::Duration;

/// A [`Backend`] built from a [`Presenter`] plus an input event queue.
///
/// `Backend` fuses input and output, which does not fit a window: some event
/// loop owns input, while a per-renderer surface owns output. `WindowBackend`
/// reunites the two so [`Terminal`](retroglyph_core::Terminal) gets the full
/// `Backend` it needs, while renderer crates implement only [`Presenter`]:
///
/// ```text
/// event loop.push_event(e) ──> VecDeque<Event> ──> app.poll_event()
///                                                        │
///                                                        v
///                                             Terminal<WindowBackend<P>>
///                                                        │
///                              draw / flush / resize     v
///                              ◄────────────────────  WindowBackend
///                                                        │
///                                                        v
///                                                 P: Presenter (output)
/// ```
///
/// With the `winit` feature enabled, `winit::run_windowed` and
/// `winit::run_app` own the event loop, call `push_event` as winit events
/// are translated, and call [`Presenter::present`] once per frame; callers
/// never touch `WindowBackend` directly. With `winit` disabled,
/// `retroglyph-window` exports no event loop at all: a caller driving its
/// own loop (SDL2, tao, a custom driver) constructs
/// `WindowBackend::new(presenter)` itself, calls `push_event` for each
/// translated input event, and calls `Terminal::present` (which drives
/// `Presenter::flush`) plus `presenter_mut().present()` once per frame.
///
/// # Example: driving without `winit`
///
/// ```rust
/// use retroglyph_core::{Backend, Event, Pos, Size, Terminal, Tile};
/// use retroglyph_window::{Presenter, WindowBackend, WindowHandle};
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// struct NullPresenter;
///
/// impl Presenter for NullPresenter {
///     type Error = core::convert::Infallible;
///     type SurfaceError = core::convert::Infallible;
///
///     fn draw<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
///     where
///         I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
///     {
///         Ok(())
///     }
///
///     fn draw_layers<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
///     where
///         I: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
///     {
///         Ok(())
///     }
///
///     fn flush(&mut self) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn size(&self) -> Size {
///         Size { width: 4, height: 2 }
///     }
///
///     fn clear(&mut self) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn resize(&mut self, _size: Size) {}
///
///     fn init_surface(&mut self, _window: Arc<dyn WindowHandle>) -> Result<(), Self::SurfaceError> {
///         Ok(())
///     }
///
///     fn resize_surface(&mut self, _width: u32, _height: u32) {}
///
///     fn present(&mut self) -> Result<(), Self::SurfaceError> {
///         Ok(())
///     }
///
///     fn cell_size(&self) -> (u32, u32) {
///         (8, 16)
///     }
/// }
///
/// // A caller driving its own loop (SDL2, tao, a hand-rolled driver) builds
/// // `WindowBackend` directly -- no `winit` feature required.
/// let backend = WindowBackend::new(NullPresenter);
/// let mut term = Terminal::new(backend);
///
/// // The loop pushes each translated input event onto the queue...
/// term.backend_mut().push_event(Event::FocusGained);
///
/// // ...and the app drains it through the normal `Terminal` polling API,
/// // which never blocks for `WindowBackend`.
/// while term.poll(Duration::ZERO).is_some() {}
///
/// // Once per frame: `Terminal::present` diffs the grid and drives
/// // `Presenter::flush`, then the caller drives `Presenter::present` itself
/// // to push pixels to the window.
/// term.present().unwrap();
/// term.backend_mut().presenter_mut().present().unwrap();
/// ```
///
/// [`poll_event`](Backend::poll_event) never blocks: frame timing is owned by
/// the event loop, not by input waits.
pub struct WindowBackend<P: Presenter> {
    presenter: P,
    events: VecDeque<Event>,
}

impl<P: Presenter> WindowBackend<P> {
    /// Wrap a presenter, creating an empty event queue.
    #[must_use]
    pub const fn new(presenter: P) -> Self {
        Self {
            presenter,
            events: VecDeque::new(),
        }
    }

    /// The wrapped presenter.
    #[must_use]
    pub const fn presenter(&self) -> &P {
        &self.presenter
    }

    /// The wrapped presenter, mutably.
    pub const fn presenter_mut(&mut self) -> &mut P {
        &mut self.presenter
    }

    /// Unwrap into the presenter, discarding queued events.
    #[must_use]
    pub fn into_presenter(self) -> P {
        self.presenter
    }
}

impl<P: Presenter> Backend for WindowBackend<P> {
    type Error = P::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
    {
        self.presenter.draw(content)
    }

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
    {
        self.presenter.draw_layers(content)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.presenter.flush()
    }

    fn size(&self) -> Size {
        self.presenter.size()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.presenter.clear()
    }

    fn resize(&mut self, size: Size) {
        self.presenter.resize(size);
    }

    fn needs_full_frame(&self) -> bool {
        self.presenter.needs_full_frame()
    }

    fn composites_layers(&self) -> bool {
        self.presenter.composites_layers()
    }

    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        // Non-blocking by design: the caller's event loop drives frame
        // timing, so there is nothing to sleep on here.
        self.events.pop_front()
    }

    fn push_event(&mut self, event: Event) {
        self.events.push_back(event);
    }

    fn set_cursor_visible(&mut self, _visible: bool) {
        // No hardware text cursor in windowed mode; games draw their own.
    }

    fn set_cursor_position(&mut self, _position: Pos) {
        // No hardware text cursor in windowed mode.
    }
}
