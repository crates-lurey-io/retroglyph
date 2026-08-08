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
//! Movement is driven by [`KeyState`]: arrow-key presses/releases feed [`KeyState::apply_event`],
//! and each tick moves the player one cell in whichever direction is currently held
//! ([`KeyState::is_held`]), rather than stepping once per key event -- this is the workspace's
//! only end-to-end exercise of `KeyState` outside its own unit tests.
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
//!
//! This is also the [`Terminal::retain_layer`] example the method's own doc comment describes:
//! the map lives on [`Layer::World`] and only gets re-blitted from [`DungeonScroll::world`] when
//! the camera's [`Camera::visible_bounds`] actually changed since last frame; a stationary camera
//! (player bumping a wall, or waiting) marks `World` retained instead and skips the blit
//! entirely, while the player glyph and header move to [`Layer::Hud`] so they still redraw on
//! frames where the player moves without the camera moving (edge-clamped rooms 1 and 4). Input is
//! drained with [`Terminal::drain_events_into`] into a buffer reused every frame instead of
//! allocating a fresh one.

use retroglyph::app::Frame;
use retroglyph::backend::Backend;
use retroglyph::color::{AnsiColor, Color, Style};
use retroglyph::event::{Event, KeyCode, KeyLocation, KeyState};
use retroglyph::grid::{Grid, Pos, Rect, Size};
use retroglyph::surface::Layer;
use retroglyph::terminal::Terminal;
use retroglyph::tile::Tile;
use retroglyph::ui::camera::Camera;
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
    /// The `World` layer's visible bounds as of the last frame it was actually redrawn, so
    /// `draw` can tell whether the camera moved this frame and the map needs re-blitting, or
    /// whether it's safe to retain the layer instead. `None` on the very first frame forces the
    /// initial blit.
    last_visible: Option<Rect>,
    /// Reused every frame by [`Terminal::drain_events_into`] instead of allocating a fresh `Vec`
    /// per tick.
    event_buf: Vec<Event>,
    /// Tracks which of the arrow keys are currently held, for continuous held-key movement (see
    /// this module's top doc comment).
    keys: KeyState,
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
            last_visible: None,
            event_buf: Vec::new(),
            keys: KeyState::new(),
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

    /// Drains pending input into [`Self::event_buf`] (reused every frame, no per-tick
    /// allocation), feeding every event to [`Self::keys`] and quitting on `q`/`Escape`.
    /// `Event::Resize` is captured rather than acted on immediately, the same way `14_resize`
    /// does it: `term.resize()` needs `&mut term`, so the requested size is recorded here and
    /// applied once the loop ends. Movement itself happens afterward, in
    /// [`Self::move_from_held`].
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        term.drain_events_into(&mut self.event_buf);
        // Taken out (rather than iterated in place) so the loop body is free to call `&mut
        // self` methods like `try_move`; moved back below so the buffer's allocation is still
        // reused next frame instead of being dropped.
        let events = core::mem::take(&mut self.event_buf);
        let mut requested_size = None;
        for event in &events {
            match event {
                Event::Key(key) if key.is_down() => {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) {
                        return false;
                    }
                }
                Event::Close => return false,
                Event::Resize(width, height) => requested_size = Some((*width, *height)),
                _ => {}
            }
            // Feed every event to `keys`, not just key events: `Event::FocusLost` clears the
            // held set, so alt-tabbing away mid-move doesn't leave the player walking forever.
            self.keys.apply_event(event);
        }
        self.event_buf = events;
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
        self.move_from_held();
        true
    }

    /// Moves the player one cell toward whichever arrow key is currently held, checked in a
    /// fixed Up/Down/Left/Right priority order so at most one cell is crossed per tick even if
    /// more than one direction is held at once.
    fn move_from_held(&mut self) {
        const DIRECTIONS: [(KeyCode, i32, i32); 4] = [
            (KeyCode::Up, 0, -1),
            (KeyCode::Down, 0, 1),
            (KeyCode::Left, -1, 0),
            (KeyCode::Right, 1, 0),
        ];
        for (code, dx, dy) in DIRECTIONS {
            if self.keys.is_held(code, KeyLocation::Standard) {
                self.try_move(dx, dy);
                break;
            }
        }
    }

    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let visible = self.camera.visible_bounds();
        if self.last_visible == Some(visible) {
            // The camera hasn't moved since last frame, so the `World` layer's last-presented
            // content is still correct: skip the blit and let `present` copy it forward
            // untouched instead of paying for a redraw that would produce identical output.
            term.retain_layer(Layer::World);
        } else {
            let viewport = self.camera.viewport();
            term.grid_mut()
                .blit(0, &self.world, visible, viewport.left(), viewport.top());
            self.last_visible = Some(visible);
        }

        // Header and player live on `Hud`, not `World`: the player can move without the camera
        // moving (edge-clamped rooms 1 and 4), so this layer has to redraw every frame even on
        // frames where `World` above is retained.
        let mut surface = term.surface();
        let mut hud = surface.on_tier(Layer::Hud);
        hud.print(
            (1, 0),
            "Dungeon scroll -- arrows move, q/Escape quits",
            Style::default(),
        );

        let style = Style::new()
            .fg(Color::Ansi(AnsiColor::BrightCyan))
            .bg(Color::Default);
        self.camera.surface(&mut hud).put(self.player, '@', style);
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
