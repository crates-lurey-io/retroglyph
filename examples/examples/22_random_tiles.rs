//! 22: Random tiles
//!
//! A full-buffer stress/capability example: every cell in the grid gets a fresh random glyph
//! (a small curated CP437-ish set of digits, letters, and box-drawing/symbol characters that
//! render the same across every backend's font) and a random bright [`Color::Rgb`] foreground
//! on a pure black background, every single frame. Unlike most examples here, which draw a
//! mostly-static scene and only touch the handful of cells that actually changed, this rewrites
//! every cell every tick -- it proves [`Surface::put`] and each backend's redraw path handle a
//! full-grid, no-diffing write correctly. It makes no throughput claim (see
//! `examples/AGENTS.md`'s "No perf/benchmark examples here"); how many cells a backend can
//! actually redraw per second belongs in a `cargo bench` setup, not here.
//!
//! Randomness is a tiny seeded xorshift-style hash (see [`next_u32`]), not a `rand`/`fastrand`
//! runtime dependency: it keeps this crate free of one, and -- since each frame reseeds from
//! [`SEED`] plus [`Frame::frame`] rather than wall-clock time or carried-over state -- makes
//! every frame's output fully reproducible. The same frame number always renders the same
//! grid, which is what lets this example's snapshots pin exact bytes instead of needing
//! `20_overworld`'s substring-assertion workaround for genuinely non-deterministic output.
//!
//! ```sh
//! cargo run --example 22_random_tiles --features crossterm
//! cargo run --example 22_random_tiles --features software
//! cargo run --example 22_random_tiles  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `q` or `Escape` quits, or close the window.

use retroglyph::backend::Backend;
use retroglyph::color::{Color, Style};
use retroglyph::event::{Event, KeyCode};
use retroglyph::grid::HasSize;
use retroglyph::terminal::Terminal;
use retroglyph_examples::Example;

/// Curated glyph set: digits, uppercase letters, and a handful of symbol/punctuation glyphs
/// present in every backend's font (the embedded default bitmap font, a real terminal's font,
/// and a browser's monospace font) -- nothing here is specific to one.
const GLYPHS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '#', '@',
    '%', '&', '*', '+', '-', '/', '\\', '|', '~', '^', '<', '>', '?',
];

/// Fixed PRNG seed: every run (and every committed snapshot) is deterministic from here plus
/// the frame number -- see the top doc comment.
const SEED: u32 = 0x5EED_1176;

/// Narrowest channel value a random foreground color can land on, keeping every swatch legible
/// against the pure black background instead of drifting toward a barely-visible dark color.
const CHANNEL_MIN: u32 = 96;
/// `256 - CHANNEL_MIN`: the span [`next_u32`]'s channel roll is reduced into.
const CHANNEL_SPAN: u32 = 256 - CHANNEL_MIN;

/// One step of a small xorshift hash PRNG, in the same "stable hash, not a crypto RNG" spirit
/// as `20_overworld`'s `noise::hash01`: cheap, dependency-free, and deterministic for the same
/// input. Advances `state` in place and returns the next pseudo-random `u32`.
const fn next_u32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// State for the random-tiles example (none needed: every frame is reseeded from [`SEED`] plus
/// the frame number, not carried forward -- see the top doc comment).
#[derive(Default)]
pub struct RandomTiles;

impl RandomTiles {
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

    /// Randomizes every cell in the grid: a [`GLYPHS`] pick, a random bright `Rgb` foreground,
    /// pure black background. The PRNG is freshly seeded from [`SEED`] and `frame` on every
    /// call, so the same frame number always renders the same grid.
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>, frame: u64) {
        let size = term.size();
        let mut surface = term.surface();

        #[allow(clippy::cast_possible_truncation)]
        let mut state = SEED ^ (frame as u32).wrapping_mul(0x9E37_79B9);
        if state == 0 {
            // xorshift is stuck at 0 forever if seeded there; SEED and the multiplier above
            // never actually produce 0, but stay correct even if a future SEED value did.
            state = 1;
        }

        for y in 0..size.height() {
            for x in 0..size.width() {
                let glyph = GLYPHS[next_u32(&mut state) as usize % GLYPHS.len()];
                // `CHANNEL_MIN + next_u32(s) % CHANNEL_SPAN` is always `96..256`, so this never
                // truncates.
                let channel = |s: &mut u32| {
                    u8::try_from(CHANNEL_MIN + next_u32(s) % CHANNEL_SPAN)
                        .expect("96..256 fits in u8")
                };
                let fg = Color::Rgb {
                    r: channel(&mut state),
                    g: channel(&mut state),
                    b: channel(&mut state),
                };
                let style = Style::new().fg(fg).bg(Color::Rgb { r: 0, g: 0, b: 0 });
                surface.put((x, y), glyph, style);
            }
        }
    }
}

impl Example for RandomTiles {
    const NAME: &'static str = "22_random_tiles";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &retroglyph::app::Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term, frame.frame);
        true
    }
}

retroglyph_examples::example_main!(RandomTiles);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_and_frame_is_deterministic() {
        let mut a = SEED ^ 7u32.wrapping_mul(0x9E37_79B9);
        let mut b = SEED ^ 7u32.wrapping_mul(0x9E37_79B9);
        for _ in 0..64 {
            assert_eq!(next_u32(&mut a), next_u32(&mut b));
        }
    }

    #[test]
    fn different_frames_diverge() {
        let mut a = SEED ^ 1u32.wrapping_mul(0x9E37_79B9);
        let mut b = SEED ^ 2u32.wrapping_mul(0x9E37_79B9);
        assert_ne!(next_u32(&mut a), next_u32(&mut b));
    }

    #[test]
    fn next_u32_visits_more_than_one_value() {
        let mut state = 1u32;
        let first = next_u32(&mut state);
        let second = next_u32(&mut state);
        assert_ne!(first, second, "xorshift must not be stuck on repeat");
    }
}
