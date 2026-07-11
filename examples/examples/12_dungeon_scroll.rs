//! 12: Dungeon scroll
//!
//! [`Camera`], the first example to exercise it: a scrolling viewport onto a world larger than
//! the 50x25 screen. `Camera` is pure geometry (world/screen coordinate conversion, edge-clamped
//! following), and this is deliberately the whole point of this example -- not a roguelike with
//! field-of-view or pathfinding. Neither of those is a rendering capability, and neither exists
//! in any workspace crate today (see ADR 019's own open-gate note on this); a scrolling dungeon
//! crawl doesn't need either one to prove something real about the library, so this example
//! skips both rather than picking a dependency for algorithms `retroglyph` was never about.
//!
//! The world is four hand-placed rooms joined by straight corridors (a fixed layout, like
//! `11_sokoban`'s level -- no RNG, so every run and every snapshot is identical). Every step,
//! [`Camera::center_on`] re-centers on the player (clamped at the world edges, per its own doc
//! comment), [`Grid::blit`] copies exactly [`Camera::visible_bounds`] into the terminal at the
//! viewport's origin, and [`Camera::world_to_screen`] places the player glyph -- the same three
//! methods a real scrolling map would use, exercised end to end.
//!
//! ```sh
//! cargo run --example 12_dungeon_scroll --features crossterm
//! cargo run --example 12_dungeon_scroll --features software
//! cargo run --example 12_dungeon_scroll  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Arrow keys move (blocked by walls); `q`/`Escape` quits.

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

/// Builds the fixed dungeon: every cell starts as wall, then each room and corridor carves
/// floor over it.
fn build_world() -> Grid {
    let mut world = Grid::new(WORLD_W, WORLD_H);
    for y in 0..WORLD_H {
        for x in 0..WORLD_W {
            world.put_tile(0, x, y, Tile::new('#', wall_style()));
        }
    }
    for &(x, y, w, h) in &ROOMS {
        for cy in y..y + h {
            for cx in x..x + w {
                world.put_tile(0, cx, cy, Tile::new('.', floor_style()));
            }
        }
    }
    for &((fx, fy), (tx, ty)) in &CORRIDORS {
        for x in fx.min(tx)..=fx.max(tx) {
            world.put_tile(0, x, fy, Tile::new('.', floor_style()));
        }
        for y in fy.min(ty)..=fy.max(ty) {
            world.put_tile(0, tx, y, Tile::new('.', floor_style()));
        }
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
        let mut camera = Camera::new(
            Rect::new(0, 1, 50, 24),
            Size {
                width: WORLD_W,
                height: WORLD_H,
            },
        );
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
        pos.x < WORLD_W && pos.y < WORLD_H && self.world.get(pos.x, pos.y).glyph() != '#'
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

    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
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
                _ => {}
            }
        }
        true
    }

    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(1, 0, "Dungeon scroll -- arrows move, q/Escape quits");

        let viewport = self.camera.viewport();
        term.grid_mut().blit(
            0,
            &self.world,
            self.camera.visible_bounds(),
            viewport.left(),
            viewport.top(),
        );

        if let Some(screen) = self.camera.world_to_screen(self.player) {
            term.reset_style()
                .fg(Color::Ansi(AnsiColor::BrightCyan))
                .bg(Color::Default);
            term.put(screen.x, screen.y, '@');
            term.reset_style();
        }

        term.present().ok();
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
