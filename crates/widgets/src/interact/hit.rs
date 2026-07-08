//! [`HitTester`]: resolve a pointer position to the topmost widget id
//! occupying it.

use retroglyph_core::{Pos, Rect};

/// A per-frame registry of `(Rect, Id)` pairs, queried by pointer position
/// to find the topmost widget under a point.
///
/// Standalone and headless -- no [`Backend`](retroglyph_core::Backend)
/// dependency, so it's usable (and unit-testable) without a
/// [`Terminal`](retroglyph_core::Terminal) or any drawing at all, e.g. for
/// hand-rolled hit-testing outside of [`Interaction`](crate::Interaction).
///
/// Registrations are draw-ordered: a later [`push`](Self::push) means drawn
/// (and therefore visually on top) later, so [`topmost_at`](Self::topmost_at)
/// scans back-to-front and returns the *last* match. This mirrors the
/// painter's algorithm every widget in this crate already draws with.
#[derive(Debug, Clone)]
pub struct HitTester<Id> {
    hits: Vec<(Rect, Id)>,
}

impl<Id> HitTester<Id> {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self { hits: Vec::new() }
    }

    /// Register `id` as occupying `rect`, on top of everything registered
    /// so far this pass.
    pub fn push(&mut self, rect: Rect, id: Id) {
        self.hits.push((rect, id));
    }

    /// Discard all registrations, e.g. at the start of a new frame's draw
    /// pass.
    pub fn clear(&mut self) {
        self.hits.clear();
    }

    /// Number of rects currently registered.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.hits.len()
    }

    /// `true` if nothing has been registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

impl<Id: Copy> HitTester<Id> {
    /// The id of the topmost (most recently [`push`](Self::push)ed)
    /// registration whose rect contains `pos`, if any.
    #[must_use]
    pub fn topmost_at(&self, pos: Pos) -> Option<Id> {
        self.hits
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains_pos(pos))
            .map(|&(_, id)| id)
    }
}

// Not `#[derive(Default)]`: that would add an unnecessary `Id: Default`
// bound to the generated impl, even though an empty `Vec<(Rect, Id)>` never
// needs one.
impl<Id> Default for HitTester<Id> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topmost_at_prefers_the_most_recently_pushed_overlap() {
        let mut hits = HitTester::new();
        hits.push(Rect::new(0, 0, 10, 10), "back");
        hits.push(Rect::new(5, 5, 10, 10), "front");

        assert_eq!(hits.topmost_at(Pos::new(6, 6)), Some("front")); // overlap
        assert_eq!(hits.topmost_at(Pos::new(1, 1)), Some("back")); // back only
        assert_eq!(hits.topmost_at(Pos::new(20, 20)), None); // neither
    }

    #[test]
    fn clear_empties_the_registry() {
        let mut hits = HitTester::new();
        hits.push(Rect::new(0, 0, 5, 5), 1);
        assert!(!hits.is_empty());
        hits.clear();
        assert!(hits.is_empty());
        assert_eq!(hits.len(), 0);
        assert_eq!(hits.topmost_at(Pos::new(0, 0)), None);
    }

    #[test]
    fn default_is_empty() {
        let hits: HitTester<()> = HitTester::default();
        assert!(hits.is_empty());
    }
}
