pub mod software;

use crate::core::Cell;

/// A backend implementation, which handles I/O.
pub trait Backend {
    /// Sets the specified coordinates to render the given `Cell`.
    ///
    /// Coordinates outside the bounds of the terminal are ignored.
    fn set(&mut self, x: i32, y: i32, cell: &Cell);

    /// Updates the terminal display.
    ///
    /// Any calls to [`set`](Backend::set) before this method will be reflected in the display.
    fn update(&mut self);

    /// Returns the width of the terminal, in cells.
    fn width(&self) -> u32;

    /// Returns the height of the terminal, in cells.
    fn height(&self) -> u32;
}
