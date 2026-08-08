//! [`replay_live`]: forward replay of an [`InputRecording`] into a live backend.

use retroglyph_core::app::{App, Flow, Frame};
use retroglyph_core::backend::Backend;
use retroglyph_core::terminal::Terminal;
use retroglyph_core::testing::InputRecording;

/// Drives `recording` into `app` through a live `term`, once, at recorded pace.
///
/// Sleeps out each event's recorded delay in real wall-clock time, pushes the event into
/// `term`'s backend, and runs one `App::update` frame for it, presenting automatically the same
/// way [`run_on`](retroglyph_core::app::run_on) does.
///
/// This is "watch it happen": an ordinary forward playback for a demo or a bug report, not an
/// interactive scrubber (no pause, rewind, or speed control). For driving a recording back
/// through a headless `App` in a test instead, with no real backend and no wall-clock sleeping,
/// use [`TestHarness::replay`](retroglyph_core::testing::TestHarness::replay) directly; both
/// ultimately drive the same `App::update` loop and so agree on the resulting state.
///
/// Returns once every recorded event has been replayed, or as soon as `app` returns
/// [`Flow::Exit`](retroglyph_core::app::Flow::Exit), whichever comes first.
///
/// # Errors
///
/// Returns the backend's error if an automatic `present()` call fails.
pub fn replay_live<B, A>(
    mut term: Terminal<B>,
    app: &mut A,
    recording: &InputRecording,
) -> Result<(), B::Error>
where
    B: Backend,
    A: App<B>,
{
    app.init(&mut term);
    let mut frame_count = 0u64;
    for (delay, event) in recording.events() {
        if !delay.is_zero() {
            std::thread::sleep(*delay);
        }
        term.backend_mut().push_event(event.clone());

        let frame = Frame::new(*delay, frame_count);
        frame_count = frame_count.wrapping_add(1);

        let present_count_before = term.present_count();
        let flow = app.update(&mut term, &frame);
        if flow != Flow::Idle && term.present_count() == present_count_before {
            term.present()?;
        }
        if flow == Flow::Exit {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::backend::Headless;
    use retroglyph_core::color::Style;
    use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use retroglyph_core::testing::TestHarness;
    use std::time::Duration;

    /// Counts key presses it has seen, and draws that count as a single digit -- enough state to
    /// compare `replay_live` against `TestHarness::replay` for the same recording.
    struct Counter {
        presses: u32,
    }

    impl<B: Backend> App<B> for Counter {
        fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
            for event in term.drain_events() {
                if matches!(event, Event::Key(_)) {
                    self.presses += 1;
                }
            }
            term.surface().put(
                (0, 0),
                char::from_digit(self.presses, 10).unwrap_or('?'),
                Style::default(),
            );
            Flow::Continue
        }
    }

    fn recording() -> InputRecording {
        let mut recording = InputRecording::new(4, 1);
        recording.push(
            Duration::ZERO,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        );
        recording.push(
            Duration::from_millis(1),
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
        );
        recording
    }

    #[test]
    fn replay_live_reaches_the_same_state_as_test_harness_replay() {
        let recording = recording();

        let term = Terminal::new(Headless::new(recording.width(), recording.height()));
        let mut live_app = Counter { presses: 0 };
        replay_live(term, &mut live_app, &recording).expect("replay_live");

        let mut harness = TestHarness::from_recording(&recording);
        let mut harness_app = Counter { presses: 0 };
        harness.replay(&recording, &mut harness_app);

        assert_eq!(live_app.presses, harness_app.presses);
        assert_eq!(live_app.presses, 2);
    }

    #[test]
    fn replay_live_stops_early_on_flow_exit() {
        struct ExitsAfterFirst {
            updates: u32,
        }
        impl<B: Backend> App<B> for ExitsAfterFirst {
            fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                self.updates += 1;
                Flow::Exit
            }
        }

        let recording = recording();
        let term = Terminal::new(Headless::new(recording.width(), recording.height()));
        let mut app = ExitsAfterFirst { updates: 0 };
        replay_live(term, &mut app, &recording).expect("replay_live");

        assert_eq!(app.updates, 1, "must stop after the first Flow::Exit");
    }

    #[test]
    fn replay_live_calls_init_once_before_the_first_update() {
        struct RecordsInit {
            init_calls: u32,
            updates: u32,
        }
        impl<B: Backend> App<B> for RecordsInit {
            fn init(&mut self, _term: &mut Terminal<B>) {
                self.init_calls += 1;
            }
            fn update(&mut self, _term: &mut Terminal<B>, _frame: &Frame) -> Flow {
                self.updates += 1;
                Flow::Continue
            }
        }

        let recording = recording();
        let term = Terminal::new(Headless::new(recording.width(), recording.height()));
        let mut app = RecordsInit {
            init_calls: 0,
            updates: 0,
        };
        replay_live(term, &mut app, &recording).expect("replay_live");

        assert_eq!(app.init_calls, 1, "init must run exactly once");
        assert_eq!(app.updates, 2);
    }
}
