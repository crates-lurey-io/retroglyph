//! [`PerfOverlay`]: a bordered live frame-time/FPS panel with a sparkline.
use retroglyph_core::{Color, FrameStats, Rect, Style};

use super::{Panel, Sparkline, Text, Widget};
use crate::Surface;
use crate::Theme;

/// [`PerfOverlay::sparkline_style`]'s default: a fixed accent, matching [`Theme::DARK`]'s
/// `accent`. See that method's docs for why it's a fixed color, not [`Sparkline`]'s own default
/// [`Meter`](super::Meter) ramp.
const DEFAULT_SPARKLINE_COLOR: Color = Color::Rgb {
    r: 90,
    g: 170,
    b: 250,
};

/// A bordered panel showing live [`FrameStats`].
///
/// An `NNNfps MM.Mms minMM.M maxMM.M <backend>` readout, any extra caller-supplied
/// metric rows (`VSync` state, resolution, render backend details, ...), and a scrolling
/// frame-time [`Sparkline`].
///
/// The richer counterpart to [`retroglyph_core::DefaultPerfRenderer`]: composed entirely from
/// existing widgets ([`Panel`], [`Text`], [`Sparkline`]), it draws through [`Surface`] like every
/// other widget in this crate, so it already works on every backend. Hand it to
/// [`PerfOverlayApp::with_closure`](retroglyph_core::PerfOverlayApp::with_closure) as a closure
/// to use it instead of the built-in renderer:
///
/// ```
/// use retroglyph_core::{App, Backend, Flow, Frame, Headless, PerfOverlayApp, Size, Terminal};
/// use retroglyph_widgets::{PerfOverlay, Widget};
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
///         .render(area, surface);
/// })
/// .size(Size::new(34, 8));
/// retroglyph_core::run_blocking(term, app).expect("run_blocking");
/// ```
///
/// `N` must match the [`FrameStats`] window it's built from; `retroglyph-core`'s
/// [`PerfOverlayApp`](retroglyph_core::PerfOverlayApp) always uses 120 samples
/// ([`FRAME_HISTORY`](retroglyph_core::perf_overlay::FRAME_HISTORY)), the default here too.
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
    /// metrics.
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
            sparkline_style: Style::new().fg(DEFAULT_SPARKLINE_COLOR),
        }
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

    /// Sets extra `(label, value)` metric rows drawn below the readout -- resolution, `VSync`
    /// state, render backend details, or anything else an app wants visible. Defaults to none.
    #[must_use]
    pub const fn metrics(mut self, metrics: &'a [(&'a str, &'a str)]) -> Self {
        self.metrics = metrics;
        self
    }

    /// Sets the readout/metric text's style. Defaults to [`Style::new()`].
    #[must_use]
    pub const fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }

    /// Sets the frame-time sparkline's bar color (every bar, uniformly -- see
    /// [`Sparkline::style`]).
    ///
    /// Defaults to a fixed accent color, not [`Sparkline`]'s own green-to-red ramp: the sparkline
    /// scrolls, so "tallest bar in the visible window" isn't the same thing as "a slow frame" --
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
    fn render(&self, area: Rect, surface: &mut Surface<'_>) {
        if area.width() < 4 || area.height() < 3 {
            return;
        }

        Panel::new()
            .title(self.title)
            .border_style(self.border_style)
            .fill_style(self.fill_style)
            .render(area, surface);

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
                .render(row(y), surface);
            y += 1;
        }

        for (label, value) in self.metrics {
            if y >= inner.bottom() {
                break;
            }
            let line = format!("{label}: {value}");
            Text::new(&line)
                .style(self.text_style)
                .render(row(y), surface);
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
                .render(row(y), surface);
        }
    }
}

/// `duration` in whole milliseconds, for display. [`FrameStats`] itself stays full
/// [`core::time::Duration`] precision; this is purely a formatting concern of this widget.
fn millis(duration: core::time::Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use retroglyph_core::{Grid, Pos};

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
        PerfOverlay::new(&stats).render(area, &mut Surface::new(&mut grid, area, 0));

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
            .render(area, &mut Surface::new(&mut grid, area, 0));

        let readout_row: String = (0..70).map(|x| grid[Pos::new(x, 1)].glyph()).collect();
        assert!(readout_row.contains("software"));
    }

    #[test]
    fn backend_label_omitted_when_empty() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        PerfOverlay::new(&stats).render(area, &mut Surface::new(&mut grid, area, 0));

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
            .render(area, &mut Surface::new(&mut grid, area, 0));

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
            .render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 2)].glyph(), '└', "bottom border intact");
    }

    #[test]
    fn too_small_is_a_no_op() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 2, 2);
        let mut grid = Grid::new(2, 2);
        PerfOverlay::new(&stats).render(area, &mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_border_fill_and_text() {
        let stats = settled::<120>(16, 5);
        let area = Rect::new(0, 0, 40, 5);
        let mut grid = Grid::new(40, 5);
        PerfOverlay::new(&stats)
            .theme(Theme::DARK)
            .render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(
            grid[Pos::new(0, 0)].style().foreground(),
            Theme::DARK.border
        );
        assert_eq!(grid[Pos::new(1, 1)].style().foreground(), Theme::DARK.fg);
    }
}
