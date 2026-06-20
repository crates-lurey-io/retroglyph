//! In-memory backend for testing. Stores presented content
//! and allows injecting synthetic events.

use crate::backend::Backend;
use crate::event::Event;
use crate::grid::{Grid, Pos, Size};
use crate::tile::Tile;
use alloc::collections::VecDeque;
use alloc::string::String;
use core::time::Duration;

/// In-memory backend for testing. Stores presented content
/// and allows injecting synthetic events.
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
    pub fn push_event(&mut self, event: Event) {
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
                let cell = self.grid.get(x, y);
                #[cfg(feature = "egc")]
                let is_spacer = cell
                    .flags()
                    .contains(crate::tile::TileFlags::WIDE_CHAR_SPACER);
                #[cfg(not(feature = "egc"))]
                let is_spacer = cell.glyph() == '\0';
                let c = if is_spacer {
                    ' '
                } else if cell.glyph() == ' ' {
                    '·'
                } else {
                    cell.glyph()
                };
                out.push(c);
            }
            out.push('\n');
        }
        out
    }
}

impl Backend for Headless {
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        for (pos, cell) in content {
            self.grid.checked_put(pos.x, pos.y, cell.clone());
        }
    }

    fn resize(&mut self, size: Size) {
        self.grid.resize(size.width, size.height);
    }

    fn flush(&mut self) {
        // Headless backend is already in memory.
    }

    fn size(&self) -> Size {
        Size {
            width: self.grid.width(),
            height: self.grid.height(),
        }
    }

    fn clear(&mut self) {
        self.grid.clear_all();
    }

    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        self.event_queue.pop_front()
    }

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

    #[test]
    fn test_format_view_snapshot() {
        use crate::Terminal;
        let backend = Headless::new(10, 3);
        let mut term = Terminal::new(backend);
        term.put(1, 1, 'H');
        term.put(2, 1, 'i');
        term.present();
        let view = term.backend().format_view();
        insta::assert_snapshot!(view, @r#"
        ··········
        ·Hi·······
        ··········
        "#);
    }
}
