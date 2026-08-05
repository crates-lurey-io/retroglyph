//! [`PerfOverlayApp`]: a live frame-time/FPS overlay for any `App`, on any `Backend`.
//!
//! Wrap an existing [`App`](retroglyph_core::app::App) once and it gains a toggleable perf readout on
//! every backend, with no backend-specific code in the wrapped app itself:
//!
//! ```
//! # #[cfg(feature = "std")]
//! # {
//! use retroglyph_core::app::run_blocking;
//! use retroglyph_core::backend::{Backend, Headless};
//! use retroglyph_core::terminal::Terminal;
//! # use retroglyph_core::app::{App, Flow, Frame};
//! use retroglyph_ui::PerfOverlayApp;
//! # struct MyGame;
//! # impl<B: Backend> App<B> for MyGame {
//! #     fn update(&mut self, _term: &mut Terminal<B>, frame: &Frame) -> Flow {
//! #         if frame.frame >= 3 { Flow::Exit } else { Flow::Continue }
//! #     }
//! # }
//!
//! let term = Terminal::new(Headless::new(40, 10));
//! let app = PerfOverlayApp::new(MyGame, "headless");
//! run_blocking(term, app).expect("run_blocking");
//! # } // `run_blocking` is `std`-only; a no-op under `--no-default-features`.
//! ```
//!
//! # What's generic and what's backend-specific
//!
//! The frame-time bookkeeping ([`FrameStats`](retroglyph_core::frames::FrameStats)), the toggle-key check,
//! and the decision of when to draw are all backend-agnostic:
//! [`PerfOverlayApp::update`](retroglyph_core::app::App::update) only ever talks to
//! [`Terminal`](retroglyph_core::terminal::Terminal)/[`Surface`](crate::Surface), which every
//! [`Backend`](retroglyph_core::backend::Backend) implements identically. The one thing every caller still
//! supplies by hand is the `backend` label string (there is no portable way to ask a `Backend`
//! what to call itself); everything else, including toggling the overlay on and off, works
//! unmodified on crossterm, a native window, or a browser tab.
//!
//! # Rendering
//!
//! [`DefaultPerfRenderer`] (used by [`PerfOverlayApp::new`]) draws a single-row `NNNfps MM.Mms
//! minMM.M maxMM.M <backend>` readout, compact enough to fit an 80-column terminal alongside a
//! long backend label. For a richer overlay (a bordered panel, a frame-time sparkline, extra
//! app-supplied metrics like resolution or vsync state), pass a closure to
//! [`PerfOverlayApp::with_closure`]: composing this crate's [`Panel`](crate::Panel)/
//! [`Sparkline`](crate::Sparkline) widgets, or [`PerfOverlay`](crate::PerfOverlay) directly,
//! requires no glue code beyond the closure itself. Implement [`PerfRenderer`] directly, and
//! construct with [`PerfOverlayApp::with_renderer`], only for a named, reusable renderer type
//! instead of a closure.
//!
//! This whole wrapper exists in large part because
//! [`FrameStats::record`](retroglyph_core::frames::FrameStats::record) needs a
//! [`Frame`](retroglyph_core::app::Frame), which a plain widget draw call had no way to reach:
//! [`PerfOverlayApp::update`](retroglyph_core::app::App::update) is what intercepts `Frame` on the way
//! through and calls `FrameStats::record` for the widget that otherwise couldn't. An app that
//! doesn't need this wrapper's other job (generic toggle-key handling across any wrapped
//! [`App`](retroglyph_core::app::App), on every backend) no longer needs it just for that:
//! [`AnimatedPerfOverlay`](crate::AnimatedPerfOverlay) reaches `Frame` directly, so an app that
//! already owns a `FrameStats` field can record and draw it in a single call, with no decorator at
//! all.
//!
//! # Toggling
//!
//! [`PerfOverlayApp::update`](retroglyph_core::app::App::update) drains every event out of the wrapped
//! [`Terminal`](retroglyph_core::terminal::Terminal) before handing control to the inner
//! [`App`](retroglyph_core::app::App), keeps any that match the toggle key (backtick, or F1 as an
//! alias, by default; see [`default_is_toggle_key`]), and re-queues the rest via
//! [`Terminal::requeue_events`](retroglyph_core::terminal::Terminal::requeue_events) so the inner app sees
//! exactly the input it would have without the overlay, minus the toggle presses. This works
//! identically on every backend because it only goes through
//! [`Terminal`](retroglyph_core::terminal::Terminal)'s own event queue, never a backend-specific input path
//! (in particular, never
//! [`Input::push_event`](retroglyph_core::backend::Input::push_event), whose documented default is
//! a no-op for backends that never receive events from outside their own `poll_event`).
//!
//! The toggle key doesn't just flip visibility: it cycles through [`PerfOverlayMode`]:
//! [`Off`](PerfOverlayMode::Off) -> [`Compact`](PerfOverlayMode::Compact) ->
//! [`Full`](PerfOverlayMode::Full) -> back to `Off`. `Full` only exists once
//! [`PerfOverlayApp::cycle_with`] registers a second, richer [`PerfRenderer`] (typically
//! [`PerfOverlay`](crate::PerfOverlay), a bordered panel with a frame-time
//! [`Sparkline`](crate::Sparkline)); without it, the cycle degrades to the plain two-state
//! `Off`/`Compact` toggle.

mod app;
mod mode;
mod renderer;

pub use app::{PerfOverlayApp, default_is_toggle_key};
pub use mode::PerfOverlayMode;
pub use renderer::{DefaultPerfRenderer, PerfRenderer};

use retroglyph_core::surface::Layer;

/// How many frames [`PerfOverlayApp`]'s internal [`FrameStats`](retroglyph_core::frames::FrameStats)
/// remembers.
///
/// About two seconds at 60fps. Not configurable per instance: pick a bigger window by building a
/// `FrameStats` directly and rendering it through a custom [`PerfRenderer`] closure instead of
/// [`PerfOverlayApp`], if a specific app genuinely needs one.
pub const FRAME_HISTORY: usize = 120;

/// [`PerfOverlayApp`]'s default overlay layer: [`Layer::Debug`].
///
/// The workspace's named top-most UI tier, so a perf HUD stays visible over whatever else is on
/// screen (including an open [`Layer::Overlay`] popup) rather than risking a lower, app-chosen
/// layer hiding it. Override with [`PerfOverlayApp::layer`] if an app's own content already
/// reaches this layer.
pub const DEFAULT_LAYER: u8 = Layer::Debug.as_u8();
