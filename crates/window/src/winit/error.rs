//! [`EventLoopError`]: an opaque wrapper around winit's own event-loop error.

use core::fmt;

/// The error winit's event loop can fail with when starting or running.
///
/// Wraps `winit`'s own `EventLoopError` opaquely rather than re-exporting it: a `winit` point
/// release can add or remove variants on its error type without that being a breaking change for
/// `winit` itself, but re-exporting it verbatim would make that a breaking change here too, since
/// downstream code matching on it would stop compiling. Nothing in this crate's public API names
/// a raw `winit` type as a result.
///
/// Use [`fmt::Display`] for a human-readable message, or
/// [`std::error::Error::source`] to reach the underlying `winit` error for programmatic
/// inspection (downcasting, logging its `Debug` form, etc.).
#[derive(Debug)]
pub struct EventLoopError(::winit::error::EventLoopError);

impl fmt::Display for EventLoopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for EventLoopError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<::winit::error::EventLoopError> for EventLoopError {
    fn from(err: ::winit::error::EventLoopError) -> Self {
        Self(err)
    }
}
