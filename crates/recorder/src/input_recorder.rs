//! [`InputRecorder`] and its [`RecorderHandle`].

use retroglyph_core::backend::{Compositing, Cursor, CursorStyle, DrawCell, Input, Output};
use retroglyph_core::event::Event;
use retroglyph_core::grid::{HasSize, Pos, Size};
use retroglyph_core::testing::InputRecording;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// State shared between an [`InputRecorder`] and every [`RecorderHandle`] cloned from it.
struct State {
    recording: InputRecording,
    /// `true` while events seen through [`Input::poll_event`] are being appended to `recording`.
    /// [`InputRecorder::new`] starts `true`, matching a `TestHarness`-style "recording starts
    /// immediately" default; [`RecorderHandle::pause`] flips it off without discarding anything
    /// already captured.
    active: bool,
    /// When the most recently recorded event was seen, so the next one's delay (time since the
    /// *previous* event, per [`InputRecording`]'s own convention) can be computed. `None` before
    /// the first event, or immediately after [`RecorderHandle::start`] resumes from a pause (the
    /// gap spent paused is not itself a delay worth recording).
    last_event_at: Option<Instant>,
}

/// A cheap, cloneable handle onto an [`InputRecorder`]'s captured state.
///
/// Exists so a call stack that doesn't own the recorder itself -- most notably
/// [`install_panic_recorder`](crate::install_panic_recorder), which runs from inside a panic
/// hook -- can still start/pause/stop it or save whatever it has captured so far. Backed by
/// `Arc<Mutex<..>>`: cloning shares the same underlying recording, it does not fork one.
#[derive(Clone)]
pub struct RecorderHandle {
    state: Arc<Mutex<State>>,
}

impl RecorderHandle {
    fn new(width: u16, height: u16) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                recording: InputRecording::new(width, height),
                active: true,
                last_event_at: None,
            })),
        }
    }

    /// Appends `event` to the recording (with the elapsed time since the previously recorded
    /// event as its delay), unless [`pause`](Self::pause) has left recording inactive.
    fn record(&self, event: Event) {
        let mut state = self.lock();
        if !state.active {
            return;
        }
        let now = Instant::now();
        let delay = state
            .last_event_at
            .map_or(Duration::ZERO, |previous| now.duration_since(previous));
        state.last_event_at = Some(now);
        state.recording.push(delay, event);
    }

    /// Locks the shared state, recovering it if a previous holder panicked while holding the
    /// lock rather than poisoning every future access: losing a few milliseconds of in-flight
    /// capture to a poisoned mutex would defeat the point of a recorder meant to survive a panic
    /// (see [`install_panic_recorder`](crate::install_panic_recorder)).
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// (Re)starts recording. A no-op if already active. The gap since the previous event (or
    /// since [`pause`](Self::pause), if that's why recording stopped) is not itself recorded as
    /// a delay: the next event's delay is measured from when it actually arrives.
    pub fn start(&self) {
        let mut state = self.lock();
        state.active = true;
        state.last_event_at = None;
    }

    /// Stops appending new events, without discarding what's already captured. Recording resumes
    /// on the next [`start`](Self::start).
    pub fn pause(&self) {
        self.lock().active = false;
    }

    /// A snapshot of everything captured so far.
    #[must_use]
    pub fn snapshot(&self) -> InputRecording {
        self.lock().recording.clone()
    }

    /// [`pause`](Self::pause)s recording and returns everything captured so far.
    #[must_use]
    pub fn stop(&self) -> InputRecording {
        let mut state = self.lock();
        state.active = false;
        state.recording.clone()
    }

    /// Writes a [`snapshot`](Self::snapshot) of the current recording to `path` as `.rgrec`.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` cannot be created or written to.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        crate::rgrec::write(&self.snapshot(), path)
    }
}

/// Wraps a backend and records every event it sees.
///
/// Works over a [`Backend`](retroglyph_core::backend::Backend), or any subset of
/// [`Output`]/[`Input`]/[`Cursor`]: taps [`Input::poll_event`] to capture, while delegating
/// everything else unchanged.
///
/// Recording starts immediately on construction; use [`handle`](Self::handle) (or the
/// convenience [`start`](Self::start)/[`pause`](Self::pause)/[`stop`](Self::stop)/
/// [`save`](Self::save) methods on this type, which forward to it) to control capture and save
/// it to a `.rgrec` file.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::{Headless, Input};
/// use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
/// use retroglyph_recorder::InputRecorder;
///
/// let mut recorder = InputRecorder::new(Headless::new(10, 3));
/// recorder.push_event(Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)));
/// let event = recorder.poll_event(std::time::Duration::ZERO);
/// assert!(event.is_some());
///
/// let recording = recorder.stop();
/// assert_eq!(recording.len(), 1);
/// ```
pub struct InputRecorder<B> {
    inner: B,
    handle: RecorderHandle,
}

impl<B: Output> InputRecorder<B> {
    /// Wraps `inner`, sizing the recording to `inner`'s current [`Output::size`].
    pub fn new(inner: B) -> Self {
        let size = inner.size();
        Self {
            inner,
            handle: RecorderHandle::new(size.width(), size.height()),
        }
    }
}

impl<B> InputRecorder<B> {
    /// A cloneable [`RecorderHandle`] onto this recorder's shared state, for reaching in from
    /// another call stack (see [`RecorderHandle`]'s own docs).
    #[must_use]
    pub fn handle(&self) -> RecorderHandle {
        self.handle.clone()
    }

    /// [`RecorderHandle::start`] on this recorder's own handle.
    pub fn start(&self) {
        self.handle.start();
    }

    /// [`RecorderHandle::pause`] on this recorder's own handle.
    pub fn pause(&self) {
        self.handle.pause();
    }

    /// [`RecorderHandle::snapshot`] on this recorder's own handle.
    #[must_use]
    pub fn snapshot(&self) -> InputRecording {
        self.handle.snapshot()
    }

    /// [`RecorderHandle::stop`] on this recorder's own handle.
    #[must_use]
    pub fn stop(&self) -> InputRecording {
        self.handle.stop()
    }

    /// [`RecorderHandle::save`] on this recorder's own handle.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` cannot be created or written to.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        self.handle.save(path)
    }

    /// The wrapped backend, for anything not exposed through the decorator itself.
    pub const fn inner(&self) -> &B {
        &self.inner
    }

    /// The wrapped backend, mutably.
    pub const fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }
}

impl<B: Output> Output for InputRecorder<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        self.inner.draw(content)
    }

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        self.inner.draw_layers(content)
    }

    fn compositing(&self) -> Compositing {
        self.inner.compositing()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn size(&self) -> Size {
        self.inner.size()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn resize(&mut self, size: Size) {
        self.inner.resize(size);
    }
}

impl<B: Input> Input for InputRecorder<B> {
    fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
        let event = self.inner.poll_event(timeout)?;
        self.handle.record(event.clone());
        Some(event)
    }

    fn push_event(&mut self, event: Event) {
        self.inner.push_event(event);
    }
}

impl<B: Cursor> Cursor for InputRecorder<B> {
    fn set_cursor_visible(&mut self, visible: bool) {
        self.inner.set_cursor_visible(visible);
    }

    fn set_cursor_position(&mut self, position: Pos) {
        self.inner.set_cursor_position(position);
    }

    fn set_cursor_style(&mut self, style: CursorStyle) {
        self.inner.set_cursor_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::backend::Headless;
    use retroglyph_core::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn records_events_seen_through_poll_event() {
        let mut recorder = InputRecorder::new(Headless::new(10, 3));
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        )));

        assert!(recorder.poll_event(Duration::ZERO).is_some());
        assert!(recorder.poll_event(Duration::ZERO).is_some());
        assert!(recorder.poll_event(Duration::ZERO).is_none());

        let recording = recorder.snapshot();
        assert_eq!(recording.width(), 10);
        assert_eq!(recording.height(), 3);
        let events: Vec<_> = recording.events().map(|(_, event)| event.clone()).collect();
        assert_eq!(
            events,
            vec![
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
                Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
            ]
        );
    }

    #[test]
    fn pause_stops_recording_without_discarding_captured_events() {
        let mut recorder = InputRecorder::new(Headless::new(4, 1));
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);
        assert_eq!(recorder.snapshot().len(), 1);

        recorder.pause();
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);
        assert_eq!(
            recorder.snapshot().len(),
            1,
            "paused recorder must not append new events"
        );

        recorder.start();
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);
        assert_eq!(
            recorder.snapshot().len(),
            2,
            "resumed recorder must append new events again"
        );
    }

    #[test]
    fn handle_reaches_the_same_state_as_the_recorder() {
        let mut recorder = InputRecorder::new(Headless::new(4, 1));
        let handle = recorder.handle();
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);

        assert_eq!(handle.snapshot().len(), 1);
        handle.pause();
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);
        assert_eq!(recorder.snapshot().len(), 1);
    }

    #[test]
    fn stop_pauses_and_returns_the_captured_recording() {
        let mut recorder = InputRecorder::new(Headless::new(4, 1));
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);

        let stopped = recorder.stop();
        assert_eq!(stopped.len(), 1);

        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(Duration::ZERO);
        assert_eq!(recorder.snapshot().len(), 1, "stop() must pause recording");
    }
}
