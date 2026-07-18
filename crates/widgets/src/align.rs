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
}
