//! Sprite stress test: measures alpha-blended sprite throughput.
//!
//! Renders up to [`MAX_SPRITES`] independently-moving sprites via the tileset
//! system. Use this to profile and optimise `SoftwareRenderer`.
//!
//! - `+`/`=` key   increase sprite count by 50
//! - `-` key        decrease sprite count by 50
//! - `d` key        dump frame-time samples to stdout (CSV)
//! - `Q` / Escape   quit
//!
//! Run with:
//! ```sh
//! cargo run --example sprite_stress \
//!     --features software-tilesets,software-default-font --release
//! ```

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::style::Style;
use retroglyph_core::{Backend, Color, Terminal};
use retroglyph_examples::util::lcg::Lcg;
use retroglyph_examples::util::perf::PerfOverlay;
#[cfg(feature = "software")]
use retroglyph_software::SoftwareBackendBuilder;
#[cfg(feature = "tilesets")]
use retroglyph_software::tileset::TilesetOptions;

// ── Sprite sheet ──────────────────────────────────────────────────────────────
//
// Four 8×8 one-bit sprites packed into a single RGBA PNG.  Copied from
// `tileset.rs` so each example remains self-contained.

#[cfg(feature = "tilesets")]
const SPRITE_SWORD: [u8; 8] = [
    0b0000_0011,
    0b0000_0110,
    0b0000_1100,
    0b0001_1000,
    0b0011_0000,
    0b0110_0000,
    0b1101_0000,
    0b1001_0000,
];

#[cfg(feature = "tilesets")]
const SPRITE_POTION: [u8; 8] = [
    0b0001_1000,
    0b0001_1000,
    0b0011_1100,
    0b0111_1110,
    0b0111_1110,
    0b0111_1110,
    0b0011_1100,
    0b0001_1000,
];

#[cfg(feature = "tilesets")]
const SPRITE_SKULL: [u8; 8] = [
    0b0001_1000,
    0b0011_1100,
    0b0111_1110,
    0b0111_1110,
    0b0001_1000,
    0b0010_0100,
    0b0100_0010,
    0b0011_1100,
];

#[cfg(feature = "tilesets")]
const SPRITE_COIN: [u8; 8] = [
    0b0011_1100,
    0b0100_0010,
    0b1001_1001,
    0b1010_0101,
    0b1010_0101,
    0b1001_1001,
    0b0100_0010,
    0b0011_1100,
];

/// Number of distinct sprite tiles in the generated sheet (kept in sync
/// with `SPRITE_DEFS` below, which only exists under software-tilesets).
const SPRITE_COUNT: u32 = 4;

#[cfg(feature = "tilesets")]
struct SpriteDef {
    bits: &'static [u8; 8],
    color: (u8, u8, u8),
}

#[cfg(feature = "tilesets")]
const SPRITE_DEFS: &[SpriteDef] = &[
    SpriteDef {
        bits: &SPRITE_SWORD,
        color: (200, 200, 220),
    },
    SpriteDef {
        bits: &SPRITE_POTION,
        color: (80, 200, 120),
    },
    SpriteDef {
        bits: &SPRITE_SKULL,
        color: (200, 200, 200),
    },
    SpriteDef {
        bits: &SPRITE_COIN,
        color: (220, 200, 40),
    },
];

/// Build a 4-tile RGBA PNG sprite sheet from the bit-pattern definitions above.
#[cfg(feature = "tilesets")]
fn make_sprite_sheet() -> Vec<u8> {
    use image::ImageEncoder;

    let tile_w = 8u32;
    let tile_h = 8u32;
    let cols = SPRITE_COUNT;
    let (img_w, img_h) = (tile_w * cols, tile_h);
    let mut pixels = vec![0u8; (img_w * img_h * 4) as usize];

    for (i, def) in SPRITE_DEFS.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let col = i as u32;
        let (fr, fg, fb) = def.color;
        for py in 0..tile_h {
            let row_bits = def.bits[py as usize];
            for px in 0..tile_w {
                let idx = ((py * img_w) + col * tile_w + px) as usize * 4;
                if (row_bits >> (7 - px)) & 1 != 0 {
                    pixels[idx] = fr;
                    pixels[idx + 1] = fg;
                    pixels[idx + 2] = fb;
                    pixels[idx + 3] = 255;
                }
                // transparent pixels stay zeroed
            }
        }
    }

    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, img_w, img_h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out.into_inner()
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Maximum number of sprites that can be active at once.
const MAX_SPRITES: usize = 2000;

/// Colour palette for sprites — picked by the LCG.
const PALETTE: &[Color] = &[
    Color::BRIGHT_RED,
    Color::BRIGHT_GREEN,
    Color::BRIGHT_YELLOW,
    Color::BRIGHT_BLUE,
    Color::BRIGHT_MAGENTA,
    Color::BRIGHT_CYAN,
    Color::BRIGHT_WHITE,
    Color::Rgb {
        r: 255,
        g: 128,
        b: 0,
    },
    Color::Rgb {
        r: 128,
        g: 0,
        b: 255,
    },
    Color::Rgb {
        r: 0,
        g: 255,
        b: 128,
    },
    Color::Rgb {
        r: 255,
        g: 200,
        b: 80,
    },
    Color::Rgb {
        r: 80,
        g: 200,
        b: 255,
    },
];

/// A single bouncing sprite.
struct Sprite {
    /// Horizontal position in cells (float for smooth sub-cell movement).
    x: f32,
    /// Vertical position in cells.
    y: f32,
    /// Horizontal velocity in cells/frame.
    vx: f32,
    /// Vertical velocity in cells/frame.
    vy: f32,
    /// Index into the PUA tileset (0 = sword, 1 = potion, 2 = skull, 3 = coin).
    tile: u32,
    /// Foreground colour applied to the sprite tile.
    color: Color,
}

/// Full demo state.
struct StressState {
    sprites: Vec<Sprite>,
    frame: u64,
    perf: PerfOverlay,
    /// Number of sprites currently rendered (1..=[`MAX_SPRITES`]).
    count: usize,
}

impl StressState {
    /// Allocate `n` sprites with deterministic positions and velocities.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn new(n: usize) -> Self {
        let count = n.clamp(1, MAX_SPRITES);
        let mut rng = Lcg::new(42);
        let mut sprites = Vec::with_capacity(MAX_SPRITES);

        for _ in 0..MAX_SPRITES {
            // Position: anywhere in an 80×40 notional grid.
            let x = (rng.next() % 80) as f32;
            let y = (rng.next() % 40) as f32;

            // Velocity magnitude: 0.3–1.3 cells/frame, random sign.
            let horiz = (rng.next() % 10 + 3) as f32 * 0.1_f32;
            let vert = (rng.next() % 10 + 3) as f32 * 0.1_f32;
            let vx = if rng.next().is_multiple_of(2) {
                horiz
            } else {
                -horiz
            };
            let vy = if rng.next().is_multiple_of(2) {
                vert
            } else {
                -vert
            };

            #[allow(clippy::cast_possible_truncation)]
            let tile = (rng.next() % u64::from(SPRITE_COUNT)) as u32;
            let color = PALETTE
                [usize::try_from(rng.next() % u64::try_from(PALETTE.len()).unwrap()).unwrap()];

            sprites.push(Sprite {
                x,
                y,
                vx,
                vy,
                tile,
                color,
            });
        }

        Self {
            sprites,
            frame: 0,
            perf: PerfOverlay::new(),
            count,
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Draw a checkerboard on layer 0. Called once on frame 0; the double-buffer
/// keeps it for subsequent frames without redrawing.
fn draw_background<B: Backend>(term: &mut Terminal<B>) {
    let size = term.size();
    term.layer(0);
    for y in 0..size.height {
        for x in 0..size.width {
            let dark = (x + y) % 2 == 0;
            term.put_styled(
                x,
                y,
                ' ',
                Style::new().bg(Color::Rgb {
                    r: if dark { 18 } else { 28 },
                    g: if dark { 18 } else { 28 },
                    b: if dark { 28 } else { 42 },
                }),
            );
        }
    }
}

/// Render one frame. Returns `false` when the user quits.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut StressState) -> bool {
    state.perf.begin_frame();

    let size = term.size();
    let w = size.width;
    let h = size.height;
    let w_f = f32::from(w);
    let h_f = f32::from(h);

    // Layer 0: checkerboard background — must be redrawn every frame because
    // Terminal::present() clears the grid after each swap; nothing persists.
    draw_background(term);

    // Layer 1: clear previous sprite positions, then draw active sprites.
    term.layer(1);
    term.clear();

    for sprite in &mut state.sprites[..state.count] {
        sprite.x += sprite.vx;
        sprite.y += sprite.vy;

        if sprite.x < 0.0 || sprite.x >= w_f {
            sprite.vx = -sprite.vx;
            sprite.x = sprite.x.clamp(0.0, w_f - 1.0);
        }
        if sprite.y < 0.0 || sprite.y >= h_f {
            sprite.vy = -sprite.vy;
            sprite.y = sprite.y.clamp(0.0, h_f - 1.0);
        }

        let ch = char::from_u32(0xE000 + sprite.tile).expect("valid PUA codepoint");
        term.put_styled(
            sprite.x as u16,
            sprite.y as u16,
            ch,
            Style::new().fg(sprite.color),
        );
    }

    // Layer 2: perf overlay and sprite-count HUD.
    term.layer(2);
    state.perf.draw(term, 0, 0); // stats from previous frame; always 1 frame stale

    let hud = format!(
        " sprites: {:>4}  [+/-] adjust  [d] dump CSV  [Q] quit ",
        state.count,
    );
    let hud_style = Style::new()
        .fg(Color::Rgb {
            r: 200,
            g: 200,
            b: 200,
        })
        .bg(Color::Rgb {
            r: 20,
            g: 20,
            b: 30,
        });
    let hud_y = h.saturating_sub(1);
    for (i, ch) in hud.chars().enumerate() {
        let cx = i as u16;
        if cx >= w {
            break;
        }
        term.put_styled(cx, hud_y, ch, hud_style);
    }

    term.present().expect("present failed");
    state.frame = state.frame.wrapping_add(1);

    for event in term.drain_events() {
        match event {
            Event::Key(k) => match k.code {
                KeyCode::Escape | KeyCode::Char('q' | 'Q') => return false,
                KeyCode::Char('+' | '=') => {
                    state.count = (state.count + 50).min(MAX_SPRITES);
                }
                KeyCode::Char('-') => {
                    state.count = state.count.saturating_sub(50).max(1);
                }
                KeyCode::Char('d' | 'D') => {
                    state.perf.dump_csv();
                }
                _ => {}
            },
            Event::Close => return false,
            _ => {}
        }
    }

    true
}

// ── Entry point ───────────────────────────────────────────────────────────────

retroglyph_examples::rg_run_software!(
    StressState,
    |_term| StressState::new(500),
    tick,
    builder = {
        let png_bytes = make_sprite_sheet();
        let tileset = TilesetOptions::from_bytes(png_bytes)
            .tile_size(8, 8)
            .start_codepoint('\u{E000}')
            .build()
            .expect("tileset options are valid");
        SoftwareBackendBuilder::new()
            .title(env!("CARGO_BIN_NAME"))
            .grid_size(80, 40)
            .scale(2)
            .tileset(tileset)
            .target_fps(60)
    }
);
