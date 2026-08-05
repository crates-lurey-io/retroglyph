//! [`SurfaceError`]: the wgpu backend's surface-lifecycle error type.

use std::fmt;

/// A failure creating or driving the wgpu device and surface.
///
/// The variants split along the one distinction the event loop acts on: whether retrying is worth
/// anything. [`RecoverableError`](retroglyph_window::RecoverableError) reads that split, and
/// `retroglyph_window::winit::run` uses it to decide between logging-and-continuing and exiting.
#[derive(Debug)]
#[non_exhaustive]
pub enum SurfaceError {
    /// No adapter, device, or surface could be created for this window.
    ///
    /// Treated as fatal: without a device there is nothing to draw with, and retrying the same
    /// request against the same hardware will fail the same way. Usually means the requested
    /// backends are all unavailable (no Vulkan/Metal/D3D12 driver), which
    /// [`WGPU_BACKEND`](crate#environment-variables) can sometimes work around.
    Init(String),
    /// Acquiring the next frame from the swap chain failed for a reason reconfiguring can't fix.
    ///
    /// Treated as recoverable so the event loop's consecutive-failure heuristic can retry: a
    /// timed-out or occluded frame usually resolves on its own, and the crate reconfigures the
    /// surface itself for the outdated/lost cases before ever returning this.
    Frame(String),
    /// The device was lost (a GPU reset, a driver update, or an eviction) and every resource
    /// created from it is now invalid.
    ///
    /// Treated as fatal at this layer: recovering means rebuilding the adapter, device, and every
    /// GPU resource, which only [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface)
    /// does. The winit driver calls that again after enough consecutive present failures, so a
    /// lost device does get one rebuild attempt, just not through a bare retry of `present`.
    DeviceLost(String),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Init(msg) => write!(f, "wgpu surface init: {msg}"),
            Self::Frame(msg) => write!(f, "wgpu frame acquire: {msg}"),
            Self::DeviceLost(msg) => write!(f, "wgpu device lost: {msg}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

impl retroglyph_window::RecoverableError for SurfaceError {
    fn is_recoverable(&self) -> bool {
        matches!(self, Self::Frame(_))
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceError;
    use retroglyph_window::RecoverableError as _;

    #[test]
    fn only_frame_failures_are_worth_retrying() {
        assert!(!SurfaceError::Init("no adapter".into()).is_recoverable());
        assert!(SurfaceError::Frame("timeout".into()).is_recoverable());
        assert!(!SurfaceError::DeviceLost("reset".into()).is_recoverable());
    }

    #[test]
    fn display_names_the_stage_and_keeps_the_message() {
        assert_eq!(
            SurfaceError::Init("no adapter".into()).to_string(),
            "wgpu surface init: no adapter"
        );
        assert_eq!(
            SurfaceError::Frame("timeout".into()).to_string(),
            "wgpu frame acquire: timeout"
        );
        assert_eq!(
            SurfaceError::DeviceLost("reset".into()).to_string(),
            "wgpu device lost: reset"
        );
    }
}
