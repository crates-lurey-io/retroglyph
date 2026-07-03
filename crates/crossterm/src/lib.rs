//! Crossterm-based terminal rendering backend.
//!
//! I/O errors from crossterm writes are silently discarded. The [`Backend`]
//! trait methods return `()` (not `Result`), so there is no channel to
//! propagate write failures to the caller. This is acceptable for the common
//! case (stdout to a real terminal) but means the library won't detect a
//! disconnected pipe or closed terminal. Future versions of the trait may add
//! error-returning variants.

// Compile the code blocks in the project README as doctests so the quick-start
// example is type-checked on every test run and cannot silently rot. The
// `cfg(doctest)` gate keeps this out of the rendered crate documentation.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;

use core::time::Duration;
use retroglyph_core::backend::Backend;
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use std::io::{BufWriter, Stdout};

// Orphan-rule note: `retroglyph_core` types and `crossterm` types are both
// foreign to this crate now that the workspace is split, so `From`/`TryFrom`
// impls between them are no longer legal (neither type is local). These are
// plain conversion functions instead.

/// Keyboard enhancement flags requested when the terminal supports the kitty
/// keyboard protocol. `REPORT_EVENT_TYPES` is what upgrades us from press-only
/// to press/repeat/release; `DISAMBIGUATE_ESCAPE_CODES` makes modified keys
/// unambiguous.
fn keyboard_enhancement_flags() -> crossterm::event::KeyboardEnhancementFlags {
    crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
}

/// Helper function to restore the terminal to its normal state.
/// This is called during drops and emergency panic hooks.
fn restore_terminal() {
    let mut stdout = std::io::stdout();
    // Pop the keyboard enhancement flags pushed in `Crossterm::new`. Terminals
    // that never understood the push ignore the pop just the same.
    let _ = crossterm::execute!(stdout, crossterm::event::PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
}

const fn to_crossterm_color(color: retroglyph_core::color::Color) -> crossterm::style::Color {
    use retroglyph_core::color::AnsiColor;

    match color {
        retroglyph_core::color::Color::Default => crossterm::style::Color::Reset,
        retroglyph_core::color::Color::Ansi(ansi) => match ansi {
            AnsiColor::Black => crossterm::style::Color::Black,
            AnsiColor::Red => crossterm::style::Color::DarkRed,
            AnsiColor::Green => crossterm::style::Color::DarkGreen,
            AnsiColor::Yellow => crossterm::style::Color::DarkYellow,
            AnsiColor::Blue => crossterm::style::Color::DarkBlue,
            AnsiColor::Magenta => crossterm::style::Color::DarkMagenta,
            AnsiColor::Cyan => crossterm::style::Color::DarkCyan,
            AnsiColor::White => crossterm::style::Color::Grey,
            AnsiColor::BrightBlack => crossterm::style::Color::DarkGrey,
            AnsiColor::BrightRed => crossterm::style::Color::Red,
            AnsiColor::BrightGreen => crossterm::style::Color::Green,
            AnsiColor::BrightYellow => crossterm::style::Color::Yellow,
            AnsiColor::BrightBlue => crossterm::style::Color::Blue,
            AnsiColor::BrightMagenta => crossterm::style::Color::Magenta,
            AnsiColor::BrightCyan => crossterm::style::Color::Cyan,
            AnsiColor::BrightWhite => crossterm::style::Color::White,
        },
        retroglyph_core::color::Color::Indexed(index) => crossterm::style::Color::AnsiValue(index),
        retroglyph_core::color::Color::Rgb { r, g, b } => crossterm::style::Color::Rgb { r, g, b },
    }
}

const fn to_crossterm_attributes(
    modifier: retroglyph_core::style::CellModifier,
) -> crossterm::style::Attributes {
    use crossterm::style::Attribute;
    use retroglyph_core::style::CellModifier;

    let mut attrs = crossterm::style::Attributes::none();
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

        // Opt into the kitty keyboard protocol so we receive key repeat and
        // release events. We push optimistically rather than gating on
        // `supports_keyboard_enhancement()`: that query blocks for the
        // terminal's response (seconds on terminals that never answer, e.g.
        // pipes and CI), stalling startup. Terminals that don't implement the
        // protocol silently ignore the CSI sequence, and we map whatever key
        // events they do send. The matching pop happens on restore.
        crossterm::execute!(
            stdout,
            crossterm::event::PushKeyboardEnhancementFlags(keyboard_enhancement_flags())
        )?;

        Ok(Self {
            writer: BufWriter::new(stdout),
        })
    }

    /// Create a crossterm terminal and drive `app` with the blocking loop until
    /// it returns [`Flow::Exit`](retroglyph_core::Flow) (ADR 015 Decision 2).
    ///
    /// This is a thin wrapper over the generic
    /// [`run_blocking`](retroglyph_core::run_blocking); the terminal is restored on the
    /// way out via `Drop`.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if the terminal fails to initialize.
    pub fn run<A>(app: A) -> Result<(), std::io::Error>
    where
        A: retroglyph_core::App<Self>,
    {
        let term = retroglyph_core::Terminal::new(Self::new()?);
        retroglyph_core::run_blocking(term, app);
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
                .contains(retroglyph_core::tile::TileFlags::WIDE_CHAR_SPACER)
            {
                continue;
            }
            #[cfg(not(feature = "egc"))]
            if cell.glyph() == '\0' {
                continue;
            }

            let fg: crossterm::style::Color = to_crossterm_color(cell.style().foreground());
            let bg: crossterm::style::Color = to_crossterm_color(cell.style().background());
            let attrs: crossterm::style::Attributes =
                to_crossterm_attributes(cell.style().modifiers());

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
                let mut glyph_buf = [0u8; 4];
                let s: &str = match cell.extra() {
                    Some(extra) => extra,
                    None => cell.glyph().encode_utf8(&mut glyph_buf),
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
                        && let Ok(mapped) = from_crossterm_event(event)
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

const fn from_crossterm_key_code(
    code: crossterm::event::KeyCode,
) -> Result<retroglyph_core::event::KeyCode, ()> {
    use crossterm::event::KeyCode as CK;
    use retroglyph_core::event::KeyCode as K;
    match code {
        CK::Char(c) => Ok(K::Char(c)),
        CK::F(n) => Ok(K::F(n)),
        CK::Backspace => Ok(K::Backspace),
        CK::Enter => Ok(K::Enter),
        CK::Left => Ok(K::Left),
        CK::Right => Ok(K::Right),
        CK::Up => Ok(K::Up),
        CK::Down => Ok(K::Down),
        CK::Home => Ok(K::Home),
        CK::End => Ok(K::End),
        CK::PageUp => Ok(K::PageUp),
        CK::PageDown => Ok(K::PageDown),
        CK::Tab => Ok(K::Tab),
        CK::BackTab => Ok(K::BackTab),
        CK::Delete => Ok(K::Delete),
        CK::Insert => Ok(K::Insert),
        CK::Esc => Ok(K::Escape),
        _ => Err(()),
    }
}

const fn from_crossterm_key_kind(
    kind: crossterm::event::KeyEventKind,
) -> retroglyph_core::event::KeyEventKind {
    use crossterm::event::KeyEventKind as CK;
    use retroglyph_core::event::KeyEventKind as K;
    match kind {
        CK::Press => K::Press,
        CK::Repeat => K::Repeat,
        CK::Release => K::Release,
    }
}

fn from_crossterm_key_modifiers(
    mods: crossterm::event::KeyModifiers,
) -> retroglyph_core::event::KeyModifiers {
    use retroglyph_core::event::KeyModifiers as M;

    let mut result = M::NONE;
    if mods.contains(crossterm::event::KeyModifiers::SHIFT) {
        result |= M::SHIFT;
    }
    if mods.contains(crossterm::event::KeyModifiers::CONTROL) {
        result |= M::CONTROL;
    }
    if mods.contains(crossterm::event::KeyModifiers::ALT) {
        result |= M::ALT;
    }
    result
}

const fn from_crossterm_mouse_button(
    btn: crossterm::event::MouseButton,
) -> retroglyph_core::event::MouseButton {
    use crossterm::event::MouseButton as CB;
    use retroglyph_core::event::MouseButton as B;
    match btn {
        CB::Left => B::Left,
        CB::Right => B::Right,
        CB::Middle => B::Middle,
    }
}

const fn from_crossterm_mouse_event_kind(
    kind: crossterm::event::MouseEventKind,
) -> Result<retroglyph_core::event::MouseEventKind, ()> {
    use crossterm::event::MouseEventKind as CM;
    use retroglyph_core::event::MouseEventKind as K;
    match kind {
        CM::Down(btn) => Ok(K::Down(from_crossterm_mouse_button(btn))),
        CM::Up(btn) => Ok(K::Up(from_crossterm_mouse_button(btn))),
        CM::Moved | CM::Drag(_) => Ok(K::Moved),
        CM::ScrollUp => Ok(K::ScrollUp),
        CM::ScrollDown => Ok(K::ScrollDown),
        _ => Err(()),
    }
}

fn from_crossterm_mouse_event(
    m: crossterm::event::MouseEvent,
) -> Result<retroglyph_core::event::MouseEvent, ()> {
    Ok(retroglyph_core::event::MouseEvent {
        kind: from_crossterm_mouse_event_kind(m.kind)?,
        position: Pos {
            x: m.column,
            y: m.row,
        },
        // Crossterm is a character-mode backend; it has no sub-cell resolution.
        pixel_position: None,
        modifiers: from_crossterm_key_modifiers(m.modifiers),
    })
}

// Taking ownership matches the call site: `crossterm::event::read()` hands us
// a freshly-owned `Event` with nothing else holding a reference to it.
#[allow(clippy::needless_pass_by_value)]
fn from_crossterm_event(event: crossterm::event::Event) -> Result<Event, ()> {
    use crossterm::event::Event as CE;
    match event {
        CE::Key(k) => Ok(Event::Key(retroglyph_core::event::KeyEvent::with_kind(
            from_crossterm_key_code(k.code)?,
            from_crossterm_key_modifiers(k.modifiers),
            from_crossterm_key_kind(k.kind),
        ))),
        CE::Mouse(m) => Ok(Event::Mouse(from_crossterm_mouse_event(m)?)),
        CE::Resize(w, h) => Ok(Event::Resize(w, h)),
        _ => Err(()),
    }
}
