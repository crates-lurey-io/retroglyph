//! No-drift regression test for retroglyph#547.
//!
//! The bug: `alpha_blend::rgba::U8x4Rgba::source_over` used to compute `floor(v / 255)` per
//! channel (via the `(v + (v >> 8) + 1) >> 8` shift trick) instead of rounding to nearest, and
//! also assumed an opaque destination. Compositing the same translucent sprite onto the same
//! buffer repeatedly without clearing between draws -- a trailing/ghosting accumulation, not an
//! unusual thing for a renderer to do -- exposed both: the color channels drifted darker than the
//! true fixed point on every pass, and the destination alpha never converged to opaque at all.
//!
//! `alpha-blend` is a non-optional dev-dependency of `retroglyph-core` specifically so this test
//! runs on every CI invocation, independent of the feature matrix (see `Cargo.toml`).

use alpha_blend::rgba::U8x4Rgba;

/// Composites a half-alpha green pixel onto the same destination 64 times without clearing it,
/// and asserts the result is bit-identical to the sprite's own opaque color: not one LSB darker
/// (the floor bug), and not stuck at a non-opaque alpha (the pre-0.3.0 translucent-destination
/// bug, fixed in the same release).
#[test]
fn repeated_translucent_composite_converges_to_the_exact_fixed_point_not_a_darker_one() {
    const PASSES: usize = 64;

    let src = U8x4Rgba::new(0, 200, 0, 128);
    let mut dst = U8x4Rgba::new(255, 255, 255, 255);

    let mut reached_fixed_point_by = None;
    for i in 0..PASSES {
        let next = src.source_over(dst);
        if next == dst && reached_fixed_point_by.is_none() {
            reached_fixed_point_by = Some(i);
        }
        dst = next;
    }

    assert!(
        reached_fixed_point_by.is_some(),
        "never reached a stable fixed point in {PASSES} composites: last = {dst:?}"
    );
    // The exact fixed point of repeatedly compositing `src` over its own prior output is `src`
    // itself, fully opaque -- not a color biased darker by repeated floor rounding, and not an
    // alpha channel stuck below 255 (the separate bug also fixed in alpha-blend 0.3.0).
    assert_eq!(dst, U8x4Rgba::new(0, 200, 0, 255));
}

/// Same shape, swept over enough distinct colors and alphas that a reintroduced floor would show
/// up as *some* channel settling one step darker than the source, even if a single hand-picked
/// case happened to round the same way under both conventions.
///
/// Alpha stays at 128 and above: each non-clearing pass shrinks the remaining distance to `src`
/// by a factor of `(1 - a / 255)`, so a smaller alpha takes more than 64 passes to fully converge
/// to the exact fixed point -- that's a convergence-rate fact about repeated compositing (see
/// `repeated_translucent_composite_converges...` above for what "eventually" looks like at
/// `a == 128`), not the bug this test is for.
#[test]
fn repeated_composite_never_settles_darker_than_the_source_for_any_color() {
    for r in [0u8, 1, 17, 63, 128, 200, 254, 255] {
        for a in [128u8, 150, 191, 224, 254] {
            let src = U8x4Rgba::new(r, 255 - r, r / 2, a);
            let mut dst = U8x4Rgba::new(255, 0, 255, 255);
            for _ in 0..64 {
                dst = src.source_over(dst);
            }
            assert_eq!(
                dst,
                U8x4Rgba::new(r, 255 - r, r / 2, 255),
                "src={src:?} settled at {dst:?}, expected the source's own color"
            );
        }
    }
}
