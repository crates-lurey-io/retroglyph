//! Crossterm-based terminal rendering backend.

use crate::backend::Backend;
use crate::cell::Cell;
use crate::event::Event;
use crate::grid::{Position, Size};
use core::time::Duration;
use std::io::{BufWriter, Stdout, Write};

/// Helper function to restore the terminal to its normal state.
/// This is called during drops and emergency panic hooks.
fn restore_terminal() {
    let mut stdout = std::io::stdout();
    let _ = crossterm::execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
}

/// A terminal rendering backend powered by `crossterm`.
pub struct Crossterm {
    writer: BufWriter<Stdout>,
}

impl Crossterm {
    /// Creates a new `Crossterm` rendering to standard output.
    ///
    /// This sets up raw mode, mouse capture, alternative screen, hides the cursor,
    /// and registers a process-wide panic hook to safely restore the terminal on crashes.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail.
    pub fn new() -> Result<Self, std::io::Error> {
        // Setup panic hook on first backend creation
        static PANIC_HOOK: std::sync::Once = std::sync::Once::new();
        PANIC_HOOK.call_once(|| {
            let original_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                restore_terminal();
                original_hook(panic_info);
            }));
        });

        // Enter raw mode
        crossterm::terminal::enable_raw_mode()?;

        let mut stdout = std::io::stdout();
        // Execute initial setup commands
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::cursor::Hide,
            crossterm::event::EnableMouseCapture
        )?;

        Ok(Self {
            writer: BufWriter::new(stdout),
        })
    }
}

impl Drop for Crossterm {
    fn drop(&mut self) {
        restore_terminal();
    }
}

impl Backend for Crossterm {
    fn draw<'a, I>(&mut self, _content: I)
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        unimplemented!()
    }

    fn flush(&mut self) {
        let _ = self.writer.flush();
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
