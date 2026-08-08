//! Serialized input sessions: [`InputRecording`], the type [`TestHarness::replay`](super::TestHarness::replay) and
//! [`TestHarness::from_recording`](super::TestHarness::from_recording) drive.

use crate::event::Event;
use alloc::vec::Vec;
use core::time::Duration;

/// A recorded input session: the backend size it was captured against, plus a timed sequence of
/// events.
///
/// Each event's [`Duration`] is the delay since the *previous* recorded event (or since the
/// start of the recording, for the first one), not an absolute timestamp measured from session
/// start -- see [`push`](Self::push). That's the shape [`TestHarness::replay`](super::TestHarness::replay) needs to
/// reproduce recorded pacing faithfully instead of collapsing every gap to zero.
///
/// `no_std` + `alloc` compatible, gated on the `testing` feature like the rest of this module.
/// [`Serialize`](serde::Serialize)/[`Deserialize`](serde::Deserialize) are additionally gated on
/// `serde`: driving a recording back through a live `App` within the same process (what this
/// type and [`TestHarness::replay`](super::TestHarness::replay) do) needs no serialization at all; turning a
/// recording into bytes on disk (the `.rgrec` format) is `retroglyph-recorder`'s job, built on
/// top of these derives.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InputRecording {
    width: u16,
    height: u16,
    events: Vec<(Duration, Event)>,
}

impl InputRecording {
    /// Creates an empty recording for a `width` x `height` backend.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            events: Vec::new(),
        }
    }

    /// The backend width this recording was captured against.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// The backend height this recording was captured against.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// Appends `event`, `delay` after the previously appended event (or after the start of the
    /// recording, for the first).
    pub fn push(&mut self, delay: Duration, event: Event) {
        self.events.push((delay, event));
    }

    /// Iterates the recorded `(delay, event)` pairs, in the order they were
    /// [`push`](Self::push)ed.
    #[must_use]
    pub fn events(&self) -> impl ExactSizeIterator<Item = &(Duration, Event)> {
        self.events.iter()
    }

    /// The number of recorded events.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if no events have been recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn new_recording_is_empty() {
        let recording = InputRecording::new(80, 24);
        assert_eq!(recording.width(), 80);
        assert_eq!(recording.height(), 24);
        assert!(recording.is_empty());
        assert_eq!(recording.len(), 0);
    }

    #[test]
    fn push_appends_in_order() {
        let mut recording = InputRecording::new(4, 2);
        let a = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        let b = Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        recording.push(Duration::ZERO, a.clone());
        recording.push(Duration::from_millis(50), b.clone());

        let events: Vec<_> = recording.events().collect();
        assert_eq!(
            events,
            vec![&(Duration::ZERO, a), &(Duration::from_millis(50), b)]
        );
        assert_eq!(recording.len(), 2);
        assert!(!recording.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn round_trips_through_serde_json() {
        let mut recording = InputRecording::new(80, 24);
        recording.push(
            Duration::ZERO,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        );
        recording.push(
            Duration::from_millis(250),
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
        );

        let json = serde_json::to_string(&recording).expect("serialize");
        let round_tripped: InputRecording = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_tripped, recording);
    }
}
