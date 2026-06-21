//! Tileset demo: custom PNG sprite sheets with alpha transparency.
//!
//! Generates a small 6-tile sprite sheet at runtime with actual pixel-art
//! sprites (sword, potion, chest, skull, shield, coin) on a transparent
//! background, registers it as a tileset with `Codepage::Unicode`, and
//! draws them overlaying the bitmap-font background.
//!
//! The transparent background lets the underlying layer-0 pattern show
//! through around the sprite pixels.
//!
//! Run with:
//!   `cargo run --example tileset_demo --features software-tilesets,software-default-font`

use rg::backend::software::SoftwareBackendBuilder;
use rg::backend::software::tileset::TilesetOptions;
use rg::event::{Event, KeyCode};
use rg::style::Style;
use rg::{Color, Terminal};
use std::time::Duration;

// ── Sprite data: four 8×8 pixel-art sprites ──────────────────────────────
//
// Each sprite is 8×8 pixels, 1-bit (0 = transparent, 1 = foreground colour).
// Stored as 8 bytes, MSB = leftmost pixel.

/// A simple sword (diagonal blade pointing up-right).
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

/// A potion bottle.
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

/// A skull.
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

/// A gold coin.
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
    bits: &'static [u8; 8],
    color: (u8, u8, u8),
}

const SPRITES: &[SpriteDef] = &[
    SpriteDef {
        name: "sword",
        bits: &SPRITE_SWORD,
        color: (200, 200, 220),
    },
    SpriteDef {
        name: "potion",
        bits: &SPRITE_POTION,
        color: (80, 200, 120),
    },
    SpriteDef {
        name: "skull",
        bits: &SPRITE_SKULL,
        color: (200, 200, 200),
    },
    SpriteDef {
        name: "coin",
        bits: &SPRITE_COIN,
        color: (220, 200, 40),
    },
];

/// Generate an RGBA PNG spritesheet from the hard-coded sprites.
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
                let bit_set = (row_bits >> (7 - px)) & 1 != 0;
                if bit_set {
                    pixels[idx] = fr;
                    pixels[idx + 1] = fg;
                    pixels[idx + 2] = fb;
                    pixels[idx + 3] = 255;
                } else {
                    pixels[idx + 3] = 0;
                }
            }
        }
    }

    let mut out = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&pixels, img_w, img_h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out.into_inner()
}

// ── Draw ────────────────────────────────────────────────────────────────

#[allow(clippy::cast_possible_truncation)]
fn draw(term: &mut Terminal<impl rg::Backend>, frame: u64) {
    let size = term.size();

    term.layer(0);
    term.clear();

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

    // Header (frame 0 only).
    if frame == 0 {
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

    // Layer 1: sprite tiles.
    term.layer(1);
    term.clear();

    // Draw each sprite with its label above it.
    for (i, def) in SPRITES.iter().enumerate() {
        let i = i as u16;
        let x = 2 + i * 9;
        let ch = char::from_u32(0xE000 + u32::from(i)).unwrap();
        let (fr, fg, fb) = def.color;

        // Label (first char of name).
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

        // Sprite tile.
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

    // Add a static '@' at a fixed position showing bitmap font fallback.
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

    term.present();
}

fn main() {
    let png_bytes = make_sprite_sheet();

    let tileset = TilesetOptions::from_bytes(png_bytes)
        .tile_size(8, 8)
        .start_codepoint('\u{E000}')
        .build()
        .expect("tileset options are valid");

    let backend = SoftwareBackendBuilder::new()
        .title("rg tileset demo [Esc to quit]")
        .grid_size(40, 16)
        .scale(4)
        .tileset(tileset)
        .build()
        .expect("backend init failed");

    let mut frame = 0u64;

    backend
        .run_windowed(move |term: &mut Terminal<_>| {
            draw(term, frame);
            frame = frame.wrapping_add(1);

            if let Some(event) = term.poll(Duration::from_millis(16)) {
                match event {
                    Event::Key(k) if k.code == KeyCode::Escape => std::process::exit(0),
                    Event::Close => std::process::exit(0),
                    _ => {}
                }
            }
        })
        .expect("event loop failed");
}
