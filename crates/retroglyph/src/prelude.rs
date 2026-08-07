//! The names a `retroglyph` program cannot avoid, glob-importable in one line
//! (`use retroglyph::prelude::*;`): the `App` contract, the trait a backend's own builder
//! implements to be [`launch`](Launch::launch)ed, the two style primitives, the input event/key
//! types, and the geometry aliases every `Surface`/`Backend` call takes or returns.
//!
//! Everything here also lives at its own module path (`retroglyph::app::App`,
//! `retroglyph::color::Style`, ...); the prelude is purely a convenience re-export, not a
//! separate surface.

pub use crate::app::{App, Flow, Frame, Launch};
pub use crate::color::{Color, Style};
pub use crate::event::{Event, KeyCode};
pub use crate::grid::{Pos, Rect, Size};
pub use crate::terminal::Terminal;
