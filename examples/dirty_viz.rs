//! Dirty-cell visualizer demo.
//!
//! Split-screen: the left half shows a bouncing ball on a static background;
//! the right half mirrors which cells changed last frame as a colour heatmap.
//!
//! This makes the double-buffer diff engine *visible*. Because the background
//! is drawn once (frame 0) and only the ball cell is redrawn each subsequent
//! frame, roughly 2 cells are dirty per frame — the old position (background
//! restored) and the new position (ball drawn).
//!
//! Run with:
//!   `cargo run --example dirty_viz --features software-default-font`
//!
//! # Dirty-tracking approach
//!
//! The plan called for `Terminal::present_stats()` to expose dirty-cell counts
//! from the library's diff engine. That API does not exist yet:
//!
//! ```text
//! todo!("present_stats not yet in library — using demo-side dirty tracking");
//! ```
//!
//! Instead, every draw call to the left half records the target position in
//! `dirty_map`. This is accurate as long as this demo controls all left-half
//! drawing (which it does). When `present_stats()` is added to the library,
//! replace the manual tracking with the library-reported count.

mod util;

use retroglyph::backend::software::SoftwareBackendBuilder;
use retroglyph::color::Color;
use retroglyph::style::Style;
use retroglyph::{Backend, Terminal};
use util::action::{Action, event_to_action};
use util::perf::PerfOverlay;

// ── Layout constants ──────────────────────────────────────────────────────────

/// Total grid columns.
const GRID_W: u16 = 80;
/// Total grid rows.
const GRID_H: u16 = 30;
/// Width of each half (left = scene, right = overlay).
const HALF_W: u16 = GRID_W / 2;
/// Last row — reserved for the footer status bar.
const FOOTER_ROW: u16 = GRID_H - 1;
/// Rows available for animation (rows 0..`ANIM_H`).
const ANIM_H: u16 = FOOTER_ROW;

// ── State ─────────────────────────────────────────────────────────────────────

/// All mutable demo state.
struct VizState {
    /// Current ball column (left-half coordinate, 0..`HALF_W`).
    ball_col: u16,
    /// Current ball row (0..`ANIM_H`).
    ball_row: u16,
    /// Ball column on the previous frame (needed for background restore).
    prev_col: u16,
    /// Ball row on the previous frame.
    prev_row: u16,
    /// Horizontal velocity in cells/frame (+1 or -1).
    vel_col: i16,
    /// Vertical velocity in cells/frame (+1 or -1).
    vel_row: i16,
    /// Frame counter.
    frame: u64,
    /// Per-cell dirty flags accumulated during *this* frame's draw calls.
    /// Indexed as `row * HALF_W + col` for the left half only.
    dirty_map: Vec<bool>,
    /// Dirty flags from the *previous* frame — what the overlay displays.
    prev_dirty_map: Vec<bool>,
    /// Dirty-cell count for the current frame.
    dirty_count: u32,
    /// Dirty-cell count from the previous frame, shown in the footer.
    prev_dirty_count: u32,
    /// Frame-time tracker for the perf overlay.
    perf: PerfOverlay,
}

impl VizState {
    fn new() -> Self {
        let map_len = usize::from(HALF_W) * usize::from(ANIM_H);
        Self {
            ball_col: HALF_W / 2,
            ball_row: ANIM_H / 2,
            prev_col: HALF_W / 2,
            prev_row: ANIM_H / 2,
            vel_col: 1,
            vel_row: 1,
            frame: 0,
            dirty_map: vec![false; map_len],
            prev_dirty_map: vec![false; map_len],
            dirty_count: 0,
            prev_dirty_count: 0,
            perf: PerfOverlay::new(),
        }
    }
}

// ── Background style ──────────────────────────────────────────────────────────

/// Return the checkerboard glyph and style at `(col, row)`.
fn bg_at(col: u16, row: u16) -> (char, Style) {
    if (col + row).is_multiple_of(2) {
        (
            '#',
            Style::new()
                .fg(Color::Rgb {
                    r: 50,
                    g: 45,
                    b: 60,
                })
                .bg(Color::Rgb {
                    r: 15,
                    g: 14,
                    b: 20,
                }),
        )
    } else {
        (
            '.',
            Style::new()
                .fg(Color::Rgb {
                    r: 35,
                    g: 32,
                    b: 42,
                })
                .bg(Color::Rgb {
                    r: 15,
                    g: 14,
                    b: 20,
                }),
        )
    }
}

// ── Dirty-tracking draw helper ────────────────────────────────────────────────

/// Draw a character in the left half and record the cell as dirty.
fn put_left<B: Backend>(
    term: &mut Terminal<B>,
    dirty_map: &mut [bool],
    dirty_count: &mut u32,
    col: u16,
    row: u16,
    ch: char,
    style: Style,
) {
    term.put_styled(col, row, ch, style);
    if col < HALF_W && row < ANIM_H {
        let idx = usize::from(row) * usize::from(HALF_W) + usize::from(col);
        if !dirty_map[idx] {
            dirty_map[idx] = true;
            *dirty_count += 1;
        }
    }
}

// ── Draw calls ────────────────────────────────────────────────────────────────

/// Restore the background glyph at the ball's previous position.
fn restore_prev<B: Backend>(term: &mut Terminal<B>, state: &mut VizState) {
    let (ch, style) = bg_at(state.prev_col, state.prev_row);
    put_left(
        term,
        &mut state.dirty_map,
        &mut state.dirty_count,
        state.prev_col,
        state.prev_row,
        ch,
        style,
    );
}

/// Draw the `@` ball at the current position.
fn draw_ball<B: Backend>(term: &mut Terminal<B>, state: &mut VizState) {
    term.layer(0);
    let style = Style::new()
        .fg(Color::Rgb {
            r: 220,
            g: 220,
            b: 100,
        })
        .bg(Color::Rgb {
            r: 15,
            g: 14,
            b: 20,
        });
    put_left(
        term,
        &mut state.dirty_map,
        &mut state.dirty_count,
        state.ball_col,
        state.ball_row,
        '@',
        style,
    );
}

/// Draw the dirty-cell heatmap on the right half using the previous frame's map.
fn draw_overlay<B: Backend>(term: &mut Terminal<B>, prev_dirty: &[bool], prev_count: u32) {
    term.layer(0);

    // ── Header label ──
    let label = " dirty-cell overlay (prev frame) ";
    let header_style = Style::new()
        .fg(Color::Rgb {
            r: 180,
            g: 180,
            b: 200,
        })
        .bg(Color::Rgb {
            r: 30,
            g: 28,
            b: 45,
        });
    for col in 0..HALF_W {
        term.put_styled(HALF_W + col, 0, ' ', header_style);
    }
    for (col, ch) in (0u16..).zip(label.chars()).take(usize::from(HALF_W)) {
        term.put_styled(HALF_W + col, 0, ch, header_style);
    }

    // ── Per-cell heatmap (rows 1..ANIM_H-1) ──
    for row in 1..ANIM_H.saturating_sub(1) {
        for col in 0..HALF_W {
            let idx = usize::from(row) * usize::from(HALF_W) + usize::from(col);
            let is_dirty = prev_dirty.get(idx).copied().unwrap_or(false);
            let (ch, style) = if is_dirty {
                // Bright red block for dirty cells.
                (
                    '\u{2588}',
                    Style::new()
                        .fg(Color::Rgb {
                            r: 255,
                            g: 80,
                            b: 80,
                        })
                        .bg(Color::Rgb {
                            r: 40,
                            g: 10,
                            b: 10,
                        }),
                )
            } else {
                // Dim dot for clean cells.
                (
                    '\u{00B7}',
                    Style::new()
                        .fg(Color::Rgb {
                            r: 35,
                            g: 32,
                            b: 42,
                        })
                        .bg(Color::Rgb {
                            r: 15,
                            g: 14,
                            b: 20,
                        }),
                )
            };
            term.put_styled(HALF_W + col, row, ch, style);
        }
    }

    // ── Summary row (last anim row before footer) ──
    // todo!("present_stats not yet in library — replace prev_count with library value when available")
    let total = u32::from(HALF_W) * u32::from(ANIM_H);
    let summary = format!(" prev dirty: {prev_count} / {total} cells ");
    let summary_style = Style::new()
        .fg(Color::Rgb {
            r: 200,
            g: 150,
            b: 100,
        })
        .bg(Color::Rgb {
            r: 30,
            g: 22,
            b: 15,
        });
    let summary_row = ANIM_H.saturating_sub(1);
    for col in 0..HALF_W {
        term.put_styled(HALF_W + col, summary_row, ' ', summary_style);
    }
    for (col, ch) in (0u16..).zip(summary.chars()).take(usize::from(HALF_W)) {
        term.put_styled(HALF_W + col, summary_row, ch, summary_style);
    }
}

/// Draw the full-width footer (last row): dirty stats + perf HUD.
fn draw_footer<B: Backend>(term: &mut Terminal<B>, state: &VizState) {
    term.layer(1);

    let total = u32::from(HALF_W) * u32::from(ANIM_H);
    #[allow(clippy::cast_precision_loss)]
    let pct = if total > 0 {
        f64::from(state.prev_dirty_count) / f64::from(total) * 100.0
    } else {
        0.0
    };

    let status = format!(
        " dirty: {} / {} ({:.1}%)  frame {}  [Q=quit]",
        state.prev_dirty_count, total, pct, state.frame,
    );

    let bg = Color::Rgb {
        r: 22,
        g: 22,
        b: 35,
    };
    let fg = Color::Rgb {
        r: 160,
        g: 200,
        b: 160,
    };

    for col in 0..GRID_W {
        term.put_styled(col, FOOTER_ROW, ' ', Style::new().bg(bg));
    }
    term.fg(fg);
    term.bg(bg);
    term.print(0, FOOTER_ROW, &status);
    term.reset_style();

    state.perf.draw(term, HALF_W, FOOTER_ROW);
}

// ── Ball physics ──────────────────────────────────────────────────────────────

/// Advance the ball and bounce it off the left-half boundary.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn update_ball(state: &mut VizState) {
    state.prev_col = state.ball_col;
    state.prev_row = state.ball_row;

    let next_col = i32::from(state.ball_col) + i32::from(state.vel_col);
    let next_row = i32::from(state.ball_row) + i32::from(state.vel_row);

    let max_col = i32::from(HALF_W) - 1;
    let max_row = i32::from(ANIM_H) - 1;

    if next_col <= 0 || next_col >= max_col {
        state.vel_col = -state.vel_col;
    }
    if next_row <= 0 || next_row >= max_row {
        state.vel_row = -state.vel_row;
    }

    state.ball_col = next_col.clamp(0, max_col) as u16;
    state.ball_row = next_row.clamp(0, max_row) as u16;
}

// ── Game loop ─────────────────────────────────────────────────────────────────

fn init<B: Backend>(_term: &mut Terminal<B>) -> VizState {
    VizState::new()
}

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut VizState) -> bool {
    state.perf.begin_frame();

    // Rotate dirty maps: previous frame's map becomes the display map for
    // the overlay; the old display map is zeroed and reused as the new
    // tracking map.
    std::mem::swap(&mut state.dirty_map, &mut state.prev_dirty_map);
    for flag in &mut state.dirty_map {
        *flag = false;
    }
    state.prev_dirty_count = state.dirty_count;
    state.dirty_count = 0;

    // Background must be redrawn every frame: Terminal::present() clears the
    // grid after each swap so nothing persists across frames. Draw it directly
    // (bypassing put_left) so the background fill doesn't inflate dirty_count.
    term.layer(0);
    for row in 0..ANIM_H {
        for col in 0..HALF_W {
            let (ch, style) = bg_at(col, row);
            term.put_styled(col, row, ch, style);
        }
    }

    // On frame 0 the entire left half is genuinely new, so mark it all dirty.
    // On later frames only the ball move touches new cells.
    if state.frame == 0 {
        for flag in &mut state.dirty_map {
            *flag = true;
        }
        state.dirty_count = u32::from(HALF_W) * u32::from(ANIM_H);
    } else {
        restore_prev(term, state);
    }
    // Ball is drawn on top every frame (including frame 0).
    draw_ball(term, state);

    // Right half: heatmap from the *previous* frame's dirty map.
    draw_overlay(term, &state.prev_dirty_map, state.prev_dirty_count);

    // Footer: dirty stats + perf HUD.
    draw_footer(term, state);

    term.present().expect("present failed");

    update_ball(state);
    state.frame = state.frame.wrapping_add(1);

    for event in term.drain_events() {
        if event_to_action(&event) == Action::Quit {
            return false;
        }
    }
    true
}

// ── Entry point ───────────────────────────────────────────────────────────────

rg_run_software!(
    VizState,
    init,
    tick,
    builder = {
        SoftwareBackendBuilder::new()
            .title(env!("CARGO_BIN_NAME"))
            .grid_size(GRID_W, GRID_H)
            .scale(2)
            .target_fps(60)
    }
);
