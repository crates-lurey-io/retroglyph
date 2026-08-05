//! [`Cell`]: the per-cell instance the vertex shader reads, and the compositing flags it carries.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary is
// intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

use bytemuck::{Pod, Zeroable};

/// [`Cell::flags`] bit: paint this cell's opaque background.
///
/// Cleared means a transparent background, which the background pass `discard`s so a lower grid
/// layer shows through.
pub(crate) const FLAG_HAS_BG: u16 = 1 << 0;

/// [`Cell::flags`] bit: draw this cell's glyph.
///
/// Cleared means an empty cell, which the glyph pass `discard`s so a higher layer's untouched
/// cells don't erase the layer beneath.
pub(crate) const FLAG_HAS_GLYPH: u16 = 1 << 1;

/// Per-cell instance data, tightly packed to 16 bytes and uploaded straight to the GPU.
///
/// There is no per-cell position: the vertex shader derives `(column, row)` from the instance
/// index and the grid's column count, so the whole grid costs 16 bytes per cell and no index
/// buffer at all.
///
/// `#[repr(C)]` with explicit padding so the field offsets match the vertex attributes
/// [`cell_layout`](crate::renderer::cell_layout) declares. Every attribute is 4 bytes wide and
/// lands on a 4-byte offset, which is what WebGPU requires of a vertex buffer layout. The
/// `glyph`/`flags` and `dx`/`dy` pairs share one attribute each (`Uint16x2` and `Sint16x2`), which
/// is why they sit adjacent.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct Cell {
    /// Atlas slot (glyph id) for this cell.
    pub glyph: u16,
    /// Compositing flags ([`FLAG_HAS_BG`] | [`FLAG_HAS_GLYPH`]).
    ///
    /// A `u16` rather than the `u8` the two bits need, so it pairs with `glyph` into one
    /// `Uint16x2` attribute instead of costing its own.
    pub flags: u16,
    /// Foreground RGB, uploaded as normalized `u8`. The fourth channel is padding, not alpha; the
    /// glyph pass takes its alpha from atlas coverage.
    pub fg: [u8; 4],
    /// Background RGB, uploaded as normalized `u8`. The fourth channel is padding; the background
    /// pass writes alpha 1 unconditionally.
    pub bg: [u8; 4],
    /// Sub-cell pixel offset from the cell's left edge, in unscaled font pixels (from
    /// [`Tile::dx`](retroglyph_core::tile::Tile::dx)). Negative shifts the glyph left.
    pub dx: i16,
    /// Sub-cell pixel offset from the cell's top edge, in unscaled font pixels (from
    /// [`Tile::dy`](retroglyph_core::tile::Tile::dy)). Negative shifts the glyph up.
    pub dy: i16,
}

/// Byte size of one [`Cell`], as a `usize` for slice and array arithmetic.
pub(crate) const CELL_BYTES: usize = 16;

/// Byte stride of one [`Cell`], which is also the vertex buffer's `array_stride`.
pub(crate) const CELL_STRIDE: u64 = CELL_BYTES as u64;

impl Cell {
    /// A cell with the given glyph slot, colors, sub-cell pixel offset, and compositing flags.
    pub(crate) const fn new(
        glyph: u16,
        fg: [u8; 3],
        bg: [u8; 3],
        dx: i16,
        dy: i16,
        flags: u16,
    ) -> Self {
        Self {
            glyph,
            flags,
            fg: [fg[0], fg[1], fg[2], 0],
            bg: [bg[0], bg[1], bg[2], 0],
            dx,
            dy,
        }
    }

    /// A fully transparent cell: nothing drawn, so whatever a lower grid layer painted survives.
    pub(crate) const fn transparent(glyph: u16) -> Self {
        Self::new(glyph, [0; 3], [0; 3], 0, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{CELL_BYTES, CELL_STRIDE, Cell, FLAG_HAS_BG, FLAG_HAS_GLYPH};
    use core::mem::offset_of;

    #[test]
    fn layout_matches_the_vertex_attribute_offsets() {
        // `renderer::cell_layout` describes this struct to the GPU by hand, as a stride plus a
        // per-attribute offset. Nothing else checks that description against the struct, so a
        // field added or reordered here without updating it would have every instance read from
        // the wrong offset and render garbage, with no compile error.
        assert_eq!(size_of::<Cell>(), CELL_BYTES);
        assert_eq!(CELL_STRIDE, CELL_BYTES as u64);
        assert_eq!(offset_of!(Cell, glyph), 0);
        assert_eq!(offset_of!(Cell, flags), 2);
        assert_eq!(offset_of!(Cell, fg), 4);
        assert_eq!(offset_of!(Cell, bg), 8);
        assert_eq!(offset_of!(Cell, dx), 12);
        assert_eq!(offset_of!(Cell, dy), 14);
    }

    #[test]
    fn every_attribute_offset_is_four_byte_aligned() {
        // WebGPU requires a vertex attribute's offset to be a multiple of 4 for every format this
        // layout uses. Asserting it here catches a repacking that would only fail at pipeline
        // creation, on a device, in a test that needs a GPU.
        for offset in [
            offset_of!(Cell, glyph),
            offset_of!(Cell, fg),
            offset_of!(Cell, bg),
            offset_of!(Cell, dx),
        ] {
            assert_eq!(offset % 4, 0, "attribute at {offset} is not 4-byte aligned");
        }
        assert_eq!(CELL_STRIDE % 4, 0);
    }

    #[test]
    fn flags_are_distinct_bits() {
        assert_eq!(FLAG_HAS_BG & FLAG_HAS_GLYPH, 0);
    }

    #[test]
    fn a_transparent_cell_draws_nothing() {
        let cell = Cell::transparent(7);
        assert_eq!(cell.flags, 0);
        assert_eq!(cell.glyph, 7);
    }

    #[test]
    fn new_pads_colors_rather_than_carrying_alpha() {
        let cell = Cell::new(1, [10, 20, 30], [40, 50, 60], -1, 2, FLAG_HAS_BG);
        assert_eq!(cell.fg, [10, 20, 30, 0]);
        assert_eq!(cell.bg, [40, 50, 60, 0]);
        assert_eq!((cell.dx, cell.dy), (-1, 2));
    }
}
