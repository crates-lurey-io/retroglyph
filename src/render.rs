mod buffer;
pub use buffer::Buffer;

mod color;
pub use color::Color;

use crate::core::{Font, Grid};

/// Renders a `Grid` to a `Buffer` using the specified `Font`.
///
/// Clears the `Buffer` before rendering.
pub fn render<const LENGTH: usize>(from: &Grid<LENGTH>, to: &mut Buffer, font: &Font) {
    to.clear();
    for (y, row) in from.rows().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let glyph = font.glyph(cell.glyph());
            to.draw_glyph(&glyph, x * 8, y * 8);
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use crate::{
        core::{Cell, Glyph},
        grid,
    };
    use alloc::{string::String, vec};

    #[test]
    fn render_empty_grid() {
        // Create a non-empty buffer as an example.
        let mut pixels = [Color::WHITE.to_argb(); 64];
        let mut buffer = Buffer::from_argb(&mut pixels, 8);

        // Create a "Font". It will be entirely empty for this test.
        let font = Font::new([Glyph::new([0; 8]); 256]);

        // Create an empty grid.
        let grid: Grid<1> = grid!(1, 1);

        // Render the empty grid to the buffer.
        render(&grid, &mut buffer, &font);

        // Check that the buffer is now entirely empty (all pixels are black).
        assert!(pixels.iter().all(|&color| color == Color::BLACK.to_argb()));
    }

    #[test]
    fn render_single_cell_grid() {
        // Create an empty buffer.
        let mut pixels = [Color::BLACK.to_argb(); 64];
        let mut buffer = Buffer::from_argb(&mut pixels, 8);

        // Create a "Font" with a single glyph that is not empty.
        let glyph = Glyph::new([
            0b1000_0001, // Example glyph data
            0b0100_0010,
            0b0010_0100,
            0b0001_1000,
            0b0001_1000,
            0b0010_0100,
            0b0100_0010,
            0b1000_0001,
        ]);
        let font = Font::new([glyph; 256]);

        // Create a grid with a single cell that has the glyph.
        let mut grid: Grid<1> = grid!(1, 1);
        *grid.get_mut(0, 0).unwrap() = Cell::new(0x58);

        // Render the grid to the buffer.
        render(&grid, &mut buffer, &font);

        // Check that the buffer has the expected pixel data for the glyph.
        let expected = vec![
            String::from("█ • • • • • • █"),
            String::from("• █ • • • • █ •"),
            String::from("• • █ • • █ • •"),
            String::from("• • • █ █ • • •"),
            String::from("• • • █ █ • • •"),
            String::from("• • █ • • █ • •"),
            String::from("• █ • • • • █ •"),
            String::from("█ • • • • • • █"),
        ];
        let actual = crate::render::buffer::tests::buffer_to_string(&buffer);
        assert_eq!(actual, expected);
    }
}
