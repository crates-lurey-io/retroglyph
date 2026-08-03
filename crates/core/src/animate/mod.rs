//! Time-driven value animations.

mod easing;
mod oscillate;
mod tween;

pub use easing::Easing;
pub use oscillate::{oscillate, oscillate_with_phase};
pub use tween::Tween;
