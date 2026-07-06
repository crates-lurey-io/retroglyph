//! A [`Backend`] implementation that renders to a real terminal via
//! `crossterm`.
//!
//! This crate owns the OS/TTY-specific parts: raw mode, the alternate
//! screen, the kitty keyboard protocol, and `crossterm::event` polling.
//! Cell-diffing and ANSI/SGR output are delegated to
//! [`retroglyph_terminal::TerminalRenderer`].
//!
//! [`draw`](Backend::draw), [`flush`](Backend::flush), and
//! [`clear`](Backend::clear) propagate `std::io::Error` through this
//! backend's [`Backend::Error`] type. `resize`, `set_cursor_visible`, and
//! `set_cursor_position` are infallible on [`Backend`], so I/O failures in
//! those methods (e.g. a closed terminal or disconnected pipe) are discarded
//! silently rather than surfaced.

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
use retroglyph_terminal::TerminalRenderer;
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

/// A terminal rendering backend powered by `crossterm`.
pub struct Crossterm {
    renderer: TerminalRenderer<BufWriter<Stdout>>,
}

impl Crossterm {
    /// Creates a new `Crossterm` backend rendering to standard output.
    ///
    /// Enables raw mode, enters the alternate screen, hides the cursor, and
    /// enables mouse capture. Registers a process-wide panic hook (once, across
    /// all instances) that restores the terminal before the default panic
    /// handler runs, so a panic mid-render doesn't leave the user's shell in
    /// raw mode or the alternate screen.
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
            renderer: TerminalRenderer::new(BufWriter::new(stdout)),
        })
    }

    /// Creates a crossterm terminal and drives `app` with the blocking loop until
    /// it returns [`Flow::Exit`](retroglyph_core::Flow).
    ///
    /// This is a thin wrapper over the generic
    /// [`run_blocking`](retroglyph_core::run_blocking); the terminal is restored on the
    /// way out via `Drop`, so raw mode and the alternate screen are left intact
    /// until the loop actually returns.
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

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile)>,
    {
        // Begin synchronized update so the terminal holds rendering until
        // flush() sends the matching End marker.
        self.renderer.begin_synchronized_update()?;
        self.renderer.draw(content)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.renderer.end_synchronized_update()?;
        self.renderer.flush()?;
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
            self.renderer.writer_mut(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
        self.renderer.writer_mut().flush()?;
        // The terminal-side state (cursor position, last color/attrs) is now
        // stale versus what's actually on screen; forget it so the next
        // draw() re-emits full escape sequences instead of skipping them.
        self.renderer.reset_state();
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
        let writer = self.renderer.writer_mut();
        if visible {
            let _ = crossterm::queue!(writer, crossterm::cursor::Show);
        } else {
            let _ = crossterm::queue!(writer, crossterm::cursor::Hide);
        }
        let _ = writer.flush();
    }

    fn set_cursor_position(&mut self, position: Pos) {
        use std::io::Write;
        let writer = self.renderer.writer_mut();
        let _ = crossterm::queue!(writer, crossterm::cursor::MoveTo(position.x, position.y));
        let _ = writer.flush();
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
