//! 16: Subcell image-to-glyph blit
//!
//! [`quantize_half_block`], [`quantize_quadrant`], and [`quantize_sextant`] posterize small
//! blocks of raw pixels to the best-matching Unicode block-element glyph -- retroglyph's
//! "render a raster image as text, no tileset required" utility (see `retroglyph_core::subcell`
//! for the algorithm). This example renders the same procedural "scene" (concentric color rings,
//! computed on the fly -- no image file, proving the "no tileset dependency" claim literally)
//! through all three fidelities side by side, sampling more source pixels per cell as fidelity
//! increases:
//!
//! - **Half-block**: 1x2 pixels/cell, `' '`/`▀`/`▄`/`█` only. Coarsest, most compatible.
//! - **Quadrant**: 2x2 pixels/cell, 16 glyphs. Double the resolution both ways.
//! - **Sextant**: 2x3 pixels/cell, 64 glyphs. Smoothest ring edges of the three, needs a font
//!   with "Symbols for Legacy Computing" coverage (a 2022 Unicode addition) to render as blocks
//!   rather than tofu/replacement characters.
//!
//! ```sh
//! cargo run --example 16_subcell_image --features crossterm
//! cargo run --example 16_subcell_image --features software
//! cargo run --example 16_subcell_image  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Press `q` or `Escape` to quit on the interactive backends, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::subcell::{Rgb, quantize_half_block, quantize_quadrant, quantize_sextant};
use retroglyph_core::{Backend, Style, Terminal};
use retroglyph_examples::Example;

/// Width/height (in cells) of each of the three panels.
const PANEL_W: u16 = 15;
const PANEL_H: u16 = 15;

/// Left edge (in cells) of each panel; `PANEL_W` apart plus a 1-cell gap.
const PANEL_X: [u16; 3] = [1, 1 + PANEL_W + 1, 1 + 2 * (PANEL_W + 1)];
/// Top edge (in cells) of every panel's pixel grid (one row below its label).
const PANEL_Y: u16 = 3;

/// The scene's color palette, one color per concentric ring.
const RING_COLORS: [Rgb; 5] = [
    (231, 76, 60),  // red
    (241, 196, 15), // yellow
    (46, 204, 113), // green
    (52, 152, 219), // blue
    (155, 89, 182), // purple
];

/// The procedural "image" this example renders: concentric rings around the center, each ring a
/// different color, with a brightness ripple layered on top so even a single ring shows glyph
/// variety instead of flattening entirely to solid-color space cells.
///
/// `u`/`v` are normalized scene coordinates in `[0, 1)`, independent of any panel's actual pixel
/// resolution -- each panel samples this same continuous function at its own pixel density, so
/// all three depict identical content at increasing fidelity.
///
/// The float-to-int casts inside are sound by construction, not just "probably fine": `dist` is
/// a `hypot` of two coordinates in `[-0.5, 0.5]`, so `dist * 12.0` is always non-negative and
/// well within `usize` range, and `ripple` is a sine-derived factor in `[0.5, 1.0]`, so `channel
/// as f32 * ripple` never leaves `u8`'s range before rounding.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]
fn scene_pixel(u: f32, v: f32) -> Rgb {
    let dx = u - 0.5;
    let dy = v - 0.5;
    let dist = dx.hypot(dy);
    let ring = &RING_COLORS[(dist * 12.0) as usize % RING_COLORS.len()];
    let ripple = 0.25f32.mul_add((dist * 40.0).sin(), 0.75);
    let shade =
        |channel: u8| u8::try_from((f32::from(channel) * ripple).round() as i32).unwrap_or(channel);
    (shade(ring.0), shade(ring.1), shade(ring.2))
}

/// Samples [`scene_pixel`] at pixel `(px, py)` of a `pixel_w`x`pixel_h` source grid.
fn sample(px: u16, py: u16, pixel_w: u16, pixel_h: u16) -> Rgb {
    let u = (f32::from(px) + 0.5) / f32::from(pixel_w);
    let v = (f32::from(py) + 0.5) / f32::from(pixel_h);
    scene_pixel(u, v)
}

/// State for the subcell example (none needed: the scene is procedural and never changes).
#[derive(Default)]
pub struct SubcellImage;

impl SubcellImage {
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

    /// Draws the half-block panel at `PANEL_X[0]`: one source pixel per cell column, two per row.
    fn draw_half_block<B: Backend>(term: &mut Terminal<B>) {
        let pixel_w = PANEL_W;
        let pixel_h = PANEL_H * 2;
        for cy in 0..PANEL_H {
            for cx in 0..PANEL_W {
                let top = sample(cx, cy * 2, pixel_w, pixel_h);
                let bottom = sample(cx, cy * 2 + 1, pixel_w, pixel_h);
                let glyph = quantize_half_block([top, bottom]);
                let style = Style::new().fg(glyph.fg).bg(glyph.bg);
                term.put_styled(PANEL_X[0] + cx, PANEL_Y + cy, glyph.ch, style);
            }
        }
    }

    /// Draws the quadrant panel at `PANEL_X[1]`: two source pixels per cell column and per row.
    fn draw_quadrant<B: Backend>(term: &mut Terminal<B>) {
        let pixel_w = PANEL_W * 2;
        let pixel_h = PANEL_H * 2;
        for cy in 0..PANEL_H {
            for cx in 0..PANEL_W {
                let (px, py) = (cx * 2, cy * 2);
                let pixels = [
                    sample(px, py, pixel_w, pixel_h),
                    sample(px + 1, py, pixel_w, pixel_h),
                    sample(px, py + 1, pixel_w, pixel_h),
                    sample(px + 1, py + 1, pixel_w, pixel_h),
                ];
                let glyph = quantize_quadrant(pixels);
                let style = Style::new().fg(glyph.fg).bg(glyph.bg);
                term.put_styled(PANEL_X[1] + cx, PANEL_Y + cy, glyph.ch, style);
            }
        }
    }

    /// Draws the sextant panel at `PANEL_X[2]`: two source pixels per cell column, three per row.
    fn draw_sextant<B: Backend>(term: &mut Terminal<B>) {
        let pixel_w = PANEL_W * 2;
        let pixel_h = PANEL_H * 3;
        for cy in 0..PANEL_H {
            for cx in 0..PANEL_W {
                let (px, py) = (cx * 2, cy * 3);
                let pixels = [
                    sample(px, py, pixel_w, pixel_h),
                    sample(px + 1, py, pixel_w, pixel_h),
                    sample(px, py + 1, pixel_w, pixel_h),
                    sample(px + 1, py + 1, pixel_w, pixel_h),
                    sample(px, py + 2, pixel_w, pixel_h),
                    sample(px + 1, py + 2, pixel_w, pixel_h),
                ];
                let glyph = quantize_sextant(pixels);
                let style = Style::new().fg(glyph.fg).bg(glyph.bg);
                term.put_styled(PANEL_X[2] + cx, PANEL_Y + cy, glyph.ch, style);
            }
        }
    }

    /// Draws this frame and presents it.
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(1, 1, "16: Subcell blit -- one scene, 3 fidelities");
        term.print(PANEL_X[0], 2, "Half-block");
        term.print(PANEL_X[1], 2, "Quadrant");
        term.print(PANEL_X[2], 2, "Sextant");

        Self::draw_half_block(term);
        Self::draw_quadrant(term);
        Self::draw_sextant(term);

        term.print(
            1,
            PANEL_Y + PANEL_H + 1,
            "No image file above -- generated on the fly. Software backend's \
             built-in font is CP437-only, so quadrant/sextant show as solid \
             colored blocks there; crossterm renders the real glyphs.",
        );

        term.present().ok();
    }
}

impl Example for SubcellImage {
    const NAME: &'static str = "16_subcell_image";

    fn tick<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        _frame: &retroglyph_core::Frame,
    ) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(SubcellImage);
