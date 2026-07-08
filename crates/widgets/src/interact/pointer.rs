//! [`Pointer`]: raw mouse/pointer state derived from a stream of
//! [`Event`]s.

use retroglyph_core::{Event, MouseButton, MouseEventKind, Pos};

/// Cell-grid pointer position and primary-button state, updated by feeding
/// it every [`Event`] you receive.
///
/// Tracks only the primary (left) button; secondary/middle clicks pass
/// through [`handle_event`](Self::handle_event) untouched, matching this
/// crate's habit of doing the common case well rather than guessing at
/// every backend's full button set up front (see
/// [`KeyState`](retroglyph_core::KeyState) for the same shape applied to
/// keyboard state).
///
/// [`pressed`](Self::pressed)/[`released`](Self::released)/[`scroll_delta`](Self::scroll_delta)
/// are one-shot: populated only for the frame the underlying event arrived
/// in, then cleared by [`end_frame`](Self::end_frame).
/// [`pos`](Self::pos)/[`is_down`](Self::is_down) are level state that
/// persists until the next change.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pointer {
    pos: Option<Pos>,
    down: bool,
    pressed: bool,
    released: bool,
    scroll_delta: i32,
}

impl Pointer {
    /// No known position, nothing pressed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pos: None,
            down: false,
            pressed: false,
            released: false,
            scroll_delta: 0,
        }
    }

    /// Update from a raw input event; ignores everything but
    /// [`Event::Mouse`].
    pub const fn handle_event(&mut self, event: &Event) {
        let Event::Mouse(mouse) = event else {
            return;
        };
        self.pos = Some(mouse.position);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.down = true;
                self.pressed = true;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.down = false;
                self.released = true;
            }
            MouseEventKind::ScrollUp => self.scroll_delta -= 1,
            MouseEventKind::ScrollDown => self.scroll_delta += 1,
            MouseEventKind::Moved | MouseEventKind::Down(_) | MouseEventKind::Up(_) => {}
        }
    }

    /// Clear this frame's one-shot `pressed`/`released`/`scroll_delta`.
    /// Call once per frame, after drawing.
    pub const fn end_frame(&mut self) {
        self.pressed = false;
        self.released = false;
        self.scroll_delta = 0;
    }

    /// The pointer's last known cell-grid position, or `None` if no mouse
    /// event has arrived yet.
    #[must_use]
    pub const fn pos(&self) -> Option<Pos> {
        self.pos
    }

    /// `true` while the primary button is held down.
    #[must_use]
    pub const fn is_down(&self) -> bool {
        self.down
    }

    /// `true` for exactly the frame the primary button went down.
    #[must_use]
    pub const fn pressed(&self) -> bool {
        self.pressed
    }

    /// `true` for exactly the frame the primary button went up.
    #[must_use]
    pub const fn released(&self) -> bool {
        self.released
    }

    /// Scroll wheel delta accumulated this frame: positive is down/forward,
    /// negative is up/backward. Zero if nothing scrolled.
    #[must_use]
    pub const fn scroll_delta(&self) -> i32 {
        self.scroll_delta
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{KeyModifiers, MouseEvent};

    use super::*;

    fn mouse(kind: MouseEventKind, pos: Pos) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn press_and_release_are_one_shot() {
        let mut p = Pointer::new();
        p.handle_event(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            Pos::new(3, 4),
        ));
        assert!(p.is_down());
        assert!(p.pressed());
        assert_eq!(p.pos(), Some(Pos::new(3, 4)));

        p.end_frame();
        assert!(p.is_down()); // level state survives end_frame
        assert!(!p.pressed()); // one-shot cleared

        p.handle_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            Pos::new(3, 4),
        ));
        assert!(!p.is_down());
        assert!(p.released());
    }

    #[test]
    fn scroll_accumulates_within_a_frame_and_clears_on_end_frame() {
        let mut p = Pointer::new();
        p.handle_event(&mouse(MouseEventKind::ScrollDown, Pos::new(0, 0)));
        p.handle_event(&mouse(MouseEventKind::ScrollDown, Pos::new(0, 0)));
        p.handle_event(&mouse(MouseEventKind::ScrollUp, Pos::new(0, 0)));
        assert_eq!(p.scroll_delta(), 1);

        p.end_frame();
        assert_eq!(p.scroll_delta(), 0);
    }

    #[test]
    fn non_mouse_events_are_ignored() {
        let mut p = Pointer::new();
        p.handle_event(&Event::Resize(80, 24));
        assert_eq!(p.pos(), None);
    }

    #[test]
    fn ignores_non_primary_buttons() {
        let mut p = Pointer::new();
        p.handle_event(&mouse(
            MouseEventKind::Down(MouseButton::Right),
            Pos::new(1, 1),
        ));
        assert!(!p.is_down());
        assert!(!p.pressed());
        assert_eq!(p.pos(), Some(Pos::new(1, 1))); // position still tracked
    }
}
