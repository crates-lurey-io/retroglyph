//! Crossterm-based terminal rendering backend.

use crate::backend::Backend;
use crate::cell::Cell;
use crate::event::Event;
use crate::grid::{Position, Size};
use core::time::Duration;
use std::io::{BufWriter, Stdout};

/// A terminal rendering backend powered by `crossterm`.
pub struct CrosstermBackend {
    _writer: BufWriter<Stdout>,
}

impl CrosstermBackend {
    /// Creates a new `CrosstermBackend` rendering to standard output.
    ///
    /// # Errors
    ///
    /// This is a stub for M11.
    pub fn new() -> Result<Self, std::io::Error> {
        Ok(Self {
            _writer: BufWriter::new(std::io::stdout()),
        })
    }
}

impl Backend for CrosstermBackend {
    fn draw<'a, I>(&mut self, _content: I)
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        unimplemented!()
    }

    fn flush(&mut self) {
        unimplemented!()
    }

    fn size(&self) -> Size {
        unimplemented!()
    }

    fn clear(&mut self) {
        unimplemented!()
    }

    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        unimplemented!()
    }

    fn set_cursor_visible(&mut self, _visible: bool) {
        unimplemented!()
    }

    fn set_cursor_position(&mut self, _position: Position) {
        unimplemented!()
    }
}
