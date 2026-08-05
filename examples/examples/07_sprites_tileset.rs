//! 07: Sprites (tileset)
//!
//! `retroglyph-software`'s, `retroglyph-gl`'s, and `retroglyph-wgpu`'s `tilesets` feature: a PNG
//! sprite sheet (`assets/tileset.png`, 4 tiles of 8x16 pixels, matching the embedded default
//! font's own cell size exactly, so no custom grid or scale is needed) loaded via
//! [`TilesetOptions`](retroglyph_window::tileset::TilesetOptions) and registered on every
//! graphical backend's builder. Each tile is keyed to an ASCII glyph via
//! [`Codepage::Custom`](retroglyph_window::tileset::Codepage::Custom): `#` (wall), `.` (floor),
//! `@` (player), `$` (coin). The same glyph that looks up a sprite on a pixel backend is also
//! the correct human-readable ASCII fallback everywhere else, so terminal and headless backends
//! render the room correctly without any separate fallback code path.
//!
//! ## Multi-cell sprites
//!
//! The chest (`assets/chest.png`, one 32x32 sprite) is four cells wide and
//! two tall, drawn with a single
//! [`Surface::put_span`](retroglyph_core::surface::Surface::put_span) call that
//! carries its own text fallback:
//!
//! ```text
//! [==]        one 4x2 chest sprite on the software, GL, and wgpu backends,
//! |__|        these eight glyphs on a terminal
//! ```
//!
//! The anchor glyph `[` is what the sprite cache is keyed on; the other
//! seven are drawn by cell backends and suppressed by pixel backends, which
//! blit one sprite across the whole footprint instead. Same one call, no
//! capability check, no `cfg`. Opening the chest hit-tests with
//! [`Grid::span_owner`](retroglyph_core::grid::Grid::span_owner), so stepping on
//! any of the eight cells counts, with no rectangle arithmetic in the
//! example.
//!
//! ## Tinting one sprite into many
//!
//! The room has one floor sprite and one wall sprite, and every cell of the room draws the same
//! two. A [`Tint`](retroglyph_core::color::Tint) recolours them per cell, so the light radius around
//! the player is a falloff over that one pair of sprites rather than a sheet of pre-shaded
//! variants:
//!
//! - [`Tint::Multiply`](retroglyph_core::color::Tint::Multiply) scales the room's sprites toward black
//!   with distance from the player. Multiply preserves the artwork's own shading, which is what
//!   makes it right for lighting.
//! - [`Tint::Mix`](retroglyph_core::color::Tint::Mix) blends the chest toward white while the player
//!   stands on it, so it reads as "press to open". Multiply can only darken, so it cannot
//!   express a highlight at all.
//!
//! Both go through [`Surface::with_tint`](retroglyph_core::surface::Surface::with_tint), which applies to
//! sprites only. On a cell backend there is no sprite to recolour, so the room renders in its
//! own style exactly as it did before, with no `cfg` and no capability check (retroglyph#537).
//!
//! Coins live on layer 1 (see `06_layers` for the layer/transparency model)
//! over a layer-0 floor, so a coin's round, partially-transparent corners
//! (see the sprite sheet's own pixels) are genuine alpha-blended
//! compositing against the floor tile drawn beneath them, not just an
//! opaque square dropped on top. The chest's rounded lid is the same story
//! at four times the size.
//!
//! ```sh
//! cargo run --example 07_sprites_tileset --features crossterm
//! cargo run --example 07_sprites_tileset --features software
//! cargo run --example 07_sprites_tileset --features gl
//! cargo run --example 07_sprites_tileset --features wgpu
//! cargo run --example 07_sprites_tileset  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: arrow keys move the player around the room, collecting coins and opening the chest.
//! `q` or `Escape` quits, or close the window.

use retroglyph_core::backend::Backend;
use retroglyph_core::color::{Style, Tint};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::{Pos, Rect};
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;

/// The room's interior (floor + player + coins), in grid cells. The wall
/// ring is drawn one cell outside this on every side.
const ROOM: Rect = Rect::new(3, 4, 20, 10);

/// Coin positions, relative to [`ROOM`]'s top-left corner.
const COIN_OFFSETS: [(u16, u16); 4] = [(2, 1), (16, 1), (9, 4), (4, 8)];

/// Top-left cell of the chest, the anchor of its 4x2 span.
const CHEST: Pos = Pos::new(ROOM.left() + 13, ROOM.top() + 5);

/// The chest's artwork as text, one string per row of its footprint.
///
/// The first character is the anchor glyph the sprite cache is keyed on; the rest are what cell
/// backends print in the cells the sprite covers. Its shape is the span's shape: four cells wide,
/// two tall, matching `assets/chest.png`'s 32x32 pixels against this example's 8x16 cells.
const CHEST_ART: [&str; 2] = ["[==]", "|__|"];

/// The chest's 4x2 footprint grown by one cell on every side: the area from which the player can
/// reach it, which is what drives its highlight.
///
/// Reach rather than occupancy, because occupancy is unreachable as a *drawn* state: stepping
/// onto the chest opens it on the same tick, so a highlight keyed to standing on it would never
/// render. Highlighting the approach is also the more useful signal, since it says "you can open
/// this" while the player can still act on it.
///
/// Hit-testing which cell actually *owns* the chest still goes through
/// [`Grid::span_owner`](retroglyph_core::grid::Grid::span_owner); this is only about drawing.
const CHEST_REACH: Rect = Rect::new(CHEST.x - 1, CHEST.y - 1, 6, 4);

/// The layer the chest and the other collectables live on.
const ITEM_LAYER: u8 = 1;

/// What opening the chest is worth, against one point per coin.
const CHEST_SCORE: u32 = 5;

/// How far the player's light reaches, in cells. Past this the room sits at [`MIN_LIGHT`].
const LIGHT_RADIUS: u16 = 7;

/// How dark an unlit cell gets, as a multiply factor.
///
/// Deliberately nowhere near zero. A convincing torch radius would drop the far corners to
/// near-black, but this example's subject is sprites and multi-cell spans, and an unreadable
/// room would obscure the thing it is actually here to show. High enough that the wall ring and
/// the far coins stay legible, low enough that the falloff is unmistakable.
const MIN_LIGHT: u8 = 135;

/// State for the sprites example: player position (in absolute grid cells),
/// which coins remain, and whether the chest is still shut.
pub struct SpritesTileset {
    player: Pos,
    coins: [bool; COIN_OFFSETS.len()],
    chest_shut: bool,
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
            chest_shut: true,
            score: 0,
        }
    }
}

impl SpritesTileset {
    /// The light level at `pos`: a linear falloff from full brightness on the player's own cell
    /// to [`MIN_LIGHT`] at [`LIGHT_RADIUS`] and beyond.
    ///
    /// Chebyshev distance (the larger of the two axis deltas) rather than Euclidean, so the lit
    /// area is a square. On a grid whose cells are twice as tall as they are wide, a round
    /// falloff computed in cells renders as an ellipse anyway, and the square reads as
    /// deliberate where the ellipse reads as a bug.
    fn light_at(&self, pos: Pos) -> Tint {
        let dx = pos.x.abs_diff(self.player.x);
        let dy = pos.y.abs_diff(self.player.y);
        let dist = dx.max(dy).min(LIGHT_RADIUS);
        let range = u32::from(255 - MIN_LIGHT);
        let fade = range * u32::from(LIGHT_RADIUS - dist) / u32::from(LIGHT_RADIUS);
        #[allow(clippy::cast_possible_truncation)]
        let level = (u32::from(MIN_LIGHT) + fade) as u8;
        Tint::multiply(level, level, level)
    }

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

    /// Draws this frame (the driver presents): layer 0 is the wall ring and floor; layer 1 is
    /// the chest, the remaining coins, and the player.
    ///
    /// The chest is drawn before it is hit-tested because a span only exists to be queried while
    /// the frame that drew it is still being built. The header is printed last, for the same
    /// reason: it reports a score this frame may still change.
    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let style = Style::default();

        // Layer 0: wall ring and floor
        {
            let mut surface = term.surface();
            let mut layer0 = surface.on_layer(0);
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
                    let glyph = if on_wall_ring { '#' } else { '.' };
                    // One `put` per cell either way; the tint is what makes each cell's copy of
                    // the same sprite look different.
                    layer0
                        .with_tint(self.light_at(Pos::new(x, y)))
                        .put((x, y), glyph, style);
                }
            }
        }

        // Layer 1: chest, coins, and player
        // Draw chest and check for collision while surface is in scope
        {
            let mut surface = term.surface();
            let mut layer_items = surface.on_layer(ITEM_LAYER);
            if self.chest_shut {
                // Standing within reach of the chest highlights it. `Mix` toward white rather
                // than a brighter multiply, because multiplying an already-lit sprite can only
                // ever darken it.
                let within_reach = CHEST_REACH.contains(self.player.x, self.player.y);
                let tint = if within_reach {
                    Tint::mix(255, 255, 255, 110)
                } else {
                    self.light_at(CHEST)
                };
                // One call for all eight cells: the anchor glyph drives the sprite lookup on pixel
                // backends, and the whole block is readable ASCII art on cell backends. The tint
                // lands on the anchor, which is the cell the sprite is drawn from.
                layer_items
                    .with_tint(tint)
                    .put_span((CHEST.x, CHEST.y), &CHEST_ART, style)
                    .expect("the chest art fits inside the room");
            }
        }
        // Drop surface here so we can access term.grid()

        // Check if player is on chest and update state
        if self.chest_shut
            && term
                .grid()
                .span_owner(ITEM_LAYER, self.player.x, self.player.y)
                == Some(CHEST)
        {
            self.chest_shut = false;
            self.score += CHEST_SCORE;
            term.grid_mut().clear_span(ITEM_LAYER, CHEST.x, CHEST.y);
        }

        // Draw coins and player
        {
            let mut surface = term.surface();
            let mut layer_items = surface.on_layer(ITEM_LAYER);
            for (i, &(dx, dy)) in COIN_OFFSETS.iter().enumerate() {
                if self.coins[i] {
                    let pos = Pos::new(ROOM.left() + dx, ROOM.top() + dy);
                    // Coins take the same falloff as the floor beneath them, so a distant one
                    // dims with its surroundings instead of floating in the dark.
                    layer_items
                        .with_tint(self.light_at(pos))
                        .put(pos, '$', style);
                }
            }
            // The player is always on their own cell, so their light is always full.
            layer_items.put((self.player.x, self.player.y), '@', style);
        }

        // Draw text (on main layer 0 implicitly)
        {
            let mut surface = term.surface();
            // No trailing full stops: `.` is keyed to the floor sprite, so a pixel backend
            // would render one as a floor tile mid-sentence.
            surface.print(
                (1, 1),
                "Arrows move; collect $ coins and open the chest",
                style,
            );
            surface.print(
                (1, 2),
                &format!("Score: {}   q / Escape quits", self.score),
                style,
            );
        }
    }
}

/// The example's two sprite sheets, as backend-agnostic options.
///
/// Shared verbatim by [`SpritesTileset::configure_software`],
/// [`SpritesTileset::configure_gl`], and [`SpritesTileset::configure_wgpu`]: the three builders
/// are different types, but the tilesets they register are the same ones, so they are described
/// once here.
///
/// The first sheet is the one-cell room tiles; the second is the chest, a single 32x32 sprite
/// that covers the 4x2 cells [`CHEST_ART`] spans. Nothing in the tileset says how many cells a
/// sprite occupies -- that is declared per draw call, by
/// [`Surface::put_span`](retroglyph_core::surface::Surface::put_span).
#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
fn tilesets() -> [retroglyph_window::tileset::TilesetOptions; 2] {
    use retroglyph_window::tileset::{Codepage, TilesetOptions};

    let room = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/tileset.png")).to_vec();
    let chest = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/chest.png")).to_vec();
    [
        TilesetOptions::builder(room)
            .tile_size(8, 16)
            .columns(2)
            .codepage(Codepage::Custom(vec!['#', '.', '@', '$']))
            .build()
            .expect("room asset is a valid 16x32 PNG, evenly divisible into 8x16 tiles"),
        TilesetOptions::builder(chest)
            .tile_size(32, 32)
            .columns(1)
            .codepage(Codepage::Custom(vec!['[']))
            .build()
            .expect("chest asset is a valid 32x32 PNG holding one tile"),
    ]
}

impl Example for SpritesTileset {
    const NAME: &'static str = "07_sprites_tileset";

    /// Registers the sprite sheet on the software backend's builder -- one of the two
    /// customization points [`Example`] threads through, so this example can still end in a plain
    /// [`retroglyph_examples::example_main!`] call like every other one, rather than hand-writing
    /// its own `main`.
    #[cfg(feature = "software")]
    fn configure_software(
        builder: retroglyph_software::SoftwareBackendBuilder,
    ) -> retroglyph_software::SoftwareBackendBuilder {
        tilesets().into_iter().fold(
            builder,
            retroglyph_software::SoftwareBackendBuilder::tileset,
        )
    }

    /// Registers the same sheets on the GL backend, so the WebGL2 build renders sprites rather
    /// than falling back to bitmap glyphs.
    #[cfg(feature = "gl")]
    fn configure_gl(builder: retroglyph_gl::GlBackendBuilder) -> retroglyph_gl::GlBackendBuilder {
        tilesets()
            .into_iter()
            .fold(builder, retroglyph_gl::GlBackendBuilder::tileset)
    }

    /// Registers the same sheets on the wgpu backend, so the native Vulkan/Metal/D3D12 build
    /// renders sprites rather than falling back to bitmap glyphs.
    #[cfg(feature = "wgpu")]
    fn configure_wgpu(
        builder: retroglyph_wgpu::WgpuBackendBuilder,
    ) -> retroglyph_wgpu::WgpuBackendBuilder {
        tilesets()
            .into_iter()
            .fold(builder, retroglyph_wgpu::WgpuBackendBuilder::tileset)
    }

    fn tick<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        _frame: &retroglyph_core::app::Frame,
    ) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(SpritesTileset);
