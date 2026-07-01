#![allow(dead_code, unreachable_pub)]
//! Hex coordinate math and screen-space mapping.
//!
//! Uses `hexal` for axial-coordinate arithmetic (neighbors, distance, rings).
//! Screen-to-hex and hex-to-screen conversions are inlined here because they
//! depend on the demo's cell/pixel dimensions, not on hexal internals.

pub use hexal::HexI;

// ── Layout constants ──────────────────────────────────────────────────────────

/// How many terminal columns wide one hex cell is.
///
/// At scale=2 with VGA 8×16: each cell = 16px wide, so 4 cells = 64px,
/// matching the 64×64 sprite exactly.  Prevents horizontal overlap.
pub const HEX_CELL_COLS: u16 = 4;
/// How many terminal rows tall one hex cell is.
///
/// At scale=2 with VGA 8×16: each cell = 32px tall, so 2 cells = 64px,
/// matching the 64×64 sprite exactly.
pub const HEX_CELL_ROWS: u16 = 2;

/// Horizontal advance in cells between adjacent hexes in the same row.
pub const HEX_COL_STRIDE: u16 = HEX_CELL_COLS;
/// Vertical advance in cells between adjacent hex rows.
pub const HEX_ROW_STRIDE: u16 = HEX_CELL_ROWS;
/// Horizontal offset (cells) applied to odd rows (pointy-top offset grid).
pub const HEX_ODD_ROW_OFFSET: u16 = HEX_CELL_COLS / 2;

/// Map offset. Top-left hex in the visible area is drawn starting at this
/// terminal cell.
pub const MAP_ORIGIN_X: u16 = 1;
pub const MAP_ORIGIN_Y: u16 = 1;

// ── Sprite codepoints (Codepage::Identity tile indices) ───────────────────────

pub const SPRITE_EMPTY: char = '\x00';
pub const SPRITE_SELECTED: char = '\x01';
pub const SPRITE_BLUE: char = '\x02';
pub const SPRITE_RED: char = '\x03';
pub const SPRITE_MOVE: char = '\x04';
pub const SPRITE_ATTACK: char = '\x05';

// ── Coordinate conversions ────────────────────────────────────────────────────

/// Offset-grid (col, row) → terminal cell (x, y).
///
/// Uses odd-r layout: odd rows are shifted right by `HEX_ODD_ROW_OFFSET`.
pub fn offset_to_cell(col: i32, row: i32) -> Option<(u16, u16)> {
    let col = u16::try_from(col).ok()?;
    let row = u16::try_from(row).ok()?;
    let ox = if row % 2 == 1 { HEX_ODD_ROW_OFFSET } else { 0 };
    let x = MAP_ORIGIN_X + col * HEX_COL_STRIDE + ox;
    let y = MAP_ORIGIN_Y + row * HEX_ROW_STRIDE;
    Some((x, y))
}

/// Axial hex (q, r) → terminal cell (x, y) via odd-r offset.
pub fn axial_to_cell(q: i32, r: i32) -> Option<(u16, u16)> {
    // Convert axial to odd-r offset.
    let col = q + (r - (r & 1)) / 2;
    let row = r;
    offset_to_cell(col, row)
}

/// Terminal cell (x, y) → axial hex (q, r).
///
/// Returns `None` if the cell falls outside the origin margin.
pub const fn cell_to_axial(x: u16, y: u16) -> Option<(i32, i32)> {
    if x < MAP_ORIGIN_X || y < MAP_ORIGIN_Y {
        return None;
    }
    let x = x - MAP_ORIGIN_X;
    let y = y - MAP_ORIGIN_Y;
    let row = y / HEX_ROW_STRIDE;
    let ox = if row % 2 == 1 { HEX_ODD_ROW_OFFSET } else { 0 };
    let x = x.saturating_sub(ox);
    let col = x / HEX_COL_STRIDE;
    // odd-r → axial
    let q = col as i32 - (row as i32 - (row as i32 & 1)) / 2;
    let r = row as i32;
    Some((q, r))
}

/// Physical pixel (px, py) → axial hex, given cell pixel dimensions.
///
/// Uses `PhysicalPos` from the software backend when available.
pub fn pixel_to_axial(px: u32, py: u32, cell_w: u32, cell_h: u32) -> Option<(i32, i32)> {
    if cell_w == 0 || cell_h == 0 {
        return None;
    }
    let cx = u16::try_from(px / cell_w).ok()?;
    let cy = u16::try_from(py / cell_h).ok()?;
    cell_to_axial(cx, cy)
}

// ── Board bounds ──────────────────────────────────────────────────────────────

pub const BOARD_COLS: i32 = 12;
pub const BOARD_ROWS: i32 = 7;

/// Returns `true` if axial (q, r) is on the visible board.
pub const fn on_board(q: i32, r: i32) -> bool {
    let col = q + (r - (r & 1)) / 2;
    col >= 0 && col < BOARD_COLS && r >= 0 && r < BOARD_ROWS
}

// ── Hex distance ──────────────────────────────────────────────────────────────

pub fn hex_distance(a: (i32, i32), b: (i32, i32)) -> u32 {
    HexI::new(a.0, a.1)
        .distance(HexI::new(b.0, b.1))
        .unsigned_abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_even_row() {
        let (q, r) = (2, 0);
        let (x, y) = axial_to_cell(q, r).unwrap();
        let (q2, r2) = cell_to_axial(x, y).unwrap();
        assert_eq!((q, r), (q2, r2));
    }

    #[test]
    fn round_trip_odd_row() {
        let (q, r) = (3, 3);
        let (x, y) = axial_to_cell(q, r).unwrap();
        let (q2, r2) = cell_to_axial(x, y).unwrap();
        assert_eq!((q, r), (q2, r2));
    }

    #[test]
    fn hex_distance_adjacent() {
        assert_eq!(hex_distance((0, 0), (1, 0)), 1);
        assert_eq!(hex_distance((0, 0), (0, 1)), 1);
    }

    #[test]
    fn on_board_boundaries() {
        assert!(on_board(0, 0));
        assert!(!on_board(-1, 0));
        assert!(!on_board(0, 7));
    }
}
