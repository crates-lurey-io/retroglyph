//! Time-driven value animation: [`Easing`] curves, a stateful, retargetable [`Tween`], and a
//! periodic [`oscillate`] helper.
//!
//! [`FrameClock`](crate::FrameClock) tracks how many fixed logic steps a frame is worth. This
//! module answers a different question: what is one `f32` value right now, partway through
//! moving from A to B (or, for [`oscillate`], partway through a wave that never starts or ends)?
//! There are two tools, for two kinds of motion:
//!
//! - [`Tween`]: moves from one value to another over a fixed duration, shaped by an [`Easing`]
//!   curve. Use it for things that start, run once, and stop, such as a fade-in or a value
//!   settling on a new target.
//! - [`oscillate`]: a repeating wave with no start or end. Use it for things that keep going,
//!   such as a pulsing indicator, a breathing effect, or the demo signal in gallery example 11.
//!
//! Both follow the same explicit, app-owned state convention as
//! [`ListState`](https://docs.rs/retroglyph-widgets/latest/retroglyph_widgets/struct.ListState.html)
//! and [`Interaction`](https://docs.rs/retroglyph-widgets/latest/retroglyph_widgets/struct.Interaction.html):
//! a plain struct the caller constructs and stores itself, updated once per frame with
//! [`Frame::delta`](crate::Frame::delta), rather than a hidden global animation manager keyed by
//! an id the way egui's `Context::animate_value` works. One [`Tween`] animates one `f32`; an app
//! with several needs several `Tween`s, the same as it needs several `ListState`s for several
//! lists.
//!
//! Split across `easing.rs`/`tween.rs`/`oscillate.rs` (this file just re-exports), one file per
//! concept, since `Tween` builds directly on `Easing` and `oscillate` is a standalone helper.

mod easing;
mod oscillate;
mod tween;

pub use easing::Easing;
pub use oscillate::oscillate;
pub use tween::Tween;
