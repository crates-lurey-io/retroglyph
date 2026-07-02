//! Crossterm-based terminal rendering backend.
//!
//! I/O errors from crossterm writes are silently discarded. The [`Backend`]
//! trait methods return `()` (not `Result`), so there is no channel to
//! propagate write failures to the caller. This is acceptable for the common
//! case (stdout to a real terminal) but means the library won't detect a
//! disconnected pipe or closed terminal. Future versions of the trait may add
//! error-returning variants.

use crate::backend::Backend;
use crate::event::Event;
use crate::grid::{Pos, Size};
use crate::tile::Tile;
use core::time::Duration;
use std::io::{BufWriter, Stdout};

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

    /// Create a crossterm terminal and drive `app` with the blocking loop until
    /// it returns [`Flow::Exit`](crate::Flow) (ADR 015 Decision 2).
    ///
    /// This is a thin wrapper over the generic
    /// [`run_blocking`](crate::run_blocking); the terminal is restored on the
    /// way out via `Drop`.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if the terminal fails to initialize.
    pub fn run<A>(app: A) -> Result<(), std::io::Error>
    where
        A: crate::App<Self>,
    {
        let term = crate::Terminal::new(Self::new()?);
        crate::run_blocking(term, app);
        Ok(())
    }
}

impl Drop for Crossterm {
    fn drop(&mut self) {
        restore_terminal();
    }
}

impl Backend for Crossterm {
    type Error = std::io::Error;

    #[allow(clippy::similar_names)]
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        // Begin synchronized update so the terminal holds rendering until
        // flush() sends the matching End marker.
        crossterm::queue!(self.writer, crossterm::terminal::BeginSynchronizedUpdate)?;

        let mut last_fg = None;
        let mut last_bg = None;
        let mut last_attrs = None;
        // Track the cursor position so we can skip redundant MoveTo
        // commands when cells are adjacent.
        let mut cursor_x: Option<u16> = None;
        let mut cursor_y: Option<u16> = None;

        for (pos, cell) in content {
            // Spacer cells are the right half of a wide character.
            // The wide char itself already drew over this position, so skip it.
            #[cfg(feature = "egc")]
            if cell
                .flags()
                .contains(crate::tile::TileFlags::WIDE_CHAR_SPACER)
            {
                continue;
            }
            #[cfg(not(feature = "egc"))]
            if cell.glyph() == '\0' {
                continue;
            }

            let fg: crossterm::style::Color = cell.style.fg.into();
            let bg: crossterm::style::Color = cell.style.bg.into();
            let attrs: crossterm::style::Attributes = cell.style.modifiers.into();

            // Only emit MoveTo when the cursor isn't already at the right position.
            let needs_move = cursor_y != Some(pos.y) || cursor_x != Some(pos.x);
            if needs_move {
                crossterm::queue!(self.writer, crossterm::cursor::MoveTo(pos.x, pos.y))?;
            }

            if last_fg != Some(fg) {
                crossterm::queue!(self.writer, crossterm::style::SetForegroundColor(fg))?;
                last_fg = Some(fg);
            }

            if last_bg != Some(bg) {
                crossterm::queue!(self.writer, crossterm::style::SetBackgroundColor(bg))?;
                last_bg = Some(bg);
            }

            if last_attrs != Some(attrs) {
                crossterm::queue!(self.writer, crossterm::style::SetAttributes(attrs))?;
                last_attrs = Some(attrs);
            }

            #[allow(unused_assignments)]
            let mut cell_width: u16 = 1;
            #[cfg(feature = "egc")]
            {
                // Print the full EGC if present; otherwise the primary glyph.
                let mut buf = [0u8; 4];
                let s: &str = match &cell.extra {
                    Some(extra) => extra.as_str(),
                    None => cell.glyph.encode_utf8(&mut buf),
                };
                #[allow(clippy::cast_possible_truncation)]
                {
                    cell_width = unicode_width::UnicodeWidthStr::width(s).max(1) as u16;
                }
                crossterm::queue!(self.writer, crossterm::style::Print(s))?;
            }
            #[cfg(not(feature = "egc"))]
            {
                #[allow(clippy::cast_possible_truncation)]
                {
                    cell_width =
                        unicode_width::UnicodeWidthChar::width(cell.glyph()).unwrap_or(1) as u16;
                }
                crossterm::queue!(self.writer, crossterm::style::Print(cell.glyph()))?;
            }

            // After printing, the terminal cursor advances by the cell's
            // display width. Track that so the next cell can skip MoveTo.
            cursor_x = Some(pos.x + cell_width);
            cursor_y = Some(pos.y);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        use std::io::Write;
        crossterm::queue!(self.writer, crossterm::terminal::EndSynchronizedUpdate)?;
        self.writer.flush()?;
        Ok(())
    }

    fn size(&self) -> Size {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 25));
        Size { width, height }
    }

    fn resize(&mut self, _size: Size) {
        let _ = self.clear();
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        use std::io::Write;
        crossterm::queue!(
            self.writer,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
        self.writer.flush()?;
        Ok(())
    }

    fn push_event(&mut self, _event: Event) {
        // Crossterm reads events from its own event stream, not from push.
    }

    fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
        let start = std::time::Instant::now();
        let mut remaining = timeout;

        loop {
            // Cap the polling timeout to 1 hour to prevent system-call overflow of massive durations (like Duration::MAX).
            let poll_timeout = if remaining > Duration::from_secs(3600) {
                Duration::from_secs(3600)
            } else {
                remaining
            };

            match crossterm::event::poll(poll_timeout) {
                Ok(true) => {
                    if let Ok(event) = crossterm::event::read()
                        && let Ok(mapped) = Event::try_from(event)
                    {
                        return Some(mapped);
                    }
                    // An unmappable event was consumed. In non-blocking mode
                    // (timeout zero, used by drain_events), retry immediately
                    // so we don't stop draining with events still buffered.
                    if timeout.is_zero() {
                        continue;
                    }
                }
                Ok(false) => {
                    // Timeout elapsed on this poll chunk.
                }
                Err(_) => {
                    return None;
                }
            }

            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return None;
            }
            remaining = timeout.checked_sub(elapsed).unwrap_or(Duration::ZERO);
        }
    }

    fn set_cursor_visible(&mut self, visible: bool) {
        use std::io::Write;
        if visible {
            let _ = crossterm::queue!(self.writer, crossterm::cursor::Show);
        } else {
            let _ = crossterm::queue!(self.writer, crossterm::cursor::Hide);
        }
        let _ = self.writer.flush();
    }

    fn set_cursor_position(&mut self, position: Pos) {
        use std::io::Write;
        let _ = crossterm::queue!(
            self.writer,
            crossterm::cursor::MoveTo(position.x, position.y)
        );
        let _ = self.writer.flush();
    }
}

impl TryFrom<crossterm::event::KeyCode> for crate::event::KeyCode {
    type Error = ();

    fn try_from(code: crossterm::event::KeyCode) -> Result<Self, Self::Error> {
        use crossterm::event::KeyCode as CK;
        match code {
            CK::Char(c) => Ok(Self::Char(c)),
            CK::F(n) => Ok(Self::F(n)),
            CK::Backspace => Ok(Self::Backspace),
            CK::Enter => Ok(Self::Enter),
            CK::Left => Ok(Self::Left),
            CK::Right => Ok(Self::Right),
            CK::Up => Ok(Self::Up),
            CK::Down => Ok(Self::Down),
            CK::Home => Ok(Self::Home),
            CK::End => Ok(Self::End),
            CK::PageUp => Ok(Self::PageUp),
            CK::PageDown => Ok(Self::PageDown),
            CK::Tab => Ok(Self::Tab),
            CK::BackTab => Ok(Self::BackTab),
            CK::Delete => Ok(Self::Delete),
            CK::Insert => Ok(Self::Insert),
            CK::Esc => Ok(Self::Escape),
            _ => Err(()),
        }
    }
}

impl From<crossterm::event::KeyModifiers> for crate::event::KeyModifiers {
    fn from(mods: crossterm::event::KeyModifiers) -> Self {
        let mut result = Self::NONE;
        if mods.contains(crossterm::event::KeyModifiers::SHIFT) {
            result |= Self::SHIFT;
        }
        if mods.contains(crossterm::event::KeyModifiers::CONTROL) {
            result |= Self::CONTROL;
        }
        if mods.contains(crossterm::event::KeyModifiers::ALT) {
            result |= Self::ALT;
        }
        result
    }
}

impl From<crossterm::event::MouseButton> for crate::event::MouseButton {
    fn from(btn: crossterm::event::MouseButton) -> Self {
        use crossterm::event::MouseButton as CB;
        match btn {
            CB::Left => Self::Left,
            CB::Right => Self::Right,
            CB::Middle => Self::Middle,
        }
    }
}

impl TryFrom<crossterm::event::MouseEventKind> for crate::event::MouseEventKind {
    type Error = ();

    fn try_from(kind: crossterm::event::MouseEventKind) -> Result<Self, Self::Error> {
        use crossterm::event::MouseEventKind as CM;
        match kind {
            CM::Down(btn) => Ok(Self::Down(btn.into())),
            CM::Up(btn) => Ok(Self::Up(btn.into())),
            CM::Moved | CM::Drag(_) => Ok(Self::Moved),
            CM::ScrollUp => Ok(Self::ScrollUp),
            CM::ScrollDown => Ok(Self::ScrollDown),
            _ => Err(()),
        }
    }
}

impl TryFrom<crossterm::event::MouseEvent> for crate::event::MouseEvent {
    type Error = ();

    fn try_from(m: crossterm::event::MouseEvent) -> Result<Self, Self::Error> {
        Ok(Self {
            kind: m.kind.try_into()?,
            position: Pos {
                x: m.column,
                y: m.row,
            },
            // Crossterm is a character-mode backend; it has no sub-cell resolution.
            pixel_position: None,
            modifiers: m.modifiers.into(),
        })
    }
}

impl TryFrom<crossterm::event::Event> for Event {
    type Error = ();

    fn try_from(event: crossterm::event::Event) -> Result<Self, Self::Error> {
        use crossterm::event::Event as CE;
        match event {
            CE::Key(k) => {
                if k.kind == crossterm::event::KeyEventKind::Release {
                    return Err(());
                }

                Ok(Self::Key(crate::event::KeyEvent {
                    code: k.code.try_into()?,
                    modifiers: k.modifiers.into(),
                }))
            }
            CE::Mouse(m) => Ok(Self::Mouse(m.try_into()?)),
            CE::Resize(w, h) => Ok(Self::Resize(w, h)),
            _ => Err(()),
        }
    }
}
