use crate::{core::Glyph, render::Color};
use core::ops::{Index, IndexMut};

/// Represents a mutable buffer of pixels for rendering.
#[derive(Debug)]
pub struct Buffer<'a> {
    pixels: &'a mut [Color],
    width: usize,
}

impl<'a> Buffer<'a> {
    /// Creates a new `Buffer` with the given mutable slice of pixels and a specified width.
    ///
    /// # Panics
    ///
    /// Panics if the length of `pixels` is not a multiple of `width`.
    #[must_use]
    pub const fn from_argb(pixels: &'a mut [u32], width: usize) -> Self {
        assert!(pixels.len() % width == 0);
        Self {
            // SAFETY: A Color is represented as a u32, so we can safely transmute the slice.
            pixels: unsafe { core::mem::transmute::<&mut [u32], &mut [Color]>(pixels) },
            width,
        }
    }

    /// Returns the width of the buffer.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the buffer.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.pixels.len() / self.width
    }

    /// Clears the buffer by setting all pixels to opaque black.
    pub fn clear(&mut self) {
        self.pixels.fill(Color::BLACK);
    }

    /// Draws a glyph at the specified position in the buffer as white pixels.
    pub fn draw_glyph(&mut self, glyph: &Glyph, x: usize, y: usize, scale: usize) {
        for (px, py) in glyph.pixels() {
            let dx = x + px as usize * scale;
            let dy = y + py as usize * scale;
            for sy in 0..scale {
                for sx in 0..scale {
                    let tx = dx + sx;
                    let ty = dy + sy;
                    if tx < self.width && ty < self.height() {
                        let index = ty * self.width + tx;
                        self.pixels[index] = Color::WHITE;
                    }
                }
            }
        }
    }
}

impl Index<usize> for Buffer<'_> {
    type Output = Color;

    fn index(&self, index: usize) -> &Self::Output {
        &self.pixels[index]
    }
}

impl IndexMut<usize> for Buffer<'_> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.pixels[index]
    }
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate alloc;
    use super::*;
    use alloc::string::String;
    use alloc::{vec, vec::Vec};

    // Helper function that visualizes a buffer as
    // █ • • • • • • █
    // • █ • • • • █ •
    // • • █ • • █ • •
    // • • • █ █ • • •
    // • • • █ █ • • •
    // • • █ • • █ • •
    // • █ • • • • █ •
    // █ • • • • • • █
    pub(crate) fn buffer_to_string(buffer: &Buffer) -> Vec<String> {
        let mut result = Vec::new();
        for y in 0..buffer.height() {
            let mut line = String::new();
            for x in 0..buffer.width() {
                let pixel = buffer.pixels[y * buffer.width + x];
                if pixel == Color::WHITE {
                    line.push('█');
                } else {
                    line.push('•');
                }
                if x + 1 < buffer.width() {
                    line.push(' ');
                }
            }
            result.push(line);
        }
        result
    }

    #[test]
    fn buffer_from_argb() {
        let mut pixels: [u32; 4] = [0xFF00_FF00, 0xFFFF_0000, 0xFF00_00FF, 0xFFFF_FFFF];
        let buffer = Buffer::from_argb(&mut pixels, 2);
        assert_eq!(buffer[0], Color::from_argb(0xFF, 0x00, 0xFF, 0x00)); // Green
        assert_eq!(buffer[1], Color::from_argb(0xFF, 0xFF, 0x00, 0x00)); // Red
        assert_eq!(buffer[2], Color::from_argb(0xFF, 0x00, 0x00, 0xFF)); // Blue
        assert_eq!(buffer[3], Color::from_argb(0xFF, 0xFF, 0xFF, 0xFF)); // White
    }

    #[test]
    fn buffer_index_mut() {
        let mut pixels = [0u32; 4];
        let mut buffer = Buffer::from_argb(&mut pixels, 2);

        // Modify the first pixel
        buffer[0] = Color::from_argb(0xFF, 0x00, 0xFF, 0x00); // Green
        assert_eq!(buffer[0], Color::from_argb(0xFF, 0x00, 0xFF, 0x00)); // Green
    }

    #[test]
    fn buffer_draw_glyph() {
        let mut pixels = [0; 64];
        let mut buffer = Buffer::from_argb(&mut pixels, 8);

        let glyph = Glyph::new([
            0b1000_0001, //
            0b0100_0010, //
            0b0010_0100, //
            0b0001_1000, //
            0b0001_1000, //
            0b0010_0100, //
            0b0100_0010, //
            0b1000_0001, //
        ]);
        buffer.draw_glyph(&glyph, 0, 0, 1);

        // Assert that we drew an 'X' glyph at the top-left corner
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
        assert_eq!(buffer_to_string(&buffer), expected);
    }
}
