//! 07: Sprites (tileset)
//!
//! `retroglyph-software`'s `tilesets` feature: a PNG sprite sheet
//! (`assets/tileset.png`, 4 tiles of 8x16 pixels -- matching the embedded
//! default font's own cell size exactly, so no custom grid or scale is
//! needed) loaded via [`TilesetOptions`](retroglyph_software::tileset::TilesetOptions)
//! and registered on the software backend's [`SoftwareBackendBuilder`]. Each
//! tile is keyed to an ASCII glyph via [`Codepage::Custom`](retroglyph_software::tileset::Codepage::Custom)
//! -- `#` (wall), `.` (floor), `@` (player), `$` (coin) -- so the same glyph
//! that looks up a sprite on the software backend also *is* the correct
//! human-readable ASCII fallback everywhere else. That is the entire
//! fallback story for this example: terminal and headless backends were
//! never going to render a PNG, but they don't need a separate code path to
//! degrade gracefully, because the glyphs were chosen to already be the
//! right answer.
//!
//! Coins live on layer 1 (see `06_layers` for the layer/transparency model)
//! over a layer-0 floor, so a coin's round, partially-transparent corners
//! (see the sprite sheet's own pixels) are genuine alpha-blended
//! compositing against the floor tile drawn beneath them, not just an
//! opaque square dropped on top.
//!
//! ```sh
//! cargo run --example 07_sprites_tileset --features crossterm
//! cargo run --example 07_sprites_tileset --features software
//! cargo run --example 07_sprites_tileset  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Arrow keys move the player around the room and collect coins; `q` or
//! `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Pos, Rect, Terminal};
use retroglyph_examples::Example;

/// The room's interior (floor + player + coins), in grid cells. The wall
/// ring is drawn one cell outside this on every side.
const ROOM: Rect = Rect::new(3, 4, 20, 10);

/// Coin positions, relative to [`ROOM`]'s top-left corner.
const COIN_OFFSETS: [(u16, u16); 4] = [(2, 1), (16, 1), (9, 4), (4, 8)];

/// State for the sprites example: player position (in absolute grid cells)
/// and which coins remain.
pub struct SpritesTileset {
    player: Pos,
    coins: [bool; COIN_OFFSETS.len()],
    score: u32,
}

impl Default for SpritesTileset {
    fn default() -> Self {
        Self {
            // Deliberately not on any of COIN_OFFSETS' cells: collect_coin runs every tick
            // regardless of movement (see handle_events), so spawning on a coin would auto-
            // collect it before the player ever presses a key.
            player: Pos::new(ROOM.left() + 9, ROOM.top() + 6),
            coins: [true; COIN_OFFSETS.len()],
            score: 0,
        }
    }
}

impl SpritesTileset {
    /// Drains pending input: arrow keys move the player (clamped to the
    /// room's floor); `q`/`Escape` quits.
    ///
    /// Gated on [`KeyEvent::is_down`](retroglyph_core::event::KeyEvent::is_down): a backend that
    /// reports both press and release as separate [`Event::Key`]s (crossterm's kitty keyboard
    /// protocol, and the software backend's real key-up events) would otherwise run each match
    /// arm's action twice per physical key tap -- once on press, once on release -- since
    /// `key.code` alone doesn't say which edge fired.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if key.is_down() => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Left if self.player.x > ROOM.left() => self.player.x -= 1,
                    KeyCode::Right if self.player.x < ROOM.right() - 1 => self.player.x += 1,
                    KeyCode::Up if self.player.y > ROOM.top() => self.player.y -= 1,
                    KeyCode::Down if self.player.y < ROOM.bottom() - 1 => self.player.y += 1,
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        self.collect_coin();
        true
    }

    /// Marks any coin under the player as collected and credits its score.
    fn collect_coin(&mut self) {
        for (i, &(dx, dy)) in COIN_OFFSETS.iter().enumerate() {
            let coin_pos = Pos::new(ROOM.left() + dx, ROOM.top() + dy);
            if self.coins[i] && self.player == coin_pos {
                self.coins[i] = false;
                self.score += 1;
            }
        }
    }

    /// Draws this frame and presents it: layer 0 is the wall ring and
    /// floor; layer 1 is the remaining coins and the player.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(1, 1, "Arrows move, collect $ coins; q / Escape quits.");
        term.print(1, 2, &format!("Score: {}", self.score));

        term.layer(0);
        let wall_rect = Rect::new(
            ROOM.left() - 1,
            ROOM.top() - 1,
            ROOM.width() + 2,
            ROOM.height() + 2,
        );
        for y in wall_rect.top()..wall_rect.bottom() {
            for x in wall_rect.left()..wall_rect.right() {
                let on_wall_ring = y == wall_rect.top()
                    || y == wall_rect.bottom() - 1
                    || x == wall_rect.left()
                    || x == wall_rect.right() - 1;
                term.put(x, y, if on_wall_ring { '#' } else { '.' });
            }
        }

        term.layer(1);
        for (i, &(dx, dy)) in COIN_OFFSETS.iter().enumerate() {
            if self.coins[i] {
                term.put(ROOM.left() + dx, ROOM.top() + dy, '$');
            }
        }
        term.put(self.player.x, self.player.y, '@');

        term.present().ok();
    }
}

impl Example for SpritesTileset {
    const NAME: &'static str = "07_sprites_tileset";

    /// Registers `assets/tileset.png` on the software backend's builder --
    /// the one customization point [`Example`] threads through to
    /// [`retroglyph_examples::run_software`] so this example can still end
    /// in a plain [`retroglyph_examples::example_main!`] call like every
    /// other one, rather than hand-writing its own `main`.
    #[cfg(feature = "software")]
    fn configure_software(
        builder: retroglyph_software::SoftwareBackendBuilder,
    ) -> retroglyph_software::SoftwareBackendBuilder {
        let png =
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/tileset.png")).to_vec();
        let opts = retroglyph_software::tileset::TilesetOptions::from_bytes(png)
            .tile_size(8, 16)
            .columns(2)
            .codepage(retroglyph_software::tileset::Codepage::Custom(vec![
                '#', '.', '@', '$',
            ]))
            .build()
            .expect("tileset asset is a valid 16x32 PNG, evenly divisible into 8x16 tiles");
        builder.tileset(opts)
    }

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

retroglyph_examples::example_main!(SpritesTileset);
