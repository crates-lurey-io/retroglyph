//! Reusable widget state, kept separate from drawing.
//!
//! Widgets in this crate are free functions with no retained state (see the
//! crate docs). But *something* still has to remember which item is
//! selected and how far a list has scrolled between frames -- that's app
//! state, not widget state, and [`ListState`] is a small, tested, headless
//! (no [`Backend`](retroglyph_core::Backend) dependency) building block for
//! it so every consumer doesn't hand-roll its own wraparound-cursor math.

/// Selection index and scroll offset for a selectable, scrollable list.
///
/// Holds no reference to the list's actual items: `len` is passed in to each
/// mutating method, so the same `ListState` can be reused across lists that
/// change size (menus, reward pools, deck views, ...) without going stale.
///
/// Selection movement wraps around `len` (pressing "next" past the last item
/// lands on the first, and vice versa), matching the cursor behavior common
/// to menu-driven TUIs. Scrolling is a separate, unbounded-above counter
/// (clamped only at zero) since only the caller knows the content length and
/// viewport height needed to clamp it from above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListState {
    selected: Option<usize>,
    offset: usize,
}

impl ListState {
    /// An empty state: nothing selected, no scroll.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: None,
            offset: 0,
        }
    }

    /// The currently selected index, if any.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The current scroll offset (index of the first visible item/line).
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Select an explicit index (or clear the selection with `None`).
    pub const fn select(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Set the scroll offset directly.
    pub const fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// Clear both the selection and the scroll offset, e.g. after the
    /// underlying list has been replaced with different content.
    pub const fn reset(&mut self) {
        self.selected = None;
        self.offset = 0;
    }

    /// Nudge the scroll offset by the minimum amount needed to bring
    /// `selected` into the `visible_height`-row window starting at `offset`.
    ///
    /// A no-op if nothing is selected, `visible_height` is zero, or the
    /// selection is already visible. Call this once per frame before
    /// rendering (with the actual, current viewport height, since that can
    /// change on terminal resize) rather than only after moving the
    /// selection -- it's cheap and idempotent, so redoing it every frame
    /// costs nothing and needs no special-casing for resize.
    pub const fn ensure_visible(&mut self, visible_height: usize) {
        let Some(selected) = self.selected else {
            return;
        };
        if visible_height == 0 {
            return;
        }
        if selected < self.offset {
            self.offset = selected;
        } else if selected >= self.offset + visible_height {
            self.offset = selected + 1 - visible_height;
        }
    }

    /// Move the scroll offset by `delta`, clamped at zero. There is no upper
    /// clamp here: only the caller knows the content length and viewport
    /// height needed to bound it from above.
    pub fn scroll_by(&mut self, delta: i32) {
        let next = i64::from(delta).saturating_add(i64::try_from(self.offset).unwrap_or(i64::MAX));
        self.offset = next.max(0).try_into().unwrap_or(usize::MAX);
    }

    /// Select the next item, wrapping past the end back to the first.
    /// Selects index 0 if nothing was selected yet. No-op (clears the
    /// selection) if `len` is zero.
    pub fn select_next(&mut self, len: usize) {
        self.selected = Self::wrapped(self.selected, 1, len);
    }

    /// Select the previous item, wrapping past the start back to the last.
    /// Selects the last item if nothing was selected yet. No-op (clears the
    /// selection) if `len` is zero.
    pub fn select_previous(&mut self, len: usize) {
        self.selected = Self::wrapped(self.selected, -1, len);
    }

    /// Select the first item, or clear the selection if `len` is zero.
    pub fn select_first(&mut self, len: usize) {
        self.selected = (len > 0).then_some(0);
    }

    /// Select the last item, or clear the selection if `len` is zero.
    pub fn select_last(&mut self, len: usize) {
        self.selected = (len > 0).then(|| len - 1);
    }

    /// Shared wraparound math for `select_next`/`select_previous`. `delta`
    /// is `1` or `-1`; a missing selection starts from the end opposite the
    /// direction of travel so the first press lands somewhere sensible.
    fn wrapped(current: Option<usize>, delta: i32, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let Ok(len) = i32::try_from(len) else {
            return current; // absurdly large len; leave selection alone
        };
        let base = current.map_or(if delta > 0 { -1 } else { 0 }, |i| {
            i32::try_from(i).unwrap_or(0)
        });
        Some(usize::try_from((base + delta).rem_euclid(len)).unwrap_or(0))
    }
}

/// Configurable physics constants for [`ScrollState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollPhysics {
    /// Exponential friction decay constant. Higher means faster deceleration.
    pub friction: f32,
    /// Stiffness of the overscroll spring.
    pub stiffness: f32,
    /// Damping of the overscroll spring.
    pub damping: f32,
    /// Maximum rows/cells the viewport can be rubber-banded past the edge.
    pub rubber_band_limit: f32,
}

impl ScrollPhysics {
    /// The default scroll physics parameters as a constant.
    pub const DEFAULT: Self = Self {
        friction: 4.5,
        stiffness: 180.0,
        damping: 24.0,
        rubber_band_limit: 4.0,
    };
}

impl Default for ScrollPhysics {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Scroll state for smooth, momentum-based scrolling with rubber-banding.
///
/// Keeps track of the current fractional scroll offset, velocity, and
/// drag-to-scroll gestures. Completely separate from drawing, and generic over
/// time: takes a time delta step to decay velocity or animate snap-back,
/// making it deterministic and suitable for unit tests.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollState {
    offset: f32,
    velocity: f32,
    dragging: bool,
    time_accumulator: f32,
    last_pointer_y: f32,
    samples: [Option<(f32, f32)>; 4],
    samples_idx: usize,
    physics: ScrollPhysics,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::suboptimal_flops)]
impl ScrollState {
    /// Create a new `ScrollState` at offset 0.0 with default physics.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            offset: 0.0,
            velocity: 0.0,
            dragging: false,
            time_accumulator: 0.0,
            last_pointer_y: 0.0,
            samples: [None; 4],
            samples_idx: 0,
            physics: ScrollPhysics::DEFAULT,
        }
    }

    /// Create a new `ScrollState` with custom physics.
    #[must_use]
    pub const fn with_physics(physics: ScrollPhysics) -> Self {
        Self {
            offset: 0.0,
            velocity: 0.0,
            dragging: false,
            time_accumulator: 0.0,
            last_pointer_y: 0.0,
            samples: [None; 4],
            samples_idx: 0,
            physics,
        }
    }

    /// The current fractional scroll offset.
    #[must_use]
    pub const fn offset(&self) -> f32 {
        self.offset
    }

    /// Set the offset directly, clamping it to bounds.
    pub const fn set_offset(&mut self, offset: f32, max_offset: f32) {
        let max = if max_offset > 0.0 { max_offset } else { 0.0 };
        self.offset = if offset < 0.0 {
            0.0
        } else if offset > max {
            max
        } else {
            offset
        };
        self.velocity = 0.0;
    }

    /// The current velocity in items/second.
    #[must_use]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Whether a drag gesture is currently active.
    #[must_use]
    pub const fn dragging(&self) -> bool {
        self.dragging
    }

    /// Returns the integer part of the offset, clamped to positive.
    #[must_use]
    pub fn integer_offset(&self) -> usize {
        if self.offset < 0.0 {
            0
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                self.offset as usize
            }
        }
    }

    /// Returns the fractional remainder of the offset (0.0..1.0).
    #[must_use]
    pub fn fractional_offset(&self) -> f32 {
        if self.offset < 0.0 {
            self.offset
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let int_part = self.offset as usize as f32;
            self.offset - int_part
        }
    }

    /// Update physics for a single frame step.
    ///
    /// Decays momentum if in bounds, or animates the rubber-band spring back to
    /// boundaries if out of bounds. Has no effect if dragging is active.
    #[allow(clippy::while_float)]
    pub fn tick(&mut self, dt: core::time::Duration, max_offset: f32) {
        let dt_secs = dt.as_secs_f32();
        if dt_secs <= 0.0 {
            return;
        }
        self.time_accumulator += dt_secs;

        if self.dragging {
            return;
        }

        let max_offset = max_offset.max(0.0);
        let max_step = 0.008; // 8ms maximum step size for stable spring integration
        let mut remaining = dt_secs;

        while remaining > 0.0 {
            let step = remaining.min(max_step);
            remaining -= step;

            if self.offset >= 0.0 && self.offset <= max_offset {
                // In bounds: apply friction decay
                self.velocity *= f32::exp(-self.physics.friction * step);
                self.offset += self.velocity * step;

                // Stop moving if velocity becomes tiny
                if self.velocity.abs() < 0.05 {
                    self.velocity = 0.0;
                }
            } else {
                // Out of bounds: apply spring snapback force
                let target = if self.offset < 0.0 { 0.0 } else { max_offset };
                let overshoot = self.offset - target;

                let force = -overshoot * self.physics.stiffness;
                let damping_force = -self.velocity * self.physics.damping;
                let acceleration = force + damping_force;

                self.velocity += acceleration * step;
                self.offset += self.velocity * step;

                // Snap when close enough to target and nearly stopped
                if (self.offset - target).abs() < 0.01 && self.velocity.abs() < 0.2 {
                    self.offset = target;
                    self.velocity = 0.0;
                    break;
                }
            }
        }
    }

    /// Begin a drag gesture at pointer coordinate `y`.
    pub const fn begin_drag(&mut self, y: f32) {
        self.dragging = true;
        self.velocity = 0.0;
        self.last_pointer_y = y;
        self.samples = [None; 4];
        self.samples_idx = 0;
        self.record_sample(self.time_accumulator, y);
    }

    /// Update the drag gesture with a new pointer coordinate `y`.
    pub fn update_drag(&mut self, y: f32, max_offset: f32) {
        if !self.dragging {
            self.begin_drag(y);
            return;
        }

        let mut delta_y = self.last_pointer_y - y; // dragging UP increases offset
        let max_offset = max_offset.max(0.0);
        let proposed = self.offset + delta_y;

        // Apply rubber-band resistance when dragging past boundaries
        if proposed < 0.0 && delta_y < 0.0 {
            let overshoot = if self.offset < 0.0 {
                -self.offset
            } else {
                -proposed / 2.0
            };
            let resistance = (1.0 - overshoot / self.physics.rubber_band_limit).clamp(0.0, 1.0);
            delta_y *= resistance;
        } else if proposed > max_offset && delta_y > 0.0 {
            let overshoot = if self.offset > max_offset {
                self.offset - max_offset
            } else {
                (proposed - max_offset) / 2.0
            };
            let resistance = (1.0 - overshoot / self.physics.rubber_band_limit).clamp(0.0, 1.0);
            delta_y *= resistance;
        }

        self.offset += delta_y;
        self.last_pointer_y = y;
        self.record_sample(self.time_accumulator, y);
    }

    /// End the current drag gesture, initiating a fling if pointer speed was sufficient.
    pub fn end_drag(&mut self) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        self.velocity = self.calculate_fling_velocity();
    }

    /// Apply a scroll wheel impulse directly to velocity.
    pub fn scroll_by_wheel(&mut self, delta: f32) {
        if !self.dragging {
            self.velocity += delta * 12.0;
        }
    }

    const fn record_sample(&mut self, time: f32, y: f32) {
        self.samples[self.samples_idx] = Some((time, y));
        self.samples_idx = (self.samples_idx + 1) % self.samples.len();
    }

    fn calculate_fling_velocity(&self) -> f32 {
        let mut valid = [None; 4];
        let mut count = 0;
        for i in 0..4 {
            let idx = (self.samples_idx + i) % 4;
            if let Some(sample) = self.samples[idx] {
                valid[count] = Some(sample);
                count += 1;
            }
        }

        if count < 2 {
            return 0.0;
        }

        let newest = valid[count - 1].unwrap();

        // If latest sample is older than 100ms, drag paused (no fling)
        if self.time_accumulator - newest.0 > 0.1 {
            return 0.0;
        }

        // Look back for oldest sample within 150ms of newest
        let mut oldest = newest;
        for i in (0..count - 1).rev() {
            let sample = valid[i].unwrap();
            if newest.0 - sample.0 <= 0.15 {
                oldest = sample;
            } else {
                break;
            }
        }

        let dt = newest.0 - oldest.0;
        if dt < 0.01 {
            return 0.0;
        }

        (oldest.1 - newest.1) / dt
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let s = ListState::new();
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn next_from_none_selects_first() {
        let mut s = ListState::new();
        s.select_next(3);
        assert_eq!(s.selected(), Some(0));
    }

    #[test]
    fn previous_from_none_selects_last() {
        let mut s = ListState::new();
        s.select_previous(3);
        assert_eq!(s.selected(), Some(2));
    }

    #[test]
    fn next_wraps_past_the_end() {
        let mut s = ListState::new();
        s.select(Some(2));
        s.select_next(3);
        assert_eq!(s.selected(), Some(0));
    }

    #[test]
    fn previous_wraps_past_the_start() {
        let mut s = ListState::new();
        s.select(Some(0));
        s.select_previous(3);
        assert_eq!(s.selected(), Some(2));
    }

    #[test]
    fn zero_length_clears_selection() {
        let mut s = ListState::new();
        s.select(Some(0));
        s.select_next(0);
        assert_eq!(s.selected(), None);
        s.select(Some(0));
        s.select_previous(0);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_first_and_last() {
        let mut s = ListState::new();
        s.select_last(5);
        assert_eq!(s.selected(), Some(4));
        s.select_first(5);
        assert_eq!(s.selected(), Some(0));
        s.select_first(0);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn ensure_visible_is_a_no_op_when_already_in_view() {
        let mut s = ListState::new();
        s.select(Some(3));
        s.set_offset(2);
        s.ensure_visible(5); // window is [2, 7); 3 is inside it
        assert_eq!(s.offset(), 2);
    }

    #[test]
    fn ensure_visible_scrolls_down_to_reveal_a_later_selection() {
        let mut s = ListState::new();
        s.select(Some(10));
        s.set_offset(0);
        s.ensure_visible(4); // window is [0, 4); 10 is below it
        assert_eq!(s.offset(), 7); // [7, 11) puts 10 as the last visible row
        assert!(s.offset() <= 10 && 10 < s.offset() + 4);
    }

    #[test]
    fn ensure_visible_scrolls_up_to_reveal_an_earlier_selection() {
        let mut s = ListState::new();
        s.select(Some(1));
        s.set_offset(5);
        s.ensure_visible(3); // window is [5, 8); 1 is above it
        assert_eq!(s.offset(), 1);
    }

    #[test]
    fn ensure_visible_is_a_no_op_with_nothing_selected_or_zero_height() {
        let mut s = ListState::new();
        s.set_offset(5);
        s.ensure_visible(10); // nothing selected
        assert_eq!(s.offset(), 5);

        s.select(Some(20));
        s.ensure_visible(0); // zero-height viewport
        assert_eq!(s.offset(), 5);
    }

    #[test]
    fn reset_clears_selection_and_offset() {
        let mut s = ListState::new();
        s.select(Some(2));
        s.set_offset(5);
        s.reset();
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn scroll_by_clamps_at_zero() {
        let mut s = ListState::new();
        s.scroll_by(-5);
        assert_eq!(s.offset(), 0);
        s.scroll_by(3);
        assert_eq!(s.offset(), 3);
        s.scroll_by(-1);
        assert_eq!(s.offset(), 2);
    }

    #[test]
    fn scroll_state_starts_at_zero() {
        let s = ScrollState::new();
        assert_eq!(s.offset(), 0.0);
        assert_eq!(s.velocity(), 0.0);
        assert!(!s.dragging());
        assert_eq!(s.integer_offset(), 0);
        assert_eq!(s.fractional_offset(), 0.0);
    }

    #[test]
    fn scroll_state_set_offset_clamps() {
        let mut s = ScrollState::new();
        s.set_offset(10.0, 5.0);
        assert_eq!(s.offset(), 5.0);
        s.set_offset(-2.0, 5.0);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn scroll_state_drag_moves_offset() {
        let mut s = ScrollState::new();
        s.begin_drag(10.0);
        assert!(s.dragging());
        s.update_drag(7.0, 10.0);
        assert_eq!(s.offset(), 3.0);
        s.update_drag(8.0, 10.0);
        assert_eq!(s.offset(), 2.0);
    }

    #[test]
    fn scroll_state_drag_resistance_past_bounds() {
        let mut s = ScrollState::new();
        s.begin_drag(10.0);
        s.update_drag(15.0, 10.0);
        assert!(s.offset() < 0.0);
        assert!(s.offset() > -5.0);

        let mut s = ScrollState::new();
        s.set_offset(10.0, 10.0);
        s.begin_drag(10.0);
        s.update_drag(5.0, 10.0);
        assert!(s.offset() > 10.0);
        assert!(s.offset() < 15.0);
    }

    #[test]
    fn scroll_state_fling_momentum_and_friction() {
        let mut s = ScrollState::new();
        s.begin_drag(10.0);
        s.tick(core::time::Duration::from_millis(50), 10.0);
        s.update_drag(5.0, 10.0);
        s.tick(core::time::Duration::from_millis(50), 10.0);
        s.update_drag(0.0, 10.0);
        s.end_drag();

        assert!(s.velocity() > 0.0);
        let init_vel = s.velocity();

        s.tick(core::time::Duration::from_millis(100), 10.0);
        assert!(s.velocity() < init_vel);
        assert!(s.offset() > 10.0);
    }

    #[test]
    fn scroll_state_spring_snapback() {
        let mut s = ScrollState::new();
        s.offset = -2.0;
        assert_eq!(s.offset(), -2.0);

        s.tick(core::time::Duration::from_millis(100), 10.0);
        assert!(s.offset() > -2.0);

        for _ in 0..50 {
            s.tick(core::time::Duration::from_millis(16), 10.0);
        }
        assert_eq!(s.offset(), 0.0);
        assert_eq!(s.velocity(), 0.0);
    }

    #[test]
    fn scroll_state_scroll_wheel() {
        let mut s = ScrollState::new();
        s.scroll_by_wheel(2.0);
        assert!(s.velocity() > 0.0);
        s.tick(core::time::Duration::from_millis(100), 10.0);
        assert!(s.offset() > 0.0);
    }
}
