//! 23: Burning keep
//!
//! A night siege scene, painted with colored cells rather than a raster image: a castle
//! silhouette on fire, smoke rolling off the walls, two dragons circling overhead, and a row of
//! graves on the hill in front. Every pixel is a background-filled [`Surface::put`] call, the
//! same "flat color as the unit of drawing" technique `20_overworld` uses for its terrain -- no
//! sprite sheet, no image asset, just per-cell [`Color`] math.
//!
//! Everything here is a pure function of `(x, y, elapsed)`, computed fresh every frame rather
//! than mutated in place: [`wall_top`] derives the castle's crenellated skyline (and its towers)
//! from `x` alone, [`hill_top`] derives the foreground silhouette the same way, and the fire's
//! flicker, the smoke's drift, and the dragons' wingbeats all come from [`flicker`]/[`sway`]
//! sampling `elapsed` and a per-cell hash -- see `20_overworld`'s noise module for the same
//! "no mutable generator state" idea applied to terrain instead of flame.
//!
//! The scene is a fixed 48x20 block of cells, not the full grid: every other example in the
//! gallery either fills whatever grid it's handed or explicitly scrolls a camera over one
//! (`20_overworld`), but this one is a single hand-composed tableau, so it stays a constant size
//! and simply gets clipped by [`Surface::put`] if the real grid is smaller than the default
//! 50x25 -- the same "assume the default, clip gracefully" approach `08_animation`'s fixed track
//! bounds take.
//!
//! ```sh
//! cargo run --example 23_burning_keep --features crossterm
//! cargo run --example 23_burning_keep --features software
//! cargo run --example 23_burning_keep  # headless fallback, prints a few frames to stdout
//! ```
//!
//! The fire burns and the dragons circle automatically. Keys: `q` or `Escape` quits, or close
//! the window.

#![allow(
    // This whole scene is deliberately formula-driven (see the module doc comment): every
    // color/flicker/drift value is a `sin`/`lerp` chain read as a formula, the same tradeoff
    // `20_overworld` makes for its terrain noise. Mechanically `mul_add`-ing each term would
    // trade that readability for a marginal precision/perf win nothing here needs.
    clippy::suboptimal_flops,
    // Every cell coordinate is a small `u16` (well under 100) and every ratio/color channel is
    // `[0, 1]`-ish, so the `f32`/`f64` <-> integer casts throughout are all in-range by
    // construction; see `20_overworld`'s module doc comment for the same call.
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use retroglyph_core::app::Frame;
use retroglyph_core::backend::Backend;
use retroglyph_core::color::{Color, Style};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;
use std::time::Duration;

/// Scene width/height, in cells -- see the module doc comment for why this is a fixed tableau
/// rather than a full-grid or camera-scrolled fill.
const WIDTH: u16 = 48;
const HEIGHT: u16 = 20;
/// Where the scene's local `(0, 0)` lands on the real grid, leaving room for the title above and
/// the key legend below.
const X_OFFSET: u16 = 1;
const Y_OFFSET: u16 = 2;
const INSTR_ROW: u16 = Y_OFFSET + HEIGHT + 1;

/// Left/right extent of the castle silhouette, in scene-local columns; outside this range the
/// column is open sky/hill, which is what leaves room for the dragons on the left of the scene.
const CASTLE_LEFT: u16 = 12;
const CASTLE_RIGHT: u16 = 46;
/// Row the plain crenellated wall top sits on, before any tower or merlon adjustment.
const WALL_TOP_ROW: u16 = 11;

/// One tower: `(center x, extra height above the plain wall, half-width)`. Columns within
/// `half_width` of `center` rise `extra` rows higher than [`WALL_TOP_ROW`], turning the flat
/// parapet into a proper keep skyline.
const TOWERS: [(u16, u16, u16); 3] = [(16, 3, 2), (29, 5, 2), (42, 3, 2)];

/// `(x range)` -- which stretches of wall are actually ablaze. Only these columns get
/// [`flame_glyph`]/smoke; the rest of the wall stays a cold silhouette, matching a fire that's
/// spread unevenly rather than engulfing the whole keep uniformly.
const FIRE_ZONES: [(u16, u16); 3] = [(14, 19), (26, 32), (38, 44)];
/// How many rows of flame rise above a burning column's wall top.
const FLAME_HEIGHT: u16 = 2;
/// How many rows of smoke rise above a burning column's flame tip.
const SMOKE_HEIGHT: u16 = 8;

/// Grave (`x` column) positions on the foreground hill.
const GRAVES: [u16; 5] = [3, 7, 22, 34, 45];
/// The flagpole's column, planted a little taller than the graves around it.
const FLAGPOLE_X: u16 = 37;
const FLAGPOLE_HEIGHT: u16 = 3;

/// `(x, y, breathes fire)` for the two circling dragons. The first breathes a streak of fire
/// down and to the right; the second is a plain silhouette, matching a lead-and-wing pair.
const DRAGONS: [(u16, u16, bool); 2] = [(7, 2, true), (4, 6, false)];

/// The near-black every color fades toward at range, so the scene reads as lit silhouettes
/// against night rather than flat color swatches -- the same fade `20_overworld`'s `NIGHT`
/// constant gives its biome fills.
const NIGHT: Color = Color::rgb(8, 6, 10);

/// A cheap deterministic hash of one cell to `[0, 1)`, for flicker/texture that should look
/// random but stay the same cell to cell and frame to frame (the animation comes from `elapsed`
/// alone, not from re-rolling this). Same bit-mixer shape as `20_overworld::noise::mix`.
fn hash01(x: u16, y: u16) -> f32 {
    let mut h = u32::from(x).wrapping_mul(0x9E37_79B9) ^ u32::from(y).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / f32::from(u16::MAX) / 256.0
}

/// A `[0, 1]` flicker value for cell `(x, y)` at `elapsed` seconds: each cell oscillates at its
/// own hashed frequency and phase, so a whole flame front doesn't pulse in lockstep.
fn flicker(x: u16, y: u16, elapsed: f64) -> f32 {
    let phase = f64::from(hash01(x, y)) * std::f64::consts::TAU;
    let freq = 6.0 + f64::from(hash01(y, x)) * 5.0;
    ((elapsed * freq + phase).sin() * 0.5 + 0.5) as f32
}

/// Horizontal drift for smoke/dragon motion: a slow sine in cells, so a column of smoke sways
/// rather than rising in a dead-straight line.
fn sway(seed: u16, row: u16, elapsed: f64) -> f32 {
    let phase = f64::from(hash01(seed, row));
    (elapsed * 0.6 + f64::from(row) * 0.25 + phase * std::f64::consts::TAU).sin() as f32
}

/// The row the castle silhouette's top edge sits on at column `x`, or `None` if `x` is outside
/// the castle's footprint (open sky/hill there instead). Crenellations are a plain `x % 4 < 2`
/// notch pattern; towers punch a locally taller (and un-notched, so their tops read as solid
/// masonry rather than teeth) block out of that pattern -- see [`TOWERS`].
fn wall_top(x: u16) -> Option<u16> {
    if !(CASTLE_LEFT..=CASTLE_RIGHT).contains(&x) {
        return None;
    }
    for &(center, extra, half_width) in &TOWERS {
        if x.abs_diff(center) <= half_width {
            return Some(WALL_TOP_ROW.saturating_sub(extra));
        }
    }
    let notch = u16::from(x % 4 < 2);
    Some(WALL_TOP_ROW + notch)
}

/// Whether column `x`'s wall is on fire.
fn is_burning(x: u16) -> bool {
    FIRE_ZONES.iter().any(|&(lo, hi)| (lo..=hi).contains(&x))
}

/// The row the foreground hill's silhouette top sits on at column `x`: two overlaid low-frequency
/// waves for a gentle rolling ridge, read as a formula rather than authored point by point (the
/// same "terrain is a function of x" idea [`wall_top`] uses for the castle), plus a small mound
/// under the flagpole so it reads as planted on a rise.
fn hill_top(x: u16) -> u16 {
    let fx = f32::from(x);
    let wave = (fx * 0.18).sin() * 1.2 + (fx * 0.05 + 1.0).sin() * 1.6;
    let mound = if FLAGPOLE_X.abs_diff(x) <= 4 {
        3.0 - f32::from(FLAGPOLE_X.abs_diff(x))
    } else {
        0.0
    };
    (16.0 - wave - mound).clamp(13.0, f32::from(HEIGHT - 1)) as u16
}

/// Picks a flame glyph/color for one burning cell: brighter and more yellow near its own
/// flicker peak, redder and dimmer in the troughs.
fn flame_glyph(x: u16, y: u16, elapsed: f64) -> (char, Color) {
    let v = flicker(x, y, elapsed);
    let ch = if v > 0.75 {
        '▲'
    } else if v > 0.4 {
        '^'
    } else {
        '.'
    };
    (
        ch,
        Color::lerp(Color::rgb(200, 40, 10), Color::rgb(255, 214, 90), v),
    )
}

/// Picks a smoke glyph/color for one cell above a fire, thinning (lower density, darker) with
/// height above the flame tip.
fn smoke_glyph(density: f32) -> (char, Color) {
    let ch = if density > 0.6 {
        '▓'
    } else if density > 0.3 {
        '▒'
    } else {
        '░'
    };
    (
        ch,
        Color::lerp(
            Color::rgb(58, 52, 56),
            Color::rgb(20, 18, 22),
            1.0 - density,
        ),
    )
}

/// Sky color at scene-local `(x, y)`: a cool dusk gradient from top to bottom, warmed near the
/// burning stretches of wall the way a real fire lights the smoke and haze above it.
fn sky_color(x: u16, y: u16) -> Color {
    let t = f32::from(y) / f32::from(HEIGHT);
    let base = Color::lerp(Color::rgb(58, 48, 66), Color::rgb(96, 78, 84), t);

    let near_fire = FIRE_ZONES
        .iter()
        .map(|&(lo, hi)| x.abs_diff(u16::midpoint(lo, hi)))
        .min()
        .unwrap_or(u16::MAX);
    let glow = (1.0 - f32::from(near_fire) / 16.0).clamp(0.0, 1.0) * (1.0 - t * 0.6);
    Color::lerp(base, Color::rgb(150, 84, 46), glow * 0.35)
}

/// The foreground hill silhouette at scene-local `(x, y)`: bare hill, a grave marker at its very
/// top row, or the flagpole/flag.
fn hill_cell(x: u16, y: u16) -> (char, Style) {
    let top = hill_top(x);
    let depth = (f32::from(y - top) / 6.0).min(1.0);
    let ground = Color::lerp(Color::rgb(20, 16, 18), Color::BLACK, depth);

    if x == FLAGPOLE_X {
        let pole_top = top.saturating_sub(FLAGPOLE_HEIGHT);
        if y == pole_top {
            return ('▶', Style::new().fg(Color::rgb(40, 32, 34)).bg(ground));
        }
        if y > pole_top && y < top {
            return ('|', Style::new().fg(Color::rgb(46, 36, 38)).bg(ground));
        }
    }

    if y == top && GRAVES.contains(&x) {
        return ('†', Style::new().fg(Color::rgb(42, 34, 36)).bg(ground));
    }

    // A solid block rather than a bare space + background: the headless text backend has no
    // color to lean on, so the hill (and, below, the castle wall) needs a real glyph to read as
    // a filled silhouette instead of vanishing into the sky's own blank cells.
    ('█', Style::new().fg(ground).bg(ground))
}

/// If scene-local `(x, y)` falls on a dragon's small silhouette (or, for the lead dragon, its
/// fire breath), returns its glyph and color; `None` leaves the cell to whatever layer is under
/// it.
///
/// Each dragon flaps between two wing shapes a little over twice a second and drifts up and down
/// slowly via [`sway`], reusing the same drift function smoke uses for the same "slow sine,
/// per-instance phase" reason.
fn dragon_cell(x: u16, y: u16, t: f64) -> Option<(char, Color)> {
    for (i, &(dx, dy_base, breathes)) in DRAGONS.iter().enumerate() {
        let seed = 200 + i as u16;
        let bob = sway(seed, 0, t * 0.7) * 1.2;
        let dy = (f32::from(dy_base) + bob).round().max(0.0) as u16;

        let flap_up = (t * 2.2 + f64::from(u16::try_from(i).unwrap_or(u16::MAX)) * 0.7).sin() > 0.0;
        let (offsets, ch): (&[(i16, i16)], char) = if flap_up {
            (&[(-1, 0), (0, -1), (1, 0)], '^')
        } else {
            (&[(-1, -1), (0, 0), (1, -1)], 'v')
        };
        for &(ox, oy) in offsets {
            let cx = i32::from(dx) + i32::from(ox);
            let cy = i32::from(dy) + i32::from(oy);
            if cx == i32::from(x) && cy == i32::from(y) {
                return Some((ch, Color::rgb(18, 15, 20)));
            }
        }
        if breathes {
            for step in 1..=6i32 {
                let bx = i32::from(dx) + step;
                let by = i32::from(dy) + step / 2;
                if bx == i32::from(x) && by == i32::from(y) {
                    let fade = 1.0 - step as f32 / 6.0;
                    let flick = flicker(x, y, t);
                    let color =
                        Color::lerp(Color::rgb(120, 30, 10), Color::rgb(255, 160, 60), flick);
                    return Some(('-', Color::lerp(NIGHT, color, fade)));
                }
            }
        }
    }
    None
}

/// The glyph/style for one scene-local cell, computed layer by layer: sky (with a warm glow near
/// the fire), then castle/flame/smoke, then the foreground hill and its graves/flag, then the
/// dragons on top -- whichever layer claims the cell first wins, back to front.
fn render_cell(x: u16, y: u16, t: f64) -> (char, Style) {
    if let Some((ch, fg)) = dragon_cell(x, y, t) {
        return (ch, Style::new().fg(fg).bg(sky_color(x, y)));
    }

    if y >= hill_top(x) {
        return hill_cell(x, y);
    }

    if let Some(top) = wall_top(x) {
        if is_burning(x) {
            let fire_top = top.saturating_sub(FLAME_HEIGHT);
            if y >= fire_top && y < top {
                let (ch, fg) = flame_glyph(x, y, t);
                let bg = Color::lerp(fg, NIGHT, 0.5);
                return (ch, Style::new().fg(fg).bg(bg));
            }
            let smoke_top = fire_top.saturating_sub(SMOKE_HEIGHT);
            if y >= smoke_top && y < fire_top {
                let density = 1.0 - f32::from(fire_top - y) / f32::from(SMOKE_HEIGHT);
                let drift = sway(x, y, t) * 1.5;
                let sx = (f32::from(x) + drift).round() as i32;
                if sx == i32::from(x) {
                    let (ch, fg) = smoke_glyph(density * 0.7);
                    return (ch, Style::new().fg(fg).bg(sky_color(x, y)));
                }
            }
        }
        if y >= top {
            let shade = 0.55 + 0.1 * hash01(x, y);
            let wall = Color::lerp(Color::rgb(46, 34, 30), NIGHT, shade);
            return ('█', Style::new().fg(wall).bg(wall));
        }
    }

    (' ', Style::new().bg(sky_color(x, y)))
}

/// State for the burning-keep example: just the running clock.
///
/// Everything drawn is a pure function of `elapsed` and cell coordinates (see the module doc
/// comment), so there's nothing else to track between frames.
#[derive(Default)]
pub struct BurningKeep {
    elapsed: Duration,
}

impl BurningKeep {
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

    /// Draws the title, the whole scene (see [`render_cell`]), and the key legend.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let t = self.elapsed.as_secs_f64();
        let mut surface = term.surface();

        surface.print((1, 0), "A keep burns in the night.", Style::default());
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let (ch, style) = render_cell(x, y, t);
                surface.put((X_OFFSET + x, Y_OFFSET + y), ch, style);
            }
        }
        surface.print((1, INSTR_ROW), "q / Escape quits.", Style::default());
    }
}

impl Example for BurningKeep {
    const NAME: &'static str = "23_burning_keep";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.elapsed += frame.delta;
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(BurningKeep);
