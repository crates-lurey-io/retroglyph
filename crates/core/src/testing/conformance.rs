//! Cross-backend conformance tests for [`Output`], [`Cursor`], and [`Input`] (retroglyph#763).
//!
//! Each of the five backends in this workspace answers the same handful of obligations
//! ([`Output::clear`]/[`Output::resize`] resetting internal state, out-of-range [`DrawCell`]
//! positions, cursor tracking staying in sync with external writes, [`Input::push_event`]
//! coalescing consecutive `Mouse(Moved)` events) independently, and nothing previously checked
//! that the answers agreed. This module is that check: [`assert_output_contract`],
//! [`assert_cursor_contract`], and [`assert_input_contract`] each drive a backend through one
//! facet's obligations and panic on the first violation, so a backend crate wires one of them
//! into a `#[test]` and gets every future regression in that facet for free.
//!
//! # Why not `B: Backend`
//!
//! `GlRenderer` deliberately implements neither [`Input`] nor [`Cursor`] (a GPU/pixel surface has
//! no text cursor and never receives external input): a single `B: Backend` bound would make the
//! harness itself impossible to use, since a bound including `Input + Cursor` could never be
//! satisfied by every backend that wants only [`assert_output_contract`]. The three entry points
//! stay separate so a backend opts into exactly the facets it implements.
//!
//! # The `Observable` hook, and why it must be a delta
//!
//! `Output`/`Cursor` have no shared way to read back "what would actually appear": a terminal
//! backend has emitted bytes, a pixel backend has a framebuffer, [`Headless`](crate::Headless)
//! has a [`Grid`](crate::grid::Grid). [`Observable::snapshot`] is the one method a backend
//! implements to bridge that gap, and every assertion below only ever compares two calls to it
//! for equality, never interpreting the `u64` any other way.
//!
//! That equality only means what it should if `snapshot` returns **what changed since the
//! previous call**, not the backend's whole history or its whole current state. The assertions
//! compare two independently-built action sequences that a real user could not tell apart from
//! this point forward; if `snapshot` hashed everything ever produced (a terminal backend's whole
//! emitted byte log, say), two sequences of different lengths could never compare equal even when
//! both are correct, and the assertions would fail on every backend, always, for a reason that has
//! nothing to do with the obligation under test. A backend whose only observable output is an
//! appended log implements this by hashing the slice appended since the last call (and advancing
//! a remembered offset past it). A framebuffer-shaped backend implements it by hashing the
//! positions that differ from the previous call's content (and remembering the new content for
//! next time) rather than the whole buffer. Either way, `snapshot` needs its own "since last
//! call" bookkeeping the production backend has no other reason to carry, which is usually
//! easiest to add via a small test-only wrapper around the real backend rather than on the
//! backend type itself; see the `tests` module below for a worked example over
//! [`Headless`](crate::backend::Headless).
//!
//! # What this does not cover
//!
//! [`Output::needs_full_frame`] only takes effect through
//! [`Terminal::present`](crate::Terminal::present) when a backend also returns `true` from
//! [`Output::composites_layers`] (see that method's docs); a bare `Output` impl has no diffing of
//! its own to exercise, so that combination is instead pinned by a `Terminal`-level test rather
//! than by this module.

use crate::backend::{Cursor, CursorStyle, DrawCell, Input, Output};
use crate::color::Style;
use crate::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::grid::{Pos, Size};
use crate::tile::Tile;
use alloc::vec::Vec;
use core::time::Duration;
use ixy::HasSize;

/// Hashes `bytes` with FNV-1a (64-bit).
///
/// `core::hash::Hasher`/`std::hash::DefaultHasher` are either the wrong shape (no portable digest
/// guarantee) or unavailable at all under `no_std`, so [`Observable`] implementors get a small,
/// dependency-free digest instead. Not cryptographic, and not guaranteed stable across
/// `retroglyph-core` versions: only ever compared within a single test run, never persisted.
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// A backend that can report a digest of what changed since the last call.
///
/// See the module docs for why "since the last call", not the whole history or the whole current
/// state, is the contract every implementation has to meet.
pub trait Observable: Output {
    /// A digest of what changed since the previous call (or since construction, for the first
    /// call).
    fn snapshot(&mut self) -> u64;
}

/// One glyph cell with no grapheme text and no tint, for feeding [`Output::draw_layers`].
const fn cell(pos: Pos, tile: &Tile) -> DrawCell<'_> {
    DrawCell::new(pos, tile)
}

/// Draws `tile` at `pos` and flushes.
fn draw_one<B: Output>(backend: &mut B, pos: Pos, tile: &Tile) -> Result<(), B::Error> {
    backend.draw_layers(core::iter::once(cell(pos, tile)))?;
    backend.flush()
}

/// Unwraps an `Output` call's result, panicking with the error rather than requiring every call
/// site above to spell out its own `expect`.
fn expect<T, E: core::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("backend Output call failed: {error:?}"),
    }
}

/// Drives `B` through [`Output`]'s obligations: `make` must return a fresh backend sized to the
/// requested [`Size`], with no cells drawn yet.
///
/// # Panics
///
/// Panics on the first obligation `B` violates, or if any `Output` call returns `Err`
/// (`Observable` backends in this workspace are all infallible; a fallible one that fails here
/// has a bug this harness cannot usefully attribute, since it isn't the obligation under test).
pub fn assert_output_contract<B: Observable, F: FnMut(Size) -> B>(mut make: F) {
    let size = Size::new(4, 3);
    let a = Tile::new('A', Style::new());
    let b = Tile::new('B', Style::new());

    // `resize(size)` updates `size()`.
    {
        let mut backend = make(size);
        assert_eq!(
            backend.size(),
            size,
            "a freshly made backend must report the size it was made with"
        );
        let grown = Size::new(size.width() + 2, size.height() + 1);
        backend.resize(grown);
        assert_eq!(
            backend.size(),
            grown,
            "Output::resize(size) must update what Output::size() reports (retroglyph#763)"
        );
    }

    // `clear()` resets diff state so an identical redraw repaints.
    {
        let mut backend = make(size);
        expect(draw_one(&mut backend, Pos::new(0, 0), &a));
        let first_paint = backend.snapshot();
        expect(backend.clear());
        let _ = backend.snapshot(); // Not asserted on; only advances the "since last call" point.
        expect(draw_one(&mut backend, Pos::new(0, 0), &a));
        let second_paint = backend.snapshot();
        assert_eq!(
            second_paint, first_paint,
            "drawing identical content after clear() must repaint it, not silently skip it \
             because it matches an internal shadow copy from before the clear (retroglyph#763)"
        );
    }

    // `clear()` leaves no stale secondary state (SGR/damage/sprite state, etc).
    {
        let mut backend = make(size);
        expect(draw_one(&mut backend, Pos::new(0, 0), &b));
        let _ = backend.snapshot();
        expect(backend.clear());
        let _ = backend.snapshot();
        expect(draw_one(&mut backend, Pos::new(0, 0), &a));
        let after_clear = backend.snapshot();
        drop(backend); // Some backends (e.g. Crossterm) allow only one live instance at a time.

        let mut fresh = make(size);
        expect(draw_one(&mut fresh, Pos::new(0, 0), &a));
        let from_fresh = fresh.snapshot();

        assert_eq!(
            after_clear, from_fresh,
            "after clear(), drawing the same content a fresh backend would draw must produce \
             the same digest; a mismatch means clear() left stale secondary state (tracked SGR \
             attributes, damage flags, sprite layers, ...) behind (retroglyph#763)"
        );
    }

    // `resize(size)` invalidates shadow state.
    {
        let mut backend = make(size);
        expect(draw_one(&mut backend, Pos::new(0, 0), &b));
        let _ = backend.snapshot();
        let grown = Size::new(size.width() + 2, size.height() + 1);
        backend.resize(grown);
        let _ = backend.snapshot();
        expect(draw_one(&mut backend, Pos::new(0, 0), &a));
        let after_resize = backend.snapshot();
        drop(backend); // Some backends (e.g. Crossterm) allow only one live instance at a time.

        let mut fresh = make(grown);
        expect(draw_one(&mut fresh, Pos::new(0, 0), &a));
        let from_fresh = fresh.snapshot();

        assert_eq!(
            after_resize, from_fresh,
            "after resize(size), drawing the same content a fresh backend of the new size would \
             draw must produce the same digest; a mismatch means resize() left stale shadow \
             state behind (retroglyph#763)"
        );
    }

    // Out-of-range `DrawCell::pos` is silently dropped, not a panic and not sent to the display.
    {
        let mut backend = make(size);
        let far = Pos::new(size.width() + 50, size.height() + 50);
        expect(draw_one(&mut backend, far, &b));
        let out_of_range = backend.snapshot();
        drop(backend); // Some backends (e.g. Crossterm) allow only one live instance at a time.

        // Ground truth: drawing nothing at all. A backend that correctly drops an out-of-range
        // cell instead of sending it produces the exact same digest as this, since as far as the
        // display is concerned nothing happened either way.
        let mut fresh = make(size);
        expect(fresh.draw_layers(core::iter::empty()));
        expect(fresh.flush());
        let nothing_drawn = fresh.snapshot();

        assert_eq!(
            out_of_range, nothing_drawn,
            "a DrawCell positioned outside size() must not panic and must be silently dropped, \
             not sent to the display (retroglyph#763)"
        );
    }
}

/// Drives `B` through [`Cursor`]'s tracked-cursor obligation.
///
/// External writes (an app calling [`Cursor::set_cursor_position`] between two draws) must not
/// desync a backend's internal cursor tracking from where the cursor actually is (retroglyph#713).
///
/// # Panics
///
/// Panics if the tracked cursor desyncs, or if any `Output` call returns `Err`.
pub fn assert_cursor_contract<B: Observable + Cursor, F: FnMut(Size) -> B>(mut make: F) {
    let size = Size::new(5, 1);
    let a = Tile::new('A', Style::new());
    let c = Tile::new('C', Style::new());
    let b = Tile::new('B', Style::new());

    // Reference: the cursor only ever moves through ordinary draws, never through the `Cursor`
    // facet, so reaching position (1, 0) for the final draw here always correctly requires
    // whatever cursor-move a backend uses to get there. `c`, drawn and left at (4, 0), plays no
    // further part once its own delta is discarded below: it only exists so this run's *shape*
    // (two draws, then a third at a position that needs a move) matches the `Cursor`-facet run
    // next, without the two runs needing to agree on anything drawn earlier.
    let mut reference = make(size);
    expect(draw_one(&mut reference, Pos::new(0, 0), &a));
    expect(draw_one(&mut reference, Pos::new(4, 0), &c));
    let _ = reference.snapshot();
    expect(draw_one(&mut reference, Pos::new(1, 0), &b));
    let reference_delta = reference.snapshot();
    drop(reference); // Some backends (e.g. Crossterm) allow only one live instance at a time.

    // Same final draw, but the intervening move to column 4 goes through
    // `Cursor::set_cursor_position` instead of a draw. A backend that doesn't resync its own
    // tracked cursor on that call produces a different (missing the move) digest here than the
    // reference above, because it wrongly believes the cursor is still where the first draw left
    // it.
    let mut backend = make(size);
    expect(draw_one(&mut backend, Pos::new(0, 0), &a));
    backend.set_cursor_position(Pos::new(4, 0));
    let _ = backend.flush();
    let _ = backend.snapshot();
    expect(draw_one(&mut backend, Pos::new(1, 0), &b));
    let via_external_write = backend.snapshot();

    assert_eq!(
        via_external_write, reference_delta,
        "an external Cursor::set_cursor_position call must keep the backend's own tracked \
         cursor in sync with reality, the same as an ordinary draw does: the next draw must \
         still emit whatever cursor-move is needed to reach its position (retroglyph#713)"
    );
}

/// Every [`CursorStyle`] variant, in the order [`Cursor::set_cursor_style`]'s docs describe.
const CURSOR_STYLE_VARIANTS: [CursorStyle; 6] = [
    CursorStyle::BlinkingBlock,
    CursorStyle::SteadyBlock,
    CursorStyle::BlinkingUnderline,
    CursorStyle::SteadyUnderline,
    CursorStyle::BlinkingBar,
    CursorStyle::SteadyBar,
];

/// Drives `B` through [`Cursor::set_cursor_style`]'s obligation: each [`CursorStyle`] variant
/// must have its own distinct, observable effect (retroglyph#920).
///
/// `crossterm` and `terminal-wasm` each map every `CursorStyle` variant to a DECSCUSR parameter
/// via their own independent `match`, with no shared source of truth between the two; this
/// assertion doesn't compare backends against each other (their emitted bytes differ by design),
/// but it does pin, once per backend, that the six variants aren't accidentally collapsed onto
/// fewer than six distinct behaviors (e.g. two arms sharing a fallthrough).
///
/// # Panics
///
/// Panics if two distinct `CursorStyle` variants produce the same digest, or if any `Output`
/// call returns `Err`.
pub fn assert_cursor_style_contract<B: Observable + Cursor, F: FnMut(Size) -> B>(mut make: F) {
    let size = Size::new(5, 1);

    let mut backend = make(size);
    let _ = backend.snapshot(); // Discard whatever construction/resize emitted.

    let mut digests = Vec::with_capacity(CURSOR_STYLE_VARIANTS.len());
    for style in CURSOR_STYLE_VARIANTS {
        backend.set_cursor_style(style);
        let _ = backend.flush();
        digests.push(backend.snapshot());
    }

    for (i, &a) in digests.iter().enumerate() {
        for (j, &b) in digests.iter().enumerate().skip(i + 1) {
            assert_ne!(
                a, b,
                "CursorStyle::{:?} and CursorStyle::{:?} must produce distinct backend effects; \
                 a match arm has collided or fallen through (retroglyph#920)",
                CURSOR_STYLE_VARIANTS[i], CURSOR_STYLE_VARIANTS[j]
            );
        }
    }
}

/// Drives `B` through [`Input`]'s coalescing obligation.
///
/// A burst of consecutive `Event::Mouse(MouseEventKind::Moved)` pushes must collapse to the
/// latest one, matching [`coalesces_with`](crate::event::coalesces_with).
///
/// # Panics
///
/// Panics if the burst does not coalesce to exactly one event, or if that event isn't the last
/// one pushed.
pub fn assert_input_contract<B: Input, F: FnMut() -> B>(mut make: F) {
    const fn moved(x: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(x, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    let mut backend = make();
    for x in 0..32u16 {
        backend.push_event(moved(x));
    }
    assert_eq!(
        backend.poll_event(Duration::ZERO),
        Some(moved(31)),
        "a burst of consecutive Mouse(Moved) pushes must coalesce to the latest one (retroglyph#763)"
    );
    assert_eq!(
        backend.poll_event(Duration::ZERO),
        None,
        "the coalesced burst must have collapsed to exactly one queued event"
    );

    // A non-`Moved` event between two `Moved` bursts must not itself be swallowed.
    backend.push_event(moved(0));
    backend.push_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        position: Pos::new(0, 0),
        pixel_position: None,
        modifiers: KeyModifiers::NONE,
    }));
    backend.push_event(moved(1));
    assert!(matches!(
        backend.poll_event(Duration::ZERO),
        Some(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            ..
        }))
    ));
    assert!(matches!(
        backend.poll_event(Duration::ZERO),
        Some(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            ..
        }))
    ));
    assert_eq!(backend.poll_event(Duration::ZERO), Some(moved(1)));
    assert_eq!(backend.poll_event(Duration::ZERO), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Headless;
    use alloc::string::String;

    #[test]
    fn fnv1a_is_deterministic_and_input_sensitive() {
        assert_eq!(fnv1a(b"retroglyph"), fnv1a(b"retroglyph"));
        assert_ne!(fnv1a(b"retroglyph"), fnv1a(b"retroglyph!"));
        assert_ne!(fnv1a(b""), fnv1a(b"\0"));
    }

    /// Wraps [`Headless`] so [`Observable::snapshot`] hashes only what changed since the
    /// previous call, per the module docs. `Headless` is framebuffer-shaped (a `Grid`, replaced
    /// rather than appended to), so "changed" means "differs from the view remembered from the
    /// previous call": this remembers [`Headless::format_view`]'s output and hashes only the
    /// `(index, char)` pairs that differ from it, rather than the whole view every time.
    struct HeadlessObserver {
        backend: Headless,
        previous: String,
    }

    impl HeadlessObserver {
        fn new(width: u16, height: u16) -> Self {
            let backend = Headless::new(width, height);
            let previous = backend.format_view();
            Self { backend, previous }
        }
    }

    impl Output for HeadlessObserver {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            self.backend.draw_layers(content)
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.backend.flush()
        }

        fn size(&self) -> Size {
            self.backend.size()
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.backend.clear()
        }

        fn resize(&mut self, size: Size) {
            self.backend.resize(size);
        }
    }

    impl Cursor for HeadlessObserver {
        fn set_cursor_visible(&mut self, visible: bool) {
            self.backend.set_cursor_visible(visible);
        }

        fn set_cursor_position(&mut self, position: Pos) {
            self.backend.set_cursor_position(position);
        }
    }

    impl Observable for HeadlessObserver {
        fn snapshot(&mut self) -> u64 {
            let current = self.backend.format_view();
            let mut hash = fnv1a(b"headless-diff");
            for (index, (was, now)) in self.previous.chars().zip(current.chars()).enumerate() {
                if was != now {
                    hash ^= fnv1a(&(index as u64).to_ne_bytes());
                    hash ^= fnv1a(&(now as u32).to_ne_bytes());
                }
            }
            self.previous = current;
            hash
        }
    }

    #[test]
    fn headless_satisfies_the_output_contract() {
        assert_output_contract(|size| HeadlessObserver::new(size.width(), size.height()));
    }

    #[test]
    fn headless_satisfies_the_cursor_contract() {
        assert_cursor_contract(|size| HeadlessObserver::new(size.width(), size.height()));
    }

    #[test]
    fn headless_satisfies_the_input_contract() {
        assert_input_contract(|| Headless::new(10, 10));
    }
}
