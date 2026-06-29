#![allow(dead_code, unreachable_pub)]
//! Backend-agnostic rendering for the hex battle demo.
//!
//! `render_frame` works with any `Backend` — crossterm draws ASCII art hexes
//! and the software renderer draws PNG sprites. The same function serves both.

use retroglyph::Terminal;
use retroglyph::backend::Backend;
use retroglyph::color::Color;
use retroglyph::grid::Rect;

use super::hexmap::{
    BOARD_COLS, BOARD_ROWS, HEX_CELL_COLS, HEX_CELL_ROWS, MAP_ORIGIN_X, MAP_ORIGIN_Y,
    SPRITE_ATTACK, SPRITE_EMPIRE, SPRITE_EMPTY, SPRITE_REBEL, SPRITE_SELECTED, axial_to_cell,
    on_board,
};
use super::sim::{Faction, GameEvent, ReplayStep, Unit};

// ── Colour palette ────────────────────────────────────────────────────────────

const COL_BG: Color = Color::Rgb {
    r: 15,
    g: 20,
    b: 35,
};
const COL_REBEL: Color = Color::Rgb {
    r: 70,
    g: 120,
    b: 200,
};
const COL_EMPIRE: Color = Color::Rgb {
    r: 200,
    g: 60,
    b: 60,
};
const COL_SELECTED: Color = Color::Rgb {
    r: 140,
    g: 200,
    b: 240,
};
const COL_HEX_NORMAL: Color = Color::Rgb {
    r: 28,
    g: 42,
    b: 68,
};
const COL_HEX_EDGE: Color = Color::Rgb {
    r: 50,
    g: 70,
    b: 110,
};
const COL_HEADER: Color = Color::Rgb {
    r: 200,
    g: 200,
    b: 200,
};
const COL_DIM: Color = Color::Rgb {
    r: 90,
    g: 100,
    b: 120,
};
const COL_ACCENT: Color = Color::Rgb {
    r: 240,
    g: 200,
    b: 80,
};

// ── Map dimensions ────────────────────────────────────────────────────────────

/// Terminal columns used by the map area (includes left margin).
// BOARD_COLS and BOARD_ROWS are i32 constants (from hexmap). They're small
// positive values (9 and 7), so the conversion to u16 is exact.
// Using literals avoids the cast lint in a const context.
const BOARD_COLS_U16: u16 = 9;
const BOARD_ROWS_U16: u16 = 7;

/// Terminal columns used by the map area (includes left margin).
pub const MAP_COLS: u16 = MAP_ORIGIN_X + BOARD_COLS_U16 * HEX_CELL_COLS + HEX_CELL_COLS / 2 + 1;
/// Terminal rows used by the map area (includes top margin).
pub const MAP_ROWS: u16 = MAP_ORIGIN_Y + BOARD_ROWS_U16 * HEX_CELL_ROWS + 1;

// ── Public entry point ────────────────────────────────────────────────────────

pub struct RenderState {
    /// Which hex the cursor is hovering over (axial).
    pub hovered: Option<(i32, i32)>,
    /// Step index in the replay.
    pub step: usize,
    /// Total steps.
    pub total_steps: usize,
}

/// Draw the full frame: map, sidebar, footer.
pub fn render_frame<B: Backend>(
    term: &mut Terminal<B>,
    step: &ReplayStep,
    render: &RenderState,
    all_steps: &[ReplayStep],
) {
    let size = term.size();
    term.clear();
    term.layer(0).reset_style().bg(COL_BG);

    // Background fill.
    for y in 0..size.height {
        for x in 0..size.width {
            term.put(x, y, ' ');
        }
    }

    draw_map(term, step, render);
    draw_sidebar(term, step, render, all_steps, size.width, size.height);
    draw_footer(term, render, size.width, size.height);
}

// ── Map ───────────────────────────────────────────────────────────────────────

fn draw_map<B: Backend>(term: &mut Terminal<B>, step: &ReplayStep, render: &RenderState) {
    // Layer 0: hex backgrounds.
    term.layer(0);
    for r in 0..BOARD_ROWS {
        for q in 0..BOARD_COLS {
            if !on_board(q, r) {
                continue;
            }
            let Some((cx, cy)) = axial_to_cell(q, r) else {
                continue;
            };

            let is_hovered = render.hovered == Some((q, r));
            let has_attack = is_attack_target(q, r, step);

            #[cfg(feature = "software-tilesets")]
            {
                let sprite = if has_attack {
                    SPRITE_ATTACK
                } else if is_hovered {
                    SPRITE_SELECTED
                } else {
                    SPRITE_EMPTY
                };
                term.reset_style()
                    .fg(Color::WHITE)
                    .bg(Color::Rgb { r: 0, g: 0, b: 0 });
                term.put(cx, cy, sprite);
            }

            #[cfg(not(feature = "software-tilesets"))]
            {
                draw_ascii_hex(term, cx, cy, is_hovered, has_attack);
            }
        }
    }

    // Layer 1: unit markers.
    term.layer(1);
    for unit in step.units.iter().filter(|u| u.is_alive()) {
        let Some((cx, cy)) = axial_to_cell(unit.pos.0, unit.pos.1) else {
            continue;
        };
        draw_unit(term, cx, cy, unit);
    }

    term.layer(0);
}

/// Returns true if (q, r) is the target of the current step's attack event.
fn is_attack_target(q: i32, r: i32, step: &ReplayStep) -> bool {
    let GameEvent::Attack { target_id, .. } = &step.event else {
        return false;
    };
    step.units
        .iter()
        .any(|u| u.id == *target_id && u.pos == (q, r))
}

#[cfg(feature = "software-tilesets")]
fn draw_unit<B: Backend>(term: &mut Terminal<B>, cx: u16, cy: u16, unit: &Unit) {
    let sprite = match unit.faction {
        Faction::Rebel => SPRITE_REBEL,
        Faction::Empire => SPRITE_EMPIRE,
    };
    term.reset_style()
        .fg(Color::WHITE)
        .bg(Color::Rgb { r: 0, g: 0, b: 0 });
    term.put(cx, cy, sprite);

    // Strength label: glyph + count, one cell below and right of sprite origin.
    let fg = match unit.faction {
        Faction::Rebel => COL_REBEL,
        Faction::Empire => COL_EMPIRE,
    };
    term.reset_style().fg(fg).bg(COL_BG);
    term.print(
        cx + HEX_CELL_COLS / 2,
        cy + HEX_CELL_ROWS - 1,
        &format!("{}x{}", unit.kind.glyph(), unit.strength),
    );
}

#[cfg(not(feature = "software-tilesets"))]
fn draw_unit<B: Backend>(term: &mut Terminal<B>, cx: u16, cy: u16, unit: &Unit) {
    let fg = match unit.faction {
        Faction::Rebel => COL_REBEL,
        Faction::Empire => COL_EMPIRE,
    };
    term.reset_style().fg(fg).bg(COL_BG);
    term.print(cx, cy, &format!("{}x{}", unit.kind.glyph(), unit.strength));
}

/// ASCII hex for crossterm: a simple diamond-ish shape using box chars.
#[cfg(not(feature = "software-tilesets"))]
fn draw_ascii_hex<B: Backend>(
    term: &mut Terminal<B>,
    cx: u16,
    cy: u16,
    selected: bool,
    attack: bool,
) {
    let (fg, bg) = if attack {
        (COL_ACCENT, COL_BG)
    } else if selected {
        (COL_SELECTED, COL_BG)
    } else {
        (COL_HEX_EDGE, COL_HEX_NORMAL)
    };
    term.reset_style().fg(fg).bg(bg);
    // Row 0: top
    term.print(cx, cy, "/‾\\");
    // Row 1: middle
    term.print(cx, cy + 1, "\\__/");
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn draw_sidebar<B: Backend>(
    term: &mut Terminal<B>,
    step: &ReplayStep,
    render: &RenderState,
    all_steps: &[ReplayStep],
    term_w: u16,
    term_h: u16,
) {
    let sidebar_x = MAP_COLS + 1;
    let sidebar_w = term_w.saturating_sub(sidebar_x).saturating_sub(1);
    if sidebar_w < 10 {
        return;
    }

    // Score header.
    let rebel_medals = count_eliminations(all_steps, Faction::Empire); // rebels earn from eliminating empire
    let empire_medals = count_eliminations(all_steps, Faction::Rebel);
    let score_line = format!(
        "Rebel {}/{} medals  Empire {}/{}",
        rebel_medals, 4, empire_medals, 4
    );
    term.reset_style().fg(COL_HEADER).bg(COL_BG);
    term.print(
        sidebar_x,
        1,
        &score_line[..score_line.len().min(sidebar_w as usize)],
    );

    // Turn / card header.
    let (turn_label, card_name, faction_col) = match &step.event {
        GameEvent::TurnStart {
            turn,
            faction,
            card,
        } => {
            let col = match faction {
                Faction::Rebel => COL_REBEL,
                Faction::Empire => COL_EMPIRE,
            };
            (*turn, card.name, col)
        }
        _ => {
            // Show info from previous turn-start in the replay.
            if let Some((t, f, c)) = find_prev_turn(all_steps, render.step) {
                let col = match f {
                    Faction::Rebel => COL_REBEL,
                    Faction::Empire => COL_EMPIRE,
                };
                (t, c, col)
            } else {
                (0, "—", COL_DIM)
            }
        }
    };

    term.reset_style().fg(faction_col).bg(COL_BG);
    term.print(sidebar_x, 3, &format!("Turn {turn_label}: {card_name}"));

    // Event log.
    term.reset_style().fg(COL_DIM).bg(COL_BG);
    term.print(sidebar_x, 5, "Events");

    let log_y_start = 6u16;
    let log_rows = (term_h.saturating_sub(log_y_start + 6)).min(8) as usize;
    let recent: Vec<_> = all_steps[..=render.step.min(all_steps.len().saturating_sub(1))]
        .iter()
        .rev()
        .take(log_rows)
        .collect();

    for (i, s) in recent.iter().rev().enumerate() {
        let desc = s.event.description(&s.units);
        let truncated = &desc[..desc.len().min(sidebar_w as usize)];
        let col = match &s.event {
            GameEvent::Attack {
                eliminated: true, ..
            }
            | GameEvent::TurnStart {
                faction: Faction::Empire,
                ..
            } => COL_EMPIRE,
            GameEvent::TurnStart {
                faction: Faction::Rebel,
                ..
            } => COL_REBEL,
            _ => COL_HEADER,
        };
        term.reset_style().fg(col).bg(COL_BG);
        term.print(
            sidebar_x,
            log_y_start + u16::try_from(i).unwrap_or(u16::MAX),
            truncated,
        );
    }

    // Hands.
    let hands_y = term_h.saturating_sub(5);
    draw_hand(
        term,
        sidebar_x,
        hands_y,
        sidebar_w,
        "Rebel hand",
        &step.rebel_hand,
        COL_REBEL,
    );
    draw_hand(
        term,
        sidebar_x,
        hands_y + 2,
        sidebar_w,
        "Empire hand",
        &step.empire_hand,
        COL_EMPIRE,
    );
}

fn draw_hand<B: Backend>(
    term: &mut Terminal<B>,
    x: u16,
    y: u16,
    max_w: u16,
    label: &str,
    cards: &[super::sim::Card],
    col: Color,
) {
    term.reset_style().fg(col).bg(COL_BG);
    term.print(x, y, label);

    let mut cx = x;
    let card_y = y + 1;
    for card in cards {
        let tag = format!("[{}]", card.name);
        if cx + u16::try_from(tag.len()).unwrap_or(u16::MAX) > x + max_w {
            break;
        }
        let fg = if card.has_special {
            COL_ACCENT
        } else {
            COL_HEADER
        };
        term.reset_style().fg(fg).bg(COL_BG);
        term.print(cx, card_y, &tag);
        cx += u16::try_from(tag.len())
            .unwrap_or(u16::MAX)
            .saturating_add(1);
    }
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn draw_footer<B: Backend>(term: &mut Terminal<B>, render: &RenderState, w: u16, h: u16) {
    let y = h.saturating_sub(2);
    term.reset_style().fg(COL_DIM).bg(COL_BG);
    term.print(1, y, "◄ Prev  ► Next  [Q] Quit");

    // Step counter.
    let counter = format!("{} / {}", render.step + 1, render.total_steps);
    let cx = w.saturating_sub(
        u16::try_from(counter.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2),
    );
    term.reset_style().fg(COL_HEADER).bg(COL_BG);
    term.print(cx, y, &counter);

    // Progress bar.
    let bar_x = 26u16;
    let bar_w = cx.saturating_sub(bar_x + 2);
    if bar_w > 0 {
        use crate::util::draw::progress_bar;
        use retroglyph::style::Style;
        let filled = Style::new().fg(COL_SELECTED).bg(COL_BG);
        let empty = Style::new().fg(COL_DIM).bg(COL_BG);
        progress_bar(
            term,
            Rect::new(bar_x, y, bar_w.into(), 1usize),
            u32::try_from(render.step).unwrap_or(u32::MAX),
            u32::try_from(render.total_steps.saturating_sub(1)).unwrap_or(u32::MAX),
            filled,
            empty,
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn count_eliminations(steps: &[ReplayStep], attacker_faction: Faction) -> u32 {
    steps
        .iter()
        .filter(|s| {
            if let GameEvent::Attack {
                attacker_id,
                eliminated: true,
                ..
            } = &s.event
            {
                s.units
                    .iter()
                    .any(|u| u.id == *attacker_id && u.faction == attacker_faction)
            } else {
                false
            }
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn find_prev_turn(steps: &[ReplayStep], current: usize) -> Option<(u32, Faction, &'static str)> {
    steps[..=current.min(steps.len().saturating_sub(1))]
        .iter()
        .rev()
        .find_map(|s| {
            if let GameEvent::TurnStart {
                turn,
                faction,
                card,
            } = &s.event
            {
                Some((*turn, *faction, card.name))
            } else {
                None
            }
        })
}
