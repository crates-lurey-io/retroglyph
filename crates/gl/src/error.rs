//! [`SurfaceError`]: the GL backend's surface-lifecycle error type.

use std::fmt;

/// A failure creating or driving the GL context/surface.
///
/// Both the native (glutin) and wasm (WebGL2) context modules produce this. It is string-backed
/// rather than a structured enum because the two platforms surface very different underlying error
/// types (glutin's `glutin::error::Error` vs. a `web_sys` `JsValue`), and the caller
/// ([`retroglyph_window`]'s event loop) only needs a message plus the recoverable/fatal signal
/// from [`RecoverableError`](retroglyph_window::RecoverableError).
#[derive(Debug)]
pub enum SurfaceError {
    /// Creating the GL display, config, context, or surface failed. Treated as fatal (not
    /// recoverable): a game cannot proceed without a context, and retrying the same creation
    /// path is very unlikely to succeed.
    Init(String),
    /// Presenting a frame failed (buffer swap on native, or the WebGL2 context was lost on wasm).
    /// Treated as potentially recoverable so the event loop's consecutive-failure heuristic can
    /// retry before giving up.
    Present(String),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Init(msg) => write!(f, "GL surface init: {msg}"),
            Self::Present(msg) => write!(f, "GL surface present: {msg}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

impl retroglyph_window::RecoverableError for SurfaceError {
    fn is_recoverable(&self) -> bool {
        // Init failures are fatal (nothing to retry into); present failures may be transient
        // (e.g. a wasm context-loss that the browser later restores).
        matches!(self, Self::Present(_))
    }
}
