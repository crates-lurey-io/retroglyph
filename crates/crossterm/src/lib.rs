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

// Compile the code blocks in both this crate's own README and the workspace root README as
// doctests so the quick-start examples are type-checked on every test run and cannot silently
// rot. The `cfg(doctest)` gate keeps these out of the rendered crate documentation.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct WorkspaceReadmeDoctests;

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
        crossterm::event::DisableBracketedPaste,
        crossterm::event::DisableFocusChange,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
}

/// Options controlling which optional terminal protocol features
/// [`Crossterm::with_options`] enables.
///
/// All features default to `true`; mouse capture and the kitty keyboard
/// protocol match the unconditional behavior of [`Crossterm::new`] prior to
/// this type's introduction. Use [`CrosstermOptions::mouse_capture`],
/// [`CrosstermOptions::kitty_protocol`], [`CrosstermOptions::focus_change`],
/// or [`CrosstermOptions::bracketed_paste`] to disable a feature entirely,
/// e.g. when running on a terminal (or through a pipe/CI harness/`tmux`/SSH
/// session) where the feature is unwanted.
///
/// This crate deliberately does not attempt to auto-detect terminal
/// capabilities (no `TERM` parsing, no `supports_keyboard_enhancement()`
/// query): those queries can block for seconds on terminals that never
/// respond. `CrosstermOptions` is the opt-out mechanism instead: callers who
/// know their environment don't support a feature can disable it explicitly.
///
/// ```
/// use retroglyph_crossterm::CrosstermOptions;
///
/// let options = CrosstermOptions::new().mouse_capture(false).kitty_protocol(false);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Four independent, unrelated terminal protocol toggles, not a state machine in disguise: each
// maps to one crossterm enable/disable command pair and is meaningful on its own.
#[allow(clippy::struct_excessive_bools)]
pub struct CrosstermOptions {
    mouse_capture: bool,
    kitty_protocol: bool,
    focus_change: bool,
    bracketed_paste: bool,
}

impl CrosstermOptions {
    /// Creates a new set of options with both features enabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to enable mouse capture (`crossterm::event::EnableMouseCapture`).
    #[must_use]
    pub const fn mouse_capture(mut self, enabled: bool) -> Self {
        self.mouse_capture = enabled;
        self
    }

    /// Sets whether to push the kitty keyboard protocol's enhancement flags
    /// (`crossterm::event::PushKeyboardEnhancementFlags`).
    #[must_use]
    pub const fn kitty_protocol(mut self, enabled: bool) -> Self {
        self.kitty_protocol = enabled;
        self
    }

    /// Sets whether to report focus gained/lost as
    /// [`Event::FocusGained`]/[`Event::FocusLost`]
    /// (`crossterm::event::EnableFocusChange`).
    #[must_use]
    pub const fn focus_change(mut self, enabled: bool) -> Self {
        self.focus_change = enabled;
        self
    }

    /// Sets whether to report bracketed paste as [`Event::Paste`]
    /// (`crossterm::event::EnableBracketedPaste`).
    #[must_use]
    pub const fn bracketed_paste(mut self, enabled: bool) -> Self {
        self.bracketed_paste = enabled;
        self
    }
}

impl Default for CrosstermOptions {
    /// All features enabled; mouse capture and the kitty keyboard protocol
    /// match [`Crossterm::new`]'s historical behavior.
    fn default() -> Self {
        Self {
            mouse_capture: true,
            kitty_protocol: true,
            focus_change: true,
            bracketed_paste: true,
        }
    }
}

/// A terminal rendering backend powered by `crossterm`.
pub struct Crossterm {
    renderer: TerminalRenderer<BufWriter<Stdout>>,
}

impl Crossterm {
    /// Creates a new `Crossterm` backend rendering to standard output.
    ///
    /// Enables raw mode, enters the alternate screen, hides the cursor, and
    /// enables mouse capture, focus-change reporting, bracketed paste, and
    /// the kitty keyboard protocol (all by default; see [`CrosstermOptions`]
    /// to disable any of them). Registers a process-wide panic hook (once,
    /// across all instances) that restores the terminal before the default
    /// panic handler runs, so a panic mid-render doesn't leave the user's
    /// shell in raw mode or the alternate screen.
    ///
    /// This is a thin wrapper over [`Crossterm::with_options`] with
    /// [`CrosstermOptions::default()`].
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail.
    pub fn new() -> Result<Self, std::io::Error> {
        Self::with_options(CrosstermOptions::default())
    }

    /// Creates a new `Crossterm` backend rendering to standard output, with
    /// explicit control over which optional protocol features are enabled.
    ///
    /// Enables raw mode, enters the alternate screen, and hides the cursor
    /// unconditionally. Mouse capture, focus-change reporting, bracketed
    /// paste, and the kitty keyboard protocol are enabled by default but can
    /// be disabled individually via `options`; see [`CrosstermOptions`].
    /// Registers a process-wide panic hook (once, across all instances) that
    /// restores the terminal before the default panic handler runs, so a
    /// panic mid-render doesn't leave the user's shell in raw mode or the
    /// alternate screen.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail.
    pub fn with_options(options: CrosstermOptions) -> Result<Self, std::io::Error> {
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
            crossterm::cursor::Hide
        )?;

        if options.mouse_capture {
            crossterm::execute!(stdout, crossterm::event::EnableMouseCapture)?;
        }

        if options.focus_change {
            crossterm::execute!(stdout, crossterm::event::EnableFocusChange)?;
        }

        if options.bracketed_paste {
            crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste)?;
        }

        if options.kitty_protocol {
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
        }

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
        CE::Key(k) => {
            // With `DISAMBIGUATE_ESCAPE_CODES` enabled (see `keyboard_enhancement_flags`),
            // terminals that support the kitty keyboard protocol report Shift+Tab as `Tab` plus
            // a shift modifier (CSI-u always encodes Tab's base codepoint, never a separate
            // "backtab" one) rather than the legacy `ESC[Z` -> `KeyCode::BackTab` escape. Without
            // this, Shift+Tab is silently indistinguishable from plain Tab on any terminal that
            // negotiated the enhanced protocol (kitty, WezTerm, foot, Ghostty, recent Alacritty).
            let is_shift_tab = matches!(k.code, crossterm::event::KeyCode::Tab)
                && k.modifiers.contains(crossterm::event::KeyModifiers::SHIFT);
            let code = if is_shift_tab {
                retroglyph_core::event::KeyCode::BackTab
            } else {
                from_crossterm_key_code(k.code)?
            };
            Ok(Event::Key(retroglyph_core::event::KeyEvent::with_kind(
                code,
                from_crossterm_key_modifiers(k.modifiers),
                from_crossterm_key_kind(k.kind),
            )))
        }
        CE::Mouse(m) => Ok(Event::Mouse(from_crossterm_mouse_event(m)?)),
        CE::Resize(w, h) => Ok(Event::Resize(w, h)),
        CE::Paste(text) => Ok(Event::Paste(text)),
        CE::FocusGained => Ok(Event::FocusGained),
        CE::FocusLost => Ok(Event::FocusLost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_code_of(ct_event: crossterm::event::Event) -> retroglyph_core::event::KeyCode {
        match from_crossterm_event(ct_event) {
            Ok(Event::Key(key)) => key.code,
            other => panic!("expected Ok(Event::Key(_)), got {other:?}"),
        }
    }

    #[test]
    fn crossterm_options_default_matches_historical_always_on_behavior() {
        // `Crossterm::new()` used to unconditionally enable mouse capture and push the kitty
        // keyboard protocol; `CrosstermOptions::default()` must preserve that behavior exactly so
        // `Crossterm::new()` (which delegates to `with_options(CrosstermOptions::default())`)
        // stays backward compatible. Focus-change and bracketed-paste reporting are new additions
        // and default to enabled as well, consistent with the other two features.
        let options = CrosstermOptions::default();
        assert!(options.mouse_capture);
        assert!(options.kitty_protocol);
        assert!(options.focus_change);
        assert!(options.bracketed_paste);
    }

    #[test]
    fn crossterm_options_can_opt_out_of_all_features() {
        // Compile-level/API-shape check: building a `CrosstermOptions` with all flags disabled
        // via the builder type-checks and round-trips its fields. Exercising the actual terminal
        // commands (`with_options` itself) requires a real TTY, which isn't available in CI, so
        // this is intentionally not a full integration test.
        let options = CrosstermOptions::new()
            .mouse_capture(false)
            .kitty_protocol(false)
            .focus_change(false)
            .bracketed_paste(false);
        assert!(!options.mouse_capture);
        assert!(!options.kitty_protocol);
        assert!(!options.focus_change);
        assert!(!options.bracketed_paste);
    }

    #[test]
    fn legacy_backtab_maps_straight_through() {
        // Terminals without the kitty protocol send the legacy `ESC[Z` escape, which crossterm
        // already decodes as `KeyCode::BackTab` with no shift modifier attached.
        let ct_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::BackTab,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(
            key_code_of(ct_event),
            retroglyph_core::event::KeyCode::BackTab
        );
    }

    #[test]
    fn kitty_protocol_shift_tab_normalizes_to_backtab() {
        // Under DISAMBIGUATE_ESCAPE_CODES, kitty-protocol terminals report Shift+Tab as plain
        // `Tab` plus a shift modifier rather than a distinct backtab code -- this is the case
        // `from_crossterm_event` has to normalize.
        let ct_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert_eq!(
            key_code_of(ct_event),
            retroglyph_core::event::KeyCode::BackTab
        );
    }

    #[test]
    fn plain_tab_is_unaffected() {
        let ct_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(key_code_of(ct_event), retroglyph_core::event::KeyCode::Tab);
    }

    #[test]
    fn shift_modifier_on_non_tab_keys_is_unaffected() {
        let ct_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert_eq!(
            key_code_of(ct_event),
            retroglyph_core::event::KeyCode::Char('a')
        );
    }

    #[test]
    fn crossterm_paste_maps_to_retroglyph_paste() {
        let ct_event = crossterm::event::Event::Paste("pasted text".to_string());
        match from_crossterm_event(ct_event) {
            Ok(Event::Paste(text)) => assert_eq!(text, "pasted text"),
            other => panic!("expected Ok(Event::Paste(_)), got {other:?}"),
        }
    }

    #[test]
    fn crossterm_focus_gained_maps_correctly() {
        let ct_event = crossterm::event::Event::FocusGained;
        assert!(matches!(
            from_crossterm_event(ct_event),
            Ok(Event::FocusGained)
        ));
    }

    #[test]
    fn crossterm_focus_lost_maps_correctly() {
        let ct_event = crossterm::event::Event::FocusLost;
        assert!(matches!(
            from_crossterm_event(ct_event),
            Ok(Event::FocusLost)
        ));
    }
}
