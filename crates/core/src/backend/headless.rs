//! In-memory backend for testing. Stores presented content and allows injecting synthetic events.
//!
//! [`Headless::format_view`] renders the current frame for snapshot testing (e.g. with `insta`)
//! and [`Headless::push_event`] queues synthetic input; see ["Driving `Headless` with synthetic
//! events"](https://github.com/crates-lurey-io/retroglyph/blob/main/docs/testing.md#driving-headless-with-synthetic-events)
//! for the full workflow.

use crate::backend::{Cursor, Input, Output};
use crate::color::Color;
use crate::event::{Event, coalesces_with};
use crate::grid::{Grid, Pos, Size};
use crate::style::Style;
use crate::tile::Tile;
use alloc::collections::VecDeque;
use alloc::string::String;
use core::fmt::Write as _;
use core::time::Duration;
use ixy::HasSize;

/// In-memory backend for testing.
///
/// Stores presented content and allows injecting synthetic events.
pub struct Headless {
    grid: Grid,
    cursor_visible: bool,
    cursor_pos: Pos,
    event_queue: VecDeque<Event>,
}

impl Headless {
    /// Creates a new headless backend of the given dimensions.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            grid: Grid::new(width, height),
            cursor_visible: false,
            cursor_pos: Pos::default(),
            event_queue: VecDeque::new(),
        }
    }

    /// Returns a reference to the grid.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.grid
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

    /// Injects a synthetic event into the queue.
    ///
    /// Coalesces consecutive `Mouse(Moved)` events with the queue's current tail (see
    /// [`coalesces_with`]), matching the `retroglyph-window` and `retroglyph-terminal-wasm`
    /// backends this stands in for during tests (retroglyph#768): a caller pushing a burst of
    /// pointer positions before draining the queue sees only the latest one, the same as it would
    /// against a real backend.
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
        let mut out = String::new();
        for y in 0..self.grid.height() {
            for x in 0..self.grid.width() {
                let cell = &self.grid[Pos::new(x, y)];
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
    /// Each run of cells that share a [`Style`] is wrapped in a `\x1b[0m` reset followed by the
    /// SGR codes for that style's non-default foreground/background; a bare `Style::default()`
    /// run only gets the reset. This keeps every row self-contained (no state leaks across rows
    /// or into terminals that render the snapshot directly).
    ///
    /// [`Color::Ansi`] and [`Color::Indexed`] map to their standard SGR codes (30-37/90-97 and
    /// `38;5;n`/`48;5;n`); [`Color::Rgb`] maps to 24-bit SGR (`38;2;r;g;b`/`48;2;r;g;b`) rather
    /// than being downgraded, so this reflects the style as authored, not as a particular
    /// terminal would render it.
    #[must_use]
    pub fn format_styled(&self) -> String {
        let mut out = String::new();
        for y in 0..self.grid.height() {
            let mut current: Option<Style> = None;
            for x in 0..self.grid.width() {
                let cell = &self.grid[Pos::new(x, y)];
                let (glyph, is_spacer) = Self::display_glyph(cell);
                let style = if is_spacer {
                    Style::default()
                } else {
                    cell.style()
                };
                if current != Some(style) {
                    out.push_str("\x1b[0m");
                    Self::push_sgr(&mut out, style);
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
            '·'
        } else {
            cell.glyph()
        };
        (glyph, is_spacer)
    }

    /// Appends the SGR codes for `style`'s non-default foreground/background to `out`, as a
    /// single `\x1b[...m` sequence.
    ///
    /// A `Color::Default` channel is left unset, relying on the caller's preceding `\x1b[0m`
    /// reset rather than emitting an explicit `39`/`49` reset code. Emits nothing at all when
    /// both channels are `Color::Default`.
    fn push_sgr(out: &mut String, style: Style) {
        let mut params = String::new();
        if let Some(code) = Self::sgr_color(style.foreground(), false) {
            let _ = write!(params, "{code}");
        }
        if let Some(code) = Self::sgr_color(style.background(), true) {
            if !params.is_empty() {
                params.push(';');
            }
            let _ = write!(params, "{code}");
        }
        if !params.is_empty() {
            let _ = write!(out, "\x1b[{params}m");
        }
    }

    /// The SGR parameter string for `color` in the foreground (`bg: false`) or background
    /// (`bg: true`) slot, or `None` for `Color::Default` (nothing to emit).
    fn sgr_color(color: Color, bg: bool) -> Option<String> {
        match color {
            Color::Default => None,
            Color::Ansi(ansi) => {
                let index = ansi.to_index();
                let base = match (index < 8, bg) {
                    (true, false) => 30,
                    (true, true) => 40,
                    (false, false) => 90,
                    (false, true) => 100,
                };
                Some(alloc::format!("{}", base + index % 8))
            }
            Color::Indexed(index) => Some(alloc::format!("{};5;{index}", if bg { 48 } else { 38 })),
            Color::Rgb { r, g, b } => {
                Some(alloc::format!("{};2;{r};{g};{b}", if bg { 48 } else { 38 }))
            }
        }
    }
}

impl Output for Headless {
    type Error = core::convert::Infallible;

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = crate::backend::DrawCell<'a>>,
    {
        for cell in content {
            let pos = cell.pos;
            self.grid.put_tile(0, pos, *cell.tile);
            // Rebuild the side-table entry from the parts that arrived, so a headless capture
            // round-trips both members rather than only the grapheme.
            let extra = crate::grid::TileExtra {
                grapheme: cell.grapheme.map(alloc::sync::Arc::from),
                tint: cell.tint,
            };
            self.grid.set_extra(0, pos.x, pos.y, extra);
        }
        Ok(())
    }

    fn resize(&mut self, size: Size) {
        self.grid.resize(size.width(), size.height());
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(self.grid.width(), self.grid.height())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.grid.clear_all();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headless_new() {
        let backend = Headless::new(80, 25);
        assert_eq!(backend.grid().width(), 80);
        assert_eq!(backend.grid().height(), 25);
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

    #[test]
    fn test_format_view_snapshot() {
        use crate::Terminal;
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
        use crate::Terminal;
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
        use crate::Terminal;
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
        use crate::Terminal;
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
        use crate::Terminal;
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
        use crate::Terminal;
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
