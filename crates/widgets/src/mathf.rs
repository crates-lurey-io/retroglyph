//! `f32::round`/`f32::exp`, but routed through `libm` under `no_std` (retroglyph#882): these are
//! std-only inherent methods, not in `core`, so a `no_std` build has no access to them at all.
//! `std`'s own intrinsics are preferred whenever they're available (matching how `gem`, `core`'s
//! own color-space dependency, already prefers `std`'s float intrinsics over `libm` when both are
//! present) rather than unconditionally routing every build through `libm`.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary
// (`unreachable_pub` agrees) already limits these to the crate, so the two lints disagree on which
// one is redundant. See `crates/gl/src/shaders.rs` for the same conflict.
#![allow(clippy::redundant_pub_crate)]

#[cfg(feature = "std")]
pub(crate) fn round(x: f32) -> f32 {
    x.round()
}

#[cfg(not(feature = "std"))]
pub(crate) fn round(x: f32) -> f32 {
    libm::roundf(x)
}

#[cfg(feature = "std")]
pub(crate) fn exp(x: f32) -> f32 {
    x.exp()
}

#[cfg(not(feature = "std"))]
pub(crate) fn exp(x: f32) -> f32 {
    libm::expf(x)
}
