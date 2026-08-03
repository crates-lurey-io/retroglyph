//! Conformance test for two upstream crates that retroglyph merely happens to be the meeting
//! point for. It lives here because there is no dependency edge between gem and alpha-blend,
//! so this is the only place the shared invariant can be mechanically enforced.
//! Do not "move it where it belongs": there is nowhere else it would run.
//!
//! Every integer channel operation in gem and alpha-blend is round-to-nearest, ties away from
//! zero, rounded exactly once from a widened intermediate (retroglyph#547 SS0). This does not
//! just check the two crates agree with each other: that's retroglyph#537, where two backends
//! agreed by accident and the agreement read as intent. It checks both independently against an
//! `i64` rational reference that uses no `/255` shortcuts and no shifts.
//!
//! `alpha-blend` is a non-optional dependency of `retroglyph-core` (see `Cargo.toml`), so this
//! runs on every CI invocation, regardless of the feature matrix.

use alpha_blend::channel::Channel;
#[cfg(feature = "indexed-quant")]
use gem::space::Srgb;
#[cfg(feature = "indexed-quant")]
use retroglyph_core::Color;

/// Rounds `num / den` to the nearest integer, ties away from zero, using only `i64` arithmetic
/// (no `/255` shortcuts, no shifts): the independent reference every rounding claim in this test
/// is checked against.
fn round_half_away(num: i64, den: i64) -> i64 {
    debug_assert!(den > 0);
    if num >= 0 {
        (2 * num + den) / (2 * den)
    } else {
        -((-2 * num + den) / (2 * den))
    }
}

/// `gem::channel::multiply_u8` and `alpha_blend::channel::Channel::scale` both compute
/// `a * b / 255`; both must equal `round_half_away(a * b, 255)`, for every one of the 65536
/// reachable `(a, b)` pairs.
#[test]
fn multiply_and_scale_are_round_to_nearest_for_every_input() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            let want = round_half_away(i64::from(a) * i64::from(b), 255);

            assert_eq!(
                i64::from(gem::channel::multiply_u8(a, b)),
                want,
                "gem::channel::multiply_u8({a}, {b})"
            );
            assert_eq!(
                i64::from(Channel::scale(a, b)),
                want,
                "alpha_blend::channel::Channel::scale({a}, {b})"
            );
        }
    }
}

/// `Color::from_srgb` (retroglyph#759, follow-up to retroglyph#698) must round each channel to
/// the nearest `u8`, not truncate. Truncation and rounding only disagree when a channel's
/// `* 255.0` value has a fractional part >= 0.5, so for every target byte `t` in `1..=255` this
/// picks an input whose scaled value is `t - 0.4` (fractional part `0.6`): round-to-nearest
/// yields `t`, while the pre-#698 `as u8` truncation yields `t - 1`.
#[cfg(feature = "indexed-quant")]
#[test]
fn from_srgb_rounds_to_nearest_not_truncates() {
    for t in 1u8..=255 {
        let scaled = f32::from(t) - 1.0 + 0.6;
        let srgb = Srgb::new(scaled / 255.0, scaled / 255.0, scaled / 255.0);
        let Color::Rgb { r, g, b } = Color::from_srgb(srgb) else {
            panic!("from_srgb must always return Color::Rgb");
        };
        assert_eq!((r, g, b), (t, t, t), "from_srgb({scaled} / 255.0)");
    }
}

/// `gem::channel::mix_u8` and `<u8 as Channel>::lerp` compute the same interpolation by
/// differently-shaped formulas (retroglyph#547 SS0: `a + (b - a) * t / 255`, signed, vs.
/// `a * (255 - t) + b * t`, then `/ 255`); both must equal an independent `i64` reference derived
/// from the same `a + (b - a) * t` shape, over all 16,777,216 `(a, b, t)` triples. ~0.3s in debug
/// on Apple Silicon; not gated to release and not sampled, per retroglyph#547.
#[test]
fn mix_and_lerp_are_round_to_nearest_for_every_input() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            let (ai, bi) = (i64::from(a), i64::from(b));
            for t in 0..=255u8 {
                let want = ai + round_half_away((bi - ai) * i64::from(t), 255);

                assert_eq!(
                    i64::from(gem::channel::mix_u8(a, b, t)),
                    want,
                    "gem::channel::mix_u8({a}, {b}, {t})"
                );
                assert_eq!(
                    i64::from(Channel::lerp(a, b, t)),
                    want,
                    "<u8 as Channel>::lerp({a}, {b}, {t})"
                );
            }
        }
    }
}
