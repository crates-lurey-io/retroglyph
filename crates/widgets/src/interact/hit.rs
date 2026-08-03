//! [`HitTester`]: resolve a pointer position to the topmost widget id
//! occupying it.

use alloc::vec::Vec;

use retroglyph_core::{Pos, Rect};

/// A per-frame registry of `(Rect, Id)` pairs, queried by pointer position
/// to find the topmost widget under a point.
///
/// Standalone and headless: no [`Backend`](retroglyph_core::Backend)
/// dependency, so it's usable (and unit-testable) without a
/// [`Terminal`](retroglyph_core::Terminal) or any drawing at all, e.g. for
/// hand-rolled hit-testing outside of [`Interaction`](crate::Interaction).
///
/// Registrations are draw-ordered: a later [`push`](Self::push) means drawn
/// (and therefore visually on top) later, so [`topmost_at`](Self::topmost_at)
/// scans back-to-front and returns the *last* match. This mirrors the
/// painter's algorithm every widget in this crate already draws with.
///
/// A [`push_barrier`](Self::push_barrier)ed rect additionally stops that scan: a point inside a
/// barrier's rect never resolves to anything registered before the barrier, regardless of overlap
/// (an overlay region claims everything under it, not just what it happens to draw over), while a
/// point outside the barrier's rect is unaffected by it and keeps scanning past it normally.
#[derive(Debug, Clone)]
pub struct HitTester<Id> {
    hits: Vec<Entry<Id>>,
}

#[derive(Debug, Clone)]
enum Entry<Id> {
    Hit(Rect, Id),
    Barrier(Rect),
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
        self.hits.push(Entry::Hit(rect, id));
    }

    /// Register `rect` as a barrier, on top of everything registered so far this pass: see the
    /// [`HitTester`] docs for what that does to [`topmost_at`](Self::topmost_at).
    pub fn push_barrier(&mut self, rect: Rect) {
        self.hits.push(Entry::Barrier(rect));
    }

    /// Discard all registrations, e.g. at the start of a new frame's draw
    /// pass.
    pub fn clear(&mut self) {
        self.hits.clear();
    }

    /// Number of rects currently registered, hits and barriers combined.
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
    ///
    /// Stops at the first (most recently registered) [`push_barrier`](Self::push_barrier)ed rect
    /// that also contains `pos`: nothing registered earlier than that barrier can win at a
    /// position the barrier covers.
    #[must_use]
    pub fn topmost_at(&self, pos: Pos) -> Option<Id> {
        for entry in self.hits.iter().rev() {
            match entry {
                Entry::Hit(rect, id) if rect.contains_pos(pos) => return Some(*id),
                Entry::Barrier(rect) if rect.contains_pos(pos) => return None,
                Entry::Hit(_, _) | Entry::Barrier(_) => {}
            }
        }
        None
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

    #[test]
    fn a_barrier_hides_hits_registered_before_it_inside_its_own_rect() {
        let mut hits = HitTester::new();
        hits.push(Rect::new(0, 0, 10, 10), "behind");
        hits.push_barrier(Rect::new(2, 2, 4, 4));

        assert_eq!(hits.topmost_at(Pos::new(3, 3)), None); // inside the barrier
        assert_eq!(hits.topmost_at(Pos::new(0, 0)), Some("behind")); // outside the barrier
    }

    #[test]
    fn a_hit_registered_after_a_barrier_still_wins_inside_it() {
        let mut hits = HitTester::new();
        hits.push(Rect::new(0, 0, 10, 10), "behind");
        hits.push_barrier(Rect::new(2, 2, 4, 4));
        hits.push(Rect::new(3, 3, 1, 1), "in front");

        assert_eq!(hits.topmost_at(Pos::new(3, 3)), Some("in front"));
    }

    #[test]
    fn a_barrier_does_not_affect_positions_outside_its_own_rect() {
        let mut hits = HitTester::new();
        hits.push(Rect::new(0, 0, 10, 10), "back");
        hits.push_barrier(Rect::new(2, 2, 2, 2));
        hits.push(Rect::new(5, 5, 3, 3), "front");

        // Outside the barrier's own rect, resolution proceeds normally past it.
        assert_eq!(hits.topmost_at(Pos::new(6, 6)), Some("front"));
        assert_eq!(hits.topmost_at(Pos::new(0, 0)), Some("back"));
    }

    #[test]
    fn an_older_barrier_does_not_shadow_a_newer_ones_uncovered_region() {
        let mut hits = HitTester::new();
        hits.push(Rect::new(0, 0, 10, 10), "back");
        hits.push_barrier(Rect::new(0, 0, 10, 10));
        hits.push(Rect::new(2, 2, 2, 2), "modal widget");

        // The most recent barrier is the modal's own full-area one, registered before its
        // widget: inside the widget's rect, the widget itself wins.
        assert_eq!(hits.topmost_at(Pos::new(2, 2)), Some("modal widget"));
        // Elsewhere inside the modal's barrier but outside its widget: nothing wins, including
        // "back", which sits behind the barrier.
        assert_eq!(hits.topmost_at(Pos::new(8, 8)), None);
    }
}
