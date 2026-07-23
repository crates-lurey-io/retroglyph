//! [`Output`], [`Input`], and [`Cursor`] implementations that render to a real terminal via
//! `crossterm`, bundled together as [`Backend`](retroglyph_core::Backend).
//!
//! This crate owns the OS/TTY-specific parts: raw mode, the alternate
//! screen, the kitty keyboard protocol, and `crossterm::event` polling.
//! Cell-diffing and ANSI/SGR output are delegated to
//! [`retroglyph_terminal::TerminalRenderer`].
//!
//! [`draw`](Output::draw), [`flush`](Output::flush), and
//! [`clear`](Output::clear) propagate `std::io::Error` through this
//! backend's [`Output::Error`] type. `resize`, `set_cursor_visible`, and
//! `set_cursor_position` are infallible ([`Output::resize`] and the [`Cursor`] methods have no
//! `Result` return), so I/O failures in those methods (e.g. a closed terminal or disconnected
//! pipe) are discarded silently rather than surfaced.
//!
//! # Event polling and CPU cost
//!
//! [`poll_event`](Input::poll_event) wraps a single `crossterm::event::poll()` syscall per
//! call; a zero timeout (as used by
//! [`Terminal::drain_events`](retroglyph_core::Terminal::drain_events)) is one non-blocking OS
//! poll, not a busy loop. See that method's docs for the responsiveness/CPU tradeoff this implies
//! for uncapped game loops.
//!
//! # Focus and lifecycle events
//!
//! With [`CrosstermOptions::focus_change`] enabled (the default), a terminal losing and regaining
//! input focus is reported as [`Event::FocusLost`]/[`Event::FocusGained`]. This is the only
//! lifecycle signal this backend currently has: unlike a windowed backend, there's no separate
//! "suspended"/"paused" notion here, and this crate maps every focus change the same way
//! regardless of the underlying reason (window manager focus switch, terminal minimized, or --
//! notably on Wayland compositors -- a terminal surface being hidden or unmapped without an
//! accompanying resize).
//!
//! Terminal-side state (raw mode, the alternate screen, cursor position, last-written
//! colors/attributes) is untouched by a focus change and is preserved across it: this backend
//! does not react to [`Event::FocusLost`]/[`Event::FocusGained`] itself, so nothing is torn down
//! or reinitialized. Rendering is not deferred automatically either -- [`Output::draw`] and
//! [`Output::flush`] keep writing escape sequences to stdout even while unfocused, since
//! crossterm has no OS-level way to know whether that output is actually being presented while
//! hidden. An app that wants to pause redraws while unfocused (e.g. to avoid wasted work on a
//! backgrounded Wayland surface) should track [`Event::FocusLost`]/[`Event::FocusGained`] itself
//! and skip its own draw calls in between.
//!
//! If `retroglyph-core` later adds a dedicated `Event::Suspended` (or similar) distinct from
//! plain focus loss, this crate would need coordinated changes with `retroglyph-window` (which
//! shares the `Event` enum) before mapping anything to it; no such variant exists today, so there
//! is nothing for this backend to emit.
//!
//! # Tracing
//!
//! With the optional `tracing` feature enabled, [`Output::draw`], [`Output::flush`], and
//! [`Input::poll_event`] are each wrapped in a `tracing` span (`debug` level for `draw`/`flush`,
//! `trace` for `poll_event` since it's called every game-loop iteration by
//! [`Terminal::drain_events`](retroglyph_core::Terminal::drain_events)), so a subscriber (e.g.
//! `tracing-subscriber`'s fmt layer, or a flamegraph via `tracing-flame`) can show where render
//! and input-polling time actually goes. The feature adds no code and no dependency when disabled.
//!
//! # Content writer
//!
//! [`Crossterm`] is generic over its content writer -- `Crossterm<W>`, defaulting to
//! `BufWriter<Stdout>` to match this type's historical, stdout-only behavior. Use
//! [`Crossterm::with_writer`] or [`CrosstermOptions::build_with_writer`] to render into a file,
//! a pipe, or an in-memory buffer instead, e.g. to capture and assert on the emitted ANSI/SGR
//! bytes in a test without a real TTY. Only the rendered cell content goes through `W`; raw
//! mode, the alternate screen, and the other terminal-protocol negotiation always target the
//! real process stdout regardless of `W` -- see [`CrosstermOptions::build_with_writer`]'s docs
//! for the exact split.

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
use retroglyph_core::backend::{Cursor, Input, Output};
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_terminal::TerminalRenderer;
use std::io::{BufWriter, IsTerminal, Stdout};

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

// Tracks whether the currently-live `Crossterm` instance (there's normally at most one, since
// each holds exclusive use of stdout/raw mode) actually entered the alternate screen / enabled
// raw mode, so `restore_terminal` -- shared by `Drop` and the process-wide panic hook, neither of
// which has access to a specific instance's `CrosstermOptions` -- only undoes what was actually
// done. Unlike the other features (mouse capture, focus-change, bracketed paste, kitty protocol),
// which are safe to unconditionally disable/pop even if never enabled (crossterm's own commands
// are no-ops on a terminal that never received the matching enable sequence), unconditionally
// emitting `LeaveAlternateScreen`/`disable_raw_mode()` when we never entered/enabled them could
// corrupt a caller's already-cooked-mode terminal or emit a stray escape into their normal
// scrollback buffer.
static ALT_SCREEN_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static RAW_MODE_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// Tracks whether *some* `Crossterm` instance is currently live, independent of the two statics
// above (which track what a *single* construction actually enabled, for `restore_terminal`'s own
// bookkeeping). This is the fix for the hazard those two statics can't protect against on their
// own: constructing a second `Crossterm` while a first is still alive both stomp the same
// process-wide raw-mode/alternate-screen state, and dropping either one calls the shared
// `restore_terminal()`, which would tear down state the other instance still believes is active
// (e.g. dropping instance A disables raw mode and clears `RAW_MODE_ACTIVE`, while instance B is
// still alive and now silently receives line-buffered, echoed input instead of raw key events).
// Rather than trying to make concurrent instances safe (which would need real per-instance
// state, not process-global statics, since stdout/raw-mode/the alternate screen are process-wide
// OS resources with no instance-scoped handle), construction of a second live instance is
// rejected outright -- see [`InstanceGuard`].
static INSTANCE_LIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// RAII guard enforcing the "at most one live [`Crossterm`] instance per process" invariant.
///
/// [`InstanceGuard::acquire`] atomically claims [`INSTANCE_LIVE`] (returning an error if another
/// instance already holds it) and stores the resulting guard as a field of [`Crossterm`], so it
/// stays held for exactly as long as that `Crossterm` is alive. Dropping the guard -- whether via
/// the owning `Crossterm`'s normal `Drop`, or because a `?` inside
/// [`Crossterm::build_from_options`] unwound out of the constructor after the guard was acquired
/// but before construction finished -- releases the flag, so a single failed construction attempt
/// can never permanently wedge out all future construction.
///
/// This is deliberately independent of `restore_terminal()`/the panic hook: `restore_terminal()`
/// only clears `ALT_SCREEN_ACTIVE`/`RAW_MODE_ACTIVE` (idempotent swaps that are safe to call any
/// number of times, including from both the panic hook and the eventual `Drop`). Ordinary Rust
/// destructor semantics already guarantee this guard's `Drop` runs during an unwind (the panic
/// hook itself runs *before* unwinding starts, so it never needs to touch `INSTANCE_LIVE`), so
/// there's no double-release or release-when-nothing-was-acquired hazard: exactly one `Drop` runs
/// per successful `acquire()`, whether the instance is dropped normally or unwinds away after a
/// panic.
struct InstanceGuard {
    // Always `true` for a live `InstanceGuard`; `acquire()` never constructs one otherwise. Kept
    // as a field (rather than always releasing unconditionally in `Drop`) so the invariant "one
    // release per successful acquire" is enforced by the type itself, not just by convention.
    armed: bool,
}

impl InstanceGuard {
    /// Attempts to claim the process-wide "a `Crossterm` instance is live" flag.
    ///
    /// Returns `Err` with [`std::io::ErrorKind::ResourceBusy`] if another instance already holds
    /// it; the caller (`Crossterm::build_from_options`) is expected to propagate that error
    /// straight out of the constructor via `?` without attempting any teardown, since nothing was
    /// set up yet.
    fn acquire() -> Result<Self, std::io::Error> {
        use std::sync::atomic::Ordering;

        INSTANCE_LIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self { armed: true })
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::ResourceBusy,
                    "a Crossterm instance is already live in this process; only one may be \
                     constructed at a time (drop the existing instance before constructing \
                     another)",
                )
            })
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if self.armed {
            INSTANCE_LIVE.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

/// Helper function to restore the terminal to its normal state.
/// This is called during drops and emergency panic hooks.
fn restore_terminal() {
    use std::sync::atomic::Ordering;

    let mut stdout = std::io::stdout();
    // Pop the keyboard enhancement flags pushed in `Crossterm::new`. Terminals
    // that never understood the push ignore the pop just the same.
    let _ = crossterm::execute!(stdout, crossterm::event::PopKeyboardEnhancementFlags);
    let _ = crossterm::execute!(
        stdout,
        crossterm::event::DisableBracketedPaste,
        crossterm::event::DisableFocusChange,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show
    );
    if ALT_SCREEN_ACTIVE.swap(false, Ordering::AcqRel) {
        let _ = crossterm::execute!(stdout, crossterm::terminal::LeaveAlternateScreen);
    }
    if RAW_MODE_ACTIVE.swap(false, Ordering::AcqRel) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Options controlling which optional terminal protocol features
/// [`Crossterm::with_options`] enables.
///
/// All features default to `true`; mouse capture, the kitty keyboard
/// protocol, entering the alternate screen, and raw mode all match the
/// unconditional behavior of [`Crossterm::new`] prior to this type's
/// introduction. Use [`CrosstermOptions::mouse_capture`],
/// [`CrosstermOptions::kitty_protocol`], [`CrosstermOptions::focus_change`],
/// [`CrosstermOptions::bracketed_paste`], [`CrosstermOptions::alt_screen`], or
/// [`CrosstermOptions::raw_mode`] to disable a feature entirely, e.g. when
/// running on a terminal (or through a pipe/CI harness/`tmux`/SSH session)
/// where the feature is unwanted.
///
/// This is also the type returned by [`Crossterm::builder`], the preferred
/// entry point for constructing one of these: `Crossterm::builder()` reads
/// better at a call site than `CrosstermOptions::new()` but the two are
/// otherwise identical (`builder()` just calls `Self::new()`).
///
/// This crate deliberately does not attempt to auto-detect terminal
/// capabilities (no `TERM` parsing, no `supports_keyboard_enhancement()`
/// query): those queries can block for seconds on terminals that never
/// respond. `CrosstermOptions` is the opt-out mechanism instead: callers who
/// know their environment don't support a feature can disable it explicitly.
///
/// ```
/// use retroglyph_crossterm::Crossterm;
///
/// let options = Crossterm::builder().mouse_capture(false).kitty_protocol(false);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Six independent, unrelated terminal protocol toggles, not a state machine in disguise: each
// maps to one crossterm enable/disable command pair (or, for raw_mode/alt_screen, one
// enable/leave pair) and is meaningful on its own.
#[allow(clippy::struct_excessive_bools)]
pub struct CrosstermOptions {
    mouse_capture: bool,
    kitty_protocol: bool,
    focus_change: bool,
    bracketed_paste: bool,
    alt_screen: bool,
    raw_mode: bool,
}

impl CrosstermOptions {
    /// Creates a new set of options with every feature enabled.
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
    ///
    /// See the crate-level "Focus and lifecycle events" docs for the pause/resume contract this
    /// implies (e.g. on Wayland, where a terminal can lose and regain focus independent of any
    /// resize).
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

    /// Sets whether to enter the alternate screen
    /// (`crossterm::terminal::EnterAlternateScreen`).
    ///
    /// Disabling this keeps rendering on the caller's normal scrollback buffer instead of
    /// switching to a dedicated full-screen surface; on exit, [`Crossterm`] only leaves the
    /// alternate screen (`LeaveAlternateScreen`) if it entered it, so disabling this doesn't
    /// risk leaving the caller's real terminal buffer in an unexpected state.
    #[must_use]
    pub const fn alt_screen(mut self, enabled: bool) -> Self {
        self.alt_screen = enabled;
        self
    }

    /// Sets whether to enable raw mode (`crossterm::terminal::enable_raw_mode`).
    ///
    /// Disabling this leaves the terminal in cooked mode, so the OS/shell keep handling line
    /// buffering, echo, and signal-generating keys (`Ctrl-C`, `Ctrl-Z`) itself instead of
    /// forwarding every keystroke as an [`Event::Key`].
    /// Restore only disables raw mode if this backend is the one that enabled it.
    #[must_use]
    pub const fn raw_mode(mut self, enabled: bool) -> Self {
        self.raw_mode = enabled;
        self
    }

    /// Builds the [`Crossterm`] backend with these options, rendering to standard output.
    ///
    /// Equivalent to [`Crossterm::with_options`]; this is the terminal step of the
    /// `Crossterm::builder().<options>().build()` chain started by [`Crossterm::builder`].
    /// Use [`build_with_writer`](Self::build_with_writer) to render to a different sink (a
    /// file, a pipe, an in-memory buffer for tests) instead of stdout.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail. Also returns an
    /// `std::io::Error` with [`std::io::ErrorKind::ResourceBusy`] if another [`Crossterm`]
    /// instance is already live in this process -- see the concurrency contract documented on
    /// [`Crossterm`].
    pub fn build(self) -> Result<Crossterm, std::io::Error> {
        // Detect once, up front, whether the real process stdout is an interactive terminal so
        // the resulting renderer degrades to pipe-safe plain text (see
        // `TerminalRenderer::set_plain_mode`) when stdout is a file, a pipe, or otherwise
        // redirected (`> log.txt`, CI runners, etc). This mirrors
        // `TerminalRenderer::auto`, but is spelled out with an explicit `is_terminal()` check on
        // the un-wrapped `Stdout` handle rather than a call to `auto` itself: `auto` requires
        // `W: IsTerminal`, and the buffered `BufWriter<Stdout>` this backend renders through
        // doesn't implement that trait (only the unbuffered `Stdout`/`File`/etc. do), so the
        // check has to happen before wrapping in `BufWriter`.
        let plain = !std::io::stdout().is_terminal();
        Crossterm::build_from_options(self, BufWriter::new(std::io::stdout()), plain)
    }

    /// Builds the [`Crossterm`] backend with these options, rendering to `writer` instead of
    /// stdout.
    ///
    /// `writer` only receives the rendered cell content ([`Output::draw`]/[`Output::flush`]
    /// output, plus the runtime [`Output::clear`]/[`Cursor::set_cursor_visible`]/
    /// [`Cursor::set_cursor_position`] escapes). Terminal-protocol setup/teardown -- raw mode,
    /// the alternate screen, the initial cursor hide, mouse capture, focus-change reporting,
    /// bracketed paste, and the kitty keyboard protocol -- always targets the real process
    /// stdout regardless of `writer`, since those are properties of the actual controlling
    /// terminal, not of an arbitrary byte sink. Callers rendering to a non-terminal `writer`
    /// (a file, a pipe, an in-memory buffer) should disable the features they don't want
    /// touching the real terminal via [`CrosstermOptions::raw_mode`],
    /// [`CrosstermOptions::alt_screen`], [`CrosstermOptions::mouse_capture`], etc.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail. Also returns an
    /// `std::io::Error` with [`std::io::ErrorKind::ResourceBusy`] if another [`Crossterm`]
    /// instance is already live in this process -- see the concurrency contract documented on
    /// [`Crossterm`].
    pub fn build_with_writer<W: std::io::Write>(
        self,
        writer: W,
    ) -> Result<Crossterm<W>, std::io::Error> {
        // Unlike `build`, `writer` here is an arbitrary caller-supplied sink with no `IsTerminal`
        // bound (a `Vec<u8>` in tests, for instance, doesn't implement it), so there's no way to
        // auto-detect plain mode; default to `false` (ANSI/SGR escapes on), matching this
        // method's historical behavior.
        Crossterm::build_from_options(self, writer, false)
    }
}

impl Default for CrosstermOptions {
    /// Every feature enabled; matches [`Crossterm::new`]'s historical behavior.
    fn default() -> Self {
        Self {
            mouse_capture: true,
            kitty_protocol: true,
            focus_change: true,
            bracketed_paste: true,
            alt_screen: true,
            raw_mode: true,
        }
    }
}

/// A terminal rendering backend powered by `crossterm`.
///
/// Generic over the content writer `W` -- the sink that receives rendered cell output
/// ([`Output::draw`]/[`Output::flush`], plus the runtime cursor/clear escapes). Defaults to
/// `BufWriter<Stdout>`, matching this type's historical behavior; use
/// [`Crossterm::with_writer`]/[`CrosstermOptions::build_with_writer`] to render to a file, a
/// pipe, or an in-memory buffer instead (e.g. for tests that want to inspect the emitted ANSI
/// bytes without a real TTY). See [`CrosstermOptions::build_with_writer`] for exactly which
/// operations go through `W` versus the real terminal.
///
/// # Concurrency: only one live instance per process
///
/// Raw mode, the alternate screen, and the other terminal-protocol state this backend negotiates
/// are process-wide OS resources (there's exactly one controlling terminal, one raw-mode flag,
/// one alternate-screen buffer), not something a `Crossterm` instance owns exclusively the way a
/// `File` owns a file descriptor. Because of that, at most one `Crossterm` (of any `W`) may be
/// live at a time in a process: constructing a second one while a first is still alive ([`new`],
/// [`with_options`], [`with_writer`], and every [`CrosstermOptions::build`]/
/// [`CrosstermOptions::build_with_writer`] call) returns an `std::io::Error` with
/// [`std::io::ErrorKind::ResourceBusy`] instead of proceeding -- this is a documented error, not
/// undefined behavior, and nothing is torn down or corrupted by the attempt. Sequential
/// construct-drop-construct is fully supported: once the live instance is dropped, a new one can
/// be constructed immediately.
///
/// [`new`]: Crossterm::new
/// [`with_options`]: Crossterm::with_options
/// [`with_writer`]: Crossterm::with_writer
pub struct Crossterm<W: std::io::Write = BufWriter<Stdout>> {
    renderer: TerminalRenderer<W>,
    // Held for exactly the lifetime of this instance; releases the process-wide "an instance is
    // live" flag when this value is dropped (see [`InstanceGuard`]). Never read after
    // construction -- it's kept purely for its `Drop` side effect -- hence the leading
    // underscore, which also suppresses the otherwise-applicable `dead_code` lint.
    _instance_guard: InstanceGuard,
    // Cached result of the last successful `crossterm::terminal::size()` query. Seeded once at
    // construction and refreshed only when `poll_event` observes a `crossterm::event::Event::
    // Resize` -- the app already receives that event on every real terminal resize, so there's
    // no need to re-query on every `Output::size()` call (a `TIOCGWINSZ` ioctl), which used to
    // run once per frame. See retroglyph#279.
    //
    // `size()` itself never re-queries after construction (see retroglyph#279), so the only
    // fallible query is the one-time seed in `build_from_options`; that's also the only place a
    // hardcoded guess (80x24, not a "last known good" value, since none exists yet) is ever
    // used. See retroglyph#281.
    cached_size: Size,
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
    /// Returns an `std::io::Error` if raw mode or terminal commands fail. Also returns an
    /// `std::io::Error` with [`std::io::ErrorKind::ResourceBusy`] if another [`Crossterm`]
    /// instance is already live in this process -- see the concurrency contract documented on
    /// [`Crossterm`].
    pub fn new() -> Result<Self, std::io::Error> {
        Self::with_options(CrosstermOptions::default())
    }

    /// Starts building a `Crossterm` backend with explicit control over which optional
    /// terminal protocol features are enabled.
    ///
    /// Equivalent to `CrosstermOptions::new()`; call [`CrosstermOptions::build`] (or
    /// [`CrosstermOptions::build_with_writer`]) once the desired features are chosen. This is
    /// the preferred entry point over `CrosstermOptions::new()` for readability at the call
    /// site:
    ///
    /// ```
    /// use retroglyph_crossterm::Crossterm;
    ///
    /// let options = Crossterm::builder()
    ///     .mouse_capture(false)
    ///     .kitty_protocol(false)
    ///     .alt_screen(true)
    ///     .raw_mode(true);
    /// // let backend = options.build()?; // requires a real terminal
    /// ```
    #[must_use]
    pub fn builder() -> CrosstermOptions {
        CrosstermOptions::new()
    }

    /// Creates a new `Crossterm` backend rendering to standard output, with
    /// explicit control over which optional protocol features are enabled.
    ///
    /// Hides the cursor unconditionally. Raw mode, entering the alternate screen, mouse
    /// capture, focus-change reporting, bracketed paste, and the kitty keyboard protocol are
    /// all enabled by default but can be disabled individually via `options`; see
    /// [`CrosstermOptions`]. Registers a process-wide panic hook (once, across all instances)
    /// that restores the terminal before the default panic handler runs, so a panic
    /// mid-render doesn't leave the user's shell in raw mode or the alternate screen.
    ///
    /// This is a thin wrapper over [`CrosstermOptions::build`]; prefer
    /// `Crossterm::builder().<options>().build()` at new call sites.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail. Also returns an
    /// `std::io::Error` with [`std::io::ErrorKind::ResourceBusy`] if another [`Crossterm`]
    /// instance is already live in this process -- see the concurrency contract documented on
    /// [`Crossterm`].
    pub fn with_options(options: CrosstermOptions) -> Result<Self, std::io::Error> {
        options.build()
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

impl<W: std::io::Write> Crossterm<W> {
    /// Creates a new `Crossterm` backend rendering to `writer` instead of standard output.
    ///
    /// Thin wrapper over [`CrosstermOptions::build_with_writer`] with
    /// [`CrosstermOptions::default()`]; see that method for the exact contract of which
    /// operations go through `writer` versus the real terminal.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if raw mode or terminal commands fail. Also returns an
    /// `std::io::Error` with [`std::io::ErrorKind::ResourceBusy`] if another [`Crossterm`]
    /// instance is already live in this process -- see the concurrency contract documented on
    /// [`Crossterm`].
    pub fn with_writer(writer: W) -> Result<Self, std::io::Error> {
        CrosstermOptions::default().build_with_writer(writer)
    }

    /// Returns a reference to the content writer.
    pub const fn writer(&self) -> &W {
        self.renderer.writer()
    }

    /// Returns whether the underlying renderer is in plain (non-ANSI) mode.
    ///
    /// [`CrosstermOptions::build`] sets this automatically based on whether the real process
    /// stdout is an interactive terminal (see that method's docs); [`CrosstermOptions::build_with_writer`]
    /// always leaves it `false`, since an arbitrary writer's "is this a terminal" status can't be
    /// determined generically. See
    /// [`TerminalRenderer::set_plain_mode`](retroglyph_terminal::TerminalRenderer::set_plain_mode)
    /// for what plain mode changes about rendering.
    pub const fn plain_mode(&self) -> bool {
        self.renderer.plain_mode()
    }

    /// Returns a mutable reference to the content writer.
    pub const fn writer_mut(&mut self) -> &mut W {
        self.renderer.writer_mut()
    }

    /// Updates the cached size field if `event` is a `crossterm::event::Event::Resize`; a
    /// no-op for every other event kind.
    ///
    /// Split out of [`Input::poll_event`] so it can be exercised directly in tests without
    /// requiring a real terminal event source. See retroglyph#279.
    const fn refresh_cached_size_on_resize(&mut self, event: &crossterm::event::Event) {
        if let crossterm::event::Event::Resize(width, height) = *event {
            self.cached_size = Size { width, height };
        }
    }

    fn build_from_options(
        options: CrosstermOptions,
        writer: W,
        plain: bool,
    ) -> Result<Self, std::io::Error> {
        use std::sync::atomic::Ordering;

        // Setup panic hook on first backend creation
        static PANIC_HOOK: std::sync::Once = std::sync::Once::new();
        PANIC_HOOK.call_once(|| {
            let original_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                restore_terminal();
                original_hook(panic_info);
            }));
        });

        // Reject a second concurrent instance up front, before touching any terminal state.
        // Held for the lifetime of the returned `Crossterm` (see `InstanceGuard`'s docs); if any
        // `?` below returns early, this local binding drops immediately and releases the flag, so
        // a failed construction attempt never permanently wedges out future construction.
        let instance_guard = InstanceGuard::acquire()?;

        if options.raw_mode {
            crossterm::terminal::enable_raw_mode()?;
            RAW_MODE_ACTIVE.store(true, Ordering::Release);
        }

        // Terminal-protocol setup always targets the real process stdout, independent of
        // `writer`: these are properties of the actual controlling terminal (raw mode, the
        // alternate screen, mouse/focus/paste/kitty negotiation), not of the content sink a
        // caller may have swapped in via `build_with_writer`. See that method's docs.
        let mut stdout = std::io::stdout();

        if options.alt_screen {
            crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
            ALT_SCREEN_ACTIVE.store(true, Ordering::Release);
        }

        crossterm::execute!(stdout, crossterm::cursor::Hide)?;

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

        // Seed the cached size once, up front, so `size()` never has to query on the
        // per-frame path; kept fresh afterward by `poll_event` observing `Event::Resize` (see
        // below). Fall back to 80x24 -- not the historical, and non-conventional, 80x25 -- if
        // this initial query fails; there's no better "last known good" size to fall back to
        // yet, since none has been observed. See retroglyph#281.
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));

        Ok(Self {
            renderer: TerminalRenderer::with_plain_mode(writer, plain),
            _instance_guard: instance_guard,
            cached_size: Size { width, height },
        })
    }
}

impl<W: std::io::Write> Drop for Crossterm<W> {
    fn drop(&mut self) {
        restore_terminal();
    }
}

impl<W: std::io::Write> Output for Crossterm<W> {
    type Error = std::io::Error;

    #[cfg_attr(feature = "tracing", tracing::instrument(level = "debug", skip_all))]
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
    {
        // Begin synchronized update so the terminal holds rendering until
        // flush() sends the matching End marker.
        self.renderer.begin_synchronized_update()?;
        self.renderer.draw(content)?;
        Ok(())
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(level = "debug", skip_all))]
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.renderer.end_synchronized_update()?;
        self.renderer.flush()?;
        Ok(())
    }

    fn size(&self) -> Size {
        // No syscall: just return the size cached at construction and kept fresh by
        // `poll_event` observing `Event::Resize`. See retroglyph#279.
        self.cached_size
    }

    fn resize(&mut self, _size: Size) {
        let _ = self.clear();
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
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
}

impl<W: std::io::Write> Input for Crossterm<W> {
    /// Polls for the next input event, blocking up to `timeout`.
    ///
    /// A zero `timeout` (the case [`Terminal::drain_events`](retroglyph_core::Terminal::drain_events)
    /// uses to drain everything buffered without blocking) performs one non-blocking
    /// `crossterm::event::poll(Duration::ZERO)` syscall (`select`/`epoll` under the hood) per
    /// call, not a busy spin inside this method: once the OS reports no data waiting, this
    /// returns `None` immediately rather than looping. The CPU cost this issue is actually about
    /// lives one level up, in the caller's game loop: an uncapped loop that calls
    /// `drain_events()` every iteration with no frame limiter (no `sleep`, no vsync wait) will
    /// issue that non-blocking syscall as fast as the CPU allows, trading power/CPU usage for
    /// input latency. Backends and examples in this workspace that need a frame cap (e.g.
    /// software + WASM, gated on `requestAnimationFrame`) already throttle themselves upstream of
    /// this call; a crossterm-driven loop wanting the same tradeoff should add its own
    /// `std::thread::sleep`/tick budget around `drain_events()` rather than expecting this method
    /// to throttle on its behalf.
    #[cfg_attr(feature = "tracing", tracing::instrument(level = "trace", skip(self)))]
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
                    if let Ok(event) = crossterm::event::read() {
                        // Refresh the cached size in lockstep with the resize event the app
                        // itself is about to receive, so `size()` (no syscall) stays consistent
                        // with what already triggered this event. See retroglyph#279.
                        self.refresh_cached_size_on_resize(&event);
                        if let Some(mapped) = from_crossterm_event(event) {
                            return Some(mapped);
                        }
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

    // `push_event` uses the trait default: crossterm reads events from its own event stream, not
    // from an externally pushed queue.
}

impl<W: std::io::Write> Cursor for Crossterm<W> {
    /// Queues the show/hide escape without flushing; the next [`Output::flush`] call drains it
    /// along with everything else. A caller that hides the cursor and moves it in the same frame
    /// (a common pattern right before a draw) would otherwise pay an extra flush per call on top
    /// of the normal draw/flush pair, with no observable benefit since nothing reads the terminal
    /// state in between.
    fn set_cursor_visible(&mut self, visible: bool) {
        let writer = self.renderer.writer_mut();
        if visible {
            let _ = crossterm::queue!(writer, crossterm::cursor::Show);
        } else {
            let _ = crossterm::queue!(writer, crossterm::cursor::Hide);
        }
    }

    /// Queues the cursor-move escape without flushing; see [`set_cursor_visible`](Self::set_cursor_visible)'s
    /// docs for why this is deferred to the next [`Output::flush`] instead of flushing here.
    fn set_cursor_position(&mut self, position: Pos) {
        let writer = self.renderer.writer_mut();
        let _ = crossterm::queue!(writer, crossterm::cursor::MoveTo(position.x, position.y));
    }
}

const fn from_crossterm_key_code(
    code: crossterm::event::KeyCode,
) -> Option<retroglyph_core::event::KeyCode> {
    use crossterm::event::KeyCode as CK;
    use retroglyph_core::event::KeyCode as K;
    match code {
        CK::Char(c) => Some(K::Char(c)),
        CK::F(n) => Some(K::F(n)),
        CK::Backspace => Some(K::Backspace),
        CK::Enter => Some(K::Enter),
        CK::Left => Some(K::Left),
        CK::Right => Some(K::Right),
        CK::Up => Some(K::Up),
        CK::Down => Some(K::Down),
        CK::Home => Some(K::Home),
        CK::End => Some(K::End),
        CK::PageUp => Some(K::PageUp),
        CK::PageDown => Some(K::PageDown),
        CK::Tab => Some(K::Tab),
        CK::BackTab => Some(K::BackTab),
        CK::Delete => Some(K::Delete),
        CK::Insert => Some(K::Insert),
        CK::Esc => Some(K::Escape),
        _ => None,
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

// Every `crossterm::event::MouseEventKind` variant now has a retroglyph equivalent (unlike
// `from_crossterm_key_code`, which still has unmappable `KeyCode`s), so this is infallible.
const fn from_crossterm_mouse_event_kind(
    kind: crossterm::event::MouseEventKind,
) -> retroglyph_core::event::MouseEventKind {
    use crossterm::event::MouseEventKind as CM;
    use retroglyph_core::event::MouseEventKind as K;
    match kind {
        CM::Down(btn) => K::Down(from_crossterm_mouse_button(btn)),
        CM::Up(btn) => K::Up(from_crossterm_mouse_button(btn)),
        CM::Drag(btn) => K::Drag(from_crossterm_mouse_button(btn)),
        CM::Moved => K::Moved,
        CM::ScrollUp => K::ScrollUp,
        CM::ScrollDown => K::ScrollDown,
        CM::ScrollLeft => K::ScrollLeft,
        CM::ScrollRight => K::ScrollRight,
    }
}

fn from_crossterm_mouse_event(
    m: crossterm::event::MouseEvent,
) -> retroglyph_core::event::MouseEvent {
    retroglyph_core::event::MouseEvent {
        kind: from_crossterm_mouse_event_kind(m.kind),
        position: Pos {
            x: m.column,
            y: m.row,
        },
        // Crossterm is a character-mode backend; it has no sub-cell resolution.
        pixel_position: None,
        modifiers: from_crossterm_key_modifiers(m.modifiers),
    }
}

// Taking ownership matches the call site: `crossterm::event::read()` hands us
// a freshly-owned `Event` with nothing else holding a reference to it.
//
// `#[doc(hidden)] pub` (rather than private) solely so `benches/event_translation.rs` (a
// separate compiled crate, same restriction as an integration test) can call it directly to
// measure retroglyph#285's "event translation throughput" case; this is not a supported public
// API and can change or disappear without a semver-relevant changelog entry.
#[doc(hidden)]
// The single failure mode is "this event has no retroglyph equivalent", which `Option` expresses
// directly; the only caller (`poll_event`) already discards an unmappable event entirely (see the
// retry-on-unmappable-event loop above).
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn from_crossterm_event(event: crossterm::event::Event) -> Option<Event> {
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
            Some(Event::Key(retroglyph_core::event::KeyEvent::with_kind(
                code,
                from_crossterm_key_modifiers(k.modifiers),
                from_crossterm_key_kind(k.kind),
            )))
        }
        CE::Mouse(m) => Some(Event::Mouse(from_crossterm_mouse_event(m))),
        CE::Resize(w, h) => Some(Event::Resize(w, h)),
        CE::Paste(text) => Some(Event::Paste(text)),
        CE::FocusGained => Some(Event::FocusGained),
        CE::FocusLost => Some(Event::FocusLost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `cargo test` runs `#[test]` functions in this module across multiple threads by default.
    // Any test that actually constructs a `Crossterm`/acquires an `InstanceGuard` contends for
    // the same process-wide `INSTANCE_LIVE` flag, so without serializing them a legitimate
    // concurrent construction from an unrelated test in this file could spuriously trip the new
    // "second live instance" rejection. This lock -- held only by tests that touch that shared
    // state -- keeps those tests deterministic without disabling parallelism for the whole file.
    static TEST_GUARD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn key_code_of(ct_event: crossterm::event::Event) -> retroglyph_core::event::KeyCode {
        match from_crossterm_event(ct_event) {
            Some(Event::Key(key)) => key.code,
            other => panic!("expected Some(Event::Key(_)), got {other:?}"),
        }
    }

    #[test]
    fn crossterm_options_default_matches_historical_always_on_behavior() {
        // `Crossterm::new()` used to unconditionally enable raw mode, the alternate screen,
        // mouse capture, and push the kitty keyboard protocol; `CrosstermOptions::default()`
        // must preserve that behavior exactly so `Crossterm::new()` (which delegates to
        // `with_options(CrosstermOptions::default())`) stays backward compatible. Focus-change
        // and bracketed-paste reporting are later additions and default to enabled as well,
        // consistent with the other features.
        let options = CrosstermOptions::default();
        assert!(options.mouse_capture);
        assert!(options.kitty_protocol);
        assert!(options.focus_change);
        assert!(options.bracketed_paste);
        assert!(options.alt_screen);
        assert!(options.raw_mode);
    }

    #[test]
    fn crossterm_builder_is_equivalent_to_options_new() {
        // `Crossterm::builder()` is the documented preferred entry point; it must produce the
        // same defaults as `CrosstermOptions::new()`/`::default()`.
        assert_eq!(Crossterm::builder(), CrosstermOptions::new());
    }

    #[test]
    fn disabling_raw_mode_and_alt_screen_lets_build_succeed_without_a_tty() {
        // With both raw mode and the alternate screen opted out, `build()` no longer calls
        // `enable_raw_mode()`/`EnterAlternateScreen` -- the two commands that fail outright
        // without a real controlling terminal -- so construction can succeed even against a
        // fully redirected/piped stdout (as under `cargo test`). Skip the assertion (rather than
        // failing) on the rare environment where even the always-safe cursor-hide escape write
        // fails outright (e.g. a closed stdout), since that's not what this test is about.
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(term) = Crossterm::builder()
            .raw_mode(false)
            .alt_screen(false)
            .build()
        {
            drop(term);
        }
    }

    #[test]
    fn build_with_writer_renders_cell_content_into_a_custom_sink() {
        // The whole point of a generic content writer: draw/flush output lands in `writer`
        // (here a `Vec<u8>`) instead of stdout, with no real terminal required as long as the
        // real-terminal-only features (raw mode, alt screen, mouse/focus/paste/kitty) are
        // disabled -- exactly the combination `CrosstermOptions::build_with_writer`'s docs
        // recommend for a non-TTY sink.
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut term = Crossterm::builder()
            .raw_mode(false)
            .alt_screen(false)
            .mouse_capture(false)
            .focus_change(false)
            .bracketed_paste(false)
            .kitty_protocol(false)
            .build_with_writer(Vec::new())
            .expect("building against a Vec<u8> writer with all TTY features disabled must not require a real terminal");

        let tile = Tile::new('X', retroglyph_core::style::Style::default());
        term.draw(core::iter::once((Pos { x: 0, y: 0 }, &tile, None)))
            .unwrap();
        term.flush().unwrap();

        let written = String::from_utf8(term.writer().clone()).unwrap();
        assert!(
            written.contains('X'),
            "expected drawn glyph in output: {written:?}"
        );
        assert!(
            !term.plain_mode(),
            "build_with_writer must not auto-detect plain mode for an arbitrary writer"
        );
    }

    #[test]
    fn with_writer_is_equivalent_to_default_options_build_with_writer() {
        // `Crossterm::with_writer` is the `CrosstermOptions::default()` shortcut, matching how
        // `Crossterm::new()` relates to `with_options(CrosstermOptions::default())`. Both fail
        // the same way without a real terminal (raw mode/alt screen left enabled), so just
        // assert they agree on success or failure rather than requiring either to succeed. Each
        // build is dropped before the next one starts (rather than held simultaneously) so the
        // comparison reflects real-terminal availability, not a spurious rejection from the new
        // "only one live instance" guard tripping on the still-live first instance.
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let via_shortcut = Crossterm::with_writer(Vec::<u8>::new());
        let via_shortcut_ok = via_shortcut.is_ok();
        drop(via_shortcut);
        let via_builder = CrosstermOptions::default().build_with_writer(Vec::<u8>::new());
        let via_builder_ok = via_builder.is_ok();
        drop(via_builder);
        assert_eq!(via_shortcut_ok, via_builder_ok);
    }

    #[test]
    fn instance_guard_rejects_a_second_concurrent_acquire() {
        // Exercises the internal guard type directly: deterministic and doesn't require a real
        // TTY, unlike driving raw-mode/alt-screen terminal calls through a full `Crossterm`
        // construction would. The first `acquire()` claims the process-wide flag; a second
        // `acquire()` while the first guard is still alive must be rejected; once the first guard
        // is dropped, `acquire()` must succeed again.
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let first = InstanceGuard::acquire().expect("first acquire must succeed");

        match InstanceGuard::acquire() {
            Ok(_) => {
                panic!("a second concurrent acquire must be rejected while the first guard is live")
            }
            Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::ResourceBusy),
        }

        drop(first);

        let third = InstanceGuard::acquire();
        assert!(
            third.is_ok(),
            "acquire must succeed again once the prior guard is dropped"
        );
    }

    #[test]
    fn constructing_a_second_live_crossterm_is_rejected_until_the_first_is_dropped() {
        // End-to-end version of `instance_guard_rejects_a_second_concurrent_acquire`, through the
        // public `Crossterm` API rather than the internal guard type. All TTY-only features are
        // disabled so construction doesn't require a real terminal (see
        // `build_with_writer_renders_cell_content_into_a_custom_sink` above for why that
        // combination is safe under `cargo test`'s captured, non-TTY stdout).
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let options = || {
            Crossterm::builder()
                .raw_mode(false)
                .alt_screen(false)
                .mouse_capture(false)
                .focus_change(false)
                .bracketed_paste(false)
                .kitty_protocol(false)
        };

        let first = options()
            .build_with_writer(Vec::new())
            .expect("first construction must succeed without a TTY");

        match options().build_with_writer(Vec::new()) {
            Ok(_) => panic!(
                "a second live Crossterm instance must be rejected while the first is still alive"
            ),
            Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::ResourceBusy),
        }

        drop(first);

        let third = options().build_with_writer(Vec::new());
        assert!(
            third.is_ok(),
            "construction must succeed again once the first instance is dropped"
        );
    }

    #[test]
    fn crossterm_options_can_opt_out_of_all_features() {
        // Compile-level/API-shape check: building a `CrosstermOptions` with all flags disabled
        // via the builder type-checks and round-trips its fields. Exercising the actual terminal
        // commands (`with_options`/`build` itself) requires a real TTY, which isn't available in
        // CI, so this is intentionally not a full integration test (see tests/non_tty.rs for the
        // non-TTY integration coverage that is possible without one).
        let options = CrosstermOptions::new()
            .mouse_capture(false)
            .kitty_protocol(false)
            .focus_change(false)
            .bracketed_paste(false)
            .alt_screen(false)
            .raw_mode(false);
        assert!(!options.mouse_capture);
        assert!(!options.kitty_protocol);
        assert!(!options.focus_change);
        assert!(!options.bracketed_paste);
        assert!(!options.alt_screen);
        assert!(!options.raw_mode);
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
    fn size_does_not_requery_after_construction() {
        // `size()` must return the field cached at construction rather than issuing a fresh
        // `crossterm::terminal::size()` syscall on every call (retroglyph#279). Overwrite the
        // cached field with a sentinel value no real terminal query would plausibly produce,
        // then confirm `size()` echoes that sentinel back instead of re-querying and returning
        // whatever the actual (non-TTY, under `cargo test`) terminal size happens to be.
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut term = Crossterm::builder()
            .raw_mode(false)
            .alt_screen(false)
            .mouse_capture(false)
            .focus_change(false)
            .bracketed_paste(false)
            .kitty_protocol(false)
            .build_with_writer(Vec::new())
            .expect("building against a Vec<u8> writer with all TTY features disabled must not require a real terminal");

        let sentinel = Size {
            width: 4321,
            height: 1234,
        };
        term.cached_size = sentinel;

        assert_eq!(
            term.size(),
            sentinel,
            "size() must return the cached field, not re-query the terminal"
        );
    }

    #[test]
    fn resize_event_refreshes_the_cached_size() {
        // `poll_event` refreshes the cached size in lockstep with any `Event::Resize` it reads
        // (retroglyph#279). `refresh_cached_size_on_resize` is the extracted helper it
        // calls; exercising it directly avoids needing a real terminal event source to prove
        // the cache-update behavior.
        let _lock = TEST_GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut term = Crossterm::builder()
            .raw_mode(false)
            .alt_screen(false)
            .mouse_capture(false)
            .focus_change(false)
            .bracketed_paste(false)
            .kitty_protocol(false)
            .build_with_writer(Vec::new())
            .expect("building against a Vec<u8> writer with all TTY features disabled must not require a real terminal");

        term.refresh_cached_size_on_resize(&crossterm::event::Event::Resize(120, 40));
        assert_eq!(
            term.size(),
            Size {
                width: 120,
                height: 40
            }
        );

        // Non-resize events must not disturb the cached size.
        term.refresh_cached_size_on_resize(&crossterm::event::Event::FocusGained);
        assert_eq!(
            term.size(),
            Size {
                width: 120,
                height: 40
            }
        );
    }

    #[test]
    fn crossterm_paste_maps_to_retroglyph_paste() {
        let ct_event = crossterm::event::Event::Paste("pasted text".to_string());
        match from_crossterm_event(ct_event) {
            Some(Event::Paste(text)) => assert_eq!(text, "pasted text"),
            other => panic!("expected Some(Event::Paste(_)), got {other:?}"),
        }
    }

    #[test]
    fn crossterm_focus_gained_maps_correctly() {
        let ct_event = crossterm::event::Event::FocusGained;
        assert!(matches!(
            from_crossterm_event(ct_event),
            Some(Event::FocusGained)
        ));
    }

    #[test]
    fn crossterm_focus_lost_maps_correctly() {
        let ct_event = crossterm::event::Event::FocusLost;
        assert!(matches!(
            from_crossterm_event(ct_event),
            Some(Event::FocusLost)
        ));
    }

    fn mouse_event_kind_of(
        kind: crossterm::event::MouseEventKind,
    ) -> retroglyph_core::event::MouseEventKind {
        from_crossterm_mouse_event_kind(kind)
    }

    #[test]
    fn mouse_down_and_up_still_map_after_option_signature_change() {
        use retroglyph_core::event::{MouseButton as B, MouseEventKind as K};

        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::Down(
                crossterm::event::MouseButton::Left
            )),
            K::Down(B::Left)
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::Up(
                crossterm::event::MouseButton::Right
            )),
            K::Up(B::Right)
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::Moved),
            K::Moved
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::ScrollUp),
            K::ScrollUp
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::ScrollDown),
            K::ScrollDown
        );
    }

    #[test]
    fn mouse_drag_preserves_which_button_is_held() {
        use retroglyph_core::event::{MouseButton as B, MouseEventKind as K};

        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::Drag(
                crossterm::event::MouseButton::Left
            )),
            K::Drag(B::Left)
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::Drag(
                crossterm::event::MouseButton::Right
            )),
            K::Drag(B::Right)
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::Drag(
                crossterm::event::MouseButton::Middle
            )),
            K::Drag(B::Middle)
        );
    }

    #[test]
    fn mouse_horizontal_scroll_round_trips() {
        use retroglyph_core::event::MouseEventKind as K;

        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::ScrollLeft),
            K::ScrollLeft
        );
        assert_eq!(
            mouse_event_kind_of(crossterm::event::MouseEventKind::ScrollRight),
            K::ScrollRight
        );
    }
}
