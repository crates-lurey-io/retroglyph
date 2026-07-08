//! 05: Layers & offsets -- `Terminal::layer`, per-layer `clear`, `put_offset`
//!
//! Two new concepts, both about drawing *between* whole cells rather than exactly on them:
//!
//! - [`Terminal::layer`] switches which z-ordered plane `put`/`put_styled`/`put_offset` write to.
//!   Layers composite on `present()`: an empty tile on a higher layer is transparent, so lower
//!   layers show through. `present()` clears the *entire* grid every frame (every layer), so
//!   nothing persists across frames on its own -- the background below is redrawn every frame
//!   too. What layers buy you is that clearing/redrawing the sprite's layer never touches the
//!   background layer's content, so you never need to track the sprite's previous cell to know
//!   what to restore underneath it.
//! - [`Terminal::put_offset`] nudges a glyph by a sub-cell pixel amount. It's visual only (does
//!   not affect grid logic or hit-testing) and only the software/pixel backend renders it --
//!   `CrosstermBackend` ignores the offset entirely, so the two sprites below will move
//!   identically on that backend.
//!
//! ```sh
//! cargo run --example 05_layers_and_offsets                          # Headless (prints a few frames)
//! cargo run --example 05_layers_and_offsets --features crossterm     # Terminal
//! cargo run --example 05_layers_and_offsets --features default-font  # Desktop window
//! cargo run --example 05_layers_and_offsets --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press any key (Terminal/Desktop) to quit.

use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};

/// Sub-cell offset units per whole cell, tuned for the default 8x16 bitmap font. Other tilesets
/// or backends may use a different pixel-per-cell width; `put_offset`'s units are always "pixels
/// at the tileset's native resolution", not a fixed constant.
const CELL_PX_W: f64 = 8.0;

/// Cells per second for both sprites.
const SPEED: f64 = 3.0;

const STEPPED_ROW: u16 = 5;
const SMOOTH_ROW: u16 = 8;

/// Column where each row's sprite path starts, leaving room for a label before it.
const PATH_START_X: u16 = 9;
/// Cells of travel available to each sprite before it bounces back.
const PATH_LEN: f64 = 29.0;

struct LayersAndOffsets {
    /// Total elapsed time, driving both sprites' shared triangle-wave position.
    elapsed: f64,
}

impl<B: Backend> App<B> for LayersAndOffsets {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        // Layer 0: the background. Redrawn every frame -- present() clears every layer, so
        // nothing sticks around on its own -- but it never has to know or care where the
        // sprites on layer 1 are.
        term.layer(0);
        term.print(0, 0, "05: Layers & Offsets");
        term.print(0, 2, "(offsets are pixel-only; invisible on this backend)");
        for y in 4..11 {
            for x in 0..38 {
                if (x + y) % 2 == 0 {
                    term.put(x, y, ':');
                }
            }
        }
        term.print(0, STEPPED_ROW, "put():");
        term.print(0, SMOOTH_ROW, "offset:");
        term.print(0, 11, "cell-stepped vs. sub-cell smooth, same speed");

        self.elapsed += frame.dt.as_secs_f64();

        // Triangle wave between 0 and PATH_LEN cells, so both sprites bounce back and forth
        // along their row instead of running off the edge.
        let period = 2.0 * PATH_LEN / SPEED;
        let t = self.elapsed % period;
        let pos = if t < PATH_LEN / SPEED {
            t * SPEED
        } else {
            (t - PATH_LEN / SPEED).mul_add(-SPEED, PATH_LEN)
        };

        // Layer 1: the moving sprites, cleared and redrawn every frame. Layer 0's background
        // (redrawn just above, on a different layer) shows through everywhere the sprites don't
        // draw, since an untouched tile is transparent -- only an explicit `put` (or
        // `put_offset`) on this layer is opaque.
        term.layer(1);
        term.clear();

        // Cell-stepped: jumps to a new whole cell only when `pos`'s integer part changes.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cell_x = PATH_START_X + pos as u16;
        term.put(cell_x, STEPPED_ROW, '@');

        // Smooth: same integer cell, plus a sub-cell pixel offset for the fractional part, so
        // it glides continuously instead of jumping.
        #[allow(clippy::cast_possible_truncation)]
        let dx = (pos.fract() * CELL_PX_W) as i16;
        term.put_offset(cell_x, SMOOTH_ROW, dx, 0, '*');

        term.present().expect("present failed");

        if any_key_pressed_or_window_closed(term) {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

rg_gallery_run!(
    LayersAndOffsets { elapsed: 0.0 },
    "05: Layers & Offsets",
    40,
    12
);
