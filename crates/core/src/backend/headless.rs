//! In-memory backend for testing. Stores presented content and allows injecting synthetic events.
//!
//! [`Headless::format_view`](crate::backend::Headless::format_view) renders the current frame for snapshot testing (e.g. with `insta`)
//! and [`Headless::push_event`](crate::backend::Headless::push_event) queues synthetic input for a
//! test to drive directly, or to feed through [`TestHarness`](crate::testing::TestHarness) when a
//! full `App` loop is involved.
//!
//! [`Headless::format_styled`] emits Select Graphic Rendition (SGR) parameters via
//! [`crate::color::sgr`], shared with `retroglyph-recorder`'s asciicast export so the two
//! encoders can't drift apart.

use crate::backend::{Cursor, CursorStyle, DrawCell, Input, Output};
use crate::color::Style;
use crate::event::{Event, coalesces_with};
use crate::grid::{Grid, HasSize, Pos, Size};
use crate::tile::Tile;
use alloc::collections::VecDeque;
use alloc::string::String;
use core::time::Duration;

/// In-memory backend for testing.
///
/// Stores presented content and allows injecting synthetic events.
pub struct Headless {
    layers: Grid,
    composited: Option<Grid>,
    cursor_visible: bool,
    cursor_pos: Pos,
    cursor_style: CursorStyle,
    event_queue: VecDeque<Event>,
}

impl Headless {
    /// The glyph [`format_view`](Self::format_view)/[`format_styled`](Self::format_styled) render in place of a literal space, so
    /// layout is visible in text diffs.
    ///
    /// Public so a caller converting a rendered view back to plain text (or a search needle's
    /// spaces into the view's own convention, as [`TestHarness::find_text`](crate::testing::TestHarness::find_text) does) has
    /// something to match against other than a hardcoded `'·'` that could silently drift out of
    /// sync with this backend's actual choice. Prefer [`TestHarness::readable_view`](crate::testing::TestHarness::readable_view) for the
    /// common case of getting a plain string back for `.contains`/`assert_eq!` checks.
    pub const SPACE_GLYPH: char = '·';

    /// Creates a new headless backend of the given dimensions.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            layers: Grid::new(width, height),
            composited: None,
            cursor_visible: false,
            cursor_pos: Pos::default(),
            cursor_style: CursorStyle::default(),
            event_queue: VecDeque::new(),
        }
    }

    /// Returns the composited frame: every layer this backend has been sent, flattened into a
    /// single layer under the [`TileFlags::EMPTY`](crate::tile::TileFlags) transparency rule
    /// documented on [`crate::grid`].
    ///
    /// For the usual case, a backend left at the default
    /// [`compositing`](Output::compositing) of [`Compositing::CellFlattened`](crate::backend::Compositing::CellFlattened),
    /// [`crate::terminal::Terminal::present`] has already flattened the frame and only layer 0 is
    /// ever written, so this is simply the received content. Use
    /// [`layer_grid`](Self::layer_grid) to inspect the raw per-layer state instead.
    #[must_use]
    pub fn grid(&self) -> &Grid {
        self.composited.as_ref().unwrap_or(&self.layers)
    }

    /// Returns the raw, un-composited per-layer state, as received.
    ///
    /// Only interesting for a backend wrapping this one that returns
    /// [`Compositing::PixelLayered`](crate::backend::Compositing::PixelLayered) from
    /// [`compositing`](Output::compositing) and so receives the multi-layer stream:
    /// this is what lets a test assert which layer a cell arrived on, rather than only what the
    /// composited frame looks like. Otherwise identical to [`grid`](Self::grid).
    #[must_use]
    pub const fn layer_grid(&self) -> &Grid {
        &self.layers
    }

    /// Recomputes [`grid`](Self::grid) from [`layer_grid`](Self::layer_grid).
    ///
    /// A no-op while only layer 0 has ever been written, which keeps the single-layer path (every
    /// cell backend) free of both the flatten and the second grid's allocation.
    fn recomposite(&mut self) {
        if self.layers.max_layer() == 0 {
            return;
        }
        let size = self.layers.size();
        let dst = self
            .composited
            .get_or_insert_with(|| Grid::new(size.width(), size.height()));
        self.layers.flatten_into(dst);
    }

    /// Returns the cursor visibility.
    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Returns the cursor position.
    #[must_use]
    pub const fn cursor_position(&self) -> Pos {
        self.cursor_pos
    }

    /// Returns the cursor's shape and blink behavior, as last set by
    /// [`Cursor::set_cursor_style`](crate::backend::Cursor::set_cursor_style).
    #[must_use]
    pub const fn cursor_style(&self) -> CursorStyle {
        self.cursor_style
    }

    /// Injects a synthetic event into the queue.
    ///
    /// Coalesces consecutive `Mouse(Moved)` or same-button `Mouse(Drag)` events with the queue's
    /// current tail (see [`coalesces_with`]), matching the `retroglyph-window` and
    /// `retroglyph-terminal-wasm` backends this stands in for during tests (retroglyph#768): a
    /// caller pushing a burst of pointer positions before draining the queue sees only the latest
    /// one, the same as it would against a real backend.
    pub fn push_event(&mut self, event: Event) {
        if let Some(back) = self.event_queue.back_mut()
            && coalesces_with(&event, back)
        {
            *back = event;
            return;
        }
        self.event_queue.push_back(event);
    }

    /// Converts the current grid state into a readable string for snapshot testing.
    ///
    /// Space cells are rendered as `·` so layout is visible in text diffs.
    #[must_use]
    pub fn format_view(&self) -> String {
        let grid = self.grid();
        let mut out = String::new();
        for row in grid.size().to_rect().rows() {
            for pos in row {
                let cell = &grid[pos];
                let (glyph, is_spacer) = Self::display_glyph(cell);
                out.push(if is_spacer { ' ' } else { glyph });
            }
            out.push('\n');
        }
        out
    }

    /// `format_view`, with each cell's colors emitted as SGR (ANSI) escape sequences.
    ///
    /// Suitable for `insta::assert_snapshot!`, which renders ANSI in its terminal diff output:
    /// a color regression that `format_view` can't see (two styles that share a glyph) shows up
    /// as a snapshot diff here. Spacer cells (the trailing half of a wide glyph) are blanked the
    /// same way `format_view` blanks them, with no style of their own.
    ///
    /// Each run of cells that share a [`Style`](crate::color::Style) is wrapped in a `\x1b[0m` reset followed by the
    /// SGR codes for that style's non-default foreground/background; a bare `Style::default()`
    /// run only gets the reset. This keeps every row self-contained (no state leaks across rows
    /// or into terminals that render the snapshot directly).
    ///
    /// [`Color::Ansi`](crate::color::Color::Ansi) and [`Color::Indexed`](crate::color::Color::Indexed) map to their standard SGR codes (30-37/90-97 and
    /// `38;5;n`/`48;5;n`); [`Color::Rgb`](crate::color::Color::Rgb) maps to 24-bit SGR (`38;2;r;g;b`/`48;2;r;g;b`) rather
    /// than being downgraded, so this reflects the style as authored, not as a particular
    /// terminal would render it.
    #[must_use]
    pub fn format_styled(&self) -> String {
        let grid = self.grid();
        let mut out = String::new();
        for row in grid.size().to_rect().rows() {
            let mut current: Option<Style> = None;
            for pos in row {
                let cell = &grid[pos];
                let (glyph, is_spacer) = Self::display_glyph(cell);
                let style = if is_spacer {
                    Style::default()
                } else {
                    cell.style()
                };
                if current != Some(style) {
                    out.push_str("\x1b[0m");
                    crate::color::sgr::push_sgr(&mut out, style);
                    current = Some(style);
                }
                out.push(if is_spacer { ' ' } else { glyph });
            }
            if current.is_some_and(|s| s != Style::default()) {
                out.push_str("\x1b[0m");
            }
            out.push('\n');
        }
        out
    }

    /// The glyph `format_view`/`format_styled` render for `cell`, and whether it's a wide-glyph
    /// spacer (rendered blank in both, with no style in `format_styled`).
    const fn display_glyph(cell: &Tile) -> (char, bool) {
        let is_spacer = cell
            .flags()
            .contains(crate::tile::TileFlags::WIDE_CHAR_SPACER);
        let glyph = if cell.glyph() == ' ' {
            Self::SPACE_GLYPH
        } else {
            cell.glyph()
        };
        (glyph, is_spacer)
    }
}

impl Output for Headless {
    type Error = core::convert::Infallible;

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        for cell in content {
            let pos = cell.pos;
            // A `DrawCell` is already-resolved content from some source grid, replayed here
            // cell-by-cell at that same source position and on that same source layer, not a
            // caller placing a new tile at an arbitrary destination. `put_tile`'s sanitizing
            // (span role, `WIDE_CHAR`/spacer synthesis, overlap clearing) exists for the latter;
            // using it here would strip `SPAN_ANCHOR`/`SPAN_COVERED` from a faithfully-positioned
            // replay (retroglyph#984) the same way it should from a moved one, so this writes
            // straight through `tile_mut_or_alloc` instead. That never fails on an in-bounds `pos`
            // (sourced from this same grid's own geometry).
            //
            // Keeping each layer separate rather than folding everything onto layer 0 is what
            // makes a raw multi-layer stream replay correctly (retroglyph#1084): the stream is a
            // per-layer diff, so a cell occluded on a higher layer this frame and revealed the
            // next arrives only as the higher layer's erase, with nothing resent for the layer
            // below. Only retained per-layer state can composite that back.
            if let Some(t) = self.layers.tile_mut_or_alloc(cell.layer, pos) {
                *t = *cell.tile;
            }
            // Rebuild the side-table entry from the parts that arrived, so a headless capture
            // round-trips both members rather than only the grapheme.
            let extra = crate::grid::TileExtra {
                grapheme: cell.grapheme.map(alloc::sync::Arc::from),
                tint: cell.tint,
            };
            self.layers.set_extra(cell.layer, pos.x, pos.y, extra);
        }
        self.recomposite();
        Ok(())
    }

    fn resize(&mut self, size: Size) {
        self.layers.resize(size.width(), size.height());
        // Resized in lockstep rather than dropped: `flatten_into` requires matching dimensions,
        // and `Grid::resize` preserves the overlapping region, so the composited view survives a
        // resize the same way a real backend's surface would.
        if let Some(composited) = self.composited.as_mut() {
            composited.resize(size.width(), size.height());
        }
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(self.layers.width(), self.layers.height())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.layers.clear_all();
        if let Some(composited) = self.composited.as_mut() {
            composited.clear_all();
        }
        Ok(())
    }
}

impl Input for Headless {
    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        self.event_queue.pop_front()
    }

    fn push_event(&mut self, event: Event) {
        Self::push_event(self, event);
    }
}

impl Cursor for Headless {
    fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    fn set_cursor_position(&mut self, position: Pos) {
        self.cursor_pos = position;
    }

    fn set_cursor_style(&mut self, style: CursorStyle) {
        self.cursor_style = style;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn test_headless_new() {
        let backend = Headless::new(80, 25);
        assert_eq!(backend.grid().width(), 80);
        assert_eq!(backend.grid().height(), 25);
    }

    #[test]
    fn test_headless_cursor_style_defaults_and_records_set_cursor_style() {
        use crate::backend::CursorStyle;

        let mut backend = Headless::new(10, 10);
        assert_eq!(backend.cursor_style(), CursorStyle::BlinkingBlock);
        Cursor::set_cursor_style(&mut backend, CursorStyle::SteadyBar);
        assert_eq!(backend.cursor_style(), CursorStyle::SteadyBar);
    }

    #[test]
    fn test_headless_events() {
        let mut backend = Headless::new(10, 10);
        let event = Event::Close;
        backend.push_event(event);
        assert_eq!(backend.poll_event(Duration::ZERO), Some(Event::Close));
        assert_eq!(backend.poll_event(Duration::ZERO), None);
    }

    fn moved(x: u16) -> Event {
        use crate::event::{KeyModifiers, MouseEvent, MouseEventKind};
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos { x, y: 0 },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    /// Regression test for retroglyph#768: `Headless` must coalesce a burst of consecutive
    /// `Moved` events the same way `retroglyph-window` and `retroglyph-terminal-wasm` do, so
    /// `TestHarness`-driven tests stay faithful to the real backends.
    #[test]
    fn consecutive_moved_events_coalesce_to_one() {
        let mut backend = Headless::new(10, 10);
        for x in 0..1_000u16 {
            backend.push_event(moved(x));
        }
        assert_eq!(backend.event_queue.len(), 1);
        assert_eq!(backend.poll_event(Duration::ZERO), Some(moved(999)));
        assert_eq!(backend.poll_event(Duration::ZERO), None);
    }

    /// A non-`Moved` event between two `Moved` bursts must not be swallowed: only *consecutive*
    /// `Moved` events collapse.
    #[test]
    fn non_moved_event_breaks_coalescing() {
        use crate::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut backend = Headless::new(10, 10);
        backend.push_event(moved(1));
        backend.push_event(moved(2));
        backend.push_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos { x: 2, y: 0 },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        backend.push_event(moved(3));
        assert_eq!(backend.event_queue.len(), 3);
        assert_eq!(backend.poll_event(Duration::ZERO), Some(moved(2)));
        assert!(matches!(
            backend.poll_event(Duration::ZERO),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                ..
            }))
        ));
        assert_eq!(backend.poll_event(Duration::ZERO), Some(moved(3)));
    }

    /// A transparent (untouched, `EMPTY`) cell on a higher layer must not erase opaque content
    /// below it when the raw multi-layer stream is replayed (retroglyph#1084). `Grid::diff`
    /// yields every cell of a newly allocated layer, so the blank parts of layer 1 arrive right
    /// after layer 0's real content.
    #[test]
    fn draw_layers_treats_empty_higher_layer_cells_as_transparent() {
        let mut backend = Headless::new(3, 1);
        let a = Tile::new('a', Style::default());
        let hash = Tile::new('#', Style::default());
        let blank = Tile::default();
        backend
            .draw_layers(
                [
                    DrawCell::on_layer(0, Pos::new(0, 0), &a),
                    // Layer 1 arrives in full, blanks included.
                    DrawCell::on_layer(1, Pos::new(0, 0), &blank),
                    DrawCell::on_layer(1, Pos::new(1, 0), &blank),
                    DrawCell::on_layer(1, Pos::new(2, 0), &hash),
                ]
                .into_iter(),
            )
            .expect("draw_layers failed");
        assert_eq!(backend.format_view(), "a·#\n");
    }

    /// The stream is a per-layer *diff*, so revealing a cell sends only the erase on the layer
    /// that used to occlude it. Nothing is resent for the layer below, which is why `Headless`
    /// has to retain each layer rather than composite eagerly into one grid.
    #[test]
    fn draw_layers_reveals_the_layer_below_when_an_occluder_is_erased() {
        let mut backend = Headless::new(3, 1);
        let a = Tile::new('a', Style::default());
        let hash = Tile::new('#', Style::default());
        let blank = Tile::default();
        backend
            .draw_layers(
                [
                    DrawCell::on_layer(0, Pos::new(0, 0), &a),
                    DrawCell::on_layer(1, Pos::new(0, 0), &hash),
                ]
                .into_iter(),
            )
            .expect("draw_layers failed");
        assert_eq!(backend.format_view(), "#··\n", "layer 1 occludes layer 0");

        // Only layer 1's erase is sent: layer 0 is unchanged, so it contributes nothing.
        backend
            .draw_layers(core::iter::once(DrawCell::on_layer(
                1,
                Pos::new(0, 0),
                &blank,
            )))
            .expect("draw_layers failed");
        assert_eq!(
            backend.format_view(),
            "a··\n",
            "erasing the occluder must restore layer 0's tile, which was never resent"
        );
    }

    /// An explicit space is `EMPTY`-clear and so stays opaque, matching `flatten_into` and the
    /// transparency model documented on [`crate::grid`].
    #[test]
    fn draw_layers_keeps_an_explicit_space_on_a_higher_layer_opaque() {
        let mut backend = Headless::new(3, 1);
        let a = Tile::new('a', Style::default());
        let space = Tile::new(' ', Style::default());
        backend
            .draw_layers(
                [
                    DrawCell::on_layer(0, Pos::new(0, 0), &a),
                    DrawCell::on_layer(1, Pos::new(0, 0), &space),
                ]
                .into_iter(),
            )
            .expect("draw_layers failed");
        assert_eq!(backend.format_view(), "···\n");
    }

    /// `layer_grid` reports where a cell actually landed, which the composited `grid` cannot.
    #[test]
    fn layer_grid_exposes_the_raw_per_layer_stream() {
        let mut backend = Headless::new(3, 1);
        let a = Tile::new('a', Style::default());
        let hash = Tile::new('#', Style::default());
        backend
            .draw_layers(
                [
                    DrawCell::on_layer(0, Pos::new(0, 0), &a),
                    DrawCell::on_layer(1, Pos::new(0, 0), &hash),
                ]
                .into_iter(),
            )
            .expect("draw_layers failed");
        let raw = backend.layer_grid();
        assert_eq!(raw.tile(0, Pos::new(0, 0)).map(Tile::glyph), Some('a'));
        assert_eq!(raw.tile(1, Pos::new(0, 0)).map(Tile::glyph), Some('#'));
        assert_eq!(backend.grid()[Pos::new(0, 0)].glyph(), '#');
    }

    /// The single-layer path (every cell backend, which receives a pre-flattened stream) must
    /// not allocate the composited grid at all: `grid` is the received content directly.
    #[test]
    fn single_layer_stream_skips_the_composited_grid() {
        let mut backend = Headless::new(3, 1);
        let a = Tile::new('a', Style::default());
        backend
            .draw_layers(core::iter::once(DrawCell::on_layer(0, Pos::new(0, 0), &a)))
            .expect("draw_layers failed");
        assert!(backend.composited.is_none());
        assert_eq!(backend.format_view(), "a··\n");
    }

    #[test]
    fn test_format_view_snapshot() {
        use crate::terminal::Terminal;
        let backend = Headless::new(10, 3);
        let mut term = Terminal::new(backend);
        term.draw(|s| {
            s.put((1, 1), 'H', Style::default());
            s.put((2, 1), 'i', Style::default());
        })
        .expect("draw failed");
        let view = term.backend().format_view();
        insta::assert_snapshot!(view, @r#"
        ··········
        ·Hi·······
        ··········
        "#);
    }

    /// A multi-cell span's covered cells are its text fallback, so a cell backend must render
    /// all four glyphs. This is the deliberate difference from `WIDE_CHAR_SPACER`, which
    /// `format_view` blanks out just above this test's code path.
    #[test]
    fn test_format_view_renders_span_fallback_glyphs() {
        use crate::terminal::Terminal;
        let backend = Headless::new(6, 3);
        let mut term = Terminal::new(backend);
        term.draw(|s| {
            s.put_span((1, 0), &["C=", "[]"], Style::default())
                .expect("span write");
        })
        .expect("draw failed");
        let view = term.backend().format_view();
        insta::assert_snapshot!(view, @r#"
        ·C=···
        ·[]···
        ······
        "#);
    }

    #[test]
    fn test_format_styled_unstyled_matches_format_view_text() {
        use crate::terminal::Terminal;
        let backend = Headless::new(6, 2);
        let mut term = Terminal::new(backend);
        term.draw(|s| {
            s.put((1, 0), 'H', Style::default());
        })
        .expect("draw failed");
        // No non-default color anywhere, so this is just format_view with a reset per row.
        assert_eq!(
            term.backend().format_styled(),
            "\x1b[0m·H····\n\x1b[0m······\n"
        );
    }

    #[test]
    fn test_format_styled_emits_fg_and_bg_sgr_on_change() {
        use crate::terminal::Terminal;
        let backend = Headless::new(3, 1);
        let mut term = Terminal::new(backend);
        term.draw(|s| {
            let style = Style::new().fg(Color::RED).bg(Color::BLUE);
            s.put((1, 0), 'x', style);
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().format_styled(),
            "\x1b[0m·\x1b[0m\x1b[31;44mx\x1b[0m·\n"
        );
    }

    #[test]
    fn test_format_styled_rgb_and_indexed() {
        use crate::terminal::Terminal;
        let backend = Headless::new(2, 1);
        let mut term = Terminal::new(backend);
        term.draw(|s| {
            s.put(
                (0, 0),
                'a',
                Style::new().fg(Color::Rgb { r: 1, g: 2, b: 3 }),
            );
            s.put((1, 0), 'b', Style::new().bg(Color::Indexed(200)));
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().format_styled(),
            "\x1b[0m\x1b[38;2;1;2;3ma\x1b[0m\x1b[48;5;200mb\x1b[0m\n"
        );
    }

    #[test]
    fn test_format_styled_spacer_cells_carry_no_style() {
        use crate::terminal::Terminal;
        let backend = Headless::new(4, 1);
        let mut term = Terminal::new(backend);
        term.draw(|s| {
            s.put_span((0, 0), &["[]"], Style::new().fg(Color::GREEN))
                .expect("span write");
        })
        .expect("draw failed");
        // Both cells of the span share the styled glyph fallback (see
        // `test_format_view_renders_span_fallback_glyphs`); this asserts a real spacer, produced
        // by a wide EGC grapheme, drops style instead of inheriting the lead cell's.
        #[cfg(feature = "egc")]
        {
            let mut term = Terminal::new(Headless::new(4, 1));
            term.draw(|s| {
                s.put((0, 0), 'あ', Style::new().fg(Color::GREEN));
            })
            .expect("draw failed");
            let styled = term.backend().format_styled();
            assert!(styled.contains("\x1b[32mあ"));
            // The spacer cell after the wide glyph is blank and resets rather than repeating
            // the green foreground.
            assert!(!styled.contains("\x1b[32m "));
        }
    }
}
