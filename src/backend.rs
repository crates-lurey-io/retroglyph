//! Pluggable rendering backends.

use crate::grid::Grid;

/// A device or implementation that can render a [Grid].
pub trait Renderer {
    /// Renders the current state of the grid.
    fn draw(&mut self, grid: &Grid);
}
