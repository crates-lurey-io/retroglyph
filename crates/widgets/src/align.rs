//! [`Align`]: horizontal alignment of a single line of text within a
//! fixed-width area.

/// Horizontal alignment of one line of text within the columns it's rendered
/// into.
///
/// A builder knob on the single-line text widgets ([`Text`](crate::Text),
/// [`PrintLine`](crate::PrintLine)) and on the titles of [`Panel`](crate::Panel)
/// and [`Modal`](crate::Modal). Text widgets default to `Left` (their
/// long-standing behavior); panel/modal titles default to `Center` (theirs).
///
/// A plain re-export of [`retroglyph_core::layout::HAlign`], not a separate type: `core::align`
/// needs nothing from the `egc` feature, so there's no reason for `widgets` to keep its own
/// copy of the enum or of [`offset`](retroglyph_core::layout::HAlign::offset)'s formula.
/// Interoperates directly with [`Surface::print_aligned`](retroglyph_core::Surface::print_aligned)
/// and [`TextLayout`](retroglyph_core::layout::TextLayout), no conversion needed.
pub use retroglyph_core::layout::HAlign as Align;

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

    #[test]
    fn is_the_same_type_as_core_h_align() {
        // No `From` conversion needed: `Align` and `HAlign` are the same type.
        let align: Align = retroglyph_core::layout::HAlign::Center;
        assert_eq!(align, Align::Center);
    }
}
