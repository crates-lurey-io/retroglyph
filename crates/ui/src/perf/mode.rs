//! [`PerfOverlayMode`]: how much detail [`super::PerfOverlayApp`] currently shows.

/// How much detail [`super::PerfOverlayApp`] currently shows, advanced by the toggle key.
///
/// Cycles `Off -> Compact -> Full -> Off`. [`Full`](Self::Full) is only reachable once
/// [`PerfOverlayApp::cycle_with`](super::PerfOverlayApp::cycle_with) has registered a second
/// renderer; without one, the cycle skips straight from [`Compact`](Self::Compact) back to `Off`,
/// i.e. the plain two-state toggle from before this enum existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PerfOverlayMode {
    /// Nothing renders.
    Off,
    /// [`PerfOverlayApp`](super::PerfOverlayApp)'s primary [`PerfRenderer`](super::PerfRenderer)
    /// (e.g. [`DefaultPerfRenderer`](super::DefaultPerfRenderer)) renders.
    Compact,
    /// The renderer registered via
    /// [`PerfOverlayApp::cycle_with`](super::PerfOverlayApp::cycle_with) renders, if any;
    /// otherwise equivalent to `Off` (this mode is unreachable through the toggle key alone in
    /// that case, but [`PerfOverlayApp::set_mode`](super::PerfOverlayApp::set_mode) can still be
    /// called with it directly).
    Full,
}

impl PerfOverlayMode {
    /// The next mode in the cycle, `Off -> Compact -> Full -> Off`, skipping `Full` when
    /// `has_full` is `false`.
    #[must_use]
    pub(super) const fn next(self, has_full: bool) -> Self {
        match self {
            Self::Off => Self::Compact,
            Self::Compact if has_full => Self::Full,
            Self::Compact | Self::Full => Self::Off,
        }
    }
}
