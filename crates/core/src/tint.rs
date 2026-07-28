//! Sprite colour modulation: how a sprite's own pixels are recoloured at draw time.

use gem::rgb::Rgb888;

/// How a sprite's own pixels are recoloured at draw time.
///
/// A sprite is composited from the artwork's pixels, and a cell's
/// [`Style::fg`](crate::Style::fg) does not touch it (see
/// [`Surface::put_span`](crate::Surface::put_span)). A tint is the separate channel that does,
/// so one piece of artwork can serve a biome variant, a damage flash, or a shadowed copy of
/// itself without a second sprite in the sheet.
///
/// Pixel backends only. Cell backends have no sprite to recolour and ignore a tint entirely;
/// they draw the cell's glyph in its own [`Style`](crate::Style), as always.
///
/// # Why not reuse `fg`
///
/// Tinting a sprite by the cell's foreground colour is what most tileset libraries do, and it
/// works for them because their foreground colour has exactly one job and defaults to white, the
/// identity of a multiply.
///
/// Neither holds here. [`Color::Default`](crate::Color::Default) means "whatever foreground the
/// terminal is configured for", not white, so it has no sensible reading as a modulation value.
/// More importantly, a cell drawn as a sprite by a pixel backend is drawn as an `fg`-coloured
/// *glyph* by a cell backend, and the colour that reads correctly as a solid character is not
/// the colour that reads correctly multiplied onto artwork that already has colour of its own.
/// One field cannot serve both.
///
/// # Choosing an operation
///
/// [`Multiply`](Self::Multiply) is the workhorse and can only darken: every channel scales
/// toward zero. It preserves the artwork's own shading, which is what makes it right for
/// variants of one material (grass to savanna, stone to mossy stone) and for lighting.
///
/// [`Mix`](Self::Mix) blends toward a colour and can therefore brighten, which multiply cannot
/// express at all. It is also the only one of the two a caller could not approximate for
/// themselves, since doing so needs the sprite's pixels. `Mix` at full strength replaces the
/// artwork's colour outright while keeping its alpha, which is how a white-on-transparent mask
/// sheet gets recoloured.
///
/// Alpha is never touched by either: a tint changes what the sprite's opaque pixels look like,
/// never which of them are opaque. Compositing and the cell background showing through
/// transparent pixels behave identically tinted or not.
///
/// # Scope: what `Tint` is not for
///
/// `Tint` is deliberately per-cell and per-draw, not per-sheet or per-frame. "Is this sheet art
/// or a mask" is a different, fixed-at-load-time question, answered once by
/// `retroglyph_window::tileset::SheetColor` rather than by this type. The two compose instead of
/// collapsing into one flag (see `retroglyph_window::sprite_cache::SpriteTint`, which resolves
/// both in one place), because "is this sheet art or a mask" (fixed when the asset is authored)
/// and "what colour to flash this cell right now" (fixed per frame) are different questions that
/// would conflict if merged into a single `modulate(bool)`-style flag: a sheet declared
/// art-not-mask still needs to be flashable.
///
/// Frame- or layer-level colour transforms -- day/night cycles, fog of war, a "remembered" map
/// render -- are not a use case for `Tint` either. Those apply to everything already drawn,
/// every frame, so routing them through per-cell `Tint` would mean writing the same value into a
/// side-table entry for every cell of every layer, every frame: the wrong lever for a
/// screen-wide effect. That is tracked as its own, not-yet-designed concern in retroglyph#562;
/// it is out of scope here.
///
/// `Tint` is `#[non_exhaustive]` so more operations (add, screen, replace) can be added later
/// without breaking either backend: the GL encoder already falls through to "no recolour" on an
/// operation it does not recognize.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Tint;
///
/// // Grass artwork, dimmed toward its own shadow.
/// let shadowed = Tint::multiply(128, 128, 128);
/// assert_eq!(shadowed.apply((200, 180, 60)), (100, 90, 30));
///
/// // The same pixels, flashed most of the way to white.
/// let hit = Tint::mix(255, 255, 255, 192);
/// assert_eq!(hit.apply((200, 180, 60)), (241, 236, 207));
///
/// // The default costs nothing and changes nothing.
/// assert_eq!(Tint::None.apply((200, 180, 60)), (200, 180, 60));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Tint {
    /// Composite the sprite's pixels verbatim.
    #[default]
    None,
    /// Scale each channel by `rgb / 255`, darkening toward black.
    ///
    /// `(255, 255, 255)` is the identity and behaves as [`None`](Self::None), just less cheaply.
    Multiply {
        /// Red scale factor.
        r: u8,
        /// Green scale factor.
        g: u8,
        /// Blue scale factor.
        b: u8,
    },
    /// Blend each channel `amount / 255` of the way toward `rgb`.
    ///
    /// `amount` of 0 is the identity; 255 replaces the sprite's colour outright, keeping its
    /// alpha.
    Mix {
        /// Red channel of the colour blended toward.
        r: u8,
        /// Green channel of the colour blended toward.
        g: u8,
        /// Blue channel of the colour blended toward.
        b: u8,
        /// How far to blend, from 0 (unchanged) to 255 (fully replaced).
        amount: u8,
    },
}

impl Tint {
    /// A [`Multiply`](Self::Multiply) tint scaling each channel by `rgb / 255`.
    #[must_use]
    pub const fn multiply(r: u8, g: u8, b: u8) -> Self {
        Self::Multiply { r, g, b }
    }

    /// A [`Mix`](Self::Mix) tint blending `amount / 255` of the way toward `rgb`.
    #[must_use]
    pub const fn mix(r: u8, g: u8, b: u8, amount: u8) -> Self {
        Self::Mix { r, g, b, amount }
    }

    /// Whether this tint leaves every pixel exactly as authored.
    ///
    /// True for [`None`](Self::None) and for the identity of either operation, so a renderer can
    /// take its untinted fast path for a tint that would do nothing.
    #[must_use]
    pub const fn is_identity(self) -> bool {
        match self {
            Self::None => true,
            Self::Multiply { r, g, b } => r == 255 && g == 255 && b == 255,
            Self::Mix { amount, .. } => amount == 0,
        }
    }

    /// Applies this tint to one straight-alpha RGB triple, returning the recoloured channels.
    ///
    /// The reference implementation of the operation. Both pixel backends produce their output
    /// from this, the software renderer by calling it per pixel and the GL renderer by matching
    /// its arithmetic in the sprite fragment shader, so that a sprite tinted on one backend
    /// matches the same sprite tinted on the other.
    ///
    /// Alpha is not an input and not an output: a tint never changes which pixels are opaque.
    #[must_use]
    pub const fn apply(self, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
        use gem::channel::{mix_u8, multiply_u8};
        let (sr, sg, sb) = rgb;
        match self {
            Self::None => (sr, sg, sb),
            Self::Multiply { r, g, b } => {
                (multiply_u8(sr, r), multiply_u8(sg, g), multiply_u8(sb, b))
            }
            Self::Mix { r, g, b, amount } => (
                mix_u8(sr, r, amount),
                mix_u8(sg, g, amount),
                mix_u8(sb, b, amount),
            ),
        }
    }

    /// Applies this tint to an [`Rgb888`], the same operation as [`apply`](Self::apply) but
    /// without the channel-order round trip through a bare `(u8, u8, u8)` tuple.
    ///
    /// `const` because [`Rgb888::to_rgb`][gem::rgb::Rgb::to_rgb] is a const inherent method
    /// (gem 0.2.0): the equivalent by way of the [`HasRed`](gem::rgb::HasRed)-family traits
    /// cannot be, since trait methods aren't const-callable on stable.
    ///
    /// ```rust
    /// use retroglyph_core::Tint;
    /// use gem::rgb::Rgb888;
    ///
    /// const PX: Rgb888 = Tint::Multiply { r: 128, g: 128, b: 128 }
    ///     .apply_rgb888(Rgb888::from_rgb(200, 180, 60));
    /// assert_eq!(PX, Rgb888::from_rgb(100, 90, 30));
    /// ```
    #[must_use]
    pub const fn apply_rgb888(self, px: Rgb888) -> Rgb888 {
        let (r, g, b) = self.apply(px.to_rgb());
        Rgb888::from_rgb(r, g, b)
    }

    /// A [`Multiply`](Self::Multiply) tint by `color`'s resolved RGB, falling back to `default`
    /// for [`Color::Default`](crate::Color::Default) (which has no intrinsic reading as a
    /// modulation value). Built for `retroglyph-window`'s sheet-level recolouring: a
    /// `SheetColor::Mask` sheet is tinted by the cell's own foreground colour this way before
    /// the cell's own [`Tint`] is applied on top.
    #[must_use]
    pub const fn multiply_color(c: crate::color::Color, default: (u8, u8, u8)) -> Self {
        let (r, g, b) = c.resolve_rgb(default);
        Self::multiply(r, g, b)
    }
}

#[cfg(test)]
mod tests {
    use super::Tint;

    #[test]
    fn none_is_the_identity() {
        assert_eq!(Tint::None.apply((13, 200, 255)), (13, 200, 255));
        assert!(Tint::None.is_identity());
        assert_eq!(Tint::default(), Tint::None);
    }

    #[test]
    fn multiply_by_white_is_exact() {
        let white = Tint::multiply(255, 255, 255);
        for c in [0u8, 1, 63, 127, 128, 200, 254, 255] {
            assert_eq!(white.apply((c, c, c)), (c, c, c), "channel {c}");
        }
        assert!(white.is_identity());
    }

    #[test]
    fn multiply_by_black_is_black() {
        assert_eq!(Tint::multiply(0, 0, 0).apply((200, 180, 60)), (0, 0, 0));
    }

    #[test]
    fn multiply_scales_per_channel() {
        // Only the green channel is scaled; the others pass through untouched.
        let green_only = Tint::multiply(255, 128, 255);
        assert_eq!(green_only.apply((200, 200, 200)), (200, 100, 200));
    }

    #[test]
    fn multiply_can_only_darken() {
        let t = Tint::multiply(200, 200, 200);
        for c in 0..=255u8 {
            let (r, _, _) = t.apply((c, c, c));
            assert!(r <= c, "multiply brightened {c} to {r}");
        }
    }

    #[test]
    fn mix_endpoints_are_exact() {
        let src = (200, 180, 60);
        assert_eq!(Tint::mix(255, 255, 255, 0).apply(src), src);
        assert_eq!(Tint::mix(255, 255, 255, 255).apply(src), (255, 255, 255));
        assert_eq!(Tint::mix(0, 0, 0, 255).apply(src), (0, 0, 0));
    }

    #[test]
    fn mix_at_zero_amount_is_identity_for_every_colour() {
        assert!(Tint::mix(1, 2, 3, 0).is_identity());
        assert_eq!(Tint::mix(1, 2, 3, 0).apply((9, 9, 9)), (9, 9, 9));
    }

    #[test]
    fn mix_halfway_is_the_midpoint() {
        // 128/255 is a hair over half, so a 0 -> 254 blend lands on 127.
        assert_eq!(
            Tint::mix(254, 254, 254, 128).apply((0, 0, 0)),
            (127, 127, 127)
        );
    }

    #[test]
    fn mix_can_brighten_which_multiply_cannot() {
        let src = (10, 10, 10);
        let (r, _, _) = Tint::mix(255, 255, 255, 128).apply(src);
        assert!(r > 10, "mix toward white should brighten, got {r}");
    }

    #[test]
    fn mix_rounds_symmetrically_in_both_directions() {
        // Blending 100 toward 200 and 200 toward 100 by the same amount should move each the
        // same distance, otherwise a pulsing flash would drift.
        let up = Tint::mix(200, 200, 200, 64).apply((100, 100, 100)).0;
        let down = Tint::mix(100, 100, 100, 64).apply((200, 200, 200)).0;
        assert_eq!(up - 100, 200 - down);
    }

    #[test]
    fn identity_variants_never_change_a_pixel() {
        let src = (37, 211, 4);
        for t in [
            Tint::None,
            Tint::multiply(255, 255, 255),
            Tint::mix(0, 0, 0, 0),
            Tint::mix(255, 255, 255, 0),
        ] {
            assert!(t.is_identity(), "{t:?} should report as identity");
            assert_eq!(t.apply(src), src, "{t:?} changed a pixel");
        }
    }

    #[test]
    fn apply_is_usable_in_const_context() {
        const SHADOWED: (u8, u8, u8) = Tint::multiply(128, 128, 128).apply((200, 180, 60));
        assert_eq!(SHADOWED, (100, 90, 30));
    }

    // The values in this type's doc example are worked arithmetic, not round numbers, so they
    // are easy to write by hand and get wrong. Pin them here: a doctest catches a stale example
    // only when someone runs the doctests, and this keeps the two in one place.
    #[test]
    fn doc_example_values_are_what_apply_actually_returns() {
        let src = (200, 180, 60);
        assert_eq!(Tint::multiply(128, 128, 128).apply(src), (100, 90, 30));
        assert_eq!(Tint::mix(255, 255, 255, 192).apply(src), (241, 236, 207));
        assert_eq!(Tint::None.apply(src), src);
    }
}
