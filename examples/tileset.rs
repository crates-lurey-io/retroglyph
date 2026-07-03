//! Tileset demo: custom PNG sprite sheets with alpha transparency.
//!
//! Generates a small 4-tile sprite sheet at runtime with actual pixel-art
//! sprites (sword, potion, skull, coin) on a transparent background,
//! registers it as a tileset with `Codepage::Unicode`, and draws them
//! overlaying the bitmap-font background.
//!
//! The transparent background lets the underlying layer-0 pattern show
//! through around the sprite pixels.
//!
//! Run with:
//!   `cargo run --example tileset --features software-tilesets,software-default-font`

mod util;

use retroglyph::event::{Event, KeyCode};
use retroglyph::style::Style;
use retroglyph::{Color, Terminal};

// ── Sprite data ───────────────────────────────────────────────────────────────
//
// Each sprite is 8×8 pixels, 1-bit (0 = transparent, 1 = foreground colour).
// Stored as 8 bytes, MSB = leftmost pixel.

#[cfg(feature = "software-tilesets")]
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

#[cfg(feature = "software-tilesets")]
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

#[cfg(feature = "software-tilesets")]
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

#[cfg(feature = "software-tilesets")]
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

struct SpriteDef {
    name: &'static str,
    #[cfg(feature = "software-tilesets")]
    bits: &'static [u8; 8],
    color: (u8, u8, u8),
}

const SPRITES: &[SpriteDef] = &[
    SpriteDef {
        name: "sword",
        #[cfg(feature = "software-tilesets")]
        bits: &SPRITE_SWORD,
        color: (200, 200, 220),
    },
    SpriteDef {
        name: "potion",
        #[cfg(feature = "software-tilesets")]
        bits: &SPRITE_POTION,
        color: (80, 200, 120),
    },
    SpriteDef {
        name: "skull",
        #[cfg(feature = "software-tilesets")]
        bits: &SPRITE_SKULL,
        color: (200, 200, 200),
    },
    SpriteDef {
        name: "coin",
        #[cfg(feature = "software-tilesets")]
        bits: &SPRITE_COIN,
        color: (220, 200, 40),
    },
];

// ── Sprite sheet generation ───────────────────────────────────────────────────

#[cfg(feature = "software-tilesets")]
fn make_sprite_sheet() -> Vec<u8> {
    use image::ImageEncoder;

    let tile_w = 8u32;
    let tile_h = 8u32;
    #[allow(clippy::cast_possible_truncation)]
    let cols = SPRITES.len() as u32;
    let (img_w, img_h) = (tile_w * cols, tile_h);
    let mut pixels = vec![0u8; (img_w * img_h * 4) as usize];

    for (i, def) in SPRITES.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let i = i as u32;
        let (fr, fg, fb) = def.color;
        for py in 0..tile_h {
            let row_bits = def.bits[py as usize];
            for px in 0..tile_w {
                let idx = ((py * img_w) + i * tile_w + px) as usize * 4;
                if (row_bits >> (7 - px)) & 1 != 0 {
                    pixels[idx] = fr;
                    pixels[idx + 1] = fg;
                    pixels[idx + 2] = fb;
                    pixels[idx + 3] = 255;
                }
                // transparent pixels remain zeroed
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

/// Demo state — just a frame counter for future animation.
struct TilesetState {
    frame: u64,
}

impl TilesetState {
    const fn new() -> Self {
        Self { frame: 0 }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Draw one frame. Returns `false` when the user quits.
#[allow(clippy::cast_possible_truncation)]
fn tick(term: &mut Terminal<impl retroglyph::Backend>, state: &mut TilesetState) -> bool {
    let size = term.size();

    term.layer(0);
    // Checkerboard background so transparency is visible.
    for y in 0..size.height {
        for x in 0..size.width {
            let dark = (x + y) % 2 == 0;
            term.put_styled(
                x,
                y,
                ' ',
                Style::new().bg(Color::Rgb {
                    r: if dark { 20 } else { 30 },
                    g: if dark { 20 } else { 30 },
                    b: if dark { 30 } else { 45 },
                }),
            );
        }
    }

    // Header and legend (only on the first frame; double-buffering keeps them).
    if state.frame == 0 {
        let header = "rg tileset demo: custom sprites with alpha [Esc to quit]";
        let hx = size.width.saturating_sub(header.len() as u16) / 2;
        term.fg(Color::BRIGHT_WHITE);
        term.bg(Color::Rgb {
            r: 40,
            g: 40,
            b: 60,
        });
        term.print(hx, 1, header);
        term.reset_style();

        let info = "E000-E003 = sprite tiles with transparent bg  |  @ = bitmap font fallback";
        let ix = size.width.saturating_sub(info.len() as u16) / 2;
        term.fg(Color::Rgb {
            r: 140,
            g: 140,
            b: 160,
        });
        term.print(ix, size.height - 2, info);
        term.reset_style();
    }

    // Layer 1: sprite tiles
    term.layer(1);
    for (i, def) in SPRITES.iter().enumerate() {
        let i = i as u16;
        let x = 2 + i * 9;
        let ch = char::from_u32(0xE000 + u32::from(i)).unwrap();
        let (fr, fg, fb) = def.color;

        // Label (first char of name)
        term.put_styled(
            x,
            3,
            def.name.as_bytes().first().copied().unwrap_or(b'?') as char,
            Style::new()
                .fg(Color::Rgb {
                    r: 120,
                    g: 120,
                    b: 140,
                })
                .bg(Color::Rgb {
                    r: 10,
                    g: 10,
                    b: 20,
                }),
        );

        // Sprite tile
        term.put_styled(
            x,
            5,
            ch,
            Style::new()
                .fg(Color::Rgb {
                    r: fr,
                    g: fg,
                    b: fb,
                })
                .bg(Color::Rgb {
                    r: 10,
                    g: 10,
                    b: 20,
                }),
        );
    }

    // Bitmap font fallback reference glyph
    term.put_styled(
        35,
        10,
        '@',
        Style::new().fg(Color::BRIGHT_GREEN).bg(Color::Rgb {
            r: 10,
            g: 10,
            b: 20,
        }),
    );

    term.present().expect("present failed");
    state.frame = state.frame.wrapping_add(1);

    // Drain all available events without blocking (avoid WASM slow-motion replay).
    for event in term.drain_events() {
        match event {
            Event::Key(k) if k.code == KeyCode::Escape => return false,
            Event::Close => return false,
            _ => {}
        }
    }

    true
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg(feature = "software")]
use retroglyph::backend::software::SoftwareBackendBuilder;
#[cfg(feature = "software-tilesets")]
use retroglyph::backend::software::tileset::TilesetOptions;

rg_run_software!(
    TilesetState,
    |_term| TilesetState::new(),
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
            .grid_size(40, 16)
            .scale(4)
            .tileset(tileset)
    }
);
