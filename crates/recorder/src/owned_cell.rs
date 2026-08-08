//! [`OwnedCell`], the owned counterpart to [`DrawCell`] that a capture source buffers instead of
//! the borrowed original.

use retroglyph_core::backend::DrawCell;
use retroglyph_core::color::Tint;
use retroglyph_core::grid::Pos;
use retroglyph_core::tile::Tile;

/// An owned copy of a single [`DrawCell`], the shape [`FrameRecorder`](crate::FrameRecorder)
/// buffers instead of the borrowed original.
///
/// `DrawCell` borrows its grapheme text from the source [`Grid`](retroglyph_core::grid::Grid), so
/// a capture that outlives one `draw_layers` call (every capture source in this crate) must copy
/// it out immediately rather than defer to export time, when the grid it borrowed from has
/// already moved on to the next frame. `grapheme` is the one field that costs an allocation here;
/// everything else (`Tile`, [`Pos`], `layer`, [`Tint`]) is already `Copy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedCell {
    /// See [`DrawCell::layer`].
    pub layer: u8,
    /// See [`DrawCell::pos`].
    pub pos: Pos,
    /// See [`DrawCell::tile`].
    pub tile: Tile,
    /// See [`DrawCell::grapheme`]. Copied to an owned `String` (rather than something
    /// reference-counted) since `DrawCell` exposes only a borrowed `&str`, not the `Arc<str>`
    /// backends like [`Headless`](retroglyph_core::backend::Headless) keep internally, so there
    /// is no cheaper handle available across the `Output` trait boundary to share instead.
    pub grapheme: Option<String>,
    /// See [`DrawCell::tint`].
    pub tint: Tint,
}

impl From<DrawCell<'_>> for OwnedCell {
    fn from(cell: DrawCell<'_>) -> Self {
        Self {
            layer: cell.layer,
            pos: cell.pos,
            tile: *cell.tile,
            grapheme: cell.grapheme.map(str::to_owned),
            tint: cell.tint,
        }
    }
}
