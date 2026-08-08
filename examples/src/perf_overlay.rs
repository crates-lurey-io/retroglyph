//! A local, gallery-only rebuild of the `App`-wrapping perf overlay retroglyph#1286 removed from
//! `retroglyph-ui` (`PerfOverlayApp`), scoped to exactly what this gallery needs: a two-line
//! `Compact` readout and a bordered `Full` panel (via `retroglyph_ui::widget::PerfOverlay`),
//! cycled by one toggle key.
//!
//! Not a general-purpose library type -- it lives here, not in `retroglyph-ui`, because the
//! wrapper is ordinary code any app can write against the crate's public `App`/`Terminal`/
//! `Surface` surface, not privileged code that needed to live inside the library it decorates.

#![allow(clippy::redundant_pub_crate)]

use std::vec::Vec;

use retroglyph::app::{App, Flow, Frame};
use retroglyph::backend::Backend;
use retroglyph::color::Style;
use retroglyph::event::{Event, KeyCode, KeyEventKind};
use retroglyph::frames::FrameStats;
use retroglyph::grid::{HasSize, Rect, Size};
use retroglyph::surface::Layer;
use retroglyph::terminal::Terminal;
use retroglyph::ui::Surface;
use retroglyph::ui::theme::Theme;
use retroglyph::ui::widget::{PerfOverlay, Text, Widget as _};

/// How many frames of [`FrameStats`] this overlay remembers. Matches `retroglyph-ui`'s own former
/// default, and what [`PerfOverlay`] is generic over by default.
const FRAME_HISTORY: usize = 120;

/// The layer the overlay draws on: the workspace's named top-most UI tier, so it stays visible
/// over whatever else an example draws.
const DEFAULT_LAYER: u8 = Layer::Debug.as_u8();

/// Whether `event` is the overlay's toggle key: backtick, or F1 as an alias, on a press (not a
/// repeat or a release, so a backend that reports releases doesn't toggle twice per physical
/// press).
pub(crate) fn is_toggle_key(event: &Event) -> bool {
    let Event::Key(key) = event else {
        return false;
    };
    key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Char('`') | KeyCode::F(1))
}

/// How much detail [`PerfOverlayApp`] currently shows, advanced by the toggle key: `Off ->
/// Compact -> Full -> Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Compact,
    Full,
}

impl Mode {
    const fn next(self) -> Self {
        match self {
            Self::Off => Self::Compact,
            Self::Compact => Self::Full,
            Self::Full => Self::Off,
        }
    }
}

/// Wraps `inner` with a toggleable perf overlay, generically across every `Backend`: drains
/// [`Terminal`]'s event queue looking for the toggle key, re-queues everything else via
/// [`Terminal::requeue_events`] so `inner` sees exactly the input it would have without this
/// wrapper, and draws `Compact`/`Full` on top of whatever `inner` drew.
pub(crate) struct PerfOverlayApp<A> {
    inner: A,
    stats: FrameStats<FRAME_HISTORY>,
    backend: &'static str,
    mode: Mode,
    /// Scratch buffer for `update`'s pass-through events, reused across frames so draining an
    /// event-free frame never allocates.
    passthrough: Vec<Event>,
}

impl<A> PerfOverlayApp<A> {
    /// Wraps `inner`; `backend` is a short label (e.g. `"crossterm"`, `"software"`) shown in the
    /// readout. Starts at [`Mode::Compact`]; see [`visible`](Self::visible) to start hidden.
    pub(crate) const fn new(inner: A, backend: &'static str) -> Self {
        Self {
            inner,
            stats: FrameStats::new(),
            backend,
            mode: Mode::Compact,
            passthrough: Vec::new(),
        }
    }

    /// Sets whether the overlay starts visible: `true` starts at [`Mode::Compact`], `false` at
    /// [`Mode::Off`]. The toggle key cycles through every mode at runtime regardless.
    pub(crate) const fn visible(mut self, visible: bool) -> Self {
        self.mode = if visible { Mode::Compact } else { Mode::Off };
        self
    }

    /// Advances to the next mode in the cycle (`Off -> Compact -> Full -> Off`). The same effect
    /// the toggle key has.
    pub(crate) const fn toggle(&mut self) {
        self.mode = self.mode.next();
    }

    /// `size` (columns, rows), flush against the top-right corner of a `term_width x
    /// term_height` terminal, clamped to its actual size.
    fn area(size: Size, term_width: u16, term_height: u16) -> Rect {
        let width = size.width().min(term_width);
        let height = size.height().min(term_height);
        Rect::new(term_width - width, 0, width, height)
    }
}

impl<B, A> App<B> for PerfOverlayApp<A>
where
    B: Backend,
    A: App<B>,
{
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        self.stats.record(frame.delta);

        let mut toggled = false;
        self.passthrough.clear();
        for event in term.drain_events() {
            if is_toggle_key(&event) {
                toggled = true;
            } else {
                self.passthrough.push(event);
            }
        }
        if toggled {
            self.mode = self.mode.next();
        }
        term.requeue_events(self.passthrough.drain(..));

        let mut flow = self.inner.update(term, frame);
        // A toggle is itself a visible change worth presenting, even on a frame the wrapped app
        // otherwise found nothing new to draw.
        if toggled && flow == Flow::Idle {
            flow = Flow::Continue;
        }

        let (term_width, term_height) = {
            let surface = term.surface();
            (surface.width(), surface.height())
        };

        match self.mode {
            Mode::Off => {}
            Mode::Compact => {
                let area = Self::area(Size::new(64, 1), term_width, term_height);
                if area.width() > 0 && area.height() > 0 {
                    let mut base = term.surface();
                    let mut surface = base.on_layer(DEFAULT_LAYER);
                    render_compact(&self.stats, self.backend, area, &mut surface);
                }
            }
            Mode::Full => {
                let area = Self::area(Size::new(46, 6), term_width, term_height);
                if area.width() > 0 && area.height() > 0 {
                    let mut base = term.surface();
                    let mut surface = base.on_layer(DEFAULT_LAYER);
                    PerfOverlay::new(&self.stats)
                        .backend(self.backend)
                        .render(&mut surface.scope(area));
                }
            }
        }

        flow
    }
}

/// `Compact` mode: a single-row `NNNfps MM.Mms minMM.M maxMM.M <backend>` readout, right-aligned
/// within `area`. A no-op before the first frame is recorded, or if the readout doesn't fit.
fn render_compact(
    stats: &FrameStats<FRAME_HISTORY>,
    backend: &str,
    area: Rect,
    surface: &mut Surface<'_>,
) {
    if stats.frame_count() == 0 || area.height() == 0 {
        return;
    }
    let text = std::format!(
        " {:>3.0}fps {:>4.1}ms min{:>4.1} max{:>4.1} {backend} ",
        stats.fps(),
        millis(stats.current()),
        millis(stats.min()),
        millis(stats.max()),
    );
    let len = text.chars().count();
    let width = usize::from(area.width());
    if len == 0 || len > width {
        return;
    }
    // `len <= width`, itself widened from `area.width()` (a `u16`), so narrowing back is exact.
    #[allow(clippy::cast_possible_truncation)]
    let len_u16 = len as u16;
    let row = Rect::new(
        area.left() + (area.width() - len_u16),
        area.top(),
        len_u16,
        1,
    );
    let style = Style::new().fg(Theme::DARK.fg).bg(Theme::DARK.panel_bg);
    Text::new(&text)
        .style(style)
        .render(&mut surface.scope(row));
}

/// `duration` in whole milliseconds, for display.
fn millis(duration: std::time::Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}
