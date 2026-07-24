//! 08: Animation
//!
//! [`Tween`] plus [`FrameClock`] driving sub-cell [`Terminal::put_offset`]. A ball travels once
//! across the track and back: [`FrameClock`] fires at [`BOUNCE_HZ`], and its one reversal
//! retargets a [`Tween`] toward the opposite end with [`Easing::EaseInOutCubic`], the same
//! "fixed-rate logic step, continuously-interpolated visual" split `06_layers` uses for its
//! discrete glyph steps -- except here the value in between two steps is what actually gets
//! drawn, not just the step itself. The tween's fractional cell position, converted to a
//! sub-cell pixel offset via [`Terminal::put_offset`], is what makes the motion smooth on the
//! software backend instead of visibly snapping from cell to cell.
//!
//! Sub-cell offsets are visual-only pixel nudges a backend may or may not represent (see
//! [`Terminal::put_offset`]'s own doc comment): the software backend renders the true
//! in-between position; the crossterm and headless backends silently ignore the offset and
//! only redraw the whole-cell position, so the same continuous motion looks like discrete
//! per-cell hops there instead of true sliding -- graceful degradation with no example-side
//! fallback code required, the same way `07_sprites_tileset`'s ASCII glyphs need none.
//!
//! Like `06_layers`, this parks rather than looping forever: after one full right-then-left
//! round trip the tween is already finished (`Tween::update` is a no-op past its duration), so
//! the ball settles at the left end and the frame stays put from then on -- a reproducible
//! resting state for every capture (a screenshot, or this crate's own crossterm SVG snapshot
//! test) to land on, instead of an arbitrary, machine-speed-dependent mid-bounce position.
//!
//! ```sh
//! cargo run --example 08_animation --features crossterm
//! cargo run --example 08_animation --features software
//! cargo run --example 08_animation  # headless fallback, prints a few frames to stdout
//! ```
//!
//! The ball travels automatically; `q` or `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{
    AnsiColor, Backend, Color, Easing, Frame, FrameClock, Style, Terminal, Tween,
};
use retroglyph_examples::Example;

/// Row the ball travels along.
const TRACK_ROW: u16 = 12;
/// Leftmost and rightmost cell the ball's center can reach.
const TRACK_LEFT: u16 = 1;
const TRACK_RIGHT: u16 = 48;
/// How wide one grid cell is in the software backend's default embedded font, in pixels --
/// see `crates/window/src/font.rs`'s `FONT` constant. [`Terminal::put_offset`]'s
/// `dx`/`dy` are raw pixel units at this scale, so a full cell of horizontal travel is exactly
/// `CELL_W_PX` of offset.
const CELL_W_PX: f32 = 8.0;
/// How often [`FrameClock`] retargets the tween toward the opposite end of the track. Slow
/// enough that a human watching the software backend sees continuous sliding motion rather than
/// a blur, fast enough that the headless/crossterm whole-cell fallback still reads as "bouncing"
/// rather than "teleporting."
const BOUNCE_HZ: u32 = 1;

/// State for the animation example.
///
/// A fixed-rate clock gates when the tween retargets, alongside the tween itself and how many
/// times the clock has fired (see the module doc comment: after the first reversal, the tween is
/// left to settle rather than being retargeted again).
pub struct Animation {
    clock: FrameClock,
    position: Tween,
    bounces: u32,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            clock: FrameClock::new(BOUNCE_HZ),
            position: Tween::new(f32::from(TRACK_LEFT), f32::from(TRACK_RIGHT))
                .duration(std::time::Duration::from_secs(1))
                .easing(Easing::EaseInOutCubic),
            bounces: 0,
        }
    }
}

impl Animation {
    /// Drains pending input, returning `false` if the user asked to quit.
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) => {
                    return false;
                }
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws this frame and presents it: the track, then the ball at its tweened position, with
    /// a sub-cell pixel offset for the fractional part of that position.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(
            1,
            1,
            "A ball travels the track below and back; q / Escape quits.",
        );
        // `bounces == 1` is the moment the ball *reaches* the right end and starts heading
        // back -- not yet parked, still a full second of return travel left. The tween (and
        // the ball) only actually settles at TRACK_LEFT once the second clock fire completes
        // that return trip.
        if self.bounces >= 2 {
            term.print(1, 2, "(parked at left end)");
        }

        let track_style = Style::new().fg(Color::Ansi(AnsiColor::BrightBlack));
        for x in TRACK_LEFT..=TRACK_RIGHT {
            term.put_styled(x, TRACK_ROW, '-', track_style);
        }

        let pos = self.position.value();
        let cell = pos.floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cell_x = cell as u16;
        #[allow(clippy::cast_possible_truncation)]
        let dx = ((pos - cell) * CELL_W_PX) as i16;

        term.reset_style()
            .fg(Color::Ansi(AnsiColor::BrightYellow))
            .bg(Color::Default);
        term.put_offset(cell_x, TRACK_ROW, dx, 0, 'o');

        term.present().ok();
    }
}

impl Example for Animation {
    const NAME: &'static str = "08_animation";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);

        self.position.update(frame.delta);
        self.clock.advance(frame.delta);
        while self.clock.tick() {
            self.bounces += 1;
            // Only the first reversal retargets (right -> left); the second clock fire, one
            // round trip later, is left alone so the tween (already finished by then) just
            // stays parked at TRACK_LEFT -- see the module doc comment.
            if self.bounces == 1 {
                self.position.retarget(f32::from(TRACK_LEFT));
            }
        }
        true
    }
}

retroglyph_examples::example_main!(Animation);
