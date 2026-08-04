//! [`PerfRenderer`] and the built-in [`DefaultPerfRenderer`].

use core::fmt::{self, Write as _};

use retroglyph_core::color::Style;
use retroglyph_core::frames::FrameStats;
use retroglyph_core::grid::Rect;

use super::FRAME_HISTORY;
use crate::Surface;
use crate::Theme;

/// Draws a [`super::PerfOverlayApp`]'s stats into a rectangular area of a [`Surface`].
///
/// Implemented for any `FnMut(&FrameStats<FRAME_HISTORY>, &str, Rect, &mut Surface<'_>)` (pass
/// such a closure to [`PerfOverlayApp::with_closure`](super::PerfOverlayApp::with_closure); see
/// its docs for why that constructor exists instead of just accepting `impl PerfRenderer`
/// everywhere), so a plain closure is enough for a custom overlay; see the [module
/// docs](super) for composing one out of this crate's widgets. [`DefaultPerfRenderer`] is the
/// built-in implementation, used by [`PerfOverlayApp::new`](super::PerfOverlayApp::new).
pub trait PerfRenderer {
    /// Draws `stats` (and the caller-supplied `backend` label) into `area`, via `surface` scoped
    /// to it.
    fn render(
        &mut self,
        stats: &FrameStats<FRAME_HISTORY>,
        backend: &str,
        area: Rect,
        surface: &mut Surface<'_>,
    );
}

impl<F> PerfRenderer for F
where
    F: FnMut(&FrameStats<FRAME_HISTORY>, &str, Rect, &mut Surface<'_>),
{
    fn render(
        &mut self,
        stats: &FrameStats<FRAME_HISTORY>,
        backend: &str,
        area: Rect,
        surface: &mut Surface<'_>,
    ) {
        self(stats, backend, area, surface);
    }
}

/// A fixed-capacity, stack-allocated [`fmt::Write`] sink, so [`DefaultPerfRenderer`] can format
/// its readout without heap-allocating a `String` every frame. Overflowing writes are rejected
/// (matching `core::fmt`'s own "stop, don't panic" policy); [`FixedBuf::as_str`] then simply
/// returns whatever was successfully written before the overflow.
struct FixedBuf<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> FixedBuf<N> {
    const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// Only ASCII is ever written into this buffer by [`DefaultPerfRenderer`] (digits, spaces,
    /// and the caller's `backend` label, which is expected to be a short ASCII identifier), so
    /// `len` bytes are always valid UTF-8; this falls back to `""` rather than panicking if that
    /// invariant is ever broken by a future caller.
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> fmt::Write for FixedBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let end = self.len + bytes.len();
        if end > N {
            return Err(fmt::Error);
        }
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

/// `duration` in whole milliseconds, for display. Precision loss below a millisecond is
/// immaterial to a live readout; [`FrameStats`] itself stays full [`core::time::Duration`]
/// precision, this is purely a formatting concern of [`DefaultPerfRenderer`].
fn millis(duration: core::time::Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

/// The built-in [`PerfRenderer`], used by [`PerfOverlayApp::new`](super::PerfOverlayApp::new).
///
/// A single-row `NNNfps MM.Mms minMM.M maxMM.M <backend>` readout, right-aligned within its area,
/// on a solid background. Colored from a [`Theme`] (see [`DefaultPerfRenderer::theme`]), so a
/// caller matching a [`Theme`]-driven UI elsewhere doesn't get a hardcoded, unrelated palette here.
///
/// A no-op before the first frame is recorded, or if the readout doesn't fit `area`'s width.
#[derive(Debug, Clone, Copy)]
pub struct DefaultPerfRenderer {
    theme: Theme,
}

impl Default for DefaultPerfRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultPerfRenderer {
    /// A renderer styled from [`Theme::DARK`].
    #[must_use]
    pub const fn new() -> Self {
        Self { theme: Theme::DARK }
    }

    /// Sets the [`Theme`] the readout's text/background colors come from: `theme.fg` on
    /// `theme.panel_bg`. Defaults to [`Theme::DARK`].
    #[must_use]
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl PerfRenderer for DefaultPerfRenderer {
    fn render(
        &mut self,
        stats: &FrameStats<FRAME_HISTORY>,
        backend: &str,
        area: Rect,
        surface: &mut Surface<'_>,
    ) {
        if stats.frame_count() == 0 || area.height() == 0 {
            return;
        }
        let mut text = FixedBuf::<96>::new();
        let _ = write!(
            text,
            " {:>3.0}fps {:>4.1}ms min{:>4.1} max{:>4.1} {backend} ",
            stats.fps(),
            millis(stats.current()),
            millis(stats.min()),
            millis(stats.max()),
        );
        let text = text.as_str();

        let width = area.width_usize();
        let len = text.chars().count();
        if len == 0 || len > width {
            return;
        }
        // `len` is bounded by `width`, itself widened from `area`'s own `u16` width, so
        // narrowing it back is always exact.
        #[allow(clippy::cast_possible_truncation)]
        let len_u16 = len as u16;
        let x0 = area.left() + (area.width() - len_u16);

        let style = Style::new().fg(self.theme.fg).bg(self.theme.panel_bg);
        for (i, ch) in text.chars().enumerate() {
            // `i` ranges over `0..len`, and `len <= width <= area.width()` (a `u16`), so
            // narrowing it back is always exact.
            #[allow(clippy::cast_possible_truncation)]
            let x = x0 + i as u16;
            surface.put((x, area.top()), ch, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::grid::{Grid, Pos};

    use super::*;

    #[test]
    fn fixed_buf_formats_without_allocating() {
        let mut buf = FixedBuf::<8>::new();
        let _ = write!(buf, "{:>3}fps", 62);
        assert_eq!(buf.as_str(), " 62fps");
    }

    #[test]
    fn fixed_buf_rejects_writes_past_capacity_and_keeps_what_fit() {
        let mut buf = FixedBuf::<4>::new();
        // "12345" (5 bytes) doesn't fit in a 4-byte buffer; the write errors out and only
        // whatever was written before the overflow (nothing, here, since it overflows on the
        // very first `write_str` call) is kept.
        assert!(write!(buf, "12345").is_err());
        assert_eq!(buf.as_str(), "");
    }

    #[test]
    fn default_impl_matches_new() {
        assert_eq!(
            DefaultPerfRenderer::default().theme,
            DefaultPerfRenderer::new().theme
        );
    }

    #[test]
    fn default_perf_renderer_is_a_noop_before_the_first_frame() {
        let stats = FrameStats::<FRAME_HISTORY>::new();
        let area = Rect::new(0, 0, 40, 1);
        let mut grid = Grid::new(40, 1);
        DefaultPerfRenderer::new().render(
            &stats,
            "headless",
            area,
            &mut Surface::new(&mut grid, area, 0),
        );
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn default_perf_renderer_is_a_noop_when_the_readout_does_not_fit() {
        let mut stats = FrameStats::<FRAME_HISTORY>::new();
        stats.record(core::time::Duration::from_millis(16));
        // A handful of columns can never fit "NNNfps ...": too narrow, not zero, so this exercises
        // the `len > width` guard rather than the `area.height() == 0` one.
        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        DefaultPerfRenderer::new().render(
            &stats,
            "headless",
            area,
            &mut Surface::new(&mut grid, area, 0),
        );
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn default_perf_renderer_shows_fps_ms_min_max_and_backend_right_aligned() {
        use alloc::string::String;

        let mut stats = FrameStats::<FRAME_HISTORY>::new();
        for _ in 0..5 {
            stats.record(core::time::Duration::from_millis(16));
        }
        let area = Rect::new(0, 0, 60, 1);
        let mut grid = Grid::new(60, 1);
        DefaultPerfRenderer::new().render(
            &stats,
            "headless",
            area,
            &mut Surface::new(&mut grid, area, 0),
        );
        let row: String = (0..60).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(row.contains("fps"), "{row}");
        assert!(row.contains("ms"), "{row}");
        assert!(row.contains("min"), "{row}");
        assert!(row.contains("max"), "{row}");
        assert!(row.contains("headless"), "{row}");
        // Right-aligned: the last character of the readout (the trailing space) sits in the
        // area's last column, not floating somewhere in the middle.
        assert_eq!(grid[Pos::new(59, 0)].glyph(), ' ');
        assert_ne!(grid[Pos::new(0, 0)].glyph(), 'h');
    }

    #[test]
    fn theme_overrides_the_default_colors() {
        let mut stats = FrameStats::<FRAME_HISTORY>::new();
        stats.record(core::time::Duration::from_millis(16));
        let area = Rect::new(0, 0, 60, 1);
        let mut grid = Grid::new(60, 1);
        DefaultPerfRenderer::new().theme(Theme::LIGHT).render(
            &stats,
            "headless",
            area,
            &mut Surface::new(&mut grid, area, 0),
        );
        // Column 59 (the trailing space) is always painted, regardless of the readout's exact
        // length, unlike column 0 which may sit left of a right-aligned short readout.
        assert_eq!(grid[Pos::new(59, 0)].style().foreground(), Theme::LIGHT.fg);
        assert_eq!(
            grid[Pos::new(59, 0)].style().background(),
            Theme::LIGHT.panel_bg
        );
    }
}
