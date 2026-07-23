//! System clipboard read/write for windowed apps (issue #296).
//!
//! Windowed apps have no equivalent of the terminal backends' bracketed-paste path (see
//! `crates/crossterm/src/lib.rs`'s `Event::Paste` handling) for pulling text *out* of the
//! clipboard on demand, nor any way to push text *into* it (e.g. a "copy" keybinding). This
//! module fills that gap with a small [`Clipboard`] trait plus a native, `arboard`-backed
//! [`SystemClipboard`] implementation.
//!
//! Deliberately kept out of [`retroglyph_core::Backend`]: clipboard access has no notion in the
//! terminal backends this workspace also supports (`crossterm`, `software`'s headless test
//! paths), and a windowed app that wants it can reach for this trait directly from its own
//! update loop instead of threading it through every `Backend` implementation.
//!
//! # Testing
//!
//! The real OS clipboard ([`SystemClipboard`]) cannot be exercised headlessly in CI (no display
//! server / clipboard manager is guaranteed to be running), so it has no automated test coverage
//! here; it needs manual verification on each target platform instead. [`Clipboard`] is a plain
//! trait specifically so app code (and this module's own tests) can substitute an in-memory fake
//! in its place; see the `tests` module below for an example.

use std::fmt;

/// Read/write access to a text clipboard.
///
/// A trait rather than a single concrete type so callers can substitute a fake for testing --
/// see this module's doc comment.
pub trait Clipboard {
    /// Returns the current clipboard contents as text.
    ///
    /// # Errors
    ///
    /// Returns [`ClipboardError`] if the clipboard is unavailable, or does not currently hold
    /// text (e.g. it holds an image, or is empty).
    fn get_text(&mut self) -> Result<String, ClipboardError>;

    /// Replaces the clipboard contents with `text`.
    ///
    /// # Errors
    ///
    /// Returns [`ClipboardError`] if the clipboard is unavailable.
    fn set_text(&mut self, text: String) -> Result<(), ClipboardError>;
}

/// Error returned by [`Clipboard::get_text`]/[`Clipboard::set_text`].
///
/// An opaque, message-carrying wrapper rather than an enum of specific failure causes: the two
/// implementations this crate ships (arboard on native, a test fake) fail for platform- or
/// fake-specific reasons that don't share a meaningful common taxonomy, so the message is kept
/// as the one thing that's actually useful across both -- surfacing it in logs/error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardError(String);

impl ClipboardError {
    /// Wraps `message` as a [`ClipboardError`].
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "clipboard error: {}", self.0)
    }
}

impl std::error::Error for ClipboardError {}

/// The native OS clipboard, backed by [`arboard`].
///
/// Not available on `wasm32`: the browser clipboard API
/// (`navigator.clipboard`) is async-only (returns a `Promise`), which does not fit
/// [`Clipboard`]'s synchronous methods, and `arboard` itself does not build for
/// `wasm32-unknown-unknown` -- see this crate's `Cargo.toml` for the target-gating.
#[cfg(not(target_arch = "wasm32"))]
pub struct SystemClipboard(arboard::Clipboard);

#[cfg(not(target_arch = "wasm32"))]
impl SystemClipboard {
    /// Opens a handle to the platform clipboard.
    ///
    /// # Errors
    ///
    /// Returns [`ClipboardError`] if the platform clipboard could not be opened (e.g. no
    /// clipboard manager / display server available).
    pub fn new() -> Result<Self, ClipboardError> {
        arboard::Clipboard::new()
            .map(Self)
            .map_err(|e| ClipboardError::new(e.to_string()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Clipboard for SystemClipboard {
    fn get_text(&mut self) -> Result<String, ClipboardError> {
        self.0
            .get_text()
            .map_err(|e| ClipboardError::new(e.to_string()))
    }

    fn set_text(&mut self, text: String) -> Result<(), ClipboardError> {
        self.0
            .set_text(text)
            .map_err(|e| ClipboardError::new(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory fake standing in for the real OS clipboard: [`SystemClipboard`] can't be
    /// exercised headlessly in CI (see this module's doc comment), so this is what actually gets
    /// covered by automated tests.
    #[derive(Default)]
    struct FakeClipboard {
        contents: Option<String>,
    }

    impl Clipboard for FakeClipboard {
        fn get_text(&mut self) -> Result<String, ClipboardError> {
            self.contents
                .clone()
                .ok_or_else(|| ClipboardError::new("clipboard is empty"))
        }

        fn set_text(&mut self, text: String) -> Result<(), ClipboardError> {
            self.contents = Some(text);
            Ok(())
        }
    }

    #[test]
    fn set_then_get_round_trips() {
        let mut clip = FakeClipboard::default();
        clip.set_text("hello".to_string()).unwrap();
        assert_eq!(clip.get_text().unwrap(), "hello");
    }

    #[test]
    fn get_before_any_set_is_an_error() {
        let mut clip = FakeClipboard::default();
        assert!(clip.get_text().is_err());
    }

    #[test]
    fn set_overwrites_previous_contents() {
        let mut clip = FakeClipboard::default();
        clip.set_text("first".to_string()).unwrap();
        clip.set_text("second".to_string()).unwrap();
        assert_eq!(clip.get_text().unwrap(), "second");
    }

    #[test]
    fn clipboard_error_display_includes_message() {
        let err = ClipboardError::new("boom");
        assert_eq!(err.to_string(), "clipboard error: boom");
    }

    #[test]
    fn clipboard_error_equality() {
        assert_eq!(ClipboardError::new("a"), ClipboardError::new("a"));
        assert_ne!(ClipboardError::new("a"), ClipboardError::new("b"));
    }
}
