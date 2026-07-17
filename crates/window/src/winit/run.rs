//! The winit event loop and the windowed app drivers.
//!
//! [`run_windowed`] drives a raw `FnMut(&mut Terminal<..>)` closure;
//! [`run_app`] drives an [`App`](retroglyph_core::App). This is the inverted
//! driver: winit owns the loop and calls back into the app on each redraw,
//! so it cannot be core's generic
//! [`run_blocking`](retroglyph_core::run_blocking), which owns its own
//! `while` loop.

use super::translate::{
    physical_pos_from, pixel_to_cell, translate_key, translate_modifiers, translate_mouse_button,
};
use crate::backend::WindowBackend;
use crate::presenter::Presenter;
use retroglyph_core::Terminal;
use retroglyph_core::backend::Backend;
use retroglyph_core::event::{Event, KeyModifiers, MouseEvent, MouseEventKind, PhysicalPos};
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// The user-event payload type threaded through winit's [`EventLoop`]. A plain `u64` so
/// [`EventProxy`] stays trivially `Send`/`Sync`/`Clone`; see [`Event::Custom`]'s doc comment for
/// why the payload is opaque rather than a boxed value.
type UserEvent = u64;

/// A thread-safe handle for injecting [`Event::Custom`] events into a running windowed event
/// loop from another thread (network, audio, timer, ...).
///
/// Obtained via the `on_proxy` callback passed to [`run_windowed_with_proxy`]/
/// [`run_app_with_proxy`], invoked synchronously right after the event loop (and this proxy) is
/// created, before the loop starts blocking the calling thread. Clone it freely to hand a copy to
/// each worker thread that needs to wake the loop; wraps winit's own
/// [`EventLoopProxy`](winit::event_loop::EventLoopProxy), which is `Send + Sync` for any
/// `'static` payload, including `u64`.
#[derive(Clone, Debug)]
pub struct EventProxy(winit::event_loop::EventLoopProxy<UserEvent>);

impl EventProxy {
    /// Injects `id` as [`Event::Custom(id)`](Event::Custom) into the event loop's queue, waking
    /// it if it's asleep. The event surfaces through the app's normal `poll_event`/frame loop
    /// like any other [`Event`].
    ///
    /// # Errors
    ///
    /// Returns [`EventProxyClosed`] if the event loop has already exited.
    pub fn send_event(&self, id: u64) -> Result<(), EventProxyClosed> {
        self.0.send_event(id).map_err(|e| EventProxyClosed(e.0))
    }
}

/// Error returned by [`EventProxy::send_event`] when the event loop it targets has already
/// exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventProxyClosed(u64);

impl EventProxyClosed {
    /// The event id that could not be delivered.
    #[must_use]
    pub const fn into_inner(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EventProxyClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event loop closed")
    }
}

impl std::error::Error for EventProxyClosed {}

/// Window configuration for [`run_windowed`] / [`run_app`].
///
/// Deliberately renderer-agnostic: pixel dimensions, not grid/font/scale.
/// Use [`fit`](Self::fit) to derive the pixel size from a presenter's own
/// cell geometry.
// Five independent window attribute toggles (`fill_viewport`, `resizable`, `decorations`,
// `fullscreen`, `transparency`), not a state machine in disguise: each maps to one winit
// `WindowAttributes` builder call and is meaningful on its own.
#[allow(clippy::struct_excessive_bools)]
pub struct WindowConfig {
    title: String,
    width: u32,
    height: u32,
    target_fps: Option<u32>,
    fill_viewport: bool,
    resizable: bool,
    decorations: bool,
    min_size: Option<(u32, u32)>,
    max_size: Option<(u32, u32)>,
    initial_position: Option<(i32, i32)>,
    fullscreen: bool,
    transparency: bool,
}

impl WindowConfig {
    /// Size the window to exactly fit `presenter`'s grid:
    /// `cols x cell_w` by `rows x cell_h` physical pixels.
    ///
    /// This is why renderer crates don't need their own windowing code: the
    /// grid/cell geometry already lives behind [`Presenter::size`] and
    /// [`Presenter::cell_size`].
    ///
    /// `target_fps` is an optional frame-rate cap: `None` runs uncapped on native (the event
    /// loop re-renders as fast as the backend allows) or at display refresh on `wasm32` (always
    /// `requestAnimationFrame`-driven there regardless of this setting).
    #[must_use]
    pub fn fit<P: Presenter>(
        presenter: &P,
        title: impl Into<String>,
        target_fps: Option<u32>,
    ) -> Self {
        let grid = presenter.size();
        let (cell_w, cell_h) = presenter.cell_size();
        Self {
            title: title.into(),
            width: u32::from(grid.width) * cell_w,
            height: u32::from(grid.height) * cell_h,
            target_fps,
            fill_viewport: false,
            resizable: true,
            decorations: true,
            min_size: None,
            max_size: None,
            initial_position: None,
            fullscreen: false,
            transparency: false,
        }
    }

    /// The window title, as set by [`fit`](Self::fit).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Initial inner width in physical pixels, as computed by [`fit`](Self::fit).
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Initial inner height in physical pixels, as computed by [`fit`](Self::fit).
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The frame-rate cap passed to [`fit`](Self::fit), if any.
    #[must_use]
    pub const fn target_fps(&self) -> Option<u32> {
        self.target_fps
    }

    /// Sets whether to size (and keep resizing) the canvas to fill the browser viewport on
    /// `wasm32`, instead of the pixel size [`fit`](Self::fit) computed -- a full-screen,
    /// mobile-web-app feel for games that want it. Has no effect on native, where the OS window
    /// is already sized by [`fit`](Self::fit) and the window manager owns further resizing
    /// either way.
    ///
    /// Defaults to `false`: most demos/examples should render at their natural grid size
    /// (`cols x cell_w` by `rows x cell_h`) wherever they land on the page, not stretch to fill
    /// whatever viewport happens to be hosting them. Opt in explicitly for an app-like,
    /// full-screen game.
    #[must_use]
    pub const fn fill_viewport(mut self, fill_viewport: bool) -> Self {
        self.fill_viewport = fill_viewport;
        self
    }

    /// Sets whether the window can be resized by the user/window manager after creation.
    ///
    /// Defaults to `true` (winit's own default). Set to `false` for fixed-size retro windows
    /// where the grid is meant to stay put -- resizing a pseudo-graphic UI usually means picking
    /// a new grid size, not stretching cells, and most callers that care already size the window
    /// to their content via [`fit`](Self::fit).
    ///
    /// On `wasm32`, winit's web backend ignores this (there is no OS-level resize grip on a
    /// canvas); it's still applied for source-level parity with native, it just has no effect.
    #[must_use]
    pub const fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Sets whether the window has OS chrome: title bar, borders, close/minimize/maximize
    /// buttons.
    ///
    /// Defaults to `true` (winit's own default). Set to `false` for a borderless window
    /// (custom-drawn title bars, retro full-bleed layouts).
    ///
    /// On `wasm32`, winit's web backend ignores this (a canvas has no OS chrome to begin with);
    /// it's still applied for source-level parity with native, it just has no effect.
    #[must_use]
    pub const fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Sets the minimum inner (content) size in physical pixels.
    ///
    /// Defaults to no minimum.
    #[must_use]
    pub const fn min_size(mut self, width: u32, height: u32) -> Self {
        self.min_size = Some((width, height));
        self
    }

    /// Sets the maximum inner (content) size in physical pixels.
    ///
    /// Defaults to no maximum.
    #[must_use]
    pub const fn max_size(mut self, width: u32, height: u32) -> Self {
        self.max_size = Some((width, height));
        self
    }

    /// Sets the desired initial outer window position in physical pixels.
    ///
    /// Defaults to letting the platform choose.
    ///
    /// On `wasm32`, winit's web backend maps this to the canvas's `position: absolute`
    /// left/top, which only does anything if the page's CSS has already opted the canvas into
    /// absolute/relative positioning; otherwise normal document flow overrides it.
    #[must_use]
    pub const fn initial_position(mut self, x: i32, y: i32) -> Self {
        self.initial_position = Some((x, y));
        self
    }

    /// Sets whether to request borderless fullscreen (on the window's current monitor) at
    /// creation.
    ///
    /// Defaults to `false`. This only exposes borderless fullscreen, not winit's
    /// exclusive-fullscreen video-mode API: retro/terminal-style apps render a fixed cell grid,
    /// not a resolution-dependent 3D scene, so there is no benefit to an exclusive video-mode
    /// switch, only extra platform-specific complexity (enumerating
    /// [`VideoModeHandle`](winit::monitor::VideoModeHandle)s) for a mode real games would rarely
    /// want here.
    ///
    /// On `wasm32`, winit's web backend maps this to the browser's Fullscreen API
    /// (`Element.requestFullscreen`), which most browsers refuse to grant without a user
    /// gesture; requesting it unconditionally at window-creation time (before any gesture) is
    /// liable to silently fail there. Still applied for source-level parity with native.
    #[must_use]
    pub const fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Sets whether the window's background supports transparency (alpha blending with whatever
    /// is behind it).
    ///
    /// Defaults to `false` (winit's own default).
    ///
    /// On `wasm32`, winit's web backend ignores this (a canvas is already alpha-blended with the
    /// page behind it via normal CSS compositing); it's still applied for source-level parity
    /// with native, it just has no effect.
    #[must_use]
    pub const fn transparency(mut self, transparency: bool) -> Self {
        self.transparency = transparency;
        self
    }
}

/// Open a window and drive `app_loop` from the winit event loop.
///
/// On native this blocks the calling thread until the loop exits; on wasm it
/// returns immediately and the loop continues on `requestAnimationFrame`.
///
/// The closure receives `&mut Terminal<WindowBackend<P>>` and is called on
/// every frame tick. Window close pushes [`Event::Close`] into the event
/// queue rather than exiting: the game decides when to terminate.
///
/// # Errors
///
/// Returns [`winit::error::EventLoopError`] if the event loop cannot be
/// created or fails while running.
pub fn run_windowed<P, F>(
    config: WindowConfig,
    presenter: P,
    app_loop: F,
) -> Result<(), winit::error::EventLoopError>
where
    P: Presenter + 'static,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
{
    run_windowed_with_proxy(config, presenter, app_loop, |_proxy| {})
}

/// Same as [`run_windowed`], but also hands `on_proxy` an [`EventProxy`] for injecting
/// cross-thread events.
///
/// `on_proxy` is called synchronously right after the event loop (and the proxy) is created,
/// before this function starts blocking the calling thread on native. Use this over
/// [`run_windowed`] whenever another thread (network, audio, timer, ...) needs to wake the event
/// loop and deliver an [`Event::Custom`] to the app; `on_proxy` is the hook to hand a clone of the
/// proxy off to that thread before the loop takes over the calling thread.
///
/// # Examples
///
/// ```ignore
/// use retroglyph_core::event::Event;
/// use retroglyph_software::SoftwareBackendBuilder;
/// use retroglyph_window::winit::{WindowConfig, run_windowed_with_proxy};
/// use std::time::Duration;
///
/// let renderer = SoftwareBackendBuilder::new()
///     .grid_size(80, 25)
///     .scale(2)
///     .build()
///     .expect("backend init failed")
///     .run_headless();
/// let config = WindowConfig::fit(&renderer, "My Game", None);
///
/// run_windowed_with_proxy(
///     config,
///     renderer,
///     move |term| {
///         if let Some(Event::Custom(id)) = term.poll(Duration::from_millis(16)) {
///             // Handle the tick/network/audio result tagged `id`.
///             println!("got custom event {id}");
///         }
///     },
///     |proxy| {
///         // Runs before the blocking call below starts, so the proxy can be
///         // handed off to a worker thread up front.
///         std::thread::spawn(move || loop {
///             std::thread::sleep(Duration::from_secs(1));
///             if proxy.send_event(1).is_err() {
///                 break; // The window closed; stop ticking.
///             }
///         });
///     },
/// )
/// .expect("event loop failed");
/// ```
///
/// # Errors
///
/// Returns [`winit::error::EventLoopError`] if the event loop cannot be
/// created or fails while running.
pub fn run_windowed_with_proxy<P, F, O>(
    config: WindowConfig,
    presenter: P,
    app_loop: F,
    on_proxy: O,
) -> Result<(), winit::error::EventLoopError>
where
    P: Presenter + 'static,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
    O: FnOnce(EventProxy),
{
    run_windowed_with_proxy_and_exit_flag(
        config,
        presenter,
        app_loop,
        on_proxy,
        Rc::new(Cell::new(false)),
    )
}

/// Shared implementation behind [`run_windowed_with_proxy`] and
/// [`run_app_with_proxy`].
///
/// `exit_requested` is checked after every [`WindowEvent::RedrawRequested`] and, when set, drives
/// [`ActiveEventLoop::exit`] so the loop unwinds normally (see [`WindowApp::exit_requested`]'s doc
/// comment for why this can't be plumbed through `app_loop`'s return value instead).
/// [`run_windowed_with_proxy`] passes a flag nobody ever sets (a plain `FnMut(&mut Terminal<..>)`
/// closure has no way to reach it); [`run_app_with_proxy`] shares one with the closure it builds
/// around `app_loop`, which sets it on [`Flow::Exit`](retroglyph_core::Flow::Exit).
fn run_windowed_with_proxy_and_exit_flag<P, F, O>(
    config: WindowConfig,
    presenter: P,
    app_loop: F,
    on_proxy: O,
    exit_requested: Rc<Cell<bool>>,
) -> Result<(), winit::error::EventLoopError>
where
    P: Presenter + 'static,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
    O: FnOnce(EventProxy),
{
    let terminal = Terminal::new(WindowBackend::new(presenter));
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    on_proxy(EventProxy(event_loop.create_proxy()));

    #[cfg(not(target_arch = "wasm32"))]
    let frame_interval = config
        .target_fps
        .map(|fps| Duration::from_secs_f64(1.0 / f64::from(fps)));

    let attrs = WindowAttrs::from(&config);
    let app = WindowApp {
        terminal: Some(terminal),
        app_loop,
        window: None,
        title: config.title,
        init_size: InitWindowSize {
            width: config.width,
            height: config.height,
        },
        attrs,
        #[cfg(target_arch = "wasm32")]
        fill_viewport: config.fill_viewport,
        current_modifiers: KeyModifiers::NONE,
        cursor_px: (0.0, 0.0),
        active_touch: None,
        #[cfg(not(target_arch = "wasm32"))]
        frame_interval,
        #[cfg(not(target_arch = "wasm32"))]
        next_frame: std::time::Instant::now(),
        exit_requested,
        needs_redraw: true,
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app)
    }

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
        Ok(())
    }
}

/// Drive an [`App`](retroglyph_core::App) from the windowed event loop.
///
/// This is the inverted driver: winit owns the event loop and calls back
/// into the app on each redraw, rather than the app owning a `while` loop.
///
/// Each frame builds a [`Frame`](retroglyph_core::Frame) with a wall-clock
/// `dt` measured via [`web_time::Instant`] -- a plain [`std::time::Instant`]
/// re-export on native, backed by the browser's `Performance.now()` on
/// `wasm32` (where `std::time::Instant` itself is unavailable). Calls
/// [`step`](retroglyph_core::step).
///
/// On [`Flow::Exit`](retroglyph_core::Flow) the event loop exits gracefully
/// (via [`ActiveEventLoop::exit`]) instead of force-exiting the process, so
/// the stack unwinds normally and `Drop` impls up the call chain (unflushed
/// writes, GPU/surface teardown, app-level RAII) run before the process
/// exits. This works the same on wasm: winit's web backend implements
/// `ActiveEventLoop::exit` by stopping its `requestAnimationFrame`-driven
/// runner rather than leaving it a no-op.
///
/// # Errors
///
/// Returns [`winit::error::EventLoopError`] if the event loop cannot be
/// created or fails while running.
pub fn run_app<P, A>(
    config: WindowConfig,
    presenter: P,
    app: A,
) -> Result<(), winit::error::EventLoopError>
where
    P: Presenter + 'static,
    A: retroglyph_core::App<WindowBackend<P>> + 'static,
{
    run_app_with_proxy(config, presenter, app, |_proxy| {})
}

/// Same as [`run_app`], but also hands `on_proxy` an [`EventProxy`] for injecting cross-thread
/// events -- see [`run_windowed_with_proxy`] for when/why to use the `_with_proxy` variant over
/// the plain one.
///
/// # Errors
///
/// Returns [`winit::error::EventLoopError`] if the event loop cannot be
/// created or fails while running.
pub fn run_app_with_proxy<P, A, O>(
    config: WindowConfig,
    presenter: P,
    mut app: A,
    on_proxy: O,
) -> Result<(), winit::error::EventLoopError>
where
    P: Presenter + 'static,
    A: retroglyph_core::App<WindowBackend<P>> + 'static,
    O: FnOnce(EventProxy),
{
    let mut frame_count = 0u64;
    let mut last = web_time::Instant::now();
    let exit_requested = Rc::new(Cell::new(false));
    let exit_requested_in_loop = exit_requested.clone();
    run_windowed_with_proxy_and_exit_flag(
        config,
        presenter,
        move |term| {
            let now = web_time::Instant::now();
            let delta = now.duration_since(last);
            last = now;
            let frame = retroglyph_core::Frame {
                delta,
                frame: frame_count,
            };
            frame_count = frame_count.wrapping_add(1);
            if retroglyph_core::step(term, &mut app, &frame) == retroglyph_core::Flow::Exit {
                exit_requested_in_loop.set(true);
            }
        },
        on_proxy,
        exit_requested,
    )
}

/// Initial window dimensions used before the first Resized event.
struct InitWindowSize {
    width: u32,
    height: u32,
}

/// The subset of [`WindowConfig`]'s builder attributes applied once, up front, to
/// `Window::default_attributes()` in [`create_window_and_surface`](WindowApp::create_window_and_surface).
///
/// Grouped into its own type (rather than six more fields directly on [`WindowApp`]) since
/// they're only ever read in that one place, unlike `fill_viewport`, which also gates per-resize
/// behavior elsewhere.
// See `WindowConfig`'s matching `#[allow]` for why these bools are independent toggles, not a
// state machine.
#[allow(clippy::struct_excessive_bools)]
struct WindowAttrs {
    resizable: bool,
    decorations: bool,
    min_size: Option<(u32, u32)>,
    max_size: Option<(u32, u32)>,
    initial_position: Option<(i32, i32)>,
    fullscreen: bool,
    transparency: bool,
}

impl From<&WindowConfig> for WindowAttrs {
    fn from(config: &WindowConfig) -> Self {
        Self {
            resizable: config.resizable,
            decorations: config.decorations,
            min_size: config.min_size,
            max_size: config.max_size,
            initial_position: config.initial_position,
            fullscreen: config.fullscreen,
            transparency: config.transparency,
        }
    }
}

impl Default for WindowAttrs {
    /// Mirrors [`WindowConfig::fit`]'s defaults, for tests that construct a [`WindowApp`]
    /// directly without going through a [`WindowConfig`].
    fn default() -> Self {
        Self {
            resizable: true,
            decorations: true,
            min_size: None,
            max_size: None,
            initial_position: None,
            fullscreen: false,
            transparency: false,
        }
    }
}

/// The winit `ApplicationHandler`: owns the window, the terminal, and the
/// per-frame closure.
struct WindowApp<P: Presenter, F> {
    terminal: Option<Terminal<WindowBackend<P>>>,
    app_loop: F,
    window: Option<Arc<Window>>,
    title: String,
    init_size: InitWindowSize,
    /// See [`WindowConfig`]'s `resizable`/`decorations`/`min_size`/`max_size`/
    /// `initial_position`/`fullscreen`/`transparency` fields; applied once at window creation.
    attrs: WindowAttrs,
    /// See [`WindowConfig::fill_viewport`]. Only meaningful on `wasm32`; not
    /// even stored on native, where it would do nothing.
    #[cfg(target_arch = "wasm32")]
    fill_viewport: bool,
    /// Current modifier key state, updated by `ModifiersChanged` events.
    current_modifiers: KeyModifiers,
    /// Last known cursor position in physical pixels.
    cursor_px: (f64, f64),
    /// The finger currently treated as the pointer, if any.
    ///
    /// Touch input (mobile browsers, touchscreens) arrives as
    /// [`WindowEvent::Touch`], not as `CursorMoved`/`MouseInput`. The first
    /// finger down is adopted as "the pointer" and synthesized into the same
    /// left-button mouse events games already handle; other fingers are
    /// ignored until it lifts, so a stray second finger can't teleport the
    /// cursor mid-drag.
    active_touch: Option<u64>,
    /// Optional frame interval for `WaitUntil` throttling. `None` = unbounded.
    #[cfg(not(target_arch = "wasm32"))]
    frame_interval: Option<Duration>,
    /// Deadline for the next frame when `frame_interval` is set.
    #[cfg(not(target_arch = "wasm32"))]
    next_frame: std::time::Instant,
    /// Set by `app_loop` (specifically [`run_app_with_proxy`]'s closure) to request the event
    /// loop stop, instead of calling `std::process::exit` directly.
    ///
    /// `app_loop` is a plain `FnMut(&mut Terminal<..>)` with no return value and no
    /// [`ActiveEventLoop`] handle, so it can't call `event_loop.exit()` itself; it can only flip
    /// this shared flag. [`handle_window_event`](Self::handle_window_event) -- which runs
    /// `app_loop` on [`WindowEvent::RedrawRequested`] -- deliberately takes no
    /// [`ActiveEventLoop`] either, so unit tests can drive it without a live winit loop (see its
    /// doc comment). `ApplicationHandler::window_event`, which does have the `ActiveEventLoop`,
    /// checks this flag right after `handle_window_event` returns and calls `event_loop.exit()`
    /// if it's set, letting the stack unwind normally (`Drop` impls run) instead of
    /// force-terminating the process.
    exit_requested: Rc<Cell<bool>>,
    /// Set whenever something happened that the app loop should get a chance to react to:
    /// window creation, an input/window event, or an injected [`Event::Custom`]. Cleared once
    /// [`about_to_wait`](ApplicationHandler::about_to_wait) turns it into a `request_redraw()`
    /// call.
    ///
    /// Retro/terminal-style apps are event-driven, not animation-driven, so "nothing happened"
    /// should mean "render nothing new" -- see this field's use in `about_to_wait` for why that
    /// keeps the loop asleep (`ControlFlow::Wait`) instead of spinning at ~100% CPU redrawing an
    /// unchanged frame forever. Only consulted when `frame_interval` is `None`: a `target_fps`
    /// throttle already redraws unconditionally once its `WaitUntil` deadline passes, animation
    /// or not.
    needs_redraw: bool,
}

impl<P: Presenter, F> WindowApp<P, F> {
    /// Create the window and initialize the surface.
    ///
    /// Returns `Some(window)` on success, logs and returns `None` on failure.
    fn create_window_and_surface(&mut self, event_loop: &ActiveEventLoop) -> Option<Arc<Window>> {
        // On native, size the window to fit the grid (`WindowConfig::fit`)
        // and let the OS window manager own further resizing. On wasm, if
        // `fill_viewport` is set, there's no OS window to fit into -- the
        // canvas *is* the page -- so size it to the browser viewport
        // instead, for a full-screen, mobile-web-app feel; otherwise it's
        // sized the same as native (`init_size`, the natural grid size),
        // which is what most demos/examples want -- see
        // `WindowConfig::fill_viewport`'s doc comment. winit sets an inline
        // `width`/`height` style on the canvas matching whatever size we
        // request here; it does not derive that size from page CSS, so this
        // has to happen in Rust.
        //
        // Crucially, the viewport-filling size *must* be the viewport size
        // at the real (uncapped) device pixel ratio, not the DPR-capped size
        // used for the software backing store below. winit's wasm backend
        // converts whatever `PhysicalSize` we pass here back to a logical
        // (CSS pixel) size using `window.devicePixelRatio()` -- the actual,
        // uncapped ratio -- to set the canvas's inline `style.width`/
        // `style.height`. Handing it a DPR-capped physical size makes it
        // divide by a *larger* real DPR than the one used to compute that
        // size, so the resulting CSS size comes out smaller than the
        // viewport (the higher the real DPR above the cap, the more the
        // canvas visibly shrinks -- on a phone with DPR 3 and our 1.5 cap,
        // that's 50% of the screen). See `web_viewport_surface_physical_size`
        // for the separate, capped size used for the raster backing store.
        // On native, `init_size` is expressed in logical (1x) pixels --
        // `WindowConfig::fit` derives it from the presenter's grid/cell
        // geometry, which assumes an unscaled cell. Requesting that count
        // directly as a `PhysicalSize` on a HiDPI display asks winit/the OS
        // for a window with fewer true pixels than the monitor actually
        // has, so it gets upscaled blurrily to fill the same logical space
        // instead of rendering crisply at native resolution from the first
        // frame. Scaling by the primary monitor's `scale_factor` up front
        // (falling back to `1.0` when no monitor is available, e.g.
        // headless/CI) avoids that: see `physical_size_for`.
        #[cfg(not(target_arch = "wasm32"))]
        let physical_size = {
            let scale_factor = event_loop
                .primary_monitor()
                .map_or(1.0, |monitor| monitor.scale_factor());
            let (width, height) =
                physical_size_for(self.init_size.width, self.init_size.height, scale_factor);
            winit::dpi::PhysicalSize::new(width, height)
        };
        #[cfg(target_arch = "wasm32")]
        let physical_size = if self.fill_viewport {
            web_viewport_layout_physical_size().unwrap_or_else(|| {
                winit::dpi::PhysicalSize::new(self.init_size.width, self.init_size.height)
            })
        } else {
            winit::dpi::PhysicalSize::new(self.init_size.width, self.init_size.height)
        };
        #[cfg(target_arch = "wasm32")]
        let surface_physical_size = if self.fill_viewport {
            web_viewport_surface_physical_size().unwrap_or(physical_size)
        } else {
            physical_size
        };
        #[cfg(not(target_arch = "wasm32"))]
        let surface_physical_size = physical_size;

        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(physical_size)
            .with_resizable(self.attrs.resizable)
            .with_decorations(self.attrs.decorations)
            .with_transparent(self.attrs.transparency);
        let attrs = match self.attrs.min_size {
            Some((w, h)) => attrs.with_min_inner_size(winit::dpi::PhysicalSize::new(w, h)),
            None => attrs,
        };
        let attrs = match self.attrs.max_size {
            Some((w, h)) => attrs.with_max_inner_size(winit::dpi::PhysicalSize::new(w, h)),
            None => attrs,
        };
        let attrs = match self.attrs.initial_position {
            Some((x, y)) => attrs.with_position(winit::dpi::PhysicalPosition::new(x, y)),
            None => attrs,
        };
        let attrs = if self.attrs.fullscreen {
            attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
        } else {
            attrs
        };

        #[cfg(target_family = "wasm")]
        let attrs = {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs.with_append(true)
        };

        let window = Arc::new(match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                log::error!("window creation failed: {e}");
                event_loop.exit();
                return None;
            }
        });

        if let Some(term) = self.terminal.as_mut() {
            // Hand the presenter a windowing-library-agnostic handle (see
            // `Presenter::init_surface`); the winit window stays owned here.
            let handle: Arc<dyn crate::presenter::WindowHandle> = window.clone();
            if let Err(e) = term.backend_mut().presenter_mut().init_surface(handle) {
                log::error!("surface init failed: {e}");
                event_loop.exit();
                return None;
            }
            // Set the initial surface size (required on WASM before first present).
            // Deliberately `surface_physical_size`, not `physical_size`: the
            // raster backing store stays DPR-capped for present() cost even
            // though the canvas's CSS size (driven by `physical_size` via
            // winit above) matches the full, uncapped viewport.
            term.backend_mut()
                .presenter_mut()
                .resize_surface(surface_physical_size.width, surface_physical_size.height);
        }

        // Keep the canvas matching the browser viewport as it changes
        // (device rotation, browser window resize, address-bar
        // show/hide): winit only reacts to size changes we ask for
        // ourselves (`request_inner_size`), so a `resize` listener is
        // required to make this genuinely responsive rather than a
        // one-shot fit at startup. Only installed when `fill_viewport` is
        // set -- otherwise the canvas should stay at its natural grid size
        // regardless of viewport changes.
        #[cfg(target_arch = "wasm32")]
        if self.fill_viewport {
            install_viewport_resize_listener(&window);
        }

        // `WindowEvent::ThemeChanged` (handled in `handle_window_event`)
        // only fires on a *change*, so an app that never sees a system
        // theme change would otherwise never learn the starting one.
        // `Window::theme()` reflects the current system theme both on
        // native and on winit's web target (backed by the
        // `prefers-color-scheme` media query there), so query it once
        // up-front and synthesize the same event a live change would send.
        if let Some(theme) = window.theme()
            && let Some(term) = self.terminal.as_mut()
        {
            term.backend_mut().push_event(system_theme_event(theme));
        }

        Some(window)
    }
}

/// Scales a logical (1x) initial window size up to true physical pixels for
/// `scale_factor`, so [`create_window_and_surface`](WindowApp::create_window_and_surface)
/// can request a window sized to the primary monitor's actual resolution
/// from the first frame, instead of a too-small physical window the OS then
/// has to upscale blurrily to fill the same on-screen space.
///
/// Pure math, kept separate from `create_window_and_surface` so it's unit
/// -testable without a live winit event loop / monitor.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn physical_size_for(logical_width: u32, logical_height: u32, scale_factor: f64) -> (u32, u32) {
    (
        (f64::from(logical_width) * scale_factor).round() as u32,
        (f64::from(logical_height) * scale_factor).round() as u32,
    )
}

/// Maps winit's [`Theme`](winit::window::Theme) to the backend-agnostic
/// [`Event::ThemeChanged`], the only place that conversion needs to happen.
const fn system_theme_event(theme: winit::window::Theme) -> Event {
    use retroglyph_core::event::SystemTheme;
    match theme {
        winit::window::Theme::Light => Event::ThemeChanged(SystemTheme::Light),
        winit::window::Theme::Dark => Event::ThemeChanged(SystemTheme::Dark),
    }
}

/// Upper bound on the device pixel ratio used to size the canvas backing
/// store. Present cost is O(pixels), so an uncapped DPR (3 on many phones,
/// 2 on most laptops) quadruples or worse the per-frame rasterize/present
/// work for marginal crispness on a pseudo-graphic UI.
#[cfg(target_arch = "wasm32")]
const MAX_DEVICE_PIXEL_RATIO: f64 = 1.5;

/// The browser viewport's CSS width/height, or `None` if running outside a
/// browser `window` context. Shared by the two physical-size helpers below.
#[cfg(target_arch = "wasm32")]
fn web_viewport_css_size() -> Option<(f64, f64)> {
    let window = web_sys::window()?;
    let width = window.inner_width().ok()?.as_f64()?;
    let height = window.inner_height().ok()?.as_f64()?;
    Some((width, height))
}

/// The browser viewport size in true physical (device) pixels -- i.e. at the
/// real, uncapped `devicePixelRatio`.
///
/// Pass this to winit's `with_inner_size`/`request_inner_size` (and *only*
/// this -- never [`web_viewport_surface_physical_size`]). winit's wasm
/// backend always converts the `PhysicalSize` it's given back to a logical
/// (CSS pixel) size by dividing by the real `devicePixelRatio` to set the
/// canvas's inline style; handing it anything scaled by a different ratio
/// (like our DPR-capped surface size) makes the canvas's CSS size come out
/// smaller than the viewport.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn web_viewport_layout_physical_size() -> Option<winit::dpi::PhysicalSize<u32>> {
    let (width, height) = web_viewport_css_size()?;
    let dpr = web_sys::window()?.device_pixel_ratio();
    Some(winit::dpi::PhysicalSize::new(
        (width * dpr).round() as u32,
        (height * dpr).round() as u32,
    ))
}

/// Ratio to convert a pointer position winit reports (always in *real*,
/// uncapped-DPR physical pixels -- see `to_physical(super::scale_factor)` in
/// `winit`'s wasm `pointer.rs`) into the raster-backing-store pixel space
/// that [`Presenter::cell_size`](crate::presenter::Presenter::cell_size),
/// and therefore [`pixel_to_cell`], are expressed in.
///
/// `1.0` whenever `real_dpr` is already at or below `capped_dpr` (desktop,
/// non-Retina): no correction needed. Below that, taps/clicks land scaled
/// past their true position -- south-east of the intended cell, growing
/// with how far `real_dpr` exceeds the cap (2x at DPR 3 against a 1.5 cap).
/// Pure math, kept separate from [`wasm_pointer_scale`] so it's unit
/// -testable without a wasm window (hence `cfg(any(.., test))`: unused on a
/// native non-test build, whose `on_cursor_moved` hardcodes `scale = 1.0`
/// instead of calling this).
#[cfg(any(target_arch = "wasm32", test))]
fn dpr_pointer_scale(real_dpr: f64, capped_dpr: f64) -> f64 {
    (capped_dpr / real_dpr).min(1.0)
}

/// [`dpr_pointer_scale`] using the page's actual `devicePixelRatio` and
/// [`MAX_DEVICE_PIXEL_RATIO`]. `1.0` if no browser `window` is available.
#[cfg(target_arch = "wasm32")]
fn wasm_pointer_scale() -> f64 {
    web_sys::window().map_or(1.0, |w| {
        dpr_pointer_scale(w.device_pixel_ratio(), MAX_DEVICE_PIXEL_RATIO)
    })
}

/// The physical pixel size of the software renderer's raster backing store,
/// capped at [`MAX_DEVICE_PIXEL_RATIO`] for `present()` cost.
///
/// Deliberately *not* the size passed to winit (see
/// [`web_viewport_layout_physical_size`]): winit's `Resized` event always
/// reports back whatever physical size we last requested, so if this capped
/// size were also used for `request_inner_size`, the canvas's CSS size would
/// shrink below the viewport on any device whose real DPR exceeds the cap
/// (i.e. almost every phone).
#[cfg(target_arch = "wasm32")]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn web_viewport_surface_physical_size() -> Option<winit::dpi::PhysicalSize<u32>> {
    let (width, height) = web_viewport_css_size()?;
    let dpr = web_sys::window()?
        .device_pixel_ratio()
        .min(MAX_DEVICE_PIXEL_RATIO);
    Some(winit::dpi::PhysicalSize::new(
        (width * dpr).round() as u32,
        (height * dpr).round() as u32,
    ))
}

/// Re-requests the window's inner size to match the browser viewport on
/// every `resize` event, so the canvas keeps filling the screen instead of
/// staying pinned to its size at first paint.
#[cfg(target_arch = "wasm32")]
fn install_viewport_resize_listener(window: &Arc<Window>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    let Some(web_window) = web_sys::window() else {
        return;
    };
    let window = window.clone();
    let closure = Closure::<dyn FnMut()>::new(move || {
        // Only the uncapped layout size goes to winit; `on_resized` (fired
        // by the `Resized` event this triggers) independently recomputes
        // the DPR-capped surface size for the backing store.
        if let Some(size) = web_viewport_layout_physical_size() {
            let _ = window.request_inner_size(size);
        }
    });
    if web_window
        .add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())
        .is_ok()
    {
        // Leaked deliberately: the listener, and the closure it wraps, need
        // to live as long as the page does -- there's no window-teardown
        // hook on wasm to drop it from.
        closure.forget();
    }
}

impl<P, F> ApplicationHandler<UserEvent> for WindowApp<P, F>
where
    P: Presenter,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = self.create_window_and_surface(event_loop) {
            self.window = Some(window);
        }
        // First frame: nothing has "happened" yet in the input-event sense, but the app still
        // needs an initial render once the window/surface exists.
        self.needs_redraw = true;
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        self.handle_window_event(event);
        // `app_loop` (run on `RedrawRequested`, inside `handle_window_event`) can only signal
        // exit by setting `exit_requested` -- see its doc comment for why. Check it here, where
        // an `ActiveEventLoop` is actually available, and ask winit to exit gracefully instead of
        // the caller force-exiting the process.
        if self.exit_requested.get() {
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        self.handle_user_event(event);
    }

    fn about_to_wait(
        &mut self,
        #[cfg_attr(target_arch = "wasm32", allow(unused_variables))] event_loop: &ActiveEventLoop,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(interval) = self.frame_interval {
            // Throttled: sleep until the next frame deadline, then render
            // unconditionally -- a `target_fps` cap is an animation-style
            // frame rate, not an idle/event-driven one, so it always
            // redraws once its deadline passes regardless of `needs_redraw`.
            let now = std::time::Instant::now();
            if self.next_frame > now {
                event_loop
                    .set_control_flow(winit::event_loop::ControlFlow::WaitUntil(self.next_frame));
                return;
            }
            // Advance the deadline by one interval, clamping to now so a
            // slow frame doesn't cause a burst of catch-up renders.
            self.next_frame = (self.next_frame + interval).max(now);
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }
        // Uncapped (`target_fps: None`): only redraw if something actually happened since the
        // last one. Otherwise leave `ControlFlow` at its default `Wait` so the loop sleeps
        // instead of spinning at ~100% CPU re-rendering an unchanged frame every iteration --
        // retro/terminal-style apps are idle most of the time and event-driven, not
        // animation-driven, so "nothing happened" should mean "render nothing new". See
        // `needs_redraw`'s doc comment.
        if self.needs_redraw {
            self.needs_redraw = false;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

impl<P, F> WindowApp<P, F>
where
    P: Presenter,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
{
    /// Drain one injected user event into the [`WindowBackend`] queue as [`Event::Custom`].
    ///
    /// Extracted from the `ApplicationHandler::user_event` impl for the same reason as
    /// [`handle_window_event`](Self::handle_window_event): so the drain logic can be exercised in
    /// unit tests without a live [`ActiveEventLoop`]. There is only ever one event to drain per
    /// call -- winit calls `user_event` once per [`EventProxy::send_event`] -- so "drain" here
    /// means "push the one event this call carries", not draining a whole queue at once.
    fn handle_user_event(&mut self, event: UserEvent) {
        if let Some(term) = self.terminal.as_mut() {
            term.backend_mut().push_event(Event::Custom(event));
        }
        self.needs_redraw = true;
    }

    /// Dispatch a [`WindowEvent`] without requiring an [`ActiveEventLoop`].
    ///
    /// Extracted from the `ApplicationHandler` impl so the translation and
    /// event-buffer logic can be called directly in unit tests, where
    /// [`ActiveEventLoop`] is not constructable.
    fn handle_window_event(&mut self, event: WindowEvent) {
        // Every branch below (other than `RedrawRequested`, which *is* the render this flag
        // exists to gate) represents something the app loop should get a chance to react to on
        // the next frame -- see `needs_redraw`'s doc comment for why that matters for idle CPU.
        // Set unconditionally up front rather than per-arm: simpler, and the only event that must
        // *not* set it (`RedrawRequested`) already clears it again in `about_to_wait` right before
        // requesting this same redraw, so a same-tick `RedrawRequested` can't retrigger itself.
        if !matches!(event, WindowEvent::RedrawRequested) {
            self.needs_redraw = true;
        }
        match event {
            WindowEvent::CloseRequested => {
                // Push the event so the game loop can process it (save game,
                // confirm dialog, etc.).  Do not call event_loop.exit() here;
                // the game decides when to terminate.
                if let Some(term) = self.terminal.as_mut() {
                    term.backend_mut().push_event(Event::Close);
                }
            }
            WindowEvent::Resized(size) => self.on_resized(size),
            WindowEvent::CursorMoved { position, .. } => self.on_cursor_moved(position),
            WindowEvent::MouseInput { state, button, .. } => self.on_mouse_input(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.on_mouse_wheel(delta),
            WindowEvent::Touch(touch) => self.on_touch(touch),
            WindowEvent::ModifiersChanged(mods) => {
                self.current_modifiers = translate_modifiers(mods.state());
            }
            WindowEvent::ThemeChanged(theme) => {
                if let Some(term) = self.terminal.as_mut() {
                    term.backend_mut().push_event(system_theme_event(theme));
                }
            }
            WindowEvent::Focused(gained) => self.on_focus_changed(gained),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(term) = self.terminal.as_mut()
                    && let Some(e) = translate_key(event, self.current_modifiers)
                {
                    term.backend_mut().push_event(e);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.on_scale_factor_changed(scale_factor);
            }

            WindowEvent::RedrawRequested => {
                let Some(term) = self.terminal.as_mut() else {
                    return;
                };
                (self.app_loop)(term);
                if let Err(e) = term.backend_mut().presenter_mut().present() {
                    log::error!("frame present failed: {e}");
                }
            }

            _ => {}
        }
    }

    fn on_resized(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        // On wasm with `fill_viewport` set, `size` is whatever (uncapped)
        // physical size we last handed winit for CSS layout purposes -- not
        // the backing store size. Recompute the DPR-capped surface size
        // independently so the raster buffer doesn't silently lose its cap
        // on every resize. Without `fill_viewport`, the canvas never resizes
        // on its own (no listener installed above), so `size` here is
        // already the natural grid size and needs no such override.
        #[cfg(target_arch = "wasm32")]
        let size = if self.fill_viewport {
            web_viewport_surface_physical_size().unwrap_or(size)
        } else {
            size
        };
        self.resize_to(size);
    }

    /// React to a scale-factor (DPI) change: notify the presenter, then
    /// realign the surface and grid to the window's new physical size.
    ///
    /// Every modern `HiDPI` display is scaled, so without this the surface
    /// silently keeps rendering at the old (pre-change) physical size --
    /// e.g. half the true resolution after moving to a 2x-scale display --
    /// until (if ever) an independent `Resized` event happens to arrive.
    /// Reusing [`resize_to`](Self::resize_to) here mirrors
    /// [`on_resized`](Self::on_resized), so both paths clamp/align the
    /// surface to whole cells the same way.
    fn on_scale_factor_changed(&mut self, scale_factor: f64) {
        if let Some(term) = self.terminal.as_mut() {
            term.backend_mut()
                .presenter_mut()
                .scale_factor_changed(scale_factor);
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.resize_to(window.inner_size());
    }

    /// Recompute the grid size (in cells) from a physical pixel size, resize
    /// the presenter's surface to the whole-cell-aligned pixel size, and push
    /// [`Event::Resize`] with the new cell dimensions.
    ///
    /// Shared by [`on_resized`](Self::on_resized) and
    /// [`on_scale_factor_changed`](Self::on_scale_factor_changed): both need
    /// the same clamp-to-cell-grid math, just triggered by different winit
    /// events.
    fn resize_to(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        let Some(term) = self.terminal.as_mut() else {
            return;
        };
        let (cell_w, cell_h) = term.backend().presenter().cell_size();
        // Clamp to at least one cell: a window smaller than one cell in
        // either dimension would otherwise divide down to 0 cols/rows,
        // which in turn asks `resize_surface` for a zero-size surface --
        // softbuffer (and likely other presenters) can't handle that and
        // panics. `Event::Resize` must report the same clamped grid the
        // surface was actually sized to, or callers reading `Event::Resize`
        // and querying the presenter's surface size would disagree.
        //
        // Integer division here also truncates any sub-cell remainder: when
        // `size` isn't an exact multiple of the cell size, `cols`/`rows`
        // round down and the surface below is sized to exactly
        // `cols * cell_w` x `rows * cell_h`, which can be smaller than
        // `size` itself. The OS window stays at the full physical `size`
        // the window manager gave it -- retroglyph never resizes the OS
        // window to match -- so a non-exact-multiple resize leaves a thin
        // strip at the window's trailing (right/bottom) edge outside the
        // surface entirely. That strip is not cleared or painted by
        // retroglyph; whatever the OS/windowing backend leaves there (old
        // frame content, backdrop color) shows through until the window is
        // resized again to a size the presenter does cover. See
        // `Presenter::resize_surface` for the documented contract.
        let cols = (size.width / cell_w).max(1);
        let rows = (size.height / cell_h).max(1);
        term.backend_mut()
            .presenter_mut()
            .resize_surface(cols * cell_w, rows * cell_h);
        #[allow(clippy::cast_possible_truncation)]
        term.backend_mut()
            .push_event(Event::Resize(cols as u16, rows as u16));
    }

    fn on_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        // winit always reports pointer positions in real-DPR physical
        // pixels; rescale to the (possibly DPR-capped, on wasm) backing-store
        // pixel space that `cell_size`/`pixel_to_cell` use, so taps land on
        // the cell actually under the finger/cursor instead of drifting
        // south-east of it as the real DPR grows past the cap. `1.0` on
        // native (no such cap exists there) *and* on wasm when
        // `fill_viewport` is off: `create_window_and_surface` only computes
        // a DPR-capped `surface_physical_size` when `fill_viewport` is set
        // (see its branch above) -- without it, the backing store already
        // matches the real, uncapped DPR 1:1, so applying the cap
        // correction anyway scales every reported position *down* toward
        // the origin for no reason, biasing every tap/click up-and-left of
        // where it actually landed on any real_dpr > 1.5 device (most
        // phones, and Retina/HiDPI desktops).
        #[cfg(target_arch = "wasm32")]
        let scale = if self.fill_viewport {
            wasm_pointer_scale()
        } else {
            1.0
        };
        #[cfg(not(target_arch = "wasm32"))]
        let scale = 1.0;
        let (x, y) = (position.x * scale, position.y * scale);
        self.cursor_px = (x, y);
        let px = physical_pos_from(x, y);
        let Some(term) = self.terminal.as_mut() else {
            return;
        };
        let (cell_w, cell_h) = term.backend().presenter().cell_size();
        let pos = pixel_to_cell(x, y, cell_w, cell_h);
        term.backend_mut().push_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: pos,
            pixel_position: Some(px),
            modifiers: self.current_modifiers,
        }));
    }

    fn on_mouse_input(
        &mut self,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) {
        let Some(btn) = translate_mouse_button(button) else {
            return;
        };
        let px = self.cursor_physical_pos();
        let Some(term) = self.terminal.as_mut() else {
            return;
        };
        let (cell_w, cell_h) = term.backend().presenter().cell_size();
        let pos = pixel_to_cell(self.cursor_px.0, self.cursor_px.1, cell_w, cell_h);
        let kind = if state.is_pressed() {
            MouseEventKind::Down(btn)
        } else {
            MouseEventKind::Up(btn)
        };
        term.backend_mut().push_event(Event::Mouse(MouseEvent {
            kind,
            position: pos,
            pixel_position: Some(px),
            modifiers: self.current_modifiers,
        }));
    }

    fn on_mouse_wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let px = self.cursor_physical_pos();
        let Some(term) = self.terminal.as_mut() else {
            return;
        };
        let (cell_w, cell_h) = term.backend().presenter().cell_size();
        let pos = pixel_to_cell(self.cursor_px.0, self.cursor_px.1, cell_w, cell_h);
        let scroll_y = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => f64::from(y),
            winit::event::MouseScrollDelta::PixelDelta(p) => p.y,
        };
        let kind = if scroll_y > 0.0 {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        };
        term.backend_mut().push_event(Event::Mouse(MouseEvent {
            kind,
            position: pos,
            pixel_position: Some(px),
            modifiers: self.current_modifiers,
        }));
    }

    /// Synthesize mouse events from a touch so tap/drag work out of the box.
    ///
    /// Mobile browsers (and native touchscreens) deliver touch input as
    /// [`WindowEvent::Touch`], which has no `CursorMoved`/`MouseInput`
    /// counterpart. Games shouldn't need a second input path for it, so the
    /// first finger down becomes the pointer: its start is a `Moved` +
    /// left-button `Down`, its motion is `Moved` (a drag), and its lift is
    /// `Up`. Additional simultaneous fingers are ignored.
    fn on_touch(&mut self, touch: winit::event::Touch) {
        use winit::event::TouchPhase;

        match touch.phase {
            TouchPhase::Started => {
                if self.active_touch.is_some() {
                    return; // a second finger; keep tracking the first
                }
                self.active_touch = Some(touch.id);
                self.on_cursor_moved(touch.location);
                self.on_mouse_input(
                    winit::event::ElementState::Pressed,
                    winit::event::MouseButton::Left,
                );
            }
            TouchPhase::Moved => {
                if self.active_touch == Some(touch.id) {
                    self.on_cursor_moved(touch.location);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.active_touch != Some(touch.id) {
                    return;
                }
                self.active_touch = None;
                self.on_cursor_moved(touch.location);
                self.on_mouse_input(
                    winit::event::ElementState::Released,
                    winit::event::MouseButton::Left,
                );
            }
        }
    }

    /// Convert the cached cursor pixel position to [`PhysicalPos`].
    const fn cursor_physical_pos(&self) -> PhysicalPos {
        physical_pos_from(self.cursor_px.0, self.cursor_px.1)
    }

    /// Push [`Event::FocusGained`]/[`Event::FocusLost`], and on loss, reset state that only makes
    /// sense while the window is focused.
    ///
    /// Winit keeps delivering `ModifiersChanged` only while focused, so a modifier key held down
    /// when focus is lost (e.g. alt-tabbing away while holding Shift) never generates the release
    /// that would normally clear it: without this, `current_modifiers` stays stuck "held" for
    /// every event after focus returns. Similarly, a finger lifted while the window is
    /// unfocused/backgrounded never delivers `TouchPhase::Ended`/`Cancelled`, so `active_touch`
    /// would otherwise stay set forever, permanently ignoring the next finger down. The stuck
    /// touch is released the same way a real lift is (see [`on_touch`](Self::on_touch)'s
    /// `Ended`/`Cancelled` arm): a left-button `Up` at the last known cursor position, so the app
    /// sees a normal, balanced Down/Up pair instead of a Down with no matching Up. No `Moved` is
    /// synthesized first, unlike a real lift -- blur carries no new pointer location, and
    /// `cursor_px` already holds the touch's last reported position from the `Started`/`Moved`
    /// arms that got it there.
    fn on_focus_changed(&mut self, gained: bool) {
        if let Some(term) = self.terminal.as_mut() {
            let event = if gained {
                Event::FocusGained
            } else {
                Event::FocusLost
            };
            term.backend_mut().push_event(event);
        }
        if !gained {
            self.current_modifiers = KeyModifiers::NONE;
            if self.active_touch.take().is_some() {
                self.on_mouse_input(
                    winit::event::ElementState::Released,
                    winit::event::MouseButton::Left,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::event::{MouseButton, MouseEvent, MouseEventKind};
    use retroglyph_core::grid::{Pos, Size};
    use retroglyph_core::tile::Tile;
    use std::cell::RefCell;
    use std::time::Duration;

    // ── dpr_pointer_scale ─────────────────────────────────────────────────────

    // ── physical_size_for ─────────────────────────────────────────────────────

    #[test]
    fn physical_size_for_unscaled_monitor_is_unchanged() {
        assert_eq!(physical_size_for(80, 80, 1.0), (80, 80));
    }

    #[test]
    fn physical_size_for_hidpi_monitor_scales_up() {
        // 2x display: a 80x80 logical window needs 160x160 true physical
        // pixels to render crisply instead of being upscaled by the OS.
        assert_eq!(physical_size_for(80, 80, 2.0), (160, 160));
    }

    #[test]
    fn physical_size_for_fractional_scale_rounds() {
        // 1.5x display: 81x81 rounds to the nearest physical pixel rather
        // than truncating.
        assert_eq!(physical_size_for(81, 81, 1.5), (122, 122));
    }

    // ── WindowConfig builder chain ───────────────────────────────────────────

    #[test]
    fn fit_defaults_match_winit_defaults() {
        // `fit` should start from the same defaults winit itself uses for a plain
        // `Window::default_attributes()`, so a caller that never touches the new builder
        // methods gets identical behavior to before this API existed.
        let presenter = MockPresenter::default();
        let config = WindowConfig::fit(&presenter, "test", None);
        assert!(config.resizable);
        assert!(config.decorations);
        assert_eq!(config.min_size, None);
        assert_eq!(config.max_size, None);
        assert_eq!(config.initial_position, None);
        assert!(!config.fullscreen);
        assert!(!config.transparency);
        assert!(!config.fill_viewport);
    }

    #[test]
    fn builder_chain_sets_each_attribute() {
        let presenter = MockPresenter::default();
        let config = WindowConfig::fit(&presenter, "test", None)
            .resizable(false)
            .decorations(false)
            .min_size(320, 240)
            .max_size(1920, 1080)
            .initial_position(10, 20)
            .fullscreen(true)
            .transparency(true);
        assert!(!config.resizable);
        assert!(!config.decorations);
        assert_eq!(config.min_size, Some((320, 240)));
        assert_eq!(config.max_size, Some((1920, 1080)));
        assert_eq!(config.initial_position, Some((10, 20)));
        assert!(config.fullscreen);
        assert!(config.transparency);
    }

    #[test]
    fn window_attrs_from_config_copies_all_fields() {
        let presenter = MockPresenter::default();
        let config = WindowConfig::fit(&presenter, "test", None)
            .resizable(false)
            .decorations(false)
            .min_size(1, 2)
            .max_size(3, 4)
            .initial_position(5, 6)
            .fullscreen(true)
            .transparency(true);
        let attrs = WindowAttrs::from(&config);
        assert!(!attrs.resizable);
        assert!(!attrs.decorations);
        assert_eq!(attrs.min_size, Some((1, 2)));
        assert_eq!(attrs.max_size, Some((3, 4)));
        assert_eq!(attrs.initial_position, Some((5, 6)));
        assert!(attrs.fullscreen);
        assert!(attrs.transparency);
    }

    #[test]
    fn dpr_pointer_scale_no_correction_below_cap() {
        // Real DPR at or below the cap: pointer positions already match the
        // (uncapped) backing store, no rescale needed.
        assert!((dpr_pointer_scale(1.0, 1.5) - 1.0).abs() < 1e-9);
        assert!((dpr_pointer_scale(1.5, 1.5) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dpr_pointer_scale_corrects_above_cap() {
        // Real DPR 3 against a 1.5 cap: the backing store is half the real
        // resolution, so pointer positions must be halved to land on the
        // right cell instead of drifting south-east of it.
        assert!((dpr_pointer_scale(3.0, 1.5) - 0.5).abs() < 1e-9);
        assert!((dpr_pointer_scale(2.0, 1.5) - 0.75).abs() < 1e-9);
    }

    /// A dependency-free [`Presenter`] with fixed 8x16 cells.
    ///
    /// The `WindowApp` tests only exercise event translation, cell math, and
    /// the `WindowBackend` queue — no rasterization or surface is needed.
    #[derive(Default)]
    struct MockPresenter {
        /// Records the last [`Presenter::scale_factor_changed`] argument, if any.
        last_scale_factor: Cell<Option<f64>>,
    }

    impl Presenter for MockPresenter {
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
            Size {
                width: 10,
                height: 5,
            }
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn resize(&mut self, _size: Size) {}

        fn init_surface(
            &mut self,
            _window: Arc<dyn crate::presenter::WindowHandle>,
        ) -> Result<(), Self::SurfaceError> {
            Ok(())
        }

        fn resize_surface(&mut self, _width: u32, _height: u32) {}

        fn present(&mut self) -> Result<(), Self::SurfaceError> {
            Ok(())
        }

        fn cell_size(&self) -> (u32, u32) {
            (8, 16)
        }

        fn scale_factor_changed(&mut self, scale_factor: f64) {
            self.last_scale_factor.set(Some(scale_factor));
        }
    }

    /// A [`Presenter`] that records every `resize_surface` call, so tests
    /// can assert on the pixel dimensions `on_resized` actually requests.
    #[derive(Default)]
    struct RecordingPresenter {
        resize_calls: Rc<RefCell<Vec<(u32, u32)>>>,
    }

    impl Presenter for RecordingPresenter {
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
            Size {
                width: 10,
                height: 5,
            }
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn resize(&mut self, _size: Size) {}

        fn init_surface(
            &mut self,
            _window: Arc<dyn crate::presenter::WindowHandle>,
        ) -> Result<(), Self::SurfaceError> {
            Ok(())
        }

        fn resize_surface(&mut self, width: u32, height: u32) {
            self.resize_calls.borrow_mut().push((width, height));
        }

        fn present(&mut self) -> Result<(), Self::SurfaceError> {
            Ok(())
        }

        fn cell_size(&self) -> (u32, u32) {
            (8, 16)
        }
    }

    type MockApp = WindowApp<MockPresenter, fn(&mut Terminal<WindowBackend<MockPresenter>>)>;

    fn test_window_app() -> MockApp {
        let terminal = Terminal::new(WindowBackend::new(MockPresenter::default()));
        WindowApp {
            terminal: Some(terminal),
            app_loop: |_| {},
            window: None,
            title: String::new(),
            init_size: InitWindowSize {
                width: 80,
                height: 80,
            },
            attrs: WindowAttrs::default(),
            current_modifiers: KeyModifiers::NONE,
            cursor_px: (0.0, 0.0),
            active_touch: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_interval: None,
            #[cfg(not(target_arch = "wasm32"))]
            next_frame: std::time::Instant::now(),
            exit_requested: Rc::new(Cell::new(false)),
            needs_redraw: false,
        }
    }

    fn poll(app: &mut MockApp) -> Option<Event> {
        app.terminal
            .as_mut()
            .unwrap()
            .backend_mut()
            .poll_event(Duration::ZERO)
    }

    // ── WindowBackend queue ───────────────────────────────────────────────────

    #[test]
    fn mouse_event_round_trips_through_event_buffer() {
        let mut backend = WindowBackend::new(MockPresenter::default());
        let ev = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos { x: 3, y: 1 },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        backend.push_event(ev.clone());
        assert_eq!(backend.poll_event(Duration::ZERO), Some(ev));
        assert_eq!(backend.poll_event(Duration::ZERO), None);
    }

    #[test]
    fn multiple_mouse_events_preserve_fifo_order() {
        let mut backend = WindowBackend::new(MockPresenter::default());
        let moved = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos { x: 1, y: 2 },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        let clicked = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos { x: 1, y: 2 },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        backend.push_event(moved.clone());
        backend.push_event(clicked.clone());
        assert_eq!(backend.poll_event(Duration::ZERO), Some(moved));
        assert_eq!(backend.poll_event(Duration::ZERO), Some(clicked));
    }

    // ── handle_window_event ──────────────────────────────────────────────────

    #[test]
    fn cursor_moved_pushes_moved_event_at_correct_cell() {
        // 8-wide × 16-tall cells; cursor at pixel (20, 32) → col 2, row 2.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(20.0_f64, 32.0_f64),
        });
        assert_eq!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                position: Pos { x: 2, y: 2 },
                pixel_position: Some(PhysicalPos { x: 20, y: 32 }),
                modifiers: KeyModifiers::NONE,
            }))
        );
    }

    #[test]
    fn cursor_moved_caches_position_for_subsequent_click() {
        // Move to pixel (16, 16) = col 2, row 1, then click — button event
        // must reuse the cached position.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(16.0_f64, 16.0_f64),
        });
        let _ = poll(&mut app); // discard the Moved event
        app.handle_window_event(WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Left,
        });
        assert_eq!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                position: Pos { x: 2, y: 1 },
                pixel_position: Some(PhysicalPos { x: 16, y: 16 }),
                modifiers: KeyModifiers::NONE,
            }))
        );
    }

    #[test]
    fn mouse_button_release_produces_up_event() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Released,
            button: winit::event::MouseButton::Right,
        });
        assert_eq!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                position: Pos { x: 0, y: 0 },
                pixel_position: Some(PhysicalPos { x: 0, y: 0 }),
                modifiers: KeyModifiers::NONE,
            }))
        );
    }

    #[test]
    fn unknown_mouse_button_produces_no_event() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Other(99),
        });
        assert_eq!(poll(&mut app), None);
    }

    fn touch(id: u64, phase: winit::event::TouchPhase, x: f64, y: f64) -> WindowEvent {
        WindowEvent::Touch(winit::event::Touch {
            device_id: winit::event::DeviceId::dummy(),
            phase,
            location: winit::dpi::PhysicalPosition::new(x, y),
            force: None,
            id,
        })
    }

    #[test]
    fn touch_tap_synthesizes_left_click() {
        use winit::event::TouchPhase;
        let mut app = test_window_app();
        // MockPresenter cells are 8x16 px; a tap at (20, 18) lands on cell (2, 1).
        app.handle_window_event(touch(7, TouchPhase::Started, 20.0, 18.0));
        // Moved (from the synthesized cursor move) then Down.
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                position: Pos { x: 2, y: 1 },
                ..
            }))
        ));
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                position: Pos { x: 2, y: 1 },
                ..
            }))
        ));

        app.handle_window_event(touch(7, TouchPhase::Ended, 20.0, 18.0));
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            }))
        ));
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                position: Pos { x: 2, y: 1 },
                ..
            }))
        ));
        assert_eq!(poll(&mut app), None);
    }

    #[test]
    fn touch_drag_synthesizes_moves_between_down_and_up() {
        use winit::event::TouchPhase;
        let mut app = test_window_app();
        app.handle_window_event(touch(1, TouchPhase::Started, 0.0, 0.0));
        poll(&mut app); // Moved
        poll(&mut app); // Down

        app.handle_window_event(touch(1, TouchPhase::Moved, 40.0, 32.0));
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                position: Pos { x: 5, y: 2 },
                ..
            }))
        ));

        app.handle_window_event(touch(1, TouchPhase::Cancelled, 40.0, 32.0));
        poll(&mut app); // Moved
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..
            }))
        ));
    }

    #[test]
    fn second_finger_is_ignored_while_first_is_down() {
        use winit::event::TouchPhase;
        let mut app = test_window_app();
        app.handle_window_event(touch(1, TouchPhase::Started, 0.0, 0.0));
        poll(&mut app); // Moved
        poll(&mut app); // Down

        // A second finger goes down, moves, and lifts: all ignored.
        app.handle_window_event(touch(2, TouchPhase::Started, 80.0, 80.0));
        app.handle_window_event(touch(2, TouchPhase::Moved, 88.0, 80.0));
        app.handle_window_event(touch(2, TouchPhase::Ended, 88.0, 80.0));
        assert_eq!(poll(&mut app), None);

        // The first finger still completes its gesture.
        app.handle_window_event(touch(1, TouchPhase::Ended, 8.0, 0.0));
        poll(&mut app); // Moved
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                position: Pos { x: 1, y: 0 },
                ..
            }))
        ));
    }

    #[test]
    fn scroll_up_line_delta() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: winit::event::MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: winit::event::TouchPhase::Moved,
        });
        let ev = poll(&mut app).unwrap();
        assert!(matches!(
            ev,
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            })
        ));
    }

    #[test]
    fn scroll_down_line_delta() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: winit::event::MouseScrollDelta::LineDelta(0.0, -1.0),
            phase: winit::event::TouchPhase::Moved,
        });
        let ev = poll(&mut app).unwrap();
        assert!(matches!(
            ev,
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            })
        ));
    }

    #[test]
    fn scroll_up_pixel_delta() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(
                0.0_f64, 15.0_f64,
            )),
            phase: winit::event::TouchPhase::Moved,
        });
        let ev = poll(&mut app).unwrap();
        assert!(matches!(
            ev,
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            })
        ));
    }

    #[test]
    fn modifiers_propagate_to_mouse_event() {
        let mut app = test_window_app();
        // Simulate a ModifiersChanged before the click.
        app.handle_window_event(WindowEvent::ModifiersChanged(
            winit::event::Modifiers::from(winit::keyboard::ModifiersState::SHIFT),
        ));
        let _ = poll(&mut app); // no event emitted for modifiers
        app.handle_window_event(WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Left,
        });
        let ev = poll(&mut app).unwrap();
        assert!(matches!(
            ev,
            Event::Mouse(MouseEvent {
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::SHIFT)
        ));
    }

    // ── user events (EventProxy) ─────────────────────────────────────────────

    #[test]
    fn user_event_pushes_custom_event() {
        let mut app = test_window_app();
        app.handle_user_event(42);
        assert_eq!(poll(&mut app), Some(Event::Custom(42)));
    }

    #[test]
    fn multiple_user_events_preserve_fifo_order() {
        let mut app = test_window_app();
        app.handle_user_event(1);
        app.handle_user_event(2);
        assert_eq!(poll(&mut app), Some(Event::Custom(1)));
        assert_eq!(poll(&mut app), Some(Event::Custom(2)));
        assert_eq!(poll(&mut app), None);
    }

    #[test]
    fn user_events_interleave_with_window_events_in_arrival_order() {
        let mut app = test_window_app();
        app.handle_user_event(7);
        app.handle_window_event(WindowEvent::CloseRequested);
        assert_eq!(poll(&mut app), Some(Event::Custom(7)));
        assert_eq!(poll(&mut app), Some(Event::Close));
    }

    #[test]
    fn event_proxy_closed_reports_the_undelivered_id() {
        let err = EventProxyClosed(42);
        assert_eq!(err.into_inner(), 42);
        assert_eq!(err.to_string(), "event loop closed");
    }

    #[test]
    fn close_requested_pushes_close_event() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::CloseRequested);
        assert_eq!(poll(&mut app), Some(Event::Close));
    }

    // ── graceful exit (issue #157) ────────────────────────────────────────────

    /// A `WindowApp` whose `app_loop` is a boxed closure, so a test can capture and flip a
    /// shared flag from inside it -- mirroring how `run_app_with_proxy`'s real closure sets
    /// `exit_requested` on `Flow::Exit` (it can't return a value or reach `ActiveEventLoop`
    /// itself; see `exit_requested`'s doc comment).
    type BoxedAppLoop = Box<dyn FnMut(&mut Terminal<WindowBackend<MockPresenter>>)>;
    type BoxedApp = WindowApp<MockPresenter, BoxedAppLoop>;

    #[test]
    fn redraw_requested_runs_app_loop_and_does_not_set_exit_by_default() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::RedrawRequested);
        assert!(!app.exit_requested.get());
    }

    #[test]
    fn app_loop_setting_exit_requested_is_observed_after_redraw() {
        // Simulates `run_app_with_proxy`'s closure: on `Flow::Exit` it sets the shared flag
        // instead of calling `std::process::exit`. `handle_window_event` itself never calls
        // `event_loop.exit()` (it can't -- no `ActiveEventLoop` -- see its doc comment); that
        // happens in `ApplicationHandler::window_event`, which this flag lets the test assert
        // on without a live winit event loop.
        let terminal = Terminal::new(WindowBackend::new(MockPresenter::default()));
        let exit_requested = Rc::new(Cell::new(false));
        let exit_requested_in_loop = exit_requested.clone();
        let app_loop: BoxedAppLoop = Box::new(move |_term| exit_requested_in_loop.set(true));
        let mut app: BoxedApp = WindowApp {
            terminal: Some(terminal),
            app_loop,
            window: None,
            title: String::new(),
            init_size: InitWindowSize {
                width: 80,
                height: 80,
            },
            attrs: WindowAttrs::default(),
            current_modifiers: KeyModifiers::NONE,
            cursor_px: (0.0, 0.0),
            active_touch: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_interval: None,
            #[cfg(not(target_arch = "wasm32"))]
            next_frame: std::time::Instant::now(),
            exit_requested,
            needs_redraw: false,
        };

        assert!(!app.exit_requested.get());
        app.handle_window_event(WindowEvent::RedrawRequested);
        assert!(app.exit_requested.get());
    }

    #[test]
    fn theme_changed_pushes_mapped_system_theme_event() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::ThemeChanged(winit::window::Theme::Light));
        assert_eq!(
            poll(&mut app),
            Some(Event::ThemeChanged(
                retroglyph_core::event::SystemTheme::Light
            ))
        );

        app.handle_window_event(WindowEvent::ThemeChanged(winit::window::Theme::Dark));
        assert_eq!(
            poll(&mut app),
            Some(Event::ThemeChanged(
                retroglyph_core::event::SystemTheme::Dark
            ))
        );
    }

    #[test]
    fn focused_pushes_focus_gained_and_lost_events() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::Focused(true));
        assert_eq!(poll(&mut app), Some(Event::FocusGained));

        app.handle_window_event(WindowEvent::Focused(false));
        assert_eq!(poll(&mut app), Some(Event::FocusLost));
    }

    #[test]
    fn focus_lost_resets_stuck_modifiers() {
        // Regression test for #153: a modifier held down when focus is lost
        // (e.g. alt-tabbing away while holding Shift) must not stay "held"
        // for events delivered after focus returns.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::ModifiersChanged(
            winit::event::Modifiers::from(winit::keyboard::ModifiersState::SHIFT),
        ));
        let _ = poll(&mut app); // no event emitted for modifiers
        assert_eq!(app.current_modifiers, KeyModifiers::SHIFT);

        app.handle_window_event(WindowEvent::Focused(false));
        assert_eq!(poll(&mut app), Some(Event::FocusLost));
        assert_eq!(app.current_modifiers, KeyModifiers::NONE);

        // A click after refocusing must not still carry the stale Shift.
        app.handle_window_event(WindowEvent::Focused(true));
        assert_eq!(poll(&mut app), Some(Event::FocusGained));
        app.handle_window_event(WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Left,
        });
        let ev = poll(&mut app).unwrap();
        assert!(matches!(
            ev,
            Event::Mouse(MouseEvent { modifiers, .. }) if modifiers == KeyModifiers::NONE
        ));
    }

    #[test]
    fn focus_lost_releases_stuck_active_touch() {
        // Regression test for #153: a finger lifted while the window is
        // unfocused/backgrounded never delivers `TouchPhase::Ended` or
        // `Cancelled`, so `active_touch` must be released on blur instead of
        // silently ignoring every subsequent finger down.
        use winit::event::TouchPhase;
        let mut app = test_window_app();
        app.handle_window_event(touch(3, TouchPhase::Started, 20.0, 18.0));
        poll(&mut app); // Moved
        poll(&mut app); // Down
        assert_eq!(app.active_touch, Some(3));

        app.handle_window_event(WindowEvent::Focused(false));
        assert_eq!(poll(&mut app), Some(Event::FocusLost));
        // Synthesized Up releasing the stuck touch at its last known
        // position; no new Moved, since blur carries no fresh location.
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..
            }))
        ));
        assert_eq!(poll(&mut app), None);
        assert_eq!(app.active_touch, None);

        // A new finger down after refocusing must be tracked, not ignored.
        app.handle_window_event(WindowEvent::Focused(true));
        assert_eq!(poll(&mut app), Some(Event::FocusGained));
        app.handle_window_event(touch(4, TouchPhase::Started, 40.0, 32.0));
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            }))
        ));
        assert!(matches!(
            poll(&mut app),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                ..
            }))
        ));
        assert_eq!(app.active_touch, Some(4));
    }

    #[test]
    fn focus_lost_without_active_touch_pushes_no_extra_events() {
        // No touch in progress: blur should push exactly one FocusLost, no
        // synthesized mouse events.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::Focused(false));
        assert_eq!(poll(&mut app), Some(Event::FocusLost));
        assert_eq!(poll(&mut app), None);
    }

    #[test]
    fn resized_pushes_resize_event_in_cells() {
        // 8x16 cells: 88x80 px -> 11 cols, 5 rows.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::Resized(winit::dpi::PhysicalSize::new(88, 80)));
        assert_eq!(poll(&mut app), Some(Event::Resize(11, 5)));
    }

    // ── scale factor changes ─────────────────────────────────────────────────

    #[test]
    fn scale_factor_changed_notifies_presenter() {
        // `handle_window_event` can't be exercised directly here: winit's
        // `InnerSizeWriter::new` is `pub(crate)`, so a real
        // `WindowEvent::ScaleFactorChanged` can't be constructed outside the
        // winit crate. `on_scale_factor_changed` is called directly instead
        // -- it's the same code the `WindowEvent::ScaleFactorChanged` arm in
        // `handle_window_event` dispatches to.
        let mut app = test_window_app();
        app.on_scale_factor_changed(2.0);
        assert_eq!(
            app.terminal
                .as_ref()
                .unwrap()
                .backend()
                .presenter()
                .last_scale_factor
                .get(),
            Some(2.0)
        );
    }

    #[test]
    fn scale_factor_changed_without_a_window_is_a_no_op_resize() {
        // `test_window_app` has no real winit window (`window: None`), so
        // there is no physical size to re-align the surface to -- this must
        // not panic, and must not push a spurious `Event::Resize`.
        let mut app = test_window_app();
        app.on_scale_factor_changed(2.0);
        assert_eq!(poll(&mut app), None);
    }

    #[test]
    fn resize_to_clamps_to_whole_cells_and_pushes_resize_event() {
        // Shared helper behind both `on_resized` and
        // `on_scale_factor_changed`: 8x16 cells, 90x81 px clamps down to
        // 11 cols x 5 rows (88x80 px), not a fractional cell.
        let mut app = test_window_app();
        app.resize_to(winit::dpi::PhysicalSize::new(90, 81));
        assert_eq!(poll(&mut app), Some(Event::Resize(11, 5)));
    }

    #[test]
    fn resized_below_one_cell_clamps_surface_and_event_to_1x1() {
        // Regression test for #140: an 8x16-cell presenter resized to a
        // window smaller than one cell (4x4 px) must not compute 0 cols/0
        // rows -- that would ask `resize_surface` for a zero-size surface,
        // which crashes softbuffer.
        type RecordingApp =
            WindowApp<RecordingPresenter, fn(&mut Terminal<WindowBackend<RecordingPresenter>>)>;
        let resize_calls = Rc::new(RefCell::new(Vec::new()));
        let presenter = RecordingPresenter {
            resize_calls: resize_calls.clone(),
        };
        let terminal = Terminal::new(WindowBackend::new(presenter));
        let mut app: RecordingApp = WindowApp {
            terminal: Some(terminal),
            app_loop: |_| {},
            window: None,
            title: String::new(),
            init_size: InitWindowSize {
                width: 80,
                height: 80,
            },
            attrs: WindowAttrs::default(),
            current_modifiers: KeyModifiers::NONE,
            cursor_px: (0.0, 0.0),
            active_touch: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_interval: None,
            #[cfg(not(target_arch = "wasm32"))]
            next_frame: std::time::Instant::now(),
            exit_requested: Rc::new(Cell::new(false)),
            needs_redraw: false,
        };

        app.handle_window_event(WindowEvent::Resized(winit::dpi::PhysicalSize::new(4, 4)));

        // Surface must be resized to at least one full cell (8x16), not
        // 0x0.
        assert_eq!(resize_calls.borrow().as_slice(), &[(8, 16)]);
        // Event::Resize must report the same clamped 1x1 grid, not 0x0.
        assert_eq!(
            app.terminal
                .as_mut()
                .unwrap()
                .backend_mut()
                .poll_event(Duration::ZERO),
            Some(Event::Resize(1, 1))
        );
    }

    // ── needs_redraw (idle/redraw-on-demand, issue #155) ─────────────────────

    #[test]
    fn fresh_app_does_not_need_a_redraw() {
        // `test_window_app` starts with `needs_redraw: false` -- unlike the real
        // `resumed()` path, which sets it `true` once the window/surface exists (a real winit
        // `ActiveEventLoop` can't be constructed in a unit test, so `resumed` itself isn't
        // exercised here; see `handle_window_event`/`handle_user_event` below for the parts of
        // the redraw-on-demand logic that are testable without one).
        let app = test_window_app();
        assert!(!app.needs_redraw);
    }

    #[test]
    fn window_event_sets_needs_redraw() {
        // Any real window event (a mouse move here, but any arm other than `RedrawRequested`
        // behaves the same -- see `handle_window_event`'s doc comment) should mark that the app
        // loop has something new to react to, so the next `about_to_wait` requests a redraw
        // instead of leaving the loop idle.
        let mut app = test_window_app();
        assert!(!app.needs_redraw);
        app.handle_window_event(WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(1.0_f64, 1.0_f64),
        });
        assert!(app.needs_redraw);
    }

    #[test]
    fn redraw_requested_does_not_itself_set_needs_redraw() {
        // `RedrawRequested` is the render this flag exists to gate, not a new event to redraw
        // again for -- an idle app that gets exactly one `RedrawRequested` (e.g. right after
        // `resumed`) must not perpetually re-arm itself into another one forever.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::RedrawRequested);
        assert!(!app.needs_redraw);
    }

    #[test]
    fn user_event_sets_needs_redraw() {
        // A cross-thread `Event::Custom` injection (network, audio, timer, ...) must wake an
        // idle loop into rendering the next frame just like a real window event does.
        let mut app = test_window_app();
        assert!(!app.needs_redraw);
        app.handle_user_event(1);
        assert!(app.needs_redraw);
    }

    #[test]
    fn unhandled_window_events_still_set_needs_redraw() {
        // Even a `WindowEvent` variant with no dedicated handling below (falls through to the
        // `_ => {}` arm in `handle_window_event`'s `match`) should still be treated as "something
        // happened": the flag is set once, up front, before the match runs.
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::Occluded(true));
        assert!(app.needs_redraw);
    }
}
