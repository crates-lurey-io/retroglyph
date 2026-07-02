#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops,
    clippy::imprecise_flops
)]
//! Build script: generates `hex_sprites.png` for the `hex_battle` example.
//!
//! Produces a sprite sheet with 6 tiles (each 32×32 RGBA pixels):
//!   0 – empty hex (dark outline, transparent fill)
//!   1 – selected hex (bright outline + tinted fill)
//!   2 – blue unit marker  (blue circle)
//!   3 – red unit marker   (red circle)
//!   4 – movement highlight (lighter hex tint)
//!   5 – attack flash       (orange/yellow hex tint)
//!
//! Tile size: 32×32 px. Sheet: 192×32 px (6 tiles wide, 1 row).
//!
//! The `SoftwareRenderer::blit_sprite` scales each source pixel by the
//! backend's `scale` factor, so these 32×32 tiles render as 64×64 pixels
//! at scale=2, fitting exactly within the 4×2 cell hex stride (4×16=64,
//! 2×32=64).  A circumradius of 13px in a 32×32 tile leaves a ~3px
//! transparent margin, giving visible gaps between adjacent hexes.

use image::{ImageBuffer, Rgba, RgbaImage};
use std::f64::consts::PI;
use std::path::PathBuf;

// ── Constants ─────────────────────────────────────────────────────────────────

const TILE_W: u32 = 32;
const TILE_H: u32 = 32;
const TILE_COUNT: u32 = 6;
const SHEET_W: u32 = TILE_W * TILE_COUNT;
const SHEET_H: u32 = TILE_H;

/// Circumradius of each hexagon in pixels.
const HEX_R: f64 = 13.0;

// Tile indices
const IDX_EMPTY: u32 = 0;
const IDX_SELECTED: u32 = 1;
const IDX_REBEL: u32 = 2;
const IDX_EMPIRE: u32 = 3;
const IDX_MOVE: u32 = 4;
const IDX_ATTACK: u32 = 5;

fn main() {
    // Only rebuild if this file changes.
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir: PathBuf = std::env::var_os("OUT_DIR").unwrap().into();
    let path = out_dir.join("hex_sprites.png");

    let mut sheet: RgbaImage = ImageBuffer::new(SHEET_W, SHEET_H);

    draw_hex_tile(&mut sheet, IDX_EMPTY, &HexStyle::Empty);
    draw_hex_tile(&mut sheet, IDX_SELECTED, &HexStyle::Selected);
    draw_hex_tile(&mut sheet, IDX_REBEL, &HexStyle::Unit(UnitFaction::Blue));
    draw_hex_tile(&mut sheet, IDX_EMPIRE, &HexStyle::Unit(UnitFaction::Red));
    draw_hex_tile(&mut sheet, IDX_MOVE, &HexStyle::MoveHighlight);
    draw_hex_tile(&mut sheet, IDX_ATTACK, &HexStyle::AttackFlash);

    sheet.save(&path).expect("failed to save hex_sprites.png");
}

// ── Style enum ────────────────────────────────────────────────────────────────

enum HexStyle {
    Empty,
    Selected,
    Unit(UnitFaction),
    MoveHighlight,
    AttackFlash,
}

enum UnitFaction {
    Blue,
    Red,
}

// ── Hex tile renderer ─────────────────────────────────────────────────────────

fn draw_hex_tile(sheet: &mut RgbaImage, idx: u32, style: &HexStyle) {
    let ox = idx * TILE_W; // x offset of this tile in the sheet

    let cx = TILE_W as f64 / 2.0;
    let cy = TILE_H as f64 / 2.0;

    // Precompute pointy-top hex vertices (vertex i at angle 60°*i - 30°).
    let vertices: Vec<(f64, f64)> = (0..6)
        .map(|i| {
            let angle = PI / 180.0 * (60.0 * i as f64 - 30.0);
            (cx + HEX_R * angle.cos(), cy + HEX_R * angle.sin())
        })
        .collect();

    for py in 0..TILE_H {
        for px in 0..TILE_W {
            let fx = px as f64 + 0.5;
            let fy = py as f64 + 0.5;

            let dist = point_to_hex_sdf(fx, fy, cx, cy, &vertices);

            let pixel = style_pixel(style, fx, fy, cx, cy, dist);
            sheet.put_pixel(ox + px, py, pixel);
        }
    }
}

/// Returns a signed distance field value for a point relative to the hex.
/// Negative = inside, positive = outside.
fn point_to_hex_sdf(px: f64, py: f64, _cx: f64, _cy: f64, verts: &[(f64, f64)]) -> f64 {
    // Winding-number in-polygon check, then compute approximate edge distance.
    let inside = point_in_polygon(px, py, verts);
    let edge_dist = verts
        .windows(2)
        .chain(std::iter::once([verts[5], verts[0]].as_slice()))
        .map(|seg| segment_distance(px, py, seg[0], seg[1]))
        .fold(f64::MAX, f64::min);

    if inside { -edge_dist } else { edge_dist }
}

fn point_in_polygon(px: f64, py: f64, verts: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = verts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = verts[i];
        let (xj, yj) = verts[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn segment_distance(px: f64, py: f64, (ax, ay): (f64, f64), (bx, by): (f64, f64)) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-10 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn style_pixel(style: &HexStyle, fx: f64, fy: f64, cx: f64, cy: f64, sdf: f64) -> Rgba<u8> {
    // Smooth alpha for anti-aliased edges.
    let edge_alpha = smooth_step(1.5, -0.5, sdf); // 1.0 inside, 0.0 outside

    if edge_alpha < 0.01 {
        return Rgba([0, 0, 0, 0]); // fully transparent
    }

    // At 32×32 tiles, thresholds are roughly half the 64×64 originals.
    match style {
        HexStyle::Empty => {
            let outline = smooth_step(1.0, -0.5, sdf - 1.2);
            let fill_col = [28u8, 42, 68];
            let line_col = [70u8, 95, 140];
            let col = lerp_rgb(fill_col, line_col, outline);
            Rgba([col[0], col[1], col[2], (edge_alpha * 255.0) as u8])
        }

        HexStyle::Selected => {
            let outline = smooth_step(1.0, -0.5, sdf - 1.2);
            let fill_col = [120u8, 180, 230];
            let line_col = [200u8, 230, 255];
            let col = lerp_rgb(fill_col, line_col, outline);
            let a = (edge_alpha * 200.0) as u8;
            Rgba([col[0], col[1], col[2], a])
        }

        HexStyle::Unit(faction) => {
            let outline = smooth_step(1.0, -0.5, sdf - 1.0);
            let (fill_base, circle_col, line_col): ([u8; 3], [u8; 3], [u8; 3]) = match faction {
                UnitFaction::Blue => ([20, 50, 100], [70, 120, 200], [120, 160, 240]),
                UnitFaction::Red => ([100, 20, 20], [200, 60, 60], [240, 120, 120]),
            };

            let r_circle = HEX_R * 0.55;
            let circle_sdf = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - r_circle;
            let circle_alpha = smooth_step(1.0, -0.5, circle_sdf);
            let circle_outline = smooth_step(1.0, -0.5, circle_sdf - 1.0);

            let col = if circle_alpha > 0.01 {
                lerp_rgb(circle_col, [255, 255, 255], circle_outline * 0.4)
            } else {
                lerp_rgb(fill_base, line_col, outline)
            };

            let a = (edge_alpha * 255.0) as u8;
            Rgba([col[0], col[1], col[2], a])
        }

        HexStyle::MoveHighlight => {
            let fill_col = [50u8, 130, 80];
            let line_col = [100u8, 200, 130];
            let outline = smooth_step(1.0, -0.5, sdf - 1.0);
            let col = lerp_rgb(fill_col, line_col, outline);
            Rgba([col[0], col[1], col[2], (edge_alpha * 160.0) as u8])
        }

        HexStyle::AttackFlash => {
            let fill_col = [180u8, 80, 10];
            let line_col = [255u8, 180, 50];
            let outline = smooth_step(1.0, -0.5, sdf - 1.0);
            let col = lerp_rgb(fill_col, line_col, outline);
            Rgba([col[0], col[1], col[2], (edge_alpha * 200.0) as u8])
        }
    }
}

fn smooth_step(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t) as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t) as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t) as u8,
    ]
}
