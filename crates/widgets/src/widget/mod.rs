//! Optional `Widget`/`StatefulWidget` traits over the free-function drawing
//! helpers in [`draw`](crate::draw).
//!
//! Nothing in this crate requires implementing these traits: every existing
//! helper (`panel`, `gauge`, `table`, ...) works exactly as before without
//! them. They exist for callers who want to box/store heterogeneous widgets
//! (e.g. a `Vec<Box<dyn Widget<B>>>` of panes to render each frame) instead
//! of calling free functions directly. Each concrete widget below (one file
//! per widget) is a thin adapter that calls straight through to its
//! corresponding [`crate::draw`] function; it adds no drawing logic of its own.
use retroglyph_core::{Backend, Rect, Terminal};

mod panel;
mod table;

pub use panel::Panel;
pub use table::Table;

/// A type that draws itself into a terminal area, without retaining any
/// state — the minimal shape shared by every widget-like consumer.
pub trait Widget<B: Backend> {
    /// Draw this widget into `area`.
    fn render(self, area: Rect, term: &mut Terminal<B>);
}

/// Like [`Widget`], but for widgets that read (and may update) externally
/// owned state — a selection index, a scroll offset — that outlives a
/// single render call. See [`crate::ListState`].
pub trait StatefulWidget<B: Backend> {
    /// The externally owned state this widget reads and/or updates while
    /// rendering.
    type State;

    /// Draw this widget into `area`, using and/or updating `state`.
    fn render(self, area: Rect, term: &mut Terminal<B>, state: &mut Self::State);
}
