//! [`PerfOverlay`]: a bordered live frame-time/FPS panel with a sparkline.
use alloc::format;

use retroglyph_core::app::Frame;
use retroglyph_core::color::Style;
use retroglyph_core::frames::FrameStats;
use retroglyph_core::grid::Rect;

use super::{AnimatedWidget, Panel, Sparkline, Text, Widget};
use crate::Surface;
use crate::Theme;

/// A bordered panel showing live [`FrameStats`].
///
/// An `NNNfps MM.Mms minMM.M maxMM.M <backend>` readout, any extra caller-supplied
/// metric rows (`VSync` state, resolution, render backend details, ...), and a scrolling
/// frame-time [`Sparkline`].
///
/// The richer counterpart to [`DefaultPerfRenderer`](crate::DefaultPerfRenderer): composed
/// entirely from existing widgets ([`Panel`], [`Text`], [`Sparkline`]), it draws through
/// [`Surface`] like every other widget in this crate, so it already works on every backend. Hand
/// it to [`PerfOverlayApp::with_closure`](crate::PerfOverlayApp::with_closure) as a closure to use
/// it instead of the built-in renderer:
///
/// ```
/// # #[cfg(feature = "std")]
/// # {
/// use retroglyph_core::app::{App, Flow, Frame};
/// use retroglyph_core::backend::{Backend, Headless};
/// use retroglyph_core::grid::Size;
/// use retroglyph_core::terminal::Terminal;
/// use retroglyph_widgets::{PerfOverlay, PerfOverlayApp, Widget};
///
/// struct MyGame;
/// impl<B: Backend> App<B> for MyGame {
///     fn update(&mut self, _term: &mut Terminal<B>, frame: &Frame) -> Flow {
///         if frame.frame >= 1 { Flow::Exit } else { Flow::Continue }
///     }
/// }
///
/// let term = Terminal::new(Headless::new(60, 12));
/// let app = PerfOverlayApp::with_closure(MyGame, "software", |stats, backend, area, surface| {
///     PerfOverlay::new(stats)
///         .backend(backend)
///         .metrics(&[("res", "1920x1080"), ("vsync", "on")])
///         .render(&mut surface.scope(area));
/// })
/// .size(Size::new(34, 8));
/// retroglyph_core::app::run_blocking(term, app).expect("run_blocking");
/// # } // `run_blocking` is `std`-only; a no-op under `--no-default-features`.
/// ```
///
/// `N` must match the [`FrameStats`] window it's built from; this crate's
/// [`PerfOverlayApp`](crate::PerfOverlayApp) always uses 120 samples
/// ([`FRAME_HISTORY`](crate::FRAME_HISTORY)), the default here too.
///
/// # As an [`AnimatedWidget`]
///
/// This type is read-only: it borrows an already-updated [`FrameStats`] (`new(stats)`), which is
/// exactly what [`Widget`] rendering wants but is the wrong shape for [`AnimatedWidget`], whose
/// `state: &mut FrameStats` argument needs to *record into* the same data a draw call reads --
/// an immutable borrow baked into `self` and a mutable one for `state` can't coexist. Use
/// [`AnimatedPerfOverlay`] instead for a call site that owns one [`FrameStats`] field and wants to
/// record and draw in a single call, with no [`PerfOverlayApp`](crate::PerfOverlayApp) decorator
/// wrapping the app.
///
/// Rows beyond the panel's available interior height are silently dropped: the readout row
/// draws first, then one row per [`metrics`](Self::metrics) entry, then the sparkline, each only
/// if there's still room, so a caller that under-sizes the area loses the least important rows
/// first rather than panicking or overflowing the border.
#[derive(Clone, Copy, Debug)]
pub struct PerfOverlay<'a, const N: usize = 120> {
    stats: &'a FrameStats<N>,
    backend: &'a str,
    title: &'a str,
    metrics: &'a [(&'a str, &'a str)],
    border_style: Style,
    fill_style: Style,
    text_style: Style,
    sparkline_style: Style,
}

impl<'a, const N: usize> PerfOverlay<'a, N> {
    /// A perf overlay reading `stats`, titled `"perf"`, with no backend label and no extra
    /// metrics, styled from [`Theme::DARK`] (as if [`PerfOverlay::theme`] had been called).
    #[must_use]
    pub fn new(stats: &'a FrameStats<N>) -> Self {
        Self {
            stats,
            backend: "",
            title: "perf",
            metrics: &[],
            border_style: Style::new(),
            fill_style: Style::new(),
            text_style: Style::new(),
            sparkline_style: Style::new(),
        }
        .theme(Theme::DARK)
    }

    /// Sets the backend label appended to the readout row (e.g. `"crossterm"`, `"software"`).
    /// Omitted entirely if left empty (the default).
    #[must_use]
    pub const fn backend(mut self, backend: &'a str) -> Self {
        self.backend = backend;
        self
    }

    /// Sets the panel's title. Defaults to `"perf"`.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// Sets extra `(label, value)` metric rows drawn below the readout: resolution, `VSync`
    /// state, render backend details, or anything else an app wants visible. Defaults to none.
    #[must_use]
    pub const fn metrics(mut self, metrics: &'a [(&'a str, &'a str)]) -> Self {
        self.metrics = metrics;
        self
    }

    /// Sets the readout/metric text's style. Defaults to [`Theme::DARK`]'s `fg` role.
    #[must_use]
    pub const fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }

    /// Sets the frame-time sparkline's bar color (every bar, uniformly; see
    /// [`Sparkline::style`]).
    ///
    /// Defaults to a fixed accent color, not [`Sparkline`]'s own green-to-red ramp: the sparkline
    /// scrolls, so "tallest bar in the visible window" isn't the same thing as "a slow frame":
    /// the same absolute frame time reads as short one moment and tall the next as the window's
    /// own max shifts. A ramp keyed to that relative height would tell a story the data doesn't
    /// support; one fixed color makes height the only signal, which is the honest one.
    #[must_use]
    pub const fn sparkline_style(mut self, style: Style) -> Self {
        self.sparkline_style = style;
        self
    }

    /// Applies `theme`'s named roles: `border_style`/`fill_style` map the same way as
    /// [`Panel::theme`], and `text_style` becomes `theme.fg` on `theme.panel_bg`.
    ///
    /// Call before any manual style override you want to keep.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.border_style = Style::new().fg(theme.border).bg(theme.title_bg);
        self.fill_style = Style::new().bg(theme.panel_bg);
        self.text_style = Style::new().fg(theme.fg).bg(theme.panel_bg);
        self.sparkline_style = Style::new().fg(theme.accent);
        self
    }
}

impl<const N: usize> Widget for PerfOverlay<'_, N> {
    fn render(&self, surface: &mut Surface<'_>) {
        let area = surface.area();
        if area.width() < 4 || area.height() < 3 {
            return;
        }

        Panel::new()
            .title(self.title)
            .border_style(self.border_style)
            .fill_style(self.fill_style)
            .render(surface);

        // Must match the 1-cell border inset the `Panel` above draws (the same rect
        // `Panel::inner` computes for a zero-padding panel). Kept inline rather than routed
        // through `Panel::inner` because the panel is built and dropped in the same statement.
        let inner = Rect::new(
            area.left() + 1,
            area.top() + 1,
            area.width() - 2,
            area.height() - 2,
        );
        let mut y = inner.top();
        let row = |y: u16| Rect::new(inner.left(), y, inner.width(), 1);

        if y < inner.bottom() {
            let readout = if self.backend.is_empty() {
                format!(
                    "{:>3.0}fps {:>4.1}ms  min {:>4.1} max {:>4.1}",
                    self.stats.fps(),
                    millis(self.stats.current()),
                    millis(self.stats.min()),
                    millis(self.stats.max()),
                )
            } else {
                format!(
                    "{:>3.0}fps {:>4.1}ms  min {:>4.1} max {:>4.1}  {}",
                    self.stats.fps(),
                    millis(self.stats.current()),
                    millis(self.stats.min()),
                    millis(self.stats.max()),
                    self.backend,
                )
            };
            Text::new(&readout)
                .style(self.text_style)
                .render(&mut surface.scope(row(y)));
            y += 1;
        }

        for (label, value) in self.metrics {
            if y >= inner.bottom() {
                break;
            }
            let line = format!("{label}: {value}");
            Text::new(&line)
                .style(self.text_style)
                .render(&mut surface.scope(row(y)));
            y += 1;
        }

        if y < inner.bottom() {
            let mut samples = [0.0f32; N];
            let mut len = 0;
            for duration in self.stats.samples() {
                if len >= N {
                    break;
                }
                samples[len] = millis(duration);
                len += 1;
            }
            Sparkline::new(&samples[..len])
                .style(self.sparkline_style)
                .render(&mut surface.scope(row(y)));
        }
    }
}

/// `duration` in whole milliseconds, for display. [`FrameStats`] itself stays full
/// [`core::time::Duration`] precision; this is purely a formatting concern of this widget.
fn millis(duration: core::time::Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

/// [`PerfOverlay`]'s [`AnimatedWidget`] counterpart.
///
/// The same readout, extra metric rows, and frame-time sparkline, but reading its [`FrameStats`]
/// from `state` at render time instead of borrowing one up front.
///
/// [`PerfOverlay::new`] takes `stats: &'a FrameStats<N>`, which [`Widget`] rendering (a pure read
/// of already-current data) wants but [`AnimatedWidget::render`] can't offer: its `state: &mut
/// FrameStats<N>` needs to record a fresh sample into the same data a draw call then reads, and an
/// immutable borrow baked into `self` can't coexist with a mutable one passed as `state` in the
/// same call. `AnimatedPerfOverlay` holds no stats reference of its own, only the same
/// backend/title/metrics/style knobs [`PerfOverlay`] has, so there's nothing to alias.
///
/// Replaces routing a `Duration` through [`PerfOverlayApp`](crate::PerfOverlayApp) just to reach
/// this widget: an app that owns one [`FrameStats`] field can record into it and draw in a single
/// call, no decorator wrapping the app at all. [`PerfOverlayApp`](crate::PerfOverlayApp) remains
/// the right choice for an app that also wants its toggle-key handling, mode cycling, and event
/// draining done generically, across any wrapped [`App`](retroglyph_core::app::App). This is only for
/// the (now unblocked) case that doesn't need any of that.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "std")]
/// # {
/// use retroglyph_core::app::{App, Flow, Frame};
/// use retroglyph_core::backend::{Backend, Headless};
/// use retroglyph_core::frames::FrameStats;
/// use retroglyph_core::grid::Rect;
/// use retroglyph_core::terminal::Terminal;
/// use retroglyph_widgets::{AnimatedPerfOverlay, AnimatedWidget};
///
/// struct MyGame {
///     stats: FrameStats,
/// }
///
/// impl<B: Backend> App<B> for MyGame {
///     fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
///         let area = Rect::new(0, 0, 34, 8);
///         let mut surface = term.surface();
///         AnimatedPerfOverlay::new()
///             .backend("software")
///             .render(&mut surface.scope(area), &mut self.stats, frame);
///         if frame.frame >= 1 { Flow::Exit } else { Flow::Continue }
///     }
/// }
///
/// let term = Terminal::new(Headless::new(60, 12));
/// let app = MyGame { stats: FrameStats::new() };
/// retroglyph_core::app::run_blocking(term, app).expect("run_blocking");
/// # } // `run_blocking` is `std`-only; a no-op under `--no-default-features`.
/// ```
#[derive(Clone, Copy, Debug)]
pub struct AnimatedPerfOverlay<'a, const N: usize = 120> {
    backend: &'a str,
    title: &'a str,
    metrics: &'a [(&'a str, &'a str)],
    border_style: Style,
    fill_style: Style,
    text_style: Style,
    sparkline_style: Style,
}

impl<const N: usize> Default for AnimatedPerfOverlay<'_, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, const N: usize> AnimatedPerfOverlay<'a, N> {
    /// An animated perf overlay titled `"perf"`, with no backend label and no extra metrics:
    /// the same defaults as [`PerfOverlay::new`], styled from [`Theme::DARK`] (as if
    /// [`AnimatedPerfOverlay::theme`] had been called). `N` must match the [`FrameStats`] window
    /// this is driven by; the default, 120, matches [`PerfOverlay`]'s and
    /// [`FrameStats`]'s own defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            backend: "",
            title: "perf",
            metrics: &[],
            border_style: Style::new(),
            fill_style: Style::new(),
            text_style: Style::new(),
            sparkline_style: Style::new(),
        }
        .theme(Theme::DARK)
    }

    /// See [`PerfOverlay::backend`].
    #[must_use]
    pub const fn backend(mut self, backend: &'a str) -> Self {
        self.backend = backend;
        self
    }

    /// See [`PerfOverlay::title`].
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// See [`PerfOverlay::metrics`].
    #[must_use]
    pub const fn metrics(mut self, metrics: &'a [(&'a str, &'a str)]) -> Self {
        self.metrics = metrics;
        self
    }

    /// See [`PerfOverlay::text_style`].
    #[must_use]
    pub const fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }

    /// See [`PerfOverlay::sparkline_style`].
    #[must_use]
    pub const fn sparkline_style(mut self, style: Style) -> Self {
        self.sparkline_style = style;
        self
    }

    /// See [`PerfOverlay::theme`].
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.border_style = Style::new().fg(theme.border).bg(theme.title_bg);
        self.fill_style = Style::new().bg(theme.panel_bg);
        self.text_style = Style::new().fg(theme.fg).bg(theme.panel_bg);
        self.sparkline_style = Style::new().fg(theme.accent);
        self
    }
}

impl<const N: usize> AnimatedWidget for AnimatedPerfOverlay<'_, N> {
    type State = FrameStats<N>;

    /// Records a sample into `state` via [`FrameStats::record`], then draws [`PerfOverlay::new`]
    /// built from the result (plus this type's own backend/title/metrics/style knobs), both in
    /// one call, so there's exactly one place, not two independently ordered ones, where the
    /// stats window advances.
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, frame: &Frame) {
        state.record(frame.delta);

        // A direct struct literal, not the public builder chain: `PerfOverlay` has no public
        // `border_style`/`fill_style` setters of its own (only `theme()` sets them together), but
        // both types live in this module, so their private fields are visible to each other here.
        let overlay = PerfOverlay {
            stats: &*state,
            backend: self.backend,
            title: self.title,
            metrics: self.metrics,
            border_style: self.border_style,
            fill_style: self.fill_style,
            text_style: self.text_style,
            sparkline_style: self.sparkline_style,
        };
        Widget::render(&overlay, surface);
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use core::time::Duration;

    use retroglyph_core::grid::{Grid, Pos};

    use super::*;

    fn settled<const N: usize>(millis: u64, frames: usize) -> FrameStats<N> {
        let mut stats = FrameStats::new();
        for _ in 0..frames {
            stats.record(Duration::from_millis(millis));
        }
        stats
    }

    #[test]
    fn draws_a_readout_border_and_title() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        PerfOverlay::new(&stats).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '┌');
        let title_row: String = (0..40).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(title_row.contains("perf"));
        let readout_row: String = (0..40).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        assert!(readout_row.contains("fps"));
        assert!(readout_row.contains("ms"));
        assert!(readout_row.contains("min"));
    }

    #[test]
    fn backend_label_is_appended_when_set() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 70, 5);
        let mut grid = Grid::new(70, 5);
        PerfOverlay::new(&stats)
            .backend("software")
            .render(&mut Surface::new(&mut grid, area, 0));

        let readout_row: String = (0..70).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        assert!(readout_row.contains("software"));
    }

    #[test]
    fn backend_label_omitted_when_empty() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        PerfOverlay::new(&stats).render(&mut Surface::new(&mut grid, area, 0));

        let readout_row: String = (0..40).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        assert!(!readout_row.contains("softw"));
    }

    #[test]
    fn extra_metric_rows_are_drawn_below_the_readout() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 40, 6);
        let mut grid = Grid::new(40, 6);
        PerfOverlay::new(&stats)
            .metrics(&[("res", "80x24"), ("vsync", "on")])
            .render(&mut Surface::new(&mut grid, area, 0));

        let row2: String = (0..40).map(|x| grid[Pos::new(x, 2)].glyph()).collect();
        let row3: String = (0..40).map(|x| grid[Pos::new(x, 3)].glyph()).collect();
        assert!(row2.contains("res") && row2.contains("80x24"));
        assert!(row3.contains("vsync") && row3.contains("on"));
    }

    #[test]
    fn metrics_beyond_available_height_are_dropped_not_overflowed() {
        let stats = settled::<120>(16, 5);
        // Interior is 1 row tall (area height 3 - 2 border rows): only the readout row fits.
        let area = Rect::new(0, 0, 40, 3);
        let mut grid = Grid::new(40, 3);
        PerfOverlay::new(&stats)
            .metrics(&[("res", "80x24")])
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 2)].glyph(), '└', "bottom border intact");
    }

    #[test]
    fn too_small_is_a_no_op() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 2, 2);
        let mut grid = Grid::new(2, 2);
        PerfOverlay::new(&stats).render(&mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_border_fill_and_text() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        PerfOverlay::new(&stats)
            .theme(Theme::DARK)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(grid[Pos::new(1, 1)].style().foreground(), Theme::DARK.fg);
    }

    fn frame(delta_ms: u64) -> Frame {
        Frame {
            delta: Duration::from_millis(delta_ms),
            frame: 0,
        }
    }

    #[test]
    fn animated_render_records_before_drawing() {
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        let mut stats = FrameStats::<120>::new();
        assert_eq!(stats.frame_count(), 0, "nothing recorded yet");

        AnimatedPerfOverlay::new().render(
            &mut Surface::new(&mut grid, area, 0),
            &mut stats,
            &frame(16),
        );

        assert_eq!(
            stats.frame_count(),
            1,
            "render should have recorded a sample"
        );
        // Drawn from the *post-record* stats, not a stale empty window: the readout row shows an
        // fps/ms reading rather than the "no frames yet" no-op PerfOverlay::render otherwise
        // takes (see too_small_is_a_no_op).
        let readout_row: String = (0..40).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        assert!(readout_row.contains("fps"), "{readout_row}");
    }

    #[test]
    fn animated_render_matches_perf_overlay_drawn_from_the_same_post_record_stats() {
        let area = Rect::new(0, 0, 60, 6);

        let mut animated_grid = Grid::new(60, 6);
        let mut stats = FrameStats::<120>::new();
        AnimatedPerfOverlay::new()
            .backend("software")
            .metrics(&[("res", "80x24")])
            .render(
                &mut Surface::new(&mut animated_grid, area, 0),
                &mut stats,
                &frame(16),
            );

        // `stats` now holds the one recorded sample; a plain `PerfOverlay` built from it (with
        // the same knobs) should draw byte-for-byte identically.
        let mut expected_grid = Grid::new(60, 6);
        PerfOverlay::new(&stats)
            .backend("software")
            .metrics(&[("res", "80x24")])
            .render(&mut Surface::new(&mut expected_grid, area, 0));

        for y in 0..6 {
            let animated_row: String = (0..60)
                .map(|x| animated_grid[Pos::new(x, y)].glyph())
                .collect();
            let expected_row: String = (0..60)
                .map(|x| expected_grid[Pos::new(x, y)].glyph())
                .collect();
            assert_eq!(animated_row, expected_row, "row {y}");
        }
    }

    #[test]
    fn animated_render_records_a_sample_every_call() {
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        let mut stats = FrameStats::<120>::new();

        for _ in 0..5 {
            AnimatedPerfOverlay::new().render(
                &mut Surface::new(&mut grid, area, 0),
                &mut stats,
                &frame(16),
            );
        }

        assert_eq!(stats.frame_count(), 5);
    }
}
