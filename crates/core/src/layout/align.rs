//! Horizontal and vertical alignment within a bounded rectangle.
//!
//! [`HAlign`] and [`VAlign`] are plain data: no allocation, no text handling, nothing that
//! depends on the `egc` feature. Private submodule of [`layout`](crate::layout), re-exported
//! unconditionally from there regardless of `egc`; [`TextLayout`](crate::layout::TextLayout) (the
//! `egc`-gated word-wrap builder) and
//! [`Surface::print_aligned`](crate::surface::Surface::print_aligned) both use them.

/// Horizontal alignment within a bounded rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HAlign {
    /// Align text to the left edge (default).
    #[default]
    Left,
    /// Centre text horizontally.
    Center,
    /// Align text to the right edge.
    Right,
}

impl HAlign {
    /// The left offset, in columns, at which a `content_width`-column line should start within
    /// an `area_width`-column area for this alignment.
    ///
    /// Saturates at `0` when the content is wider than the area, so the caller clips from the
    /// left edge rather than underflowing.
    #[must_use]
    pub const fn offset(self, area_width: u16, content_width: u16) -> u16 {
        let slack = area_width.saturating_sub(content_width);
        match self {
            Self::Left => 0,
            Self::Center => slack / 2,
            Self::Right => slack,
        }
    }
}

/// Vertical alignment within a bounded rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VAlign {
    /// Align text to the top edge (default).
    #[default]
    Top,
    /// Centre text vertically.
    Middle,
    /// Align text to the bottom edge.
    Bottom,
}

impl VAlign {
    /// The top offset, in rows, at which `content_height` rows of content should start within
    /// an `area_height`-row area for this alignment.
    ///
    /// Saturates at `0` when the content is taller than the area, so the caller clips from the
    /// top edge rather than underflowing.
    #[must_use]
    pub const fn offset(self, area_height: u16, content_height: u16) -> u16 {
        let slack = area_height.saturating_sub(content_height);
        match self {
            Self::Top => 0,
            Self::Middle => slack / 2,
            Self::Bottom => slack,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h_align_offset_places_content_per_alignment() {
        // 4-column word in a 10-column area: 6 columns of slack.
        assert_eq!(HAlign::Left.offset(10, 4), 0);
        assert_eq!(HAlign::Center.offset(10, 4), 3);
        assert_eq!(HAlign::Right.offset(10, 4), 6);
    }

    #[test]
    fn h_align_offset_wider_than_area_saturates_to_zero() {
        assert_eq!(HAlign::Left.offset(3, 8), 0);
        assert_eq!(HAlign::Center.offset(3, 8), 0);
        assert_eq!(HAlign::Right.offset(3, 8), 0);
    }

    #[test]
    fn v_align_offset_places_content_per_alignment() {
        // 2-row block in a 10-row area: 8 rows of slack.
        assert_eq!(VAlign::Top.offset(10, 2), 0);
        assert_eq!(VAlign::Middle.offset(10, 2), 4);
        assert_eq!(VAlign::Bottom.offset(10, 2), 8);
    }

    #[test]
    fn v_align_offset_taller_than_area_saturates_to_zero() {
        assert_eq!(VAlign::Top.offset(3, 8), 0);
        assert_eq!(VAlign::Middle.offset(3, 8), 0);
        assert_eq!(VAlign::Bottom.offset(3, 8), 0);
    }
}
