/// Represents a color in ARGB (`0xAARRGGBB`) format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Color(u32);

impl Color {
    /// Default color, represented by fully transparent black (`0xFF000000`).
    pub const BLACK: Self = Color(0xFF00_0000);

    /// Fully opaque black color (`0xFF000000`).
    pub const WHITE: Self = Color(0xFFFF_FFFF);

    /// Creates a new `Color` from a 32-bit ARGB value.
    #[must_use]
    pub const fn new(argb: u32) -> Self {
        Color(argb)
    }

    /// Creates a new `Color` from individual ARGB components.
    #[must_use]
    pub const fn from_argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    /// Creates a new `Color` from RGB components, with full opacity.
    #[must_use]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color::from_argb(0xFF, r, g, b)
    }

    /// Returns the ARGB value of this color.
    #[must_use]
    pub const fn to_argb(&self) -> u32 {
        self.0
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_black() {
        let color = Color::default();
        assert_eq!(color, Color::BLACK);
    }

    #[test]
    fn new_color() {
        let color = Color::new(0xFF00_FF00); // Fully opaque green
        assert_eq!(color.to_argb(), 0xFF00_FF00);
    }

    #[test]
    fn from_argb() {
        let color = Color::from_argb(0xFF, 0x00, 0xFF, 0x00); // Fully opaque green
        assert_eq!(color.to_argb(), 0xFF00_FF00);
    }

    #[test]
    fn from_rgb() {
        let color = Color::from_rgb(0x00, 0xFF, 0x00); // Fully opaque green
        assert_eq!(color.to_argb(), 0xFF00_FF00);
    }
}
