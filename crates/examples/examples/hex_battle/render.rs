#![allow(dead_code, unreachable_pub)]
//! Backend-agnostic rendering for the hex battle demo.
//!
//! Works with any `Backend`: crossterm draws two-tone colored blocks for each
//! hex cell; the software renderer draws PNG sprites.  The same `render_frame`
//! function serves both paths.
//!
//! # Layout
//!
//! ```text
//! ┌────────────────── map ──────────────────┬──────── sidebar ────────────┐
//! │                                         │  Blue 0/4      Red 0/4      │
//! │  hex grid (MAP_COLS wide × MAP_ROWS)    │  Turn 1: BLUE  Advance      │
//! │                                         │  ─────────────────────────  │
//! │                                         │  Events                     │
//! │                                         │  ...                        │
//! ├─────────────────────────────────────────┴─────────────────────────────┤
//! │  Blue hand: [Advance] [Assault] [Scout]  │  Red: [Advance] [Blitz⚡]  │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │  ◄ Prev   ► Next   [Q] Quit    ████░░░░░░░░░░░░░░░░   2 / 3          │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```

use retroglyph_core::Backend;
use retroglyph_core::Terminal;
use retroglyph_core::color::Color;
use retroglyph_core::grid::Rect;

use super::hexmap::{
    BOARD_COLS, BOARD_ROWS, HEX_CELL_COLS, HEX_CELL_ROWS, MAP_ORIGIN_X, MAP_ORIGIN_Y, axial_to_cell,
};
#[cfg(feature = "tilesets")]
use super::hexmap::{SPRITE_ATTACK, SPRITE_BLUE, SPRITE_EMPTY, SPRITE_RED, SPRITE_SELECTED};
use super::sim::{Faction, GameEvent, ReplayStep, Unit};

// ── Colour palette ────────────────────────────────────────────────────────────

const COL_BG: Color = Color::Rgb {
    r: 15,
    g: 20,
    b: 35,
};
const COL_BLUE: Color = Color::Rgb {
    r: 70,
    g: 130,
    b: 210,
};
const COL_RED: Color = Color::Rgb {
    r: 210,
    g: 65,
    b: 65,
};
const COL_SELECTED: Color = Color::Rgb {
    r: 140,
    g: 200,
    b: 240,
};
/// Hex fill — top row (slightly lighter for a subtle two-tone effect).
const COL_HEX_TOP: Color = Color::Rgb {
    r: 30,
    g: 46,
    b: 75,
};
/// Hex fill — bottom row (slightly darker).
const COL_HEX_BOT: Color = Color::Rgb {
    r: 22,
    g: 35,
    b: 58,
};
/// Hovered hex fill.
const COL_HEX_HOVER: Color = Color::Rgb {
    r: 50,
    g: 75,
    b: 120,
};
/// Attack-target hex fill.
const COL_HEX_ATTACK: Color = Color::Rgb {
    r: 80,
    g: 55,
    b: 20,
};
const COL_HEADER: Color = Color::Rgb {
    r: 200,
    g: 200,
    b: 210,
};
const COL_DIM: Color = Color::Rgb {
    r: 90,
    g: 100,
    b: 125,
};
const COL_ACCENT: Color = Color::Rgb {
    r: 240,
    g: 200,
    b: 80,
};
/// Sidebar background — just slightly different from map bg for visual separation.
const COL_SIDEBAR_BG: Color = Color::Rgb {
    r: 18,
    g: 23,
    b: 40,
};
/// Background behind the active-turn label.
const COL_TURN_BG: Color = Color::Rgb {
    r: 25,
    g: 38,
    b: 65,
};
/// Vertical separator between map and sidebar.
const COL_BORDER: Color = Color::Rgb {
    r: 40,
    g: 55,
    b: 85,
};

// ── Map dimensions ────────────────────────────────────────────────────────────

const BOARD_COLS_U16: u16 = 12;
const BOARD_ROWS_U16: u16 = 7;

/// Terminal columns used by the map area (includes right-side margin).
pub const MAP_COLS: u16 =
    MAP_ORIGIN_X + BOARD_COLS_U16 * HEX_CELL_COLS + HEX_CELL_COLS / 2 + HEX_CELL_COLS;
/// Terminal rows used by the map area.
pub const MAP_ROWS: u16 = MAP_ORIGIN_Y + BOARD_ROWS_U16 * HEX_CELL_ROWS + 1;
/// Column of the vertical separator.  Nothing in the map should render at
/// or past this column.
const SEP_X: u16 = MAP_COLS + 1;

// ── Public entry point ────────────────────────────────────────────────────────

pub struct RenderState {
    /// Which hex the cursor is hovering over (axial).
    pub hovered: Option<(i32, i32)>,
    /// Step index in the replay.
    pub step: usize,
    /// Total steps.
    pub total_steps: usize,
}

/// Draw the full frame: map, sidebar, footer, card hands.
pub fn render_frame<B: Backend>(
    term: &mut Terminal<B>,
    step: &ReplayStep,
    render: &RenderState,
    all_steps: &[ReplayStep],
) {
    let size = term.size();
    term.clear();
    term.layer(0).reset_style().bg(COL_BG);

    // 1. Full-screen background fill on layer 0.
    for y in 0..size.height {
        for x in 0..size.width {
            term.put(x, y, ' ');
        }
    }

    // 2. Map (crossterm: layer 0; software: layers 1–2).
    draw_map(term, step, render);

    // 3. Sidebar region on layer 0 — drawn AFTER the map so it overwrites
    //    any hex ASCII art that leaked into the sidebar columns.
    //    (Hex sprites are clipped by draw_map, so this is just belt-and-
    //    suspenders for the crossterm/headless path.)
    let sep_x = SEP_X;
    let sidebar_x = sep_x + 1;
    term.layer(0);
    draw_sidebar_bg(term, sep_x, sidebar_x, size.width, size.height);

    // 4. Sidebar text content, cards, footer — all on layer 0.
    draw_sidebar(term, render, all_steps, sidebar_x, size.width, size.height);
    draw_cards(term, step, size.width, size.height);
    draw_footer(term, render, size.width, size.height);
}

/// Fill the sidebar background region: separator + fill.
fn draw_sidebar_bg<B: Backend>(
    term: &mut Terminal<B>,
    sep_x: u16,
    sidebar_x: u16,
    term_w: u16,
    term_h: u16,
) {
    for y in 0..term_h.saturating_sub(4) {
        term.reset_style().fg(COL_BORDER).bg(COL_BG);
        term.put(sep_x, y, '│');
        for x in sidebar_x..term_w {
            term.reset_style().bg(COL_SIDEBAR_BG);
            term.put(x, y, ' ');
        }
    }
}

// ── Map ───────────────────────────────────────────────────────────────────────

fn draw_map<B: Backend>(term: &mut Terminal<B>, step: &ReplayStep, render: &RenderState) {
    // ── Software-tilesets path ─────────────────────────────────────────
    //
    // The SoftwareRenderer's draw_layers handles multiple layers properly:
    // layer 0 gets bg-filled, higher layers composite with alpha blending.
    //
    // Multi-cell sprites overflow from their anchor cell into neighboring
    // cells.  If the overflow cells have layer-0 tiles, their bg fill
    // paints over the sprite.  To prevent that, hex sprites go on layer 1
    // (no bg fill) and unit sprites go on layer 2.  Layer 0 only carries
    // the full-screen background fill from render_frame.
    //
    // ── Crossterm / headless path ─────────────────────────────────────
    //
    // The default Backend::draw_layers drops layer ≥ 1.  Everything must
    // be on layer 0, drawn in painter's order (ASCII hexes, then units).

    // Clip: skip any hex whose right edge would reach the separator.
    let clip = |cx: u16| cx + HEX_CELL_COLS < SEP_X;

    // Iterate offset columns for a proper rectangular board.  The naive
    // `for q in 0..BOARD_COLS` produces a parallelogram because the axial→
    // offset shift grows with r.  Iterating offset columns 0..BOARD_COLS
    // and converting back to axial gives a uniform rectangle.
    let board_hexes = || {
        (0..BOARD_ROWS).flat_map(|r| {
            let shift = (r - (r & 1)) / 2;
            (0..BOARD_COLS).map(move |offset_col| {
                let q = offset_col - shift;
                (q, r)
            })
        })
    };

    #[cfg(feature = "tilesets")]
    {
        term.layer(1);
        for (q, r) in board_hexes() {
            let Some((cx, cy)) = axial_to_cell(q, r) else {
                continue;
            };
            if !clip(cx) {
                continue;
            }
            let is_hovered = render.hovered == Some((q, r));
            let has_attack = is_attack_target(q, r, step);
            let sprite = if has_attack {
                SPRITE_ATTACK
            } else if is_hovered {
                SPRITE_SELECTED
            } else {
                SPRITE_EMPTY
            };
            term.reset_style().fg(Color::WHITE).bg(COL_BG);
            term.put(cx, cy, sprite);
        }

        term.layer(2);
        for unit in step.units.iter().filter(|u| u.is_alive()) {
            let Some((cx, cy)) = axial_to_cell(unit.pos.0, unit.pos.1) else {
                continue;
            };
            if !clip(cx) {
                continue;
            }
            draw_unit_sprite(term, cx, cy, unit);
        }

        term.layer(0);
    }

    #[cfg(not(feature = "tilesets"))]
    {
        term.layer(0);
        for (q, r) in board_hexes() {
            let Some((cx, cy)) = axial_to_cell(q, r) else {
                continue;
            };
            if !clip(cx) {
                continue;
            }
            let is_hovered = render.hovered == Some((q, r));
            let has_attack = is_attack_target(q, r, step);
            draw_ascii_hex(term, cx, cy, is_hovered, has_attack);
        }
        for unit in step.units.iter().filter(|u| u.is_alive()) {
            let Some((cx, cy)) = axial_to_cell(unit.pos.0, unit.pos.1) else {
                continue;
            };
            if !clip(cx) {
                continue;
            }
            draw_unit_ascii(term, cx, cy, unit);
        }
    }
}

fn is_attack_target(q: i32, r: i32, step: &ReplayStep) -> bool {
    let GameEvent::Attack { target_id, .. } = &step.event else {
        return false;
    };
    step.units
        .iter()
        .any(|u| u.id == *target_id && u.pos == (q, r))
}

// ── ASCII art hex (crossterm / headless) ───────────────────────────────────────

/// Draw a 4×2 ASCII art hex cell with uniform fill background.
///
/// ```text
/// /  \      empty hex (outline on fill bg)
/// \──/
///
/// /T4\      with unit label
/// \──/
/// ```
///
/// All 8 cells (4×2) share the same fill background. The outline characters
/// (`/`, `\`, `─`) define the hex shape visually. Adjacent hexes share
/// borders (`\/` and `/\`), forming a honeycomb.
#[cfg(not(feature = "tilesets"))]
fn draw_ascii_hex<B: Backend>(
    term: &mut Terminal<B>,
    cx: u16,
    cy: u16,
    hovered: bool,
    attack: bool,
) {
    let (fg, fill) = if attack {
        (COL_ACCENT, COL_HEX_ATTACK)
    } else if hovered {
        (COL_SELECTED, COL_HEX_HOVER)
    } else {
        (COL_BORDER, COL_HEX_BOT)
    };
    // Uniform fill bg for all cells; outline chars define the shape.
    term.reset_style().fg(fg).bg(fill);
    term.put(cx, cy, '/');
    term.put(cx + 1, cy, ' ');
    term.put(cx + 2, cy, ' ');
    term.put(cx + 3, cy, '\\');
    term.put(cx, cy + 1, '\\');
    term.put(cx + 1, cy + 1, '─');
    term.put(cx + 2, cy + 1, '─');
    term.put(cx + 3, cy + 1, '/');
}

/// Draw a unit label inside an ASCII hex, centered in the interior.
#[cfg(not(feature = "tilesets"))]
fn draw_unit_ascii<B: Backend>(term: &mut Terminal<B>, cx: u16, cy: u16, unit: &Unit) {
    let fg = match unit.faction {
        Faction::Blue => COL_BLUE,
        Faction::Red => COL_RED,
    };
    let strength_char = char::from_digit(u32::from(unit.strength), 10).unwrap_or('+');
    // Overwrite the 2 interior cells at row 0 (cols 1–2).
    term.reset_style().fg(fg).bg(COL_HEX_BOT);
    term.put(cx + 1, cy, unit.kind.glyph());
    term.put(cx + 2, cy, strength_char);
}

// ── Sprite rendering (software-tilesets) ─────────────────────────────────────

/// Draw a unit sprite marker with label text.
///
/// The faction sprite (blue/red circle) is placed on layer 2, where the
/// `SoftwareRenderer` alpha-blends it on top of the hex tile beneath.
/// No text labels are overlaid — the sprite itself contains the circle
/// and letter area from build.rs.
#[cfg(feature = "tilesets")]
fn draw_unit_sprite<B: Backend>(term: &mut Terminal<B>, cx: u16, cy: u16, unit: &Unit) {
    let sprite = match unit.faction {
        Faction::Blue => SPRITE_BLUE,
        Faction::Red => SPRITE_RED,
    };
    // bg = COL_BG so the transparent parts of the unit circle blend into
    // the scene background, not black.
    term.reset_style().fg(Color::WHITE).bg(COL_BG);
    term.put(cx, cy, sprite);
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn draw_sidebar<B: Backend>(
    term: &mut Terminal<B>,
    render: &RenderState,
    all_steps: &[ReplayStep],
    sidebar_x: u16,
    term_w: u16,
    term_h: u16,
) {
    let sidebar_w = term_w.saturating_sub(sidebar_x).saturating_sub(1);
    if sidebar_w < 8 {
        return;
    }

    // ── Score row ──────────────────────────────────────────────────────────
    // Only count eliminations that have occurred up to the current step.
    let visible = &all_steps[..=render.step.min(all_steps.len().saturating_sub(1))];
    let blue_score = count_eliminations(visible, Faction::Red);
    let red_score = count_eliminations(visible, Faction::Blue);

    term.reset_style().fg(COL_BLUE).bg(COL_SIDEBAR_BG);
    term.print(sidebar_x, 1, &format!("Blue {blue_score}/4"));

    let red_label = format!("Red {red_score}/4");
    let rx = term_w
        .saturating_sub(u16::try_from(red_label.len()).unwrap_or(20))
        .saturating_sub(1);
    term.reset_style().fg(COL_RED).bg(COL_SIDEBAR_BG);
    term.print(rx, 1, &red_label);

    // ── Turn / card header ─────────────────────────────────────────────────
    let (turn_num, faction, card_name) = current_turn_info(all_steps, render.step);
    let faction_col = faction_color(faction);
    let faction_name = faction.name();

    // Highlight box behind turn label.
    for x in sidebar_x..term_w.saturating_sub(1) {
        term.reset_style().bg(COL_TURN_BG);
        term.put(x, 3, ' ');
    }
    term.reset_style().fg(COL_DIM).bg(COL_TURN_BG);
    term.print(sidebar_x, 3, &format!("Turn {turn_num}:"));
    let after_turn = sidebar_x + u16::try_from(format!("Turn {turn_num}:").len()).unwrap_or(8);
    term.reset_style().fg(faction_col).bg(COL_TURN_BG);
    term.print(after_turn + 1, 3, faction_name);
    let after_faction = after_turn + 1 + u16::try_from(faction_name.len()).unwrap_or(4);
    term.reset_style().fg(COL_HEADER).bg(COL_TURN_BG);
    let card_trunc = truncate(card_name, term_w.saturating_sub(after_faction + 2) as usize);
    term.print(after_faction + 1, 3, card_trunc);

    // ── Event log ──────────────────────────────────────────────────────────
    // Thin separator.
    for x in sidebar_x..term_w.saturating_sub(1) {
        term.reset_style().fg(COL_BORDER).bg(COL_SIDEBAR_BG);
        term.put(x, 5, '─');
    }

    term.reset_style().fg(COL_DIM).bg(COL_SIDEBAR_BG);
    term.print(sidebar_x, 6, "Events");

    let log_y = 7u16;
    // Reserve bottom rows for cards + footer.
    // Reserve 4 rows at the bottom: card-strip separator + Blue row +
    // Red row + footer.  The extra subtract gives one blank buffer row.
    let log_rows = term_h.saturating_sub(log_y).saturating_sub(5) as usize;

    let recent: Vec<_> = all_steps[..=render.step.min(all_steps.len().saturating_sub(1))]
        .iter()
        .rev()
        .take(log_rows)
        .collect();

    for (i, s) in recent.iter().rev().enumerate() {
        let desc = s.event.description(&s.units);
        let col = event_color(&s.event);
        term.reset_style().fg(col).bg(COL_SIDEBAR_BG);
        term.print(
            sidebar_x,
            log_y + u16::try_from(i).unwrap_or(u16::MAX),
            truncate(&desc, sidebar_w as usize),
        );
    }
}

// ── Card hands ────────────────────────────────────────────────────────────────

fn draw_cards<B: Backend>(term: &mut Terminal<B>, step: &ReplayStep, term_w: u16, term_h: u16) {
    // Stacked layout: separator, Blue row, Red row.  Each hand gets the full
    // terminal width, so no cards are truncated by a half-width column split.
    let sep_y = term_h.saturating_sub(4);
    let blue_y = sep_y + 1;
    let red_y = sep_y + 2;

    term.reset_style().fg(COL_BORDER).bg(COL_BG);
    for x in 0..term_w {
        term.put(x, sep_y, '─');
    }

    draw_hand(term, blue_y, term_w, "Blue:", &step.blue_hand, COL_BLUE);
    draw_hand(term, red_y, term_w, "Red: ", &step.red_hand, COL_RED);
}

fn draw_hand<B: Backend>(
    term: &mut Terminal<B>,
    y: u16,
    term_w: u16,
    label: &str,
    cards: &[super::sim::Card],
    label_col: Color,
) {
    term.reset_style().fg(label_col).bg(COL_BG);
    term.print(1, y, label);
    let mut cx = 1u16 + u16::try_from(label.len()).unwrap_or(6) + 1;
    for card in cards {
        let tag = format!("[{}]", card.name);
        let tag_w = u16::try_from(tag.len()).unwrap_or(u16::MAX);
        if cx + tag_w >= term_w.saturating_sub(1) {
            break;
        }
        let fg = if card.has_special {
            COL_ACCENT
        } else {
            COL_HEADER
        };
        term.reset_style().fg(fg).bg(COL_BG);
        term.print(cx, y, &tag);
        cx += tag_w + 1;
    }
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn draw_footer<B: Backend>(term: &mut Terminal<B>, render: &RenderState, w: u16, h: u16) {
    let y = h.saturating_sub(1);
    term.reset_style().fg(COL_DIM).bg(COL_BG);
    term.print(1, y, "◄ Prev   ► Next   [Q] Quit");

    let counter = format!("{} / {}", render.step + 1, render.total_steps);
    let cx = w.saturating_sub(
        u16::try_from(counter.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2),
    );
    term.reset_style().fg(COL_HEADER).bg(COL_BG);
    term.print(cx, y, &counter);

    let bar_x = 27u16;
    let bar_w = cx.saturating_sub(bar_x + 2);
    if bar_w > 0 {
        use retroglyph_core::style::Style;
        use retroglyph_widgets::progress_bar;
        let filled = Style::new().fg(COL_SELECTED).bg(COL_BG);
        let empty = Style::new().fg(COL_DIM).bg(COL_BG);
        progress_bar(
            term,
            Rect::new(bar_x, y, bar_w, 1),
            u32::try_from(render.step).unwrap_or(u32::MAX),
            u32::try_from(render.total_steps.saturating_sub(1)).unwrap_or(u32::MAX),
            filled,
            empty,
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const fn faction_color(faction: Faction) -> Color {
    match faction {
        Faction::Blue => COL_BLUE,
        Faction::Red => COL_RED,
    }
}

const fn event_color(event: &GameEvent) -> Color {
    match event {
        GameEvent::TurnStart {
            faction: Faction::Blue,
            ..
        } => COL_BLUE,
        GameEvent::TurnStart {
            faction: Faction::Red,
            ..
        } => COL_RED,
        GameEvent::Attack {
            eliminated: true, ..
        } => COL_ACCENT,
        _ => COL_HEADER,
    }
}

/// Find the most recent `TurnStart` at or before `current` step.
fn current_turn_info(steps: &[ReplayStep], current: usize) -> (u32, Faction, &'static str) {
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
        .unwrap_or((1, Faction::Blue, "—"))
}

fn count_eliminations(steps: &[ReplayStep], eliminated_faction: Faction) -> u32 {
    steps
        .iter()
        .filter(|s| {
            if let GameEvent::Attack {
                target_id,
                eliminated: true,
                ..
            } = &s.event
            {
                s.units
                    .iter()
                    .any(|u| u.id == *target_id && u.faction == eliminated_faction)
            } else {
                false
            }
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// Truncate `s` to at most `max_chars` bytes (ASCII-safe).
fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        s
    } else {
        &s[..max_chars]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::Headless;

    fn make_step0() -> (ReplayStep, Vec<ReplayStep>) {
        let (_, steps) = crate::sim::build_scenario();
        let step = steps[0].clone();
        (step, steps)
    }

    // The headless backend records raw codepoints. With tilesets active the
    // cells contain sprite indices (\x00, \x02 …) — format_view is meaningless
    // for that path. Only run against the colored-block path.
    #[cfg(not(feature = "tilesets"))]
    #[test]
    fn hex_battle_initial_layout() {
        let backend = Headless::new(80, 24);
        let mut term = Terminal::new(backend);
        let (step, all_steps) = make_step0();
        let render = RenderState {
            hovered: None,
            step: 0,
            total_steps: all_steps.len(),
        };
        render_frame(&mut term, &step, &render, &all_steps);
        term.present().expect("present");

        let view = term.backend().format_view();
        // Sidebar score header should appear.
        assert!(view.contains("Blue"), "Blue score missing");
        assert!(view.contains("Red"), "Red score missing");
        // Footer should appear.
        assert!(view.contains("Prev"), "footer missing");
        // Card hands should appear.
        assert!(view.contains("Advance"), "blue hand missing");

        insta::assert_snapshot!(view);
    }

    #[cfg(not(feature = "tilesets"))]
    #[test]
    fn hex_battle_attack_step() {
        let backend = Headless::new(80, 24);
        let mut term = Terminal::new(backend);
        let (_, all_steps) = make_step0();
        let step = all_steps[2].clone(); // attack step
        let render = RenderState {
            hovered: None,
            step: 2,
            total_steps: all_steps.len(),
        };
        render_frame(&mut term, &step, &render, &all_steps);
        term.present().expect("present");

        let view = term.backend().format_view();
        // Attack event should appear in the event log.
        assert!(view.contains("eliminated"), "attack event missing");

        insta::assert_snapshot!(view);
    }

    // ── Software renderer tests ───────────────────────────────────────────────
    //
    // Two complementary pixel-level snapshot tests:
    //   • `software_colored_blocks`  – `software-default-font` only (no sprites)
    //   • `software_sprites`         – `software-tilesets` + `software-default-font`
    //
    // Each produces a PNG baseline in `examples/hex_battle/snapshots/`.
    // Update with: RG_SNAPSHOT_UPDATE=overwrite cargo test ...

    /// Software renderer pixel tests.
    ///
    /// `assert_png_snapshot` takes an explicit `scale` so pixel dimensions
    /// match the `SoftwareBackendBuilder::scale()` value used in each test.
    /// Pixel buffer size = `cols × GLYPH_W × scale` × `rows × GLYPH_H × scale`.
    #[cfg(all(feature = "default-font", not(target_arch = "wasm32")))]
    mod software {
        use super::super::*;
        use retroglyph_core::Terminal;
        use retroglyph_software::{SoftwareBackendBuilder, SoftwareRenderer};

        const GLYPH_W: u32 = 8;
        const GLYPH_H: u32 = 16;

        fn snapshot_dir() -> std::path::PathBuf {
            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples/hex_battle/snapshots");
            std::fs::create_dir_all(&dir).ok();
            dir
        }

        fn make_step0() -> (ReplayStep, Vec<ReplayStep>) {
            let (_, steps) = crate::sim::build_scenario();
            (steps[0].clone(), steps)
        }

        /// Assert the renderer's pixel buffer matches a stored PNG baseline.
        ///
        /// On mismatch: writes `{name}_actual.png` and `{name}_diff.png` to the
        /// snapshot directory, then panics.  Set `RG_SNAPSHOT_UPDATE=overwrite`
        /// to regenerate the baseline.
        fn assert_png_snapshot(renderer: &SoftwareRenderer, name: &str, scale: u32) {
            let size = renderer.size();
            let img_w = u32::from(size.width) * GLYPH_W * scale;
            let img_h = u32::from(size.height) * GLYPH_H * scale;
            let pixels = renderer.pixels();

            let dir = snapshot_dir();
            let expected_path = dir.join(format!("{name}.png"));
            let actual_path = dir.join(format!("{name}_actual.png"));
            let diff_path = dir.join(format!("{name}_diff.png"));

            // Encode the current output as PNG.
            let actual_png = {
                use image::ImageEncoder as _;
                let mut buf = Vec::new();
                let raw: Vec<u8> = pixels
                    .iter()
                    .flat_map(|&p| {
                        [
                            ((p >> 16) & 0xFF) as u8,
                            ((p >> 8) & 0xFF) as u8,
                            (p & 0xFF) as u8,
                            255u8,
                        ]
                    })
                    .collect();
                image::codecs::png::PngEncoder::new(&mut buf)
                    .write_image(&raw, img_w, img_h, image::ExtendedColorType::Rgba8)
                    .expect("encode PNG");
                buf
            };

            if std::env::var("RG_SNAPSHOT_UPDATE").as_deref() == Ok("overwrite") {
                std::fs::write(&expected_path, &actual_png).expect("write baseline");
                let _ = std::fs::remove_file(&actual_path);
                let _ = std::fs::remove_file(&diff_path);
                return;
            }

            let baseline = match std::fs::read(&expected_path) {
                Ok(b) => b,
                Err(_) => {
                    std::fs::write(&actual_path, &actual_png).expect("write actual");
                    panic!(
                        "no baseline at {}\n  inspect: {}\n  accept: RG_SNAPSHOT_UPDATE=overwrite cargo test",
                        expected_path.display(),
                        actual_path.display(),
                    );
                }
            };

            if actual_png == baseline {
                return;
            }

            // Pixel-level diff.
            let load = |data: &[u8]| image::load_from_memory(data).expect("decode").into_rgba8();
            let expected_img = load(&baseline);
            // `img_w` here is the correct stride — pixels are laid out as
            // `(grid_cols * GLYPH_W * scale)` per row.
            let actual_img = image::RgbaImage::from_fn(img_w, img_h, |x, y| {
                let p = pixels[(y * img_w + x) as usize];
                image::Rgba([
                    ((p >> 16) & 0xFF) as u8,
                    ((p >> 8) & 0xFF) as u8,
                    (p & 0xFF) as u8,
                    255,
                ])
            });
            assert_eq!(
                expected_img.dimensions(),
                actual_img.dimensions(),
                "image dimensions differ",
            );

            let mut diff_img = actual_img.clone();
            let mut first_diff: Option<(u32, u32, image::Rgba<u8>, image::Rgba<u8>)> = None;
            for y in 0..img_h {
                for x in 0..img_w {
                    let exp = expected_img.get_pixel(x, y);
                    let act = actual_img.get_pixel(x, y);
                    if exp != act {
                        first_diff.get_or_insert((x, y, *exp, *act));
                        diff_img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
                    }
                }
            }

            actual_img.save(&actual_path).expect("write actual");
            diff_img.save(&diff_path).expect("write diff");

            let (dx, dy, exp, act) = first_diff.expect("byte diff but no pixel diff");
            panic!(
                "pixel mismatch at ({dx},{dy}): expected ({},{},{}) got ({},{},{})\n\
                 actual: {}\n\
                 diff:   {}\n\
                 accept: RG_SNAPSHOT_UPDATE=overwrite cargo test",
                exp[0],
                exp[1],
                exp[2],
                act[0],
                act[1],
                act[2],
                actual_path.display(),
                diff_path.display(),
            );
        }

        /// Software renderer, no tilesets: exercises the colored-block rendering
        /// path through the actual pixel pipeline (VGA font, scale=1, 80×24).
        #[cfg(not(feature = "tilesets"))]
        #[test]
        fn software_colored_blocks() {
            let renderer = SoftwareBackendBuilder::new()
                .grid_size(100, 24)
                .scale(1)
                .build()
                .expect("build")
                .run_headless();

            let mut term = Terminal::new(renderer);
            let (step, all_steps) = make_step0();
            let render = RenderState {
                hovered: None,
                step: 0,
                total_steps: all_steps.len(),
            };
            render_frame(&mut term, &step, &render, &all_steps);
            term.present().expect("present");

            assert_png_snapshot(term.backend(), "hex_battle_software_colored_blocks", 1);
        }

        /// Software renderer with PNG sprites: exercises the tileset rendering
        /// path (hexagon sprites from build.rs).
        ///
        /// Uses `scale(2)` — the production setting — so each hex cell renders
        /// at 48×64 px (close to the native 64×64 sprite) and text labels are
        /// readable.  Image: (MAP_COLS+36)×16 × (MAP_ROWS+4)×32 pixels.
        #[cfg(feature = "tilesets")]
        #[test]
        fn software_sprites() {
            use retroglyph_software::tileset::{Codepage, TilesetOptions};

            static HEX_SPRITE_BYTES: &[u8] =
                include_bytes!(concat!(env!("OUT_DIR"), "/hex_sprites.png"));

            let tileset = TilesetOptions::from_bytes(HEX_SPRITE_BYTES.to_vec())
                .tile_size(32, 32)
                .codepage(Codepage::Identity)
                .spacing(crate::hexmap::HEX_CELL_COLS, crate::hexmap::HEX_CELL_ROWS)
                .build()
                .expect("tileset build");

            let renderer = SoftwareBackendBuilder::new()
                .grid_size(MAP_COLS + 50, MAP_ROWS + 4)
                .scale(2)
                .tileset(tileset)
                .build()
                .expect("build")
                .run_headless();

            let mut term = Terminal::new(renderer);
            let (step, all_steps) = make_step0();
            let render = RenderState {
                hovered: None,
                step: 0,
                total_steps: all_steps.len(),
            };
            render_frame(&mut term, &step, &render, &all_steps);
            term.present().expect("present");

            assert_png_snapshot(term.backend(), "hex_battle_software_sprites", 2);
        }
    }
}
