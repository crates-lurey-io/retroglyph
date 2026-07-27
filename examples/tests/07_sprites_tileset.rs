//! Snapshot tests for the `07_sprites_tileset` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 07_sprites_tileset` runs.
//!
//! The headless snapshot drives synthetic movement through
//! [`Headless::push_event`] to actually walk the player onto a coin and then onto the
//! chest -- this is what proves collection, multi-cell hit testing, and the score
//! readout work, not just that the room renders.
//!
//! The three snapshots are also where the multi-cell fallback is checked end to end,
//! since they are the same scene rendered by both kinds of backend: the headless and
//! SVG snapshots must show the chest's eight ASCII glyphs, and the PNG must show one
//! chest sprite covering those cells instead. The PNG is the only one of the three
//! where `Example::configure_software`'s tileset registration (see
//! `support::png_snapshot`'s doc comment) renders real sprites at all.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/07_sprites_tileset.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod sprites_tileset;

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use sprites_tileset::SpritesTileset;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn headless_snapshot() {
    // Up 2 onto the coin at `COIN_OFFSETS[2]` (+1), back down 2, then right 4 onto the
    // chest's bottom-left cell (+5). Deliberately lands on a *covered* cell rather than
    // the span's anchor, so the last frame proves `Grid::span_owner` hit-tests the whole
    // 4x2 footprint and not just the one cell the sprite is keyed to.
    let events = [
        key(KeyCode::Up),
        key(KeyCode::Up),
        key(KeyCode::Down),
        key(KeyCode::Down),
        key(KeyCode::Right),
        key(KeyCode::Right),
        key(KeyCode::Right),
        key(KeyCode::Right),
    ];

    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = SpritesTileset::init(&mut term);

    let mut views = Vec::new();
    for (i, event) in events.iter().enumerate() {
        term.backend_mut().push_event(event.clone());
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i as u64,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        term.present().ok();
        views.push(term.backend().format_view());
    }
    insta::assert_snapshot!(views.join("\n--- frame ---\n"));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<SpritesTileset>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("07_sprites_tileset");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Score:");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("collect"),
        "SVG output missing expected header text"
    );
    support::write_snapshot_file("07_sprites_tileset.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}

// ── Tinting (retroglyph#537) ──────────────────────────────────────────────
//
// A tint is grid state rather than tile state, so it is read back through `Grid::tint` rather
// than off a `Tile`. These assert the values the pixel backends will apply; that they are
// applied correctly to pixels is `retroglyph-software`'s and `retroglyph-gl`'s own business, and
// tested there.

use retroglyph_core::{Pos, Tint};

/// Runs one frame with `events` already delivered, and hands back the terminal to inspect.
fn after(events: &[Event]) -> Terminal<Headless> {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = SpritesTileset::init(&mut term);
    for (i, event) in events.iter().enumerate() {
        term.backend_mut().push_event(event.clone());
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i as u64,
        };
        state.tick(&mut term, &frame);
    }
    // One more tick so the final frame reflects the post-movement state.
    let frame = Frame {
        delta: HEADLESS_FRAME_DELTA,
        frame: events.len() as u64,
    };
    state.tick(&mut term, &frame);
    term
}

/// The multiply factor of a room cell's tint, or `None` if it carries no multiply.
fn light_level(term: &Terminal<Headless>, pos: Pos) -> Option<u8> {
    match term.grid().tint(0, pos.x, pos.y) {
        Tint::Multiply { r, .. } => Some(r),
        _ => None,
    }
}

#[test]
fn room_cells_dim_with_distance_from_the_player() {
    let term = after(&[]);
    // The player starts at ROOM.left() + 9, ROOM.top() + 6 == (12, 10).
    let player = Pos::new(12, 10);

    let here = light_level(&term, player).expect("the player's own cell is lit");
    let near = light_level(&term, Pos::new(player.x + 2, player.y)).expect("a near cell is lit");
    let far = light_level(&term, Pos::new(player.x + 6, player.y)).expect("a far cell is lit");

    assert_eq!(here, 255, "the player's own cell is at full brightness");
    assert!(near < here, "{near} should be dimmer than {here}");
    assert!(far < near, "{far} should be dimmer than {near}");
}

#[test]
fn the_falloff_is_one_sprite_at_many_levels_not_many_sprites() {
    let term = after(&[]);
    let player = Pos::new(12, 10);
    // Every one of these cells draws the same floor glyph; only the tint differs. That is the
    // whole point of retroglyph#537: without it, each shade would need its own tile.
    let glyphs: Vec<char> = (1..=5)
        .map(|d| term.grid()[Pos::new(player.x + d, player.y)].glyph())
        .collect();
    assert!(
        glyphs.iter().all(|&g| g == '.'),
        "expected one repeated floor glyph, got {glyphs:?}"
    );

    let levels: Vec<u8> = (1..=5)
        .map(|d| light_level(&term, Pos::new(player.x + d, player.y)).expect("lit"))
        .collect();
    let mut sorted = levels.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert!(
        sorted.len() > 1,
        "the same sprite should render at several levels, got {levels:?}"
    );
    assert!(
        levels.windows(2).all(|w| w[0] >= w[1]),
        "levels should fall off monotonically, got {levels:?}"
    );
}

#[test]
fn coming_within_reach_of_the_chest_switches_its_tint_from_multiply_to_mix() {
    // Chest anchor is at ROOM.left() + 13, ROOM.top() + 5 == (16, 9), on the item layer.
    let chest = Pos::new(16, 9);

    let away = after(&[]);
    assert!(
        matches!(away.grid().tint(1, chest.x, chest.y), Tint::Multiply { .. }),
        "a chest out of reach just takes the room's falloff"
    );

    // Right 3 stops one cell short of the chest's footprint: within reach, not yet on it. A
    // fourth step would open the chest on the same tick and there would be nothing to highlight.
    let near = after(&[
        key(KeyCode::Right),
        key(KeyCode::Right),
        key(KeyCode::Right),
    ]);
    assert!(
        matches!(near.grid().tint(1, chest.x, chest.y), Tint::Mix { .. }),
        "a chest within reach is highlighted, which multiply cannot express"
    );
}

#[test]
fn a_tint_never_changes_what_a_cell_backend_draws() {
    // The headless and SVG snapshots are unchanged by this feature, which is the real proof;
    // this states the contract directly so it cannot regress quietly. A cell backend has no
    // sprite to recolour, so a tinted cell keeps its own glyph and style.
    let term = after(&[]);
    let player = Pos::new(12, 10);
    let lit = term.grid()[Pos::new(player.x + 1, player.y)];
    let dim = term.grid()[Pos::new(player.x + 6, player.y)];

    assert_eq!(
        lit.glyph(),
        dim.glyph(),
        "same glyph either side of the falloff"
    );
    assert_eq!(
        lit.style(),
        dim.style(),
        "same style either side of the falloff"
    );
    assert_ne!(
        term.grid().tint(0, player.x + 1, player.y),
        term.grid().tint(0, player.x + 6, player.y),
        "only the tint differs, and only a pixel backend reads it"
    );
}
