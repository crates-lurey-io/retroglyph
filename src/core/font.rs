mod ibm_clasix_8x8;

/// A fixed-width `8x8` bitmap font.
#[derive(Debug, Clone)]
pub struct Font {
    glyphs: [Glyph; 256],
}

impl Font {
    /// The classic IBM PC/VGA 8x8 font for [Codepage 437][].
    ///
    /// [Codepage 437]: https://en.wikipedia.org/wiki/Code_page_437
    pub const IBM_CLASSIC_8X8: Font = ibm_clasix_8x8::FONT;

    /// Creates a new `Font` with the given glyph data.
    #[must_use]
    pub const fn new(glyphs: [Glyph; 256]) -> Self {
        Font { glyphs }
    }

    /// Returns the glyph for the specified CP437 index.
    #[must_use]
    pub const fn glyph(&self, index: u8) -> Glyph {
        self.glyphs[index as usize]
    }
}

impl Default for Font {
    fn default() -> Self {
        Self::IBM_CLASSIC_8X8
    }
}

/// A single `8x8` glyph in a fixed-width font.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Glyph([u8; 8]);

impl Glyph {
    /// An empty glyph.
    pub const EMPTY: Self = Glyph([0; 8]);

    /// Creates a new `Glyph` with the given bytes, each byte representing a row of pixels.
    ///
    /// Each byte should contain 8 bits, where each bit represents a pixel (1 for on, 0 for off).
    #[must_use]
    pub const fn new(rows: [u8; 8]) -> Self {
        Glyph(rows)
    }

    /// Returns each offset of set bits in the glyph (pixels that are "on") for each row.
    #[must_use]
    pub fn pixels(&self) -> Pixels {
        Pixels {
            glyph: self,
            row: 0,
            col: 0,
        }
    }

    /// Returns the width of the glyph in pixels.
    #[must_use]
    pub const fn width(&self) -> u8 {
        8
    }

    /// Returns the height of the glyph in pixels.
    #[must_use]
    pub const fn height(&self) -> u8 {
        8
    }
}

/// An iterator over the pixels of a `Glyph`.
#[derive(Debug, Clone)]
pub struct Pixels<'a> {
    glyph: &'a Glyph,
    row: u8,
    col: u8,
}

impl Iterator for Pixels<'_> {
    type Item = (u8, u8);

    fn next(&mut self) -> Option<Self::Item> {
        while self.row < 8 {
            let row_data = self.glyph.0[self.row as usize];
            if self.col < 8 {
                let bit = (row_data >> (7 - self.col)) & 1;
                if bit == 1 {
                    let pixel = (self.col, self.row);
                    self.col += 1;
                    return Some(pixel);
                }
                self.col += 1;
            } else {
                self.row += 1;
                self.col = 0;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::{vec, vec::Vec};

    #[test]
    fn new() {
        let mut glyphs = [Glyph::new([0; 8]); 256];
        // Set 'X' glyph for testing
        glyphs[0x58] = Glyph::new([
            0b1000_0001, //
            0b0100_0010, //
            0b0010_0100, //
            0b0001_1000, //
            0b0001_1000, //
            0b0010_0100, //
            0b0100_0010, //
            0b1000_0001, //
        ]);

        let font = Font::new(glyphs);
        assert_eq!(
            font.glyph(0x58),
            Glyph::new([
                0b1000_0001, //
                0b0100_0010, //
                0b0010_0100, //
                0b0001_1000, //
                0b0001_1000, //
                0b0010_0100, //
                0b0100_0010, //
                0b1000_0001, //
            ])
        );
    }

    #[test]
    fn pixels() {
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

        let pixels = glyph.pixels().collect::<Vec<_>>();
        assert_eq!(
            pixels,
            vec![
                (0, 0),
                (7, 0),
                (1, 1),
                (6, 1),
                (2, 2),
                (5, 2),
                (3, 3),
                (4, 3),
                (3, 4),
                (4, 4),
                (2, 5),
                (5, 5),
                (1, 6),
                (6, 6),
                (0, 7),
                (7, 7)
            ]
        );
    }

    #[test]
    fn dimensions_8x8() {
        let glyph = Glyph::new([0u8; 8]);

        assert_eq!(glyph.width(), 8);
        assert_eq!(glyph.height(), 8);
    }
}
