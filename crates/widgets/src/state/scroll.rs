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
