//! Float ops [`animate`](super::animate) and [`grid`](super::grid)'s separable blend math need but
//! the core language doesn't provide outside `std`: dispatched to `std`'s own intrinsics when the
//! `std` feature is on, and to `libm`'s software implementation otherwise.
//!
//! One of `std`/`libm` is required crate-wide (see the `compile_error!` in `lib.rs`), so every
//! function here can assume a backend is present -- there is no third "no float backend" case for
//! these to handle.
//!
//! [`exp`] has no caller in this crate: it's here for `retroglyph-widgets`, which shares this
//! module (`#[doc(hidden)] pub`, see its declaration in `lib.rs`) instead of vendoring its own
//! copy. Don't remove it as unused.
//!
//! `std` and `libm`'s trig/pow functions are not bit-identical (`libm` is a pure-Rust port,
//! platform `libm`s vary by ULP), so [`sin`]/[`cos`]/[`powf`] can disagree by a ULP between a
//! `std` and a `libm` build of the same call. [`mul_add`]/[`round`] have no such gap: `f32::round`
//! and `libm::roundf` are bit-identical, and so are `f32::mul_add` and `libm::fmaf` (both lower to
//! a single fused multiply-add instruction where the target has one).

#[inline]
#[must_use]
/// `a * b + c`, as one rounding step (a fused multiply-add) rather than two.
pub fn mul_add(a: f32, b: f32, c: f32) -> f32 {
    #[cfg(feature = "std")]
    return f32::mul_add(a, b, c);
    #[cfg(not(feature = "std"))]
    return libm::fmaf(a, b, c);
}

#[inline]
#[must_use]
/// Rounds to the nearest integer, ties away from zero.
pub fn round(x: f32) -> f32 {
    #[cfg(feature = "std")]
    return f32::round(x);
    #[cfg(not(feature = "std"))]
    return libm::roundf(x);
}

#[inline]
#[must_use]
/// Sine of `x`, in radians.
pub fn sin(x: f32) -> f32 {
    #[cfg(feature = "std")]
    return f32::sin(x);
    #[cfg(not(feature = "std"))]
    return libm::sinf(x);
}

#[inline]
#[must_use]
/// Cosine of `x`, in radians.
pub fn cos(x: f32) -> f32 {
    #[cfg(feature = "std")]
    return f32::cos(x);
    #[cfg(not(feature = "std"))]
    return libm::cosf(x);
}

#[inline]
#[must_use]
/// `x` raised to the power `y`.
pub fn powf(x: f32, y: f32) -> f32 {
    #[cfg(feature = "std")]
    return f32::powf(x, y);
    #[cfg(not(feature = "std"))]
    return libm::powf(x, y);
}

#[inline]
#[must_use]
/// `e` raised to the power `x`.
pub fn exp(x: f32) -> f32 {
    #[cfg(feature = "std")]
    return f32::exp(x);
    #[cfg(not(feature = "std"))]
    return libm::expf(x);
}
