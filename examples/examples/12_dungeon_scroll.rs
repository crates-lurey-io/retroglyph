//! 12: Dungeon scroll
//!
//! [`Camera`]: a scrolling viewport onto a world larger than the 50x25 screen. `Camera` is pure
//! geometry (world/screen coordinate conversion, edge-clamped following), and that geometry is
//! the whole point of this example, not field-of-view or pathfinding. Neither of those is a
//! rendering capability, neither exists in any workspace crate today, and a scrolling dungeon
//! crawl doesn't need either one to show something real about the library, so this example
//! skips both.
//!
//! The world is four hand-placed rooms joined by straight corridors, a fixed layout like
//! `11_sokoban`'s level: no RNG, so every run and every snapshot is identical. Every step,
//! [`Camera::center_on`] re-centers on the player (clamped at the world edges, per its own doc
//! comment), [`Grid::blit`] copies exactly [`Camera::visible_bounds`] into the terminal at the
//! viewport's origin, and the player glyph is drawn through [`Camera::surface`]: a surface
//! clipped and translated into the camera's world space, so the player is placed with its world
//! position directly and anything that ever scrolled off-viewport is dropped by the surface's
//! own bounds check rather than a manual [`Camera::world_to_screen`] guard.
//!
//! This is also the one example with a world (90x36) fixed at build time meeting a terminal
//! whose size the app doesn't control: every other scrolling demo either matches its terminal to
//! its world or never resizes at all. [`Event::Resize`] is handled the same way `14_resize` does
//! (captured during the drain, applied once the loop's borrow ends), and the new area is handed
//! to [`Camera::set_viewport_fitted`] rather than [`Camera::set_viewport`]: grow the terminal
//! past the world's edge on either axis and the map letterboxes and centers instead of pinning
//! to the top-left with a dead margin on the right and bottom.
//!
//! ```sh
//! cargo run --example 12_dungeon_scroll --features crossterm
//! cargo run --example 12_dungeon_scroll --features software
//! cargo run --example 12_dungeon_scroll  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: arrow keys move, blocked by walls. `q` or `Escape` quits. Resize the terminal
//! (crossterm) or window (software) to see the world letterbox once it's smaller than the
//! resized viewport.
//!
//! Room 1 also carries four decorative floor tiles ([`decorations`]) that exercise the extended
//! ASCII glyphs the embedded Unscii 16 font (`default-font` feature) adds on top of plain ASCII:
//! a hut (U+2302 HOUSE), a torch (U+263C WHITE SUN WITH RAYS), a cracked rune (U+2310 REVERSED
//! NOT SIGN), and loose rubble (U+2219 BULLET OPERATOR). All four sit in room 1, which is on
//! screen from the very first frame, so the software backend's `png_snapshot` test alone proves
//! they rasterize correctly. No walk to a later room is required.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{
    AnsiColor, Backend, Camera, Color, Frame, Grid, Pos, Rect, Size, Style, Terminal, Tile,
};
use retroglyph_examples::Example;

/// World dimensions: comfortably larger than the 50x24 viewport in both axes, so the camera
/// actually has room to scroll and clamp at every edge as the player crosses the map.
const WORLD_W: u16 = 90;
const WORLD_H: u16 = 36;

/// Rooms as `(x, y, w, h)` in world cells, connected corridor-to-corridor below. Room 1's
/// center is the player's start; room 4's center is the far end of the walk.
const ROOMS: [(u16, u16, u16, u16); 4] =
    [(2, 2, 9, 5), (36, 2, 9, 5), (36, 28, 9, 5), (76, 28, 9, 5)];

/// Straight corridors as `(from, to)` room-center pairs: horizontal (shared `y`) or vertical
/// (shared `x`) only, matching the room centers above -- no L-shaped pathfinding-adjacent logic,
/// just carving a straight line of floor between two points that already share an axis.
const CORRIDORS: [((u16, u16), (u16, u16)); 3] =
    [((6, 4), (40, 4)), ((40, 4), (40, 30)), ((40, 30), (80, 30))];

/// The player's start: room 1's center.
const START: (u16, u16) = (6, 4);

fn wall_style() -> Style {
    Style::new().fg(Color::Ansi(AnsiColor::White))
}

fn floor_style() -> Style {
    Style::new().fg(Color::Ansi(AnsiColor::BrightBlack))
}

/// Four hand-placed floor decorations inside room 1 (`(x, y, glyph, style)`), each one of the
/// extended-ASCII glyphs the embedded Unscii 16 font adds beyond plain ASCII -- see this module's
/// top doc comment for what each one represents and why room 1 specifically.
fn decorations() -> [(u16, u16, char, Style); 4] {
    [
        (3, 3, '⌂', Style::new().fg(Color::Ansi(AnsiColor::White))), // hut
        (
            9,
            3,
            '☼',
            Style::new().fg(Color::Ansi(AnsiColor::BrightYellow)),
        ), // torch
        (
            3,
            5,
            '⌐',
            Style::new().fg(Color::Ansi(AnsiColor::BrightMagenta)),
        ), // cracked rune
        (9, 5, '∙', Style::new().fg(Color::Ansi(AnsiColor::Green))), // rubble
    ]
}

/// Builds the fixed dungeon: every cell starts as wall, then each room and corridor carves
/// floor over it.
fn build_world() -> Grid {
    let mut world = Grid::new(WORLD_W, WORLD_H);
    for y in 0..WORLD_H {
        for x in 0..WORLD_W {
            world.put_tile(0, (x, y), Tile::new('#', wall_style()));
        }
    }
    for &(x, y, w, h) in &ROOMS {
        for cy in y..y + h {
            for cx in x..x + w {
                world.put_tile(0, (cx, cy), Tile::new('.', floor_style()));
            }
        }
    }
    for &((fx, fy), (tx, ty)) in &CORRIDORS {
        for x in fx.min(tx)..=fx.max(tx) {
            world.put_tile(0, (x, fy), Tile::new('.', floor_style()));
        }
        for y in fy.min(ty)..=fy.max(ty) {
            world.put_tile(0, (tx, y), Tile::new('.', floor_style()));
        }
    }
    for (x, y, glyph, style) in decorations() {
        world.put_tile(0, (x, y), Tile::new(glyph, style));
    }
    world
}

/// State for the dungeon-scroll example.
pub struct DungeonScroll {
    world: Grid,
    camera: Camera,
    player: Pos,
}

impl Default for DungeonScroll {
    fn default() -> Self {
        let player = Pos::new(START.0, START.1);
        let mut camera = Camera::new(Rect::new(0, 1, 50, 24), Size::new(WORLD_W, WORLD_H));
        camera.center_on(player);
        Self {
            world: build_world(),
            camera,
            player,
        }
    }
}

impl DungeonScroll {
    fn is_floor(&self, pos: Pos) -> bool {
        pos.x < WORLD_W
            && pos.y < WORLD_H
            && self
                .world
                .tile(0, pos)
                .is_some_and(|tile| tile.glyph() != '#')
    }

    fn try_move(&mut self, dx: i32, dy: i32) {
        let (nx, ny) = (i32::from(self.player.x) + dx, i32::from(self.player.y) + dy);
        let (Ok(nx), Ok(ny)) = (u16::try_from(nx), u16::try_from(ny)) else {
            return; // negative: off the top/left edge
        };
        let target = Pos::new(nx, ny);
        if self.is_floor(target) {
            self.player = target;
            self.camera.center_on(self.player);
        }
    }

    /// Drains pending input, moving the player on arrow keys. `Event::Resize` is captured
    /// rather than acted on immediately, the same way `14_resize` does it: it arrives mixed in
    /// with other events in the same drain, and `term.resize()` needs `&mut term` while
    /// [`Terminal::drain_events`]'s iterator still holds one, so the requested size is recorded
    /// here and applied once the loop (and the borrow) ends.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        let mut requested_size = None;
        for event in term.drain_events() {
            match event {
                Event::Key(key) if key.is_down() => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Up => self.try_move(0, -1),
                    KeyCode::Down => self.try_move(0, 1),
                    KeyCode::Left => self.try_move(-1, 0),
                    KeyCode::Right => self.try_move(1, 0),
                    _ => {}
                },
                Event::Close => return false,
                Event::Resize(width, height) => requested_size = Some((width, height)),
                _ => {}
            }
        }
        if let Some((width, height)) = requested_size {
            term.resize(width, height);
            // Row 0 is reserved for the header text (see `draw`), so the camera's viewport is
            // everything below it. `set_viewport_fitted`, not `set_viewport`: this is a fixed
            // 90x36 world meeting a terminal size the app doesn't control, exactly the
            // letterbox-and-center case `set_viewport_fitted` exists for.
            let area = term.area();
            let viewport = Rect::new(
                area.left(),
                area.top() + 1,
                area.width(),
                area.height().saturating_sub(1),
            );
            self.camera.set_viewport_fitted(viewport);
        }
        true
    }

    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.surface().print(
            (1, 0),
            "Dungeon scroll -- arrows move, q/Escape quits",
            Style::default(),
        );

        let viewport = self.camera.viewport();
        term.grid_mut().blit(
            0,
            &self.world,
            self.camera.visible_bounds(),
            viewport.left(),
            viewport.top(),
        );

        let style = Style::new()
            .fg(Color::Ansi(AnsiColor::BrightCyan))
            .bg(Color::Default);
        let mut root = term.surface();
        self.camera.surface(&mut root).put(self.player, '@', style);
    }
}

impl Example for DungeonScroll {
    const NAME: &'static str = "12_dungeon_scroll";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(DungeonScroll);
