//! [`Windowed`]: pairs a [`PresenterBuilder`] with the window title `Launch` needs.

use core::fmt;

use retroglyph_core::app::{App, Launch, RunOptions};
use retroglyph_core::terminal::Terminal;

use crate::backend::WindowBackend;
use crate::presenter_builder::PresenterBuilder;
use crate::winit::EventLoopError;
use crate::winit::run::{WindowConfig, run_app_on};

/// Pairs a windowed backend's [`PresenterBuilder`] with the window title [`Launch`] needs to
/// open one.
///
/// `PresenterBuilder` (retroglyph#1192) describes renderer configuration only (grid, scale,
/// font, tileset): a window title isn't part of that surface, and adding one would mean
/// `retroglyph-software`'s headless pixel-test path (which never opens a window) carries a field
/// it has no use for. This wrapper is the seam that adds it back for the one call site that
/// does need it, [`Launch::launch`], without touching `PresenterBuilder` itself.
///
/// Generic over `B: PresenterBuilder` rather than duplicated per backend crate: `Launch` is
/// implemented here, once, for any `Windowed<B>`, so `retroglyph-software`, `retroglyph-gl`, and
/// `retroglyph-wgpu` need no impl (and no dependency on this crate's `winit` feature beyond what
/// they already carry) of their own to be launchable this way.
///
/// # Examples
///
/// ```no_run
/// use retroglyph_core::app::{App, Flow, Frame, Launch, RunOptions};
/// use retroglyph_core::backend::Backend;
/// use retroglyph_core::terminal::Terminal;
/// use retroglyph_window::presenter_builder::PresenterBuilder;
/// use retroglyph_window::winit::Windowed;
///
/// struct MyGame;
/// impl<B: Backend> App<B> for MyGame {
///     fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
///         Flow::Exit
///     }
/// }
///
/// # fn launch<B: PresenterBuilder + 'static>(builder: B) -> Result<(), Box<dyn std::error::Error>> {
/// // Opens a real window, so this example is `no_run`.
/// Windowed::new(builder, "demo").launch(MyGame, RunOptions::animated(60))?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Windowed<B> {
    builder: B,
    title: String,
}

impl<B: PresenterBuilder> Windowed<B> {
    /// Pairs `builder` with `title`, the window's title bar text.
    pub fn new(builder: B, title: impl Into<String>) -> Self {
        Self {
            builder,
            title: title.into(),
        }
    }
}

/// The error [`Windowed`]'s [`Launch`] impl can fail with.
///
/// Spans the two independent ways launching a windowed backend can fail: the presenter builder's
/// own [`PresenterBuilder::Error`] (a bad grid/font/tileset configuration), and
/// [`EventLoopError`] (winit's event loop failing to start or to run). Kept as a small enum
/// naming both rather than degraded to a `String`: a caller matching on it can still tell which
/// half failed, and `?` off of either [`PresenterBuilder::build_presenter`] or [`run_app_on`]
/// converts automatically.
#[derive(Debug)]
#[non_exhaustive]
pub enum WindowedLaunchError<E> {
    /// [`PresenterBuilder::build_presenter`] failed to build the presenter.
    Build(E),
    /// The winit event loop failed to start or failed while running.
    EventLoop(EventLoopError),
}

impl<E: fmt::Display> fmt::Display for WindowedLaunchError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build(err) => write!(f, "failed to build the presenter: {err}"),
            Self::EventLoop(err) => write!(f, "windowed event loop failed: {err}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for WindowedLaunchError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Build(err) => Some(err),
            Self::EventLoop(err) => Some(err),
        }
    }
}

impl<B> Launch for Windowed<B>
where
    B: PresenterBuilder,
{
    type Backend = WindowBackend<B::Presenter>;
    type Error = WindowedLaunchError<B::Error>;

    /// Builds `self`'s presenter, opens a window sized to fit it (paced by `options`), and
    /// drives `app` on it until it returns [`Flow::Exit`](retroglyph_core::app::Flow::Exit).
    ///
    /// [`RunOptions::idle_wake`](retroglyph_core::app::RunOptions::idle_wake) has no windowed
    /// meaning and is ignored; see [`WindowConfig::with_run_options`] for why.
    ///
    /// # Errors
    ///
    /// Returns [`WindowedLaunchError::Build`] if the presenter builder's configuration is
    /// invalid, or [`WindowedLaunchError::EventLoop`] if the event loop cannot be created or
    /// fails while running.
    fn launch<A>(self, app: A, options: RunOptions) -> Result<(), Self::Error>
    where
        A: App<Self::Backend> + 'static,
    {
        let presenter = self
            .builder
            .build_presenter()
            .map_err(WindowedLaunchError::Build)?;
        let config =
            WindowConfig::fit(&presenter, self.title, None, true).with_run_options(options);
        let terminal = Terminal::new(WindowBackend::new(presenter));
        run_app_on(config, terminal, app).map_err(WindowedLaunchError::EventLoop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_error_displays_and_sources_the_inner_error() {
        let err: WindowedLaunchError<std::io::Error> =
            WindowedLaunchError::Build(std::io::Error::other("bad grid"));
        assert!(err.to_string().contains("bad grid"));
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn event_loop_error_displays_and_sources_the_inner_error() {
        // `RecreationAttempt` is the one `EventLoopError` variant with no private fields, so it's
        // the only one constructible outside `winit` itself.
        let err: WindowedLaunchError<std::io::Error> =
            WindowedLaunchError::EventLoop(EventLoopError::RecreationAttempt);
        assert!(err.to_string().contains("windowed event loop failed"));
        assert!(std::error::Error::source(&err).is_some());
    }
}
