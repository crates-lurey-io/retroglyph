//! Reusable widget state, kept separate from drawing.
//!
//! Widgets in this crate are free functions with no retained state (see the
//! crate docs). But *something* still has to remember which item is
//! selected and how far a list has scrolled between frames -- that's app
//! state, not widget state, and [`ListState`] is a small, tested, headless
//! (no [`Backend`](retroglyph_core::Backend) dependency) building block for
//! it so every consumer doesn't hand-roll its own selection-cursor math.

mod list;
mod scroll;

pub use list::{ListState, SelectionWrap};
pub use scroll::{ScrollPhysics, ScrollState};
