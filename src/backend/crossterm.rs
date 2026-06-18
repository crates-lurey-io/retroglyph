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

impl From<crate::color::Color> for crossterm::style::Color {
    fn from(color: crate::color::Color) -> Self {
        use crate::color::AnsiColor;

        match color {
            crate::color::Color::Default => Self::Reset,
            crate::color::Color::Ansi(ansi) => match ansi {
                AnsiColor::Black => Self::Black,
                AnsiColor::Red => Self::DarkRed,
                AnsiColor::Green => Self::DarkGreen,
                AnsiColor::Yellow => Self::DarkYellow,
                AnsiColor::Blue => Self::DarkBlue,
                AnsiColor::Magenta => Self::DarkMagenta,
                AnsiColor::Cyan => Self::DarkCyan,
                AnsiColor::White => Self::Grey,
                AnsiColor::BrightBlack => Self::DarkGrey,
                AnsiColor::BrightRed => Self::Red,
                AnsiColor::BrightGreen => Self::Green,
                AnsiColor::BrightYellow => Self::Yellow,
                AnsiColor::BrightBlue => Self::Blue,
                AnsiColor::BrightMagenta => Self::Magenta,
                AnsiColor::BrightCyan => Self::Cyan,
                AnsiColor::BrightWhite => Self::White,
            },
            crate::color::Color::Indexed(index) => Self::AnsiValue(index),
            crate::color::Color::Rgb { r, g, b } => Self::Rgb { r, g, b },
        }
    }
}

impl From<crate::style::CellModifier> for crossterm::style::Attributes {
    fn from(modifier: crate::style::CellModifier) -> Self {
        use crate::style::CellModifier;
        use crossterm::style::Attribute;

        let mut attrs = Self::none();
        if modifier.contains(CellModifier::BOLD) {
            attrs = attrs.with(Attribute::Bold);
        }
        if modifier.contains(CellModifier::DIM) {
            attrs = attrs.with(Attribute::Dim);
        }
        if modifier.contains(CellModifier::ITALIC) {
            attrs = attrs.with(Attribute::Italic);
        }
        if modifier.contains(CellModifier::UNDERLINE) {
            attrs = attrs.with(Attribute::Underlined);
        }
        if modifier.contains(CellModifier::BLINK) {
            attrs = attrs.with(Attribute::SlowBlink);
        }
        if modifier.contains(CellModifier::REVERSE) {
            attrs = attrs.with(Attribute::Reverse);
        }
        if modifier.contains(CellModifier::HIDDEN) {
            attrs = attrs.with(Attribute::Hidden);
        }
        if modifier.contains(CellModifier::STRIKETHROUGH) {
            attrs = attrs.with(Attribute::CrossedOut);
        }
        attrs
    }
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
    #[allow(clippy::similar_names)]
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut last_fg = None;
        let mut last_bg = None;
        let mut last_attrs = None;

        for (x, y, cell) in content {
            let fg: crossterm::style::Color = cell.style.fg.into();
            let bg: crossterm::style::Color = cell.style.bg.into();
            let attrs: crossterm::style::Attributes = cell.style.modifiers.into();

            let _ = crossterm::queue!(self.writer, crossterm::cursor::MoveTo(x, y));

            if last_fg != Some(fg) {
                let _ = crossterm::queue!(self.writer, crossterm::style::SetForegroundColor(fg));
                last_fg = Some(fg);
            }

            if last_bg != Some(bg) {
                let _ = crossterm::queue!(self.writer, crossterm::style::SetBackgroundColor(bg));
                last_bg = Some(bg);
            }

            if last_attrs != Some(attrs) {
                let _ = crossterm::queue!(self.writer, crossterm::style::SetAttributes(attrs));
                last_attrs = Some(attrs);
            }

            let _ = crossterm::queue!(self.writer, crossterm::style::Print(cell.glyph));
        }
    }

    fn flush(&mut self) {
        let _ = crossterm::queue!(self.writer, crossterm::terminal::BeginSynchronizedUpdate);
        let _ = self.writer.flush();
        let _ = crossterm::queue!(self.writer, crossterm::terminal::EndSynchronizedUpdate);
        let _ = self.writer.flush();
    }

    fn size(&self) -> Size {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 25));
        Size { width, height }
    }

    fn clear(&mut self) {
        let _ = crossterm::queue!(
            self.writer,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        );
        let _ = self.writer.flush();
    }

    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        unimplemented!()
    }

    fn set_cursor_visible(&mut self, visible: bool) {
        if visible {
            let _ = crossterm::queue!(self.writer, crossterm::cursor::Show);
        } else {
            let _ = crossterm::queue!(self.writer, crossterm::cursor::Hide);
        }
        let _ = self.writer.flush();
    }

    fn set_cursor_position(&mut self, position: Position) {
        let _ = crossterm::queue!(
            self.writer,
            crossterm::cursor::MoveTo(position.x, position.y)
        );
        let _ = self.writer.flush();
    }
}
