//! 08: Animation
//!
//! [`Tween`] plus [`FrameClock`] driving sub-cell [`Surface::put_offset`]. A ball travels once
//! across the track and back. [`FrameClock`] fires at [`BOUNCE_HZ`], and its one reversal
//! retargets a [`Tween`] toward the opposite end with [`Easing::EaseInOutCubic`]: the same
//! fixed-rate logic step, continuously interpolated visual split that `06_layers` uses for its
//! discrete glyph steps, except here the value in between two steps is what actually gets
//! drawn, not just the step itself. The tween's fractional cell position, converted to a
//! sub-cell pixel offset via [`Surface::put_offset`], is what makes the motion smooth on the
//! software backend instead of visibly snapping from cell to cell.
//!
//! Sub-cell offsets are visual-only pixel nudges a backend may or may not represent (see
//! [`Surface::put_offset`]'s own doc comment). The software backend renders the true
//! in-between position; the crossterm and headless backends silently ignore the offset and only
//! redraw the whole-cell position, so the same continuous motion looks like discrete per-cell
//! hops there instead of true sliding. No example-side fallback code is needed, the same way
//! `07_sprites_tileset`'s ASCII glyphs need none.
//!
//! Like `06_layers`, this parks rather than looping forever: after one full right-then-left
//! round trip the tween is already finished (`Tween::update` is a no-op past its duration), so
//! the ball settles at the left end and the frame stays put from then on. That gives every
//! capture, a screenshot or this crate's own crossterm SVG snapshot test, a reproducible resting
//! state to land on instead of an arbitrary, machine-speed-dependent mid-bounce position.
//!
//! Below the track, three status dots pulse continuously (no start or end, unlike the tween
//! above) via [`oscillate_with_phase`]: same shared `elapsed` and [`DOT_PERIOD`], but each dot
//! is given a different [`DOT_PHASES`] offset, so they light up out of sync with each other
//! instead of in lockstep -- the scenario plain [`oscillate`] alone can't express.
//!
//! ```sh
//! cargo run --example 08_animation --features crossterm
//! cargo run --example 08_animation --features software
//! cargo run --example 08_animation  # headless fallback, prints a few frames to stdout
//! ```
//!
//! The ball travels automatically. Keys: `q` or `Escape` quits, or close the window.

use retroglyph_core::app::Frame;
use retroglyph_core::backend::Backend;
use retroglyph_core::color::{AnsiColor, Color, Style};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::frames::FrameClock;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;
use retroglyph_ui::{Easing, Tween, oscillate_with_phase};
use std::time::Duration;

/// Row the ball travels along.
const TRACK_ROW: u16 = 12;
/// Leftmost and rightmost cell the ball's center can reach.
const TRACK_LEFT: u16 = 1;
const TRACK_RIGHT: u16 = 48;
/// How wide one grid cell is in the software backend's default embedded font, in pixels --
/// see `crates/window/src/font.rs`'s `FONT` constant. [`Surface::put_offset`]'s
/// `dx`/`dy` are raw pixel units at this scale, so a full cell of horizontal travel is exactly
/// `CELL_W_PX` of offset.
const CELL_W_PX: f32 = 8.0;
/// How often [`FrameClock`] retargets the tween toward the opposite end of the track. Slow
/// enough that a human watching the software backend sees continuous sliding motion rather than
/// a blur, fast enough that the headless/crossterm whole-cell fallback still reads as "bouncing"
/// rather than "teleporting."
const BOUNCE_HZ: u32 = 1;

/// Row the status dots pulse on, just below the track.
const DOT_ROW: u16 = TRACK_ROW + 2;
/// Column of the first status dot; each subsequent dot is two cells to the right.
const DOT_LEFT: u16 = TRACK_LEFT;
/// How long each status dot takes to complete one pulse cycle.
const DOT_PERIOD: Duration = Duration::from_secs(3);
/// Per-dot phase offsets (in full cycles) passed to [`oscillate_with_phase`]: evenly spread
/// across one cycle, so with a shared `elapsed` and [`DOT_PERIOD`] the three dots are always a
/// third of a cycle apart instead of pulsing in lockstep.
const DOT_PHASES: [f32; 3] = [0.0, 1.0 / 3.0, 2.0 / 3.0];

/// State for the animation example.
///
/// A fixed-rate clock gates when the tween retargets, alongside the tween itself and how many
/// times the clock has fired (see the module doc comment: after the first reversal, the tween is
/// left to settle rather than being retargeted again). `elapsed` accumulates every frame's delta
/// and feeds the status dots' [`oscillate_with_phase`] calls -- unlike the tween, it never
/// finishes, so the dots keep pulsing even after the ball parks.
pub struct Animation {
    clock: FrameClock,
    position: Tween,
    bounces: u32,
    elapsed: Duration,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            clock: FrameClock::new(BOUNCE_HZ),
            position: Tween::new(f32::from(TRACK_LEFT), f32::from(TRACK_RIGHT))
                .duration(Duration::from_secs(1))
                .easing(Easing::EaseInOutCubic),
            bounces: 0,
            elapsed: Duration::ZERO,
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

    /// Draws this frame (the driver presents): the track, then the ball at its tweened position, with
    /// a sub-cell pixel offset for the fractional part of that position.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let mut surface = term.surface();
        surface.print(
            (1, 1),
            "A ball travels the track below and back; q / Escape quits.",
            Style::default(),
        );
        // `bounces == 1` is the moment the ball *reaches* the right end and starts heading
        // back -- not yet parked, still a full second of return travel left. The tween (and
        // the ball) only actually settles at TRACK_LEFT once the second clock fire completes
        // that return trip.
        if self.bounces >= 2 {
            surface.print((1, 2), "(parked at left end)", Style::default());
        }

        let track_style = Style::new().fg(Color::Ansi(AnsiColor::BrightBlack));
        for x in TRACK_LEFT..=TRACK_RIGHT {
            surface.put((x, TRACK_ROW), '-', track_style);
        }

        let pos = self.position.value();
        let cell = pos.floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cell_x = cell as u16;
        #[allow(clippy::cast_possible_truncation)]
        let dx = ((pos - cell) * CELL_W_PX) as i16;

        let ball_style = Style::new()
            .fg(Color::Ansi(AnsiColor::BrightYellow))
            .bg(Color::Default);
        surface.put_offset((cell_x, TRACK_ROW), (dx, 0), 'o', ball_style);

        surface.print((1, DOT_ROW - 1), "Status:", Style::default());
        let dot_style = Style::new().fg(Color::Ansi(AnsiColor::BrightGreen));
        for (i, phase) in DOT_PHASES.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let x = DOT_LEFT + (i as u16) * 2;
            let value = oscillate_with_phase(self.elapsed, DOT_PERIOD, *phase);
            surface.put((x, DOT_ROW), dot_glyph(value), dot_style);
        }
    }
}

/// Maps an `oscillate`-style `0.0..=1.0` value to a small ASCII brightness ramp, dimmest to
/// brightest, so a pulsing dot reads as a fade rather than an on/off blink.
fn dot_glyph(value: f32) -> char {
    match value {
        v if v < 0.2 => '.',
        v if v < 0.4 => 'o',
        v if v < 0.6 => 'O',
        v if v < 0.8 => '0',
        _ => '@',
    }
}

impl Example for Animation {
    const NAME: &'static str = "08_animation";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);

        // Stops accumulating once parked (`bounces >= 2`): a real-time SVG capture (see this
        // example's own test) keeps rendering frames for an arbitrary, machine-speed-dependent
        // stretch after "parked at left end" appears, waiting for the capture to notice and
        // stop. Letting `elapsed` keep growing through that window would make `dot_glyph`'s
        // captured output as unreproducible as the ball's position would be if `position` never
        // clamped -- see the module doc comment for the same idea applied to the tween.
        if self.bounces < 2 {
            self.elapsed += frame.delta;
        }
        self.position.update(frame.delta);
        self.clock.advance(frame.delta);
        while self.clock.tick() {
            self.bounces += 1;
            // Only the first reversal retargets (right -> left); the second clock fire, one
            // round trip later, is left alone so the tween (already finished by then) just
            // stays parked at TRACK_LEFT -- see the module doc comment.
            if self.bounces == 1 {
                self.position.retarget(f32::from(TRACK_LEFT));
            } else if self.bounces == 2 {
                // Pin to the exact logical time two `BOUNCE_HZ` periods represent, rather than
                // whatever real wall-clock total `elapsed` happened to reach this frame -- see
                // the comment above `if self.bounces < 2` for why that matters.
                self.elapsed = Duration::from_secs(2);
            }
        }
        true
    }
}

retroglyph_examples::example_main!(Animation);
