//! [`Align`]: horizontal alignment of a single line of text within a
//! fixed-width area.

/// Horizontal alignment of one line of text within the columns it's rendered
/// into.
///
/// A builder knob on the single-line text widgets ([`Text`](crate::Text),
/// [`PrintLine`](crate::PrintLine)) and on the titles of [`Panel`](crate::Panel)
/// and [`Modal`](crate::Modal). Text widgets default to [`Left`](Self::Left)
/// (their long-standing behavior); panel/modal titles default to
/// [`Center`](Self::Center) (theirs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Text starts at the left edge; leftover space trails on the right.
    #[default]
    Left,
    /// Leftover space is split evenly on both sides (an odd extra column goes
    /// on the right).
    Center,
    /// Text ends at the right edge; leftover space leads on the left.
    Right,
}

impl Align {
    /// The left offset, in columns, at which a `content_width`-column line
    /// should start within an `area_width`-column area for this alignment.
    ///
    /// Saturates at `0` when the content is wider than the area, so the caller
    /// clips from the left edge rather than underflowing.
    ///
    /// Duplicates [`HAlign::offset`](retroglyph_core::layout::HAlign::offset) rather than
    /// delegating to it: `core::layout` (and `HAlign`) only exists when core's `egc` feature is
    /// on, but `Align` and its callers ([`draw_clipped`](crate::draw_clipped),
    /// [`Button`](crate::Button)) don't need `egc` and must keep working without it. See the
    /// `From` impls below for converting into `HAlign` once `egc` is available.
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

/// Converts to [`core::layout::HAlign`](retroglyph_core::layout::HAlign), the richer type
/// [`Surface::print_aligned`](retroglyph_core::Surface::print_aligned) and
/// [`TextLayout`](retroglyph_core::layout::TextLayout) take, so a widget holding an [`Align`]
/// can reach those APIs without every widget reimplementing them. Only available when core's
/// `egc` feature is on, since `HAlign` lives behind it.
#[cfg(feature = "egc")]
impl From<Align> for retroglyph_core::layout::HAlign {
    fn from(align: Align) -> Self {
        match align {
            Align::Left => Self::Left,
            Align::Center => Self::Center,
            Align::Right => Self::Right,
        }
    }
}

/// The inverse of `From<Align> for HAlign`, for callers building an [`Align`] from a value
/// already expressed in core's alignment type.
#[cfg(feature = "egc")]
impl From<retroglyph_core::layout::HAlign> for Align {
    fn from(align: retroglyph_core::layout::HAlign) -> Self {
        use retroglyph_core::layout::HAlign;
        match align {
            HAlign::Left => Self::Left,
            HAlign::Center => Self::Center,
            HAlign::Right => Self::Right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_places_content_per_alignment() {
        // 4-column word in a 10-column area: 6 columns of slack.
        assert_eq!(Align::Left.offset(10, 4), 0);
        assert_eq!(Align::Center.offset(10, 4), 3);
        assert_eq!(Align::Right.offset(10, 4), 6);
    }

    #[test]
    fn center_puts_the_odd_column_on_the_right() {
        // 4-column word in a 9-column area: 5 columns of slack, 2 on the left.
        assert_eq!(Align::Center.offset(9, 4), 2);
    }

    #[test]
    fn wider_than_area_saturates_to_zero() {
        assert_eq!(Align::Left.offset(3, 8), 0);
        assert_eq!(Align::Center.offset(3, 8), 0);
        assert_eq!(Align::Right.offset(3, 8), 0);
    }

    #[test]
    fn default_is_left() {
        assert_eq!(Align::default(), Align::Left);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn converts_to_and_from_core_h_align() {
        use retroglyph_core::layout::HAlign;

        assert_eq!(HAlign::from(Align::Left), HAlign::Left);
        assert_eq!(HAlign::from(Align::Center), HAlign::Center);
        assert_eq!(HAlign::from(Align::Right), HAlign::Right);

        assert_eq!(Align::from(HAlign::Left), Align::Left);
        assert_eq!(Align::from(HAlign::Center), Align::Center);
        assert_eq!(Align::from(HAlign::Right), Align::Right);
    }
}
