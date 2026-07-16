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
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Window configuration for [`run_windowed`] / [`run_app`].
///
/// Deliberately renderer-agnostic: pixel dimensions, not grid/font/scale.
/// Use [`fit`](Self::fit) to derive the pixel size from a presenter's own
/// cell geometry.
pub struct WindowConfig {
    /// Window title.
    pub title: String,
    /// Initial inner width in physical pixels.
    pub width: u32,
    /// Initial inner height in physical pixels.
    pub height: u32,
    /// Optional frame-rate cap. `None` = uncapped (native) / display refresh
    /// (wasm, which is always rAF-driven).
    pub target_fps: Option<u32>,
    /// On `wasm32`, size (and keep resizing) the canvas to fill the browser
    /// viewport instead of `width`/`height` -- a full-screen, mobile-web-app
    /// feel for games that want it. Has no effect on native, where the OS
    /// window is already sized to `width`/`height` and the window manager
    /// owns further resizing either way.
    ///
    /// Defaults to `false` in [`fit`](Self::fit): most demos/examples
    /// should render at their natural grid size (`cols x cell_w` by `rows x
    /// cell_h`) wherever they land on the page, not stretch to fill
    /// whatever viewport happens to be hosting them. Opt in explicitly for
    /// an app-like, full-screen game.
    pub fill_viewport: bool,
}

impl WindowConfig {
    /// Size the window to exactly fit `presenter`'s grid:
    /// `cols x cell_w` by `rows x cell_h` physical pixels.
    ///
    /// This is why renderer crates don't need their own windowing code: the
    /// grid/cell geometry already lives behind [`Presenter::size`] and
    /// [`Presenter::cell_size`].
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
        }
    }

    /// Sets [`fill_viewport`](Self::fill_viewport), returning `self` for chaining off
    /// [`fit`](Self::fit).
    #[must_use]
    pub const fn fill_viewport(mut self, fill_viewport: bool) -> Self {
        self.fill_viewport = fill_viewport;
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
    let terminal = Terminal::new(WindowBackend::new(presenter));
    let event_loop = EventLoop::new()?;

    #[cfg(not(target_arch = "wasm32"))]
    let frame_interval = config
        .target_fps
        .map(|fps| Duration::from_secs_f64(1.0 / f64::from(fps)));

    let app = WindowApp {
        terminal: Some(terminal),
        app_loop,
        window: None,
        title: config.title,
        init_size: InitWindowSize {
            width: config.width,
            height: config.height,
        },
        #[cfg(target_arch = "wasm32")]
        fill_viewport: config.fill_viewport,
        current_modifiers: KeyModifiers::NONE,
        cursor_px: (0.0, 0.0),
        active_touch: None,
        #[cfg(not(target_arch = "wasm32"))]
        frame_interval,
        #[cfg(not(target_arch = "wasm32"))]
        next_frame: std::time::Instant::now(),
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
/// On [`Flow::Exit`](retroglyph_core::Flow) the process exits on native (the
/// window is torn down); on wasm the requestAnimationFrame loop cannot be
/// stopped, so exit is a no-op.
///
/// # Errors
///
/// Returns [`winit::error::EventLoopError`] if the event loop cannot be
/// created or fails while running.
pub fn run_app<P, A>(
    config: WindowConfig,
    presenter: P,
    mut app: A,
) -> Result<(), winit::error::EventLoopError>
where
    P: Presenter + 'static,
    A: retroglyph_core::App<WindowBackend<P>> + 'static,
{
    let mut frame_count = 0u64;
    let mut last = web_time::Instant::now();
    run_windowed(config, presenter, move |term| {
        let now = web_time::Instant::now();
        let delta = now.duration_since(last);
        last = now;
        let frame = retroglyph_core::Frame {
            delta,
            frame: frame_count,
        };
        frame_count = frame_count.wrapping_add(1);
        if retroglyph_core::step(term, &mut app, &frame) == retroglyph_core::Flow::Exit {
            #[cfg(not(target_arch = "wasm32"))]
            std::process::exit(0);
        }
    })
}

/// Initial window dimensions used before the first Resized event.
struct InitWindowSize {
    width: u32,
    height: u32,
}

/// The winit `ApplicationHandler`: owns the window, the terminal, and the
/// per-frame closure.
struct WindowApp<P: Presenter, F> {
    terminal: Option<Terminal<WindowBackend<P>>>,
    app_loop: F,
    window: Option<Arc<Window>>,
    title: String,
    init_size: InitWindowSize,
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
        #[cfg(not(target_arch = "wasm32"))]
        let physical_size =
            winit::dpi::PhysicalSize::new(self.init_size.width, self.init_size.height);
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
            .with_inner_size(physical_size);

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

impl<P, F> ApplicationHandler for WindowApp<P, F>
where
    P: Presenter,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = self.create_window_and_surface(event_loop) {
            self.window = Some(window);
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        self.handle_window_event(event);
    }

    fn about_to_wait(
        &mut self,
        #[cfg_attr(target_arch = "wasm32", allow(unused_variables))] event_loop: &ActiveEventLoop,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(interval) = self.frame_interval {
            // Throttled: sleep until the next frame deadline, then render.
            let now = std::time::Instant::now();
            if self.next_frame > now {
                event_loop
                    .set_control_flow(winit::event_loop::ControlFlow::WaitUntil(self.next_frame));
                return;
            }
            // Advance the deadline by one interval, clamping to now so a
            // slow frame doesn't cause a burst of catch-up renders.
            self.next_frame = (self.next_frame + interval).max(now);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl<P, F> WindowApp<P, F>
where
    P: Presenter,
    F: FnMut(&mut Terminal<WindowBackend<P>>) + 'static,
{
    /// Dispatch a [`WindowEvent`] without requiring an [`ActiveEventLoop`].
    ///
    /// Extracted from the `ApplicationHandler` impl so the translation and
    /// event-buffer logic can be called directly in unit tests, where
    /// [`ActiveEventLoop`] is not constructable.
    fn handle_window_event(&mut self, event: WindowEvent) {
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
            WindowEvent::Focused(gained) => {
                if let Some(term) = self.terminal.as_mut() {
                    let event = if gained {
                        Event::FocusGained
                    } else {
                        Event::FocusLost
                    };
                    term.backend_mut().push_event(event);
                }
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::event::{MouseButton, MouseEvent, MouseEventKind};
    use retroglyph_core::grid::{Pos, Size};
    use retroglyph_core::tile::Tile;
    use std::time::Duration;

    // ── dpr_pointer_scale ─────────────────────────────────────────────────────

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
        last_scale_factor: std::cell::Cell<Option<f64>>,
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
        resize_calls: std::rc::Rc<std::cell::RefCell<Vec<(u32, u32)>>>,
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
            current_modifiers: KeyModifiers::NONE,
            cursor_px: (0.0, 0.0),
            active_touch: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_interval: None,
            #[cfg(not(target_arch = "wasm32"))]
            next_frame: std::time::Instant::now(),
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

    #[test]
    fn close_requested_pushes_close_event() {
        let mut app = test_window_app();
        app.handle_window_event(WindowEvent::CloseRequested);
        assert_eq!(poll(&mut app), Some(Event::Close));
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
        let resize_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
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
            current_modifiers: KeyModifiers::NONE,
            cursor_px: (0.0, 0.0),
            active_touch: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_interval: None,
            #[cfg(not(target_arch = "wasm32"))]
            next_frame: std::time::Instant::now(),
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
}
