//! [`Pointer`]: raw mouse/pointer state derived from a stream of
//! [`Event`]s.

use retroglyph_core::{Event, MouseButton, MouseEventKind, Pos};

/// Per-button down/pressed/released state, tracked independently for each
/// [`MouseButton`].
#[derive(Debug, Clone, Copy, Default)]
struct ButtonState {
    down: bool,
    pressed: bool,
    released: bool,
}

/// Index into [`Pointer::buttons`] for a given [`MouseButton`]. A plain
/// match over three fixed variants rather than a `HashMap`: no allocation,
/// no hashing, and the array stays small/`Copy` -- fits this crate's
/// dependency-minimal, `no_std`-friendly habits (see
/// [`Sense`](crate::Sense)'s doc comment for the same reasoning applied to
/// bitflags).
const fn button_slot(button: MouseButton) -> usize {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
    }
}

/// Cell-grid pointer position and per-button state, updated by feeding it
/// every [`Event`] you receive.
///
/// Tracks all three [`MouseButton`] variants independently (unlike
/// [`Interaction`](crate::Interaction)'s higher-level click/drag/focus
/// resolution, which only ever resolves the primary button plus a narrower
/// secondary-click signal -- see [`Sense::SECONDARY_CLICK`](crate::Sense::SECONDARY_CLICK)).
/// Mirrors [`KeyState`](retroglyph_core::KeyState)'s "feed events in, query
/// state out" shape.
///
/// [`pressed`](Self::pressed)/[`released`](Self::released)/[`scroll_delta`](Self::scroll_delta)
/// are one-shot: populated only for the frame the underlying event arrived
/// in, then cleared by [`end_frame`](Self::end_frame).
/// [`pos`](Self::pos)/[`is_down`](Self::is_down) are level state that
/// persists until the next change.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pointer {
    pos: Option<Pos>,
    buttons: [ButtonState; 3],
    scroll_delta: i32,
}

impl Pointer {
    /// No known position, nothing pressed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pos: None,
            buttons: [ButtonState {
                down: false,
                pressed: false,
                released: false,
            }; 3],
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
            MouseEventKind::Down(button) => {
                let slot = &mut self.buttons[button_slot(button)];
                slot.down = true;
                slot.pressed = true;
            }
            MouseEventKind::Up(button) => {
                let slot = &mut self.buttons[button_slot(button)];
                slot.down = false;
                slot.released = true;
            }
            MouseEventKind::ScrollUp => self.scroll_delta -= 1,
            MouseEventKind::ScrollDown => self.scroll_delta += 1,
            MouseEventKind::Moved => {}
        }
    }

    /// Clear every button's one-shot `pressed`/`released` and this frame's
    /// `scroll_delta`. Call once per frame, after drawing.
    pub const fn end_frame(&mut self) {
        let mut i = 0;
        while i < self.buttons.len() {
            self.buttons[i].pressed = false;
            self.buttons[i].released = false;
            i += 1;
        }
        self.scroll_delta = 0;
    }

    /// The pointer's last known cell-grid position, or `None` if no mouse
    /// event has arrived yet.
    #[must_use]
    pub const fn pos(&self) -> Option<Pos> {
        self.pos
    }

    /// `true` while `button` is held down.
    #[must_use]
    pub const fn is_down(&self, button: MouseButton) -> bool {
        self.buttons[button_slot(button)].down
    }

    /// `true` for exactly the frame `button` went down.
    #[must_use]
    pub const fn pressed(&self, button: MouseButton) -> bool {
        self.buttons[button_slot(button)].pressed
    }

    /// `true` for exactly the frame `button` went up.
    #[must_use]
    pub const fn released(&self, button: MouseButton) -> bool {
        self.buttons[button_slot(button)].released
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
        assert!(p.is_down(MouseButton::Left));
        assert!(p.pressed(MouseButton::Left));
        assert_eq!(p.pos(), Some(Pos::new(3, 4)));

        p.end_frame();
        assert!(p.is_down(MouseButton::Left)); // level state survives end_frame
        assert!(!p.pressed(MouseButton::Left)); // one-shot cleared

        p.handle_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            Pos::new(3, 4),
        ));
        assert!(!p.is_down(MouseButton::Left));
        assert!(p.released(MouseButton::Left));
    }

    #[test]
    fn buttons_are_tracked_independently() {
        let mut p = Pointer::new();
        p.handle_event(&mouse(
            MouseEventKind::Down(MouseButton::Right),
            Pos::new(1, 1),
        ));
        assert!(p.is_down(MouseButton::Right));
        assert!(p.pressed(MouseButton::Right));
        // Left is untouched by a Right-button event.
        assert!(!p.is_down(MouseButton::Left));
        assert!(!p.pressed(MouseButton::Left));
        assert!(!p.is_down(MouseButton::Middle));
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
}
