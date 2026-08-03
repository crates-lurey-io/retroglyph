//! [`PerfOverlayApp`]: wraps an [`App`] with a toggleable perf overlay. See the [module
//! docs](super).

use alloc::boxed::Box;
use alloc::vec::Vec;

use retroglyph_core::{
    App, Backend, Event, Flow, Frame, FrameStats, HasSize, KeyCode, KeyEventKind, Rect, Size,
    Terminal,
};

use super::mode::PerfOverlayMode;
use super::renderer::{DefaultPerfRenderer, PerfRenderer};
use super::{DEFAULT_LAYER, FRAME_HISTORY};
use crate::Surface;

/// Whether `event` is the overlay's default toggle key.
///
/// Backtick, or F1 as an alias, on a press (not a repeat or a release, so a backend that reports
/// releases doesn't toggle twice per physical key press). Override with
/// [`PerfOverlayApp::toggle_key`].
#[must_use]
pub fn default_is_toggle_key(event: &Event) -> bool {
    let Event::Key(key) = event else {
        return false;
    };
    key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Char('`') | KeyCode::F(1))
}

/// [`PerfOverlayApp::cycle_with`]'s registered [`PerfOverlayMode::Full`] renderer and its area
/// size.
///
/// Kept separate from [`PerfOverlayApp::size`] (which stays
/// [`Compact`](PerfOverlayMode::Compact)-only) since a richer renderer typically needs a taller,
/// wider area than the single-row default.
struct FullMode {
    renderer: Box<dyn PerfRenderer>,
    size: Size,
}

/// Wraps an [`App`] with a toggleable perf overlay. See the [module docs](super).
pub struct PerfOverlayApp<A, R = DefaultPerfRenderer> {
    inner: A,
    stats: FrameStats<FRAME_HISTORY>,
    backend: &'static str,
    renderer: R,
    mode: PerfOverlayMode,
    full: Option<FullMode>,
    layer: u8,
    size: Size,
    toggle_key: fn(&Event) -> bool,
    /// Scratch buffer for [`update`](App::update)'s pass-through events, reused across frames (via
    /// `clear` rather than a fresh `Vec` each call) so draining an event-free frame (the common
    /// case in an unpaced game loop calling this many times a second) never allocates.
    passthrough: Vec<Event>,
}

impl<A> PerfOverlayApp<A, DefaultPerfRenderer> {
    /// Wraps `inner`, drawing with [`DefaultPerfRenderer`]. `backend` is a short label (e.g.
    /// `"crossterm"`, `"software"`, `"gl"`) shown in the readout; there is no portable way to ask
    /// a [`Backend`] what to call itself, so every caller supplies it directly.
    #[must_use]
    pub fn new(inner: A, backend: &'static str) -> Self {
        Self::with_renderer(inner, backend, DefaultPerfRenderer::new())
    }
}

impl<A, F> PerfOverlayApp<A, F>
where
    F: FnMut(&FrameStats<FRAME_HISTORY>, &str, Rect, &mut Surface<'_>),
{
    /// Wraps `inner`, drawing with a plain closure instead of [`DefaultPerfRenderer`].
    ///
    /// Prefer this over [`with_renderer`](Self::with_renderer) for a closure: writing the bound
    /// directly as `FnMut(...)` here (rather than the [`PerfRenderer`] trait `with_renderer`
    /// takes) is what lets Rust infer a bare closure's parameter types from context, the same way
    /// it does for any other `Fn`-bounded API: a closure passed to `with_renderer` needs its
    /// parameters annotated by hand, or type inference has nothing to pin them down to.
    #[must_use]
    pub fn with_closure(inner: A, backend: &'static str, renderer: F) -> Self {
        Self::with_renderer(inner, backend, renderer)
    }
}

impl<A, R: PerfRenderer> PerfOverlayApp<A, R> {
    /// Wraps `inner`, drawing with a custom [`PerfRenderer`] instead of [`DefaultPerfRenderer`].
    /// This constructor is for a named type that implements [`PerfRenderer`] directly (there is no
    /// stable way for a plain struct to implement `FnMut` itself, which is why the trait exists
    /// at all: see the [module docs](super)). See [`PerfOverlayApp::with_closure`] for why a bare
    /// closure should go through that constructor instead.
    ///
    /// Defaults: visible, [`DEFAULT_LAYER`](super::DEFAULT_LAYER), a `64x1` area (wide enough for
    /// [`DefaultPerfRenderer`]'s single-row readout with a short backend label), and
    /// [`default_is_toggle_key`]; override any of these with the chainable methods below before
    /// the wrapped app first runs.
    #[must_use]
    pub fn with_renderer(inner: A, backend: &'static str, renderer: R) -> Self {
        Self {
            inner,
            stats: FrameStats::new(),
            backend,
            renderer,
            mode: PerfOverlayMode::Compact,
            full: None,
            layer: DEFAULT_LAYER,
            size: Size::new(64, 1),
            toggle_key: default_is_toggle_key,
            passthrough: Vec::new(),
        }
    }

    /// Registers a second, richer [`PerfRenderer`] (a plain closure works, the same way
    /// [`with_closure`](Self::with_closure) does for the primary one) at [`PerfOverlayMode::Full`],
    /// `size` columns by rows, and makes it reachable: the toggle key now cycles `Off -> Compact
    /// -> Full -> Off` instead of just `Off -> Compact -> Off`.
    ///
    /// A natural pairing is [`DefaultPerfRenderer`] (or a closure) at
    /// [`Compact`](PerfOverlayMode::Compact) and [`PerfOverlay`](crate::PerfOverlay) (a bordered
    /// panel with a frame-time sparkline) at `Full`; see the [module docs](super).
    #[must_use]
    pub fn cycle_with<F>(mut self, size: Size, renderer: F) -> Self
    where
        F: FnMut(&FrameStats<FRAME_HISTORY>, &str, Rect, &mut Surface<'_>) + 'static,
    {
        // Bound as `FnMut(...)` directly for the same reason `with_closure` is; see its doc
        // comment. `F: FnMut(...)` still satisfies `PerfRenderer` via its blanket impl, so this
        // boxes fine.
        self.full = Some(FullMode {
            renderer: Box::new(renderer),
            size,
        });
        self
    }

    /// Sets whether the overlay starts visible: `true` starts at [`PerfOverlayMode::Compact`],
    /// `false` at [`PerfOverlayMode::Off`]. Defaults to `true`; the toggle key cycles through
    /// every registered mode at runtime regardless; see [`set_mode`](Self::set_mode) to start
    /// directly at [`PerfOverlayMode::Full`] instead.
    #[must_use]
    pub const fn visible(mut self, visible: bool) -> Self {
        self.mode = if visible {
            PerfOverlayMode::Compact
        } else {
            PerfOverlayMode::Off
        };
        self
    }

    /// Sets which grid layer the overlay draws on. Defaults to
    /// [`DEFAULT_LAYER`](super::DEFAULT_LAYER); raise it if the wrapped app's own content already
    /// reaches that layer.
    #[must_use]
    pub const fn layer(mut self, layer: u8) -> Self {
        self.layer = layer;
        self
    }

    /// Sets [`Compact`](PerfOverlayMode::Compact)'s area size, placed flush against the
    /// top-right corner of the terminal (clamped to its actual size). Defaults to `64x1`, sized
    /// for [`DefaultPerfRenderer`]'s single-row readout; a custom [`PerfRenderer`] that draws a
    /// panel or a sparkline at `Compact` needs a taller (and often wider) area: size it to fit,
    /// or register it as the richer [`Full`](PerfOverlayMode::Full) mode via
    /// [`cycle_with`](Self::cycle_with) instead, which takes its own size.
    #[must_use]
    pub const fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Overrides which events toggle the overlay's visibility. Defaults to
    /// [`default_is_toggle_key`].
    #[must_use]
    pub const fn toggle_key(mut self, matches: fn(&Event) -> bool) -> Self {
        self.toggle_key = matches;
        self
    }

    /// The frame-time statistics fed by every [`update`](App::update) call so far.
    #[must_use]
    pub const fn stats(&self) -> &FrameStats<FRAME_HISTORY> {
        &self.stats
    }

    /// Whether the overlay currently renders: `true` for [`Compact`](PerfOverlayMode::Compact) or
    /// [`Full`](PerfOverlayMode::Full), `false` for [`Off`](PerfOverlayMode::Off). See
    /// [`mode`](Self::mode) to distinguish `Compact` from `Full`.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        !matches!(self.mode, PerfOverlayMode::Off)
    }

    /// The overlay's current [`PerfOverlayMode`].
    #[must_use]
    pub const fn mode(&self) -> PerfOverlayMode {
        self.mode
    }

    /// Sets the overlay's [`PerfOverlayMode`] directly: the one way to mutate visibility at
    /// runtime, including reaching [`Full`](PerfOverlayMode::Full) directly rather than only
    /// through [`toggle`](Self::toggle)'s cycle. Setting `Full` without a renderer registered via
    /// [`cycle_with`](Self::cycle_with) is accepted but renders nothing (the same as `Off`) until
    /// one is.
    pub const fn set_mode(&mut self, mode: PerfOverlayMode) {
        self.mode = mode;
    }

    /// Advances to the next [`PerfOverlayMode`] in the cycle (`Off -> Compact -> Full -> Off`,
    /// skipping `Full` if nothing was registered via [`cycle_with`](Self::cycle_with)). The same
    /// effect the toggle key has.
    pub const fn toggle(&mut self) {
        self.mode = self.mode.next(self.full.is_some());
    }

    /// A shared reference to the wrapped app.
    #[must_use]
    pub const fn inner(&self) -> &A {
        &self.inner
    }

    /// A mutable reference to the wrapped app.
    pub const fn inner_mut(&mut self) -> &mut A {
        &mut self.inner
    }

    /// Unwraps this overlay, discarding its accumulated [`FrameStats`] and returning the wrapped
    /// app.
    #[must_use]
    pub fn into_inner(self) -> A {
        self.inner
    }

    /// `size` (columns, rows), flush against the top-right corner of a `term_width x
    /// term_height` terminal, clamped to its actual size.
    fn area(size: Size, term_width: u16, term_height: u16) -> Rect {
        let width = size.width().min(term_width);
        let height = size.height().min(term_height);
        Rect::new(term_width - width, 0, width, height)
    }
}

impl<B, A, R> App<B> for PerfOverlayApp<A, R>
where
    B: Backend,
    A: App<B>,
    R: PerfRenderer,
{
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        self.stats.record(frame.delta);

        // Drain every pending event before the wrapped app runs, keep the toggle presses, and
        // re-queue everything else via `Terminal::requeue_events` so the wrapped app sees exactly
        // the input it would have without this wrapper, minus the toggles. This goes entirely
        // through `Terminal`'s own event queue, never a backend-specific input path (in
        // particular, never `Input::push_event`, whose documented default is a no-op for
        // backends with no external event source): see the module docs' "Toggling" section.
        let mut toggled = false;
        self.passthrough.clear();
        for event in term.drain_events() {
            if (self.toggle_key)(&event) {
                toggled = true;
            } else {
                self.passthrough.push(event);
            }
        }
        if toggled {
            self.mode = self.mode.next(self.full.is_some());
        }
        term.requeue_events(self.passthrough.drain(..));

        let mut flow = self.inner.update(term, frame);
        // A toggle is itself a visible change worth presenting, even on a frame the wrapped app
        // otherwise found nothing new to draw (`Flow::Idle`): upgrade to `Continue` so the flip
        // actually reaches the screen this frame instead of waiting for the next redraw.
        if toggled && flow == Flow::Idle {
            flow = Flow::Continue;
        }

        // `Compact` and `Full` each have their own renderer and area size (see `FullMode`'s
        // docs); `Off`, and `Full` without a registered `FullMode`, draw nothing.
        let drawn = match self.mode {
            PerfOverlayMode::Off => None,
            PerfOverlayMode::Compact => Some((self.size, None)),
            PerfOverlayMode::Full => self.full.as_mut().map(|full| (full.size, Some(full))),
        };
        if let Some((size, full)) = drawn {
            let (term_width, term_height) = {
                let surface = term.surface();
                (surface.width(), surface.height())
            };
            let area = Self::area(size, term_width, term_height);
            if area.width() > 0 && area.height() > 0 {
                let mut base = term.surface();
                let mut surface = base.on_layer(self.layer);
                match full {
                    None => self
                        .renderer
                        .render(&self.stats, self.backend, area, &mut surface),
                    Some(full) => {
                        full.renderer
                            .render(&self.stats, self.backend, area, &mut surface);
                    }
                }
            }
        }

        flow
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::backend::{Cursor, Headless, Input, Output};
    use retroglyph_core::{KeyEvent, KeyModifiers, Pos, Style};

    use super::*;

    struct CountingApp {
        updates: u32,
        exit_at: u64,
    }

    impl<B: Backend> App<B> for CountingApp {
        fn update(&mut self, _term: &mut Terminal<B>, frame: &Frame) -> Flow {
            self.updates += 1;
            if frame.frame >= self.exit_at {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
    }

    fn frame(n: u64) -> Frame {
        Frame {
            delta: core::time::Duration::from_millis(16),
            frame: n,
        }
    }

    /// Wraps [`Headless`] but never overrides `Input::push_event`, relying on the trait's
    /// documented no-op default the way a backend with no external event source legitimately
    /// can (see `retroglyph_core::backend`'s [`Input`] doc). Exists only to prove
    /// `PerfOverlayApp` no longer needs that override to pass input through.
    struct NoPushEventBackend(Headless);

    impl Output for NoPushEventBackend {
        type Error = <Headless as Output>::Error;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = retroglyph_core::backend::DrawCell<'a>>,
        {
            self.0.draw_layers(content)
        }

        fn resize(&mut self, size: Size) {
            self.0.resize(size);
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.0.flush()
        }

        fn size(&self) -> Size {
            self.0.size()
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.0.clear()
        }
    }

    impl Input for NoPushEventBackend {
        fn poll_event(&mut self, timeout: core::time::Duration) -> Option<Event> {
            self.0.poll_event(timeout)
        }
        // `push_event` intentionally not overridden: this backend relies on the trait's
        // documented no-op default, as `Crossterm` and windowed backends' doc comments say a
        // backend with no external event source is free to do.
    }

    impl Cursor for NoPushEventBackend {
        fn set_cursor_visible(&mut self, visible: bool) {
            self.0.set_cursor_visible(visible);
        }

        fn set_cursor_position(&mut self, position: Pos) {
            self.0.set_cursor_position(position);
        }
    }

    struct SeesInput {
        saw: bool,
    }

    impl<B: Backend> App<B> for SeesInput {
        fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
            if term.drain_events().next().is_some() {
                self.saw = true;
            }
            Flow::Continue
        }
    }

    #[test]
    fn perf_overlay_passes_input_through_on_a_backend_with_the_default_push_event() {
        let mut term = Terminal::new(NoPushEventBackend(Headless::new(40, 5)));
        term.backend_mut().0.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));

        let mut overlay = PerfOverlayApp::new(SeesInput { saw: false }, "custom");
        App::update(&mut overlay, &mut term, &frame(0));

        assert!(overlay.inner().saw);
    }

    #[test]
    fn default_is_toggle_key_matches_backtick_and_f1_presses_only() {
        for code in [KeyCode::Char('`'), KeyCode::F(1)] {
            let press = Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
            assert!(default_is_toggle_key(&press));
        }
        let release = Event::Key(KeyEvent::with_kind(
            KeyCode::Char('`'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!default_is_toggle_key(&release));
        assert!(!default_is_toggle_key(&Event::Close));
        let other = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!default_is_toggle_key(&other));
    }

    #[test]
    fn forwards_updates_and_records_frame_stats() {
        let mut term = Terminal::new(Headless::new(40, 5));
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 2,
            },
            "headless",
        );

        for n in 0..3 {
            let flow = App::update(&mut overlay, &mut term, &frame(n));
            if flow == Flow::Exit {
                break;
            }
        }

        assert_eq!(overlay.inner().updates, 3);
        assert_eq!(overlay.stats().frame_count(), 3);
    }

    #[test]
    fn swallows_toggle_key_and_passes_other_input_through() {
        let mut term = Terminal::new(Headless::new(40, 5));
        term.backend_mut().push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('`'),
            KeyModifiers::NONE,
        )));
        term.backend_mut().push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));

        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        );
        assert!(overlay.is_visible());
        let _ = App::update(&mut overlay, &mut term, &frame(0));
        assert!(!overlay.is_visible(), "backtick should have toggled off");

        // The non-toggle key was re-queued for the wrapped app (and anyone else) to see.
        let remaining: Vec<Event> = term.drain_events().collect();
        assert_eq!(
            remaining,
            alloc::vec![Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                KeyModifiers::NONE
            ))]
        );
    }

    #[test]
    fn toggling_while_idle_upgrades_flow_to_continue() {
        struct AlwaysIdle;
        impl<B: Backend> App<B> for AlwaysIdle {
            fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                Flow::Idle
            }
        }

        let mut term = Terminal::new(Headless::new(40, 5));
        term.backend_mut().push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('`'),
            KeyModifiers::NONE,
        )));
        let mut overlay = PerfOverlayApp::new(AlwaysIdle, "headless");

        let flow = App::update(&mut overlay, &mut term, &frame(0));
        assert_eq!(
            flow,
            Flow::Continue,
            "toggle should upgrade Idle to Continue"
        );

        // A second, toggle-free frame stays Idle.
        let flow = App::update(&mut overlay, &mut term, &frame(1));
        assert_eq!(flow, Flow::Idle);
    }

    #[test]
    fn hidden_overlay_draws_nothing() {
        let mut term = Terminal::new(Headless::new(40, 5));
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        )
        .visible(false);

        let _ = App::update(&mut overlay, &mut term, &frame(0));
        term.present().expect("present");
        assert!(
            !term.backend().format_view().contains("fps"),
            "hidden overlay should not draw"
        );
    }

    #[test]
    fn visible_overlay_draws_a_readout_after_the_first_frame() {
        let mut term = Terminal::new(Headless::new(80, 5));
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        );

        let _ = App::update(&mut overlay, &mut term, &frame(0));
        term.present().expect("present");
        let view = term.backend().format_view();
        assert!(view.contains("fps"), "{view}");
        assert!(view.contains("headless"), "{view}");
    }

    #[test]
    fn custom_renderer_closure_is_used_instead_of_the_default() {
        let mut term = Terminal::new(Headless::new(40, 5));
        let mut overlay = PerfOverlayApp::with_closure(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
            |_stats, backend, area, surface| {
                surface.print((area.left(), area.top()), backend, Style::new());
            },
        );

        let _ = App::update(&mut overlay, &mut term, &frame(0));
        term.present().expect("present");
        let view = term.backend().format_view();
        assert!(view.contains("headless"), "{view}");
        assert!(
            !view.contains("fps"),
            "custom renderer should replace the default, {view}"
        );
    }

    #[test]
    fn without_cycle_with_toggle_only_visits_off_and_compact() {
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        );
        assert_eq!(overlay.mode(), PerfOverlayMode::Compact);
        overlay.toggle();
        assert_eq!(overlay.mode(), PerfOverlayMode::Off);
        overlay.toggle();
        assert_eq!(overlay.mode(), PerfOverlayMode::Compact);
    }

    #[test]
    fn cycle_with_makes_toggle_visit_off_compact_full() {
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        )
        .cycle_with(Size::new(40, 6), |_stats, _backend, _area, _surface| {});

        assert_eq!(overlay.mode(), PerfOverlayMode::Compact, "starts visible");
        overlay.toggle();
        assert_eq!(overlay.mode(), PerfOverlayMode::Full);
        overlay.toggle();
        assert_eq!(overlay.mode(), PerfOverlayMode::Off);
        overlay.toggle();
        assert_eq!(overlay.mode(), PerfOverlayMode::Compact, "cycle repeats");
    }

    #[test]
    fn set_mode_jumps_directly_to_full() {
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        )
        .cycle_with(Size::new(40, 6), |_stats, _backend, _area, _surface| {});

        overlay.set_mode(PerfOverlayMode::Full);
        assert_eq!(overlay.mode(), PerfOverlayMode::Full);
        assert!(overlay.is_visible());
    }

    #[test]
    fn full_mode_draws_with_its_own_renderer_at_its_own_size() {
        let mut term = Terminal::new(Headless::new(60, 10));
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        )
        .cycle_with(Size::new(30, 5), |_stats, _backend, area, surface| {
            // No space in the marker: `Headless::format_view` renders a space glyph as `·`,
            // so a literal `" "` in the assertion below would never match.
            surface.print((area.left(), area.top()), "FULLMODE", Style::new());
        });

        // Compact mode: the default renderer's readout shows, not the Full-mode marker.
        let _ = App::update(&mut overlay, &mut term, &frame(0));
        term.present().expect("present");
        let view = term.backend().format_view();
        assert!(view.contains("fps"), "{view}");
        assert!(!view.contains("FULLMODE"), "{view}");

        // Advance to Full: now the registered renderer draws instead.
        overlay.set_mode(PerfOverlayMode::Full);
        let _ = App::update(&mut overlay, &mut term, &frame(1));
        term.present().expect("present");
        let view = term.backend().format_view();
        assert!(view.contains("FULLMODE"), "{view}");
    }

    #[test]
    fn area_places_the_overlay_flush_against_the_top_right_corner() {
        let area =
            PerfOverlayApp::<CountingApp, DefaultPerfRenderer>::area(Size::new(20, 3), 80, 24);
        assert_eq!(area, Rect::new(60, 0, 20, 3));
    }

    #[test]
    fn area_clamps_to_a_terminal_smaller_than_the_requested_size() {
        let area =
            PerfOverlayApp::<CountingApp, DefaultPerfRenderer>::area(Size::new(20, 3), 10, 2);
        assert_eq!(area, Rect::new(0, 0, 10, 2));
    }

    #[test]
    fn toggle_key_can_be_overridden() {
        let mut term = Terminal::new(Headless::new(40, 5));
        term.backend_mut().push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::NONE,
        )));
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 0,
                exit_at: 100,
            },
            "headless",
        )
        .toggle_key(|event| {
            matches!(
                event,
                Event::Key(k) if k.code == KeyCode::Char('p') && k.kind == KeyEventKind::Press
            )
        });

        assert!(overlay.is_visible());
        let _ = App::update(&mut overlay, &mut term, &frame(0));
        assert!(
            !overlay.is_visible(),
            "the overridden toggle key ('p') should have cycled the overlay off"
        );

        // The default toggle key (backtick) no longer does anything once overridden.
        term.backend_mut().push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('`'),
            KeyModifiers::NONE,
        )));
        let _ = App::update(&mut overlay, &mut term, &frame(1));
        assert!(
            !overlay.is_visible(),
            "backtick should no longer be the toggle key"
        );
    }

    #[test]
    fn inner_accessors_and_into_inner_round_trip() {
        let mut overlay = PerfOverlayApp::new(
            CountingApp {
                updates: 3,
                exit_at: 100,
            },
            "headless",
        );
        assert_eq!(overlay.inner().updates, 3);
        overlay.inner_mut().updates = 7;
        assert_eq!(overlay.inner().updates, 7);
        assert_eq!(overlay.into_inner().updates, 7);
    }
}
