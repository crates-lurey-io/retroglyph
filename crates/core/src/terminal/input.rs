//! Input polling: [`poll`](Terminal::poll) and the queue/lookahead helpers built on it.
//!
//! Every event enters through [`poll_backend`](Terminal::poll_backend), the single point where a
//! backend-sourced [`Event::Resize`] gets applied to the grids; [`poll`](Terminal::poll) and the
//! other methods here all route through it (directly, or via `queued_events`) so a resize is
//! never applied twice for the same logical event.

use super::Terminal;
use crate::backend::Backend;
use crate::event::Event;
use alloc::vec::Vec;
use core::time::Duration;

impl<B: Backend> Terminal<B> {
    /// Polls for an input event, waiting up to `timeout`.
    ///
    /// If an event was previously buffered by [`has_input`](Self::has_input), it is
    /// returned immediately. Otherwise, the backend is polled for a new event.
    ///
    /// [`Event::Resize`] events arriving from the backend are automatically applied: both
    /// grids are resized before the event is returned to the caller, so the game loop can
    /// immediately redraw at the new size. An event coming back off this terminal's own queue
    /// (from [`requeue_events`](Self::requeue_events), or buffered by
    /// [`wait_for_input`](Self::wait_for_input)) was already applied when it first entered, so
    /// it is returned as-is rather than resized again. See `poll_backend`.
    pub fn poll(&mut self, timeout: Duration) -> Option<Event> {
        if let Some(event) = self.queued_events.pop_front() {
            // Already passed through `poll_backend` (or was requeued from an event that did),
            // so any `Resize` it carries was applied then; applying it again here would resize
            // twice for one logical event. See `poll_backend`.
            return Some(event);
        }
        self.poll_backend(timeout)
    }

    /// Polls the backend directly (bypassing `queued_events`), applying [`Event::Resize`]
    /// immediately when found.
    ///
    /// This is the single point where a freshly-polled event enters the terminal, so it's also
    /// the only place `resize` should be called for an event on its way in: callers handling an
    /// event already taken from `queued_events` (via [`poll`](Self::poll)'s queue-pop branch, or
    /// via [`requeue_events`](Self::requeue_events)) must not call `resize` again for it, or a
    /// single resize gets applied twice.
    fn poll_backend(&mut self, timeout: Duration) -> Option<Event> {
        let event = self.backend.poll_event(timeout)?;
        if let Event::Resize(w, h) = event {
            self.resize(w, h);
        }
        Some(event)
    }

    /// Hands `events` back to this terminal's own queue, in order, so a later
    /// [`poll`](Self::poll)/[`drain_events`](Self::drain_events)/[`drain_events_into`](Self::drain_events_into)
    /// call yields them again before the backend is polled for anything new.
    ///
    /// This is the supported way for a wrapper that drains events to intercept some of them
    /// (e.g. `retroglyph-widgets`' `PerfOverlayApp` filtering out its own toggle key) to give
    /// the rest back: it goes through `Terminal`'s own queue, never a backend-specific input
    /// path, so it works identically on every [`Backend`] regardless of how (or whether) that
    /// backend implements [`Input::push_event`](crate::backend::Input::push_event).
    pub fn requeue_events(&mut self, events: impl IntoIterator<Item = Event>) {
        self.queued_events.extend(events);
    }

    /// Drains all available events without blocking.
    ///
    /// Returns an iterator that yields every pending event: the internal queued event
    /// followed by all events buffered in the backend. The iterator polls the backend
    /// with zero timeout repeatedly until `None` is returned.
    ///
    /// This is needed for frame-based game loops (e.g. software backend + WASM, where
    /// frames are gated by `requestAnimationFrame`). Multiple keypresses can arrive
    /// between frames; draining all of them ensures accumulated input doesn't replay in
    /// slow motion.
    ///
    /// Crossterm and headless backends can also use this, but the single-event `poll`
    /// pattern works for them because their loops aren't frame-capped.
    ///
    /// # One-shot semantics
    ///
    /// The returned iterator borrows `self` and drains the queue as it is consumed: the
    /// first caller to iterate it gets every pending event, and a second, independent
    /// call to `drain_events` afterward gets nothing. If more than one subsystem needs
    /// this frame's input (e.g. persistent chrome and an active screen), collect once
    /// and share the collected events by reference, or use
    /// [`drain_events_into`](Self::drain_events_into) to drain into a reusable buffer
    /// instead of allocating a fresh `Vec` every frame.
    pub fn drain_events(&mut self) -> impl Iterator<Item = Event> + use<'_, B> {
        struct DrainEvents<'a, B: Backend> {
            terminal: &'a mut Terminal<B>,
        }

        impl<B: Backend> Iterator for DrainEvents<'_, B> {
            type Item = Event;

            fn next(&mut self) -> Option<Event> {
                self.terminal.poll(Duration::ZERO)
            }
        }

        impl<B: Backend> core::iter::FusedIterator for DrainEvents<'_, B> {}

        DrainEvents { terminal: self }
    }

    /// Drains all available events without blocking, appending them to `buf`.
    ///
    /// `buf` is cleared first, then filled with every pending event in the same order
    /// [`drain_events`](Self::drain_events) would yield them. Unlike `drain_events`, the
    /// borrow of `self` ends when this call returns, so the terminal is free to draw or
    /// be polled again afterward, and the caller can hand `buf` to multiple consumers by
    /// shared reference without materializing a new `Vec` every frame.
    ///
    /// This is the same shape as `std::io::Read::read_to_end`: allocate the buffer once
    /// at startup, reuse it every frame, and let this method manage its contents.
    pub fn drain_events_into(&mut self, buf: &mut Vec<Event>) {
        buf.clear();
        while let Some(event) = self.poll(Duration::ZERO) {
            buf.push(event);
        }
    }

    /// Checks if a pending input event is available without blocking.
    ///
    /// If an event is already buffered, returns `true`. Otherwise, polls the backend
    /// with zero timeout. If the backend returns an event, it is stored in the internal
    /// buffer and `true` is returned; otherwise, returns `false`.
    pub fn has_input(&mut self) -> bool {
        self.wait_for_input(Duration::ZERO)
    }

    /// Blocks until an input event is available or `timeout` elapses, without consuming it.
    ///
    /// Like [`has_input`](Self::has_input), a discovered event is buffered internally so a
    /// subsequent [`poll`](Self::poll), [`has_input`](Self::has_input), or
    /// [`drain_events`](Self::drain_events) call still observes it: this method only answers
    /// "did something happen", it never hands the event to the caller. That's what lets a driver
    /// loop block between frames without stealing the event the app's own `update` reads; see
    /// [`run_blocking_with`](crate::run_blocking_with)'s use of this for [`Flow::Idle`](crate::Flow::Idle).
    ///
    /// Returns `true` if an event arrived within `timeout`, `false` if `timeout` elapsed with
    /// nothing pending. Pass [`Duration::MAX`] to block indefinitely.
    ///
    /// Backends that never block (e.g. [`Headless`](crate::backend::Headless), which returns
    /// immediately regardless of `timeout`; see [`Input::poll_event`](crate::backend::Input::poll_event))
    /// return promptly rather than actually waiting; this method is a real wait only on
    /// backends that genuinely block (crossterm, window).
    pub fn wait_for_input(&mut self, timeout: Duration) -> bool {
        if !self.queued_events.is_empty() {
            return true;
        }
        let Some(event) = self.poll_backend(timeout) else {
            return false;
        };
        self.queued_events.push_back(event);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Cursor, DrawCell, Headless, Input, Output};
    use crate::grid::{Rect, Size};

    #[test]
    fn test_terminal_poll_and_read() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.poll(Duration::ZERO), None);

        terminal.backend_mut().push_event(Event::Close);
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        terminal.backend_mut().push_event(Event::Resize(80, 25));
        assert_eq!(terminal.poll(Duration::MAX), Some(Event::Resize(80, 25)));
    }

    #[test]
    fn test_terminal_has_input() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert!(!terminal.has_input());

        terminal.backend_mut().push_event(Event::Close);
        assert!(terminal.has_input());
        assert!(terminal.has_input()); // Repeated calls should still be true

        // Read/Poll should retrieve the buffered event
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        // After taking, it should be false again
        assert!(!terminal.has_input());
    }

    #[test]
    fn test_terminal_drain_events_into() {
        use alloc::vec;

        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Close);
        terminal.backend_mut().push_event(Event::Resize(80, 25));

        let mut buf = vec![Event::Close]; // pre-existing contents must be cleared
        terminal.drain_events_into(&mut buf);
        assert_eq!(buf, [Event::Close, Event::Resize(80, 25)]);

        // The borrow ends at the call, so the terminal is immediately usable again.
        assert_eq!(terminal.area(), Rect::new(0, 0, 80, 25));

        // Draining again with nothing pending clears the buffer.
        terminal.drain_events_into(&mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_terminal_drain_events_into_applies_a_backend_resize() {
        use alloc::vec::Vec;

        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Close);
        terminal.backend_mut().push_event(Event::Resize(4, 2));

        let mut buf = Vec::new();
        terminal.drain_events_into(&mut buf);
        assert_eq!(buf, [Event::Close, Event::Resize(4, 2)]);
        // The resize was applied on the way in, same as `poll`/`drain_events`, not just handed
        // back as an inert event for the caller to notice and apply itself.
        assert_eq!(terminal.size(), Size::new(4, 2));
    }

    #[test]
    fn test_terminal_drain_events_into_does_not_reapply_a_requeued_resize() {
        use alloc::vec::Vec;

        let backend = ResizeCounting::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().inner.push_event(Event::Resize(4, 2));

        let mut buf = Vec::new();
        terminal.drain_events_into(&mut buf);
        assert_eq!(buf, [Event::Resize(4, 2)]);
        assert_eq!(terminal.backend().resize_calls, 1);

        // A wrapper that drained the event and handed it back gets it again, but the resize is
        // not applied a second time for the one logical event. See `poll_backend`.
        terminal.requeue_events(buf.iter().cloned());
        terminal.drain_events_into(&mut buf);
        assert_eq!(buf, [Event::Resize(4, 2)]);
        assert_eq!(terminal.backend().resize_calls, 1);
    }

    #[test]
    fn test_terminal_drain_events_is_one_shot_and_fused() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Close);
        terminal.backend_mut().push_event(Event::Close);

        let mut drained = terminal.drain_events();
        assert_eq!(drained.next(), Some(Event::Close));
        assert_eq!(drained.next(), Some(Event::Close));
        // Fused: repeated calls past exhaustion keep returning `None` rather than panicking.
        assert_eq!(drained.next(), None);
        assert_eq!(drained.next(), None);
        drop(drained);

        // One-shot: a second, independent call after the first already drained the queue sees
        // only what was pushed since, not a second copy of anything already handed out.
        terminal.backend_mut().push_event(Event::Close);
        let second: Vec<_> = terminal.drain_events().collect();
        assert_eq!(second, [Event::Close]);
    }

    #[test]
    fn test_terminal_requeue_events_replays_before_the_backend() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        // Push directly to the backend first, so a requeued event has to come out ahead of it.
        terminal.backend_mut().push_event(Event::Close);
        terminal.requeue_events([Event::Resize(80, 25), Event::Resize(1, 1)]);

        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(80, 25)));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(1, 1)));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));
        assert_eq!(terminal.poll(Duration::ZERO), None);
    }

    #[test]
    fn test_terminal_wait_for_input_buffers_the_event_instead_of_consuming_it() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        // Nothing queued: returns false rather than blocking (`Headless` ignores the timeout).
        assert!(!terminal.wait_for_input(Duration::from_millis(1)));

        terminal.backend_mut().push_event(Event::Close);
        assert!(terminal.wait_for_input(Duration::MAX));
        // Repeated calls stay true: the event was buffered, not handed out and lost.
        assert!(terminal.wait_for_input(Duration::MAX));
        assert!(terminal.has_input());

        // A caller reading through the normal input API (as an app's own `update` would) still
        // observes the exact event `wait_for_input` woke up for.
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));
        assert!(!terminal.has_input());
    }

    #[test]
    fn test_terminal_wait_for_input_applies_pending_resize() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Resize(4, 2));
        assert!(terminal.wait_for_input(Duration::MAX));
        // The grid resizes immediately, same as `poll`, even though the event is still buffered
        // rather than consumed.
        assert_eq!(terminal.size(), Size::new(4, 2));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(4, 2)));
    }

    /// Wraps [`Headless`] and counts [`resize`](Output::resize) calls, so a test can prove
    /// `Terminal` applies a backend resize at most once per logical `Event::Resize`, even when
    /// the event is buffered by `wait_for_input` and then consumed by `poll` (retroglyph#959).
    struct ResizeCounting {
        inner: Headless,
        resize_calls: usize,
    }

    impl ResizeCounting {
        fn new(width: u16, height: u16) -> Self {
            Self {
                inner: Headless::new(width, height),
                resize_calls: 0,
            }
        }
    }

    impl Output for ResizeCounting {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            self.inner.draw_layers(content)
        }

        fn resize(&mut self, size: Size) {
            self.resize_calls += 1;
            self.inner.resize(size);
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
    }

    impl Input for ResizeCounting {
        fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
            self.inner.poll_event(timeout)
        }

        fn push_event(&mut self, event: Event) {
            self.inner.push_event(event);
        }
    }

    impl Cursor for ResizeCounting {}

    #[test]
    fn test_terminal_poll_does_not_reapply_resize_buffered_by_wait_for_input() {
        let backend = ResizeCounting::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Resize(8, 2));
        assert!(terminal.wait_for_input(Duration::ZERO));
        assert_eq!(terminal.backend().resize_calls, 1);

        // The event was only buffered, not consumed, by `wait_for_input`; `poll` must return the
        // same event without resizing again.
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(8, 2)));
        assert_eq!(terminal.backend().resize_calls, 1);
        assert_eq!(terminal.size(), Size::new(8, 2));
        assert_eq!(terminal.backend().size(), Size::new(8, 2));

        // Exercise the rest of `ResizeCounting`'s `Output` passthrough too, so the mock's own
        // plumbing (not just `resize_calls`) is covered rather than asserted by inspection.
        terminal.present().unwrap();
        terminal.backend_mut().clear().unwrap();
    }

    #[test]
    fn test_terminal_poll_does_not_reapply_resize_from_requeue_events() {
        let backend = ResizeCounting::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().inner.push_event(Event::Resize(9, 3));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(9, 3)));
        assert_eq!(terminal.backend().resize_calls, 1);

        // A wrapper (e.g. `PerfOverlayApp`) hands the event straight back via `requeue_events`;
        // the next `poll` must not resize a second time for it.
        terminal.requeue_events([Event::Resize(9, 3)]);
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(9, 3)));
        assert_eq!(terminal.backend().resize_calls, 1);
    }

    #[test]
    fn test_terminal_retain_layer_survives_wait_for_input_then_poll_of_a_resize() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Resize(8, 2));
        assert!(terminal.wait_for_input(Duration::ZERO));

        // A caller that decides retention for this frame in between `wait_for_input` waking it
        // up and the matching `poll` that actually reads the event (a plausible ordering: e.g.
        // deciding retention from state gathered before reading input) must not have that
        // decision silently undone by `poll` re-applying the already-handled resize.
        terminal.retain_layer(0u8);
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(8, 2)));
        assert_eq!(terminal.retained_layers, [true]);
    }

    #[test]
    fn test_terminal_requeue_events_interleaves_with_wait_for_input_lookahead() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        // `wait_for_input` buffers one event ahead of anything requeued afterward.
        terminal.backend_mut().push_event(Event::Close);
        assert!(terminal.wait_for_input(Duration::MAX));

        // Requeueing now must not jump the queue ahead of the event `wait_for_input` already
        // buffered: both go through the same internal queue, in call order.
        terminal.requeue_events([Event::Resize(4, 2)]);

        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(4, 2)));
        assert_eq!(terminal.poll(Duration::ZERO), None);
    }
}
