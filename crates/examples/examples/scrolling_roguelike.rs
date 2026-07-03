//! Scrolling roguelike demo.
//!
//! A world larger than the screen, viewed through a [`Camera`] that follows the
//! player. Exercises the features that landed with ADR 015 and after:
//!
//! - [`Grid::from_charmap`] to build the terrain from a generated map
//! - [`Camera`] world/screen conversion with edge clamping
//! - multi-layer rendering (terrain / entities / UI) — composited on every
//!   backend, including crossterm
//! - symmetric shadowcasting field of view with persistent fog of war
//! - procedural rooms-and-corridors generation and BFS monster pathfinding
//!
//! # Controls
//!
//! - Arrow keys / WASD — move (bump a monster to attack)
//! - R — new dungeon on the death screen
//! - Q / Escape — quit
//!
//! # Run
//!
//! ```sh
//! cargo run --example scrolling_roguelike --features crossterm
//! cargo run --example scrolling_roguelike --features software-default-font
//! ```

use std::collections::VecDeque;

use retroglyph_core::{Backend, Camera, Color, Grid, Pos, Rect, Size, Style, Terminal, Tile};
use retroglyph_examples::util::action::{Action, next_action};
use retroglyph_examples::util::fov;
use retroglyph_examples::util::lcg::Lcg;

// ── World ───────────────────────────────────────────────────────────────────

const WORLD_W: u16 = 80;
const WORLD_H: u16 = 48;
const FOV_RADIUS: u16 = 9;
const ROOM_ATTEMPTS: u32 = 24;
const MAX_MONSTERS: usize = 14;
const PLAYER_MAX_HP: i32 = 20;
const MONSTER_HP: i32 = 3;

// ── Colors ────────────────────────────────────────────────────────────────────

const COL_BG: Color = Color::Rgb {
    r: 12,
    g: 12,
    b: 18,
};
const COL_WALL: Color = Color::Rgb {
    r: 120,
    g: 110,
    b: 130,
};
const COL_FLOOR: Color = Color::Rgb {
    r: 70,
    g: 66,
    b: 82,
};
const COL_PLAYER: Color = Color::Rgb {
    r: 220,
    g: 220,
    b: 255,
};
const COL_MONSTER: Color = Color::Rgb {
    r: 210,
    g: 90,
    b: 90,
};
const COL_UI_BG: Color = Color::Rgb {
    r: 24,
    g: 24,
    b: 38,
};
const COL_UI_FG: Color = Color::Rgb {
    r: 180,
    g: 180,
    b: 200,
};

// ── State ─────────────────────────────────────────────────────────────────────

struct Monster {
    pos: Pos,
    hp: i32,
    alive: bool,
}

struct GameState {
    /// Wall mask, indexed `y * WORLD_W + x`.
    walls: Vec<bool>,
    /// Styled render source built once with [`Grid::from_charmap`].
    terrain: Grid,
    player: Pos,
    monsters: Vec<Monster>,
    visible: Vec<bool>,
    seen: Vec<bool>,
    hp: i32,
    turn: u32,
    dead: bool,
    camera: Camera,
}

impl GameState {
    fn new<B: Backend>(term: &Terminal<B>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut rng = Lcg::from_time();
        #[cfg(target_arch = "wasm32")]
        let mut rng = Lcg::new(0x51ED_2A17);
        Self::with_rng(term, &mut rng)
    }

    fn with_rng<B: Backend>(term: &Terminal<B>, rng: &mut Lcg) -> Self {
        let (walls, rooms) = generate(rng);
        let terrain = build_terrain(&walls);

        let player = room_center(rooms[0]);
        let monsters = rooms
            .iter()
            .skip(1)
            .take(MAX_MONSTERS)
            .map(|&r| Monster {
                pos: room_center(r),
                hp: MONSTER_HP,
                alive: true,
            })
            .collect();

        let cell_count = usize::from(WORLD_W) * usize::from(WORLD_H);
        let mut state = Self {
            walls,
            terrain,
            player,
            monsters,
            visible: vec![false; cell_count],
            seen: vec![false; cell_count],
            hp: PLAYER_MAX_HP,
            turn: 0,
            dead: false,
            camera: Camera::new(viewport(term.size()), Size::from((WORLD_W, WORLD_H))),
        };
        state.recompute_fov();
        state
    }

    fn is_wall(&self, x: u16, y: u16) -> bool {
        self.walls[usize::from(y) * usize::from(WORLD_W) + usize::from(x)]
    }

    fn recompute_fov(&mut self) {
        for v in &mut self.visible {
            *v = false;
        }
        let walls = &self.walls;
        let visible = &mut self.visible;
        let seen = &mut self.seen;
        fov::compute(
            self.player,
            FOV_RADIUS,
            |x, y| !in_bounds(x, y) || walls[idx(x, y)],
            |x, y| {
                if in_bounds(x, y) {
                    visible[idx(x, y)] = true;
                    seen[idx(x, y)] = true;
                }
            },
        );
    }

    fn player_move(&mut self, dx: i16, dy: i16) {
        if self.dead {
            return;
        }
        let nx = self.player.x.wrapping_add_signed(dx);
        let ny = self.player.y.wrapping_add_signed(dy);
        if nx >= WORLD_W || ny >= WORLD_H || self.is_wall(nx, ny) {
            return;
        }
        let dest = Pos::new(nx, ny);
        if let Some(m) = self.monsters.iter_mut().find(|m| m.alive && m.pos == dest) {
            m.hp -= 2;
            if m.hp <= 0 {
                m.alive = false;
            }
        } else {
            self.player = dest;
        }
        self.monster_turn();
        self.turn = self.turn.wrapping_add(1);
        self.recompute_fov();
        if self.hp <= 0 {
            self.dead = true;
        }
    }

    fn monster_turn(&mut self) {
        for i in 0..self.monsters.len() {
            if !self.monsters[i].alive {
                continue;
            }
            let mpos = self.monsters[i].pos;
            if !self.visible[idx_pos(mpos)] {
                continue; // asleep outside the player's view
            }
            let blocked: Vec<Pos> = self
                .monsters
                .iter()
                .enumerate()
                .filter(|(j, m)| *j != i && m.alive)
                .map(|(_, m)| m.pos)
                .collect();
            if let Some(step) = step_toward(&self.walls, mpos, self.player, &blocked) {
                if step == self.player {
                    self.hp -= 1;
                } else {
                    self.monsters[i].pos = step;
                }
            }
        }
    }

    fn alive_monsters(&self) -> usize {
        self.monsters.iter().filter(|m| m.alive).count()
    }
}

// ── Coordinate helpers ──────────────────────────────────────────────────────

#[allow(clippy::cast_sign_loss)] // callers guarantee non-negative, in-bounds coords
fn idx(x: i32, y: i32) -> usize {
    y as usize * usize::from(WORLD_W) + x as usize
}

fn idx_pos(p: Pos) -> usize {
    usize::from(p.y) * usize::from(WORLD_W) + usize::from(p.x)
}

fn in_bounds(x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && x < i32::from(WORLD_W) && y < i32::from(WORLD_H)
}

/// The map viewport: the full screen minus a top status bar and bottom hint bar.
fn viewport(size: Size) -> Rect {
    let h = size.height.saturating_sub(2).max(1);
    Rect::new(0, 1, size.width.into(), h.into())
}

const fn room_center(r: Rect) -> Pos {
    Pos::new(r.left() + r.width() / 2, r.top() + r.height() / 2)
}

// ── Map generation ──────────────────────────────────────────────────────────

fn generate(rng: &mut Lcg) -> (Vec<bool>, Vec<Rect>) {
    let mut walls = vec![true; usize::from(WORLD_W) * usize::from(WORLD_H)];
    let mut rooms: Vec<Rect> = Vec::new();

    for _ in 0..ROOM_ATTEMPTS {
        let rw = 4 + rng_range(rng, 8);
        let rh = 3 + rng_range(rng, 5);
        let rx = 1 + rng_range(rng, WORLD_W - rw - 2);
        let ry = 1 + rng_range(rng, WORLD_H - rh - 2);
        let room = Rect::new(rx, ry, rw.into(), rh.into());

        for y in room.top()..room.bottom() {
            for x in room.left()..room.right() {
                carve(&mut walls, x, y);
            }
        }
        if let Some(&prev) = rooms.last() {
            let a = room_center(prev);
            let b = room_center(room);
            // L-shaped corridor connecting this room to the previous one.
            h_tunnel(&mut walls, a.x, b.x, a.y);
            v_tunnel(&mut walls, a.y, b.y, b.x);
        }
        rooms.push(room);
    }
    (walls, rooms)
}

fn rng_range(rng: &mut Lcg, max: u16) -> u16 {
    if max == 0 {
        0
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            (rng.next() % u64::from(max)) as u16
        }
    }
}

fn carve(walls: &mut [bool], x: u16, y: u16) {
    if x < WORLD_W && y < WORLD_H {
        walls[usize::from(y) * usize::from(WORLD_W) + usize::from(x)] = false;
    }
}

fn h_tunnel(walls: &mut [bool], x1: u16, x2: u16, y: u16) {
    for x in x1.min(x2)..=x1.max(x2) {
        carve(walls, x, y);
    }
}

fn v_tunnel(walls: &mut [bool], y1: u16, y2: u16, x: u16) {
    for y in y1.min(y2)..=y1.max(y2) {
        carve(walls, x, y);
    }
}

/// Build the styled terrain grid from the wall mask via [`Grid::from_charmap`].
fn build_terrain(walls: &[bool]) -> Grid {
    let mut map = String::with_capacity((usize::from(WORLD_W) + 1) * usize::from(WORLD_H));
    for y in 0..WORLD_H {
        for x in 0..WORLD_W {
            map.push(
                if walls[usize::from(y) * usize::from(WORLD_W) + usize::from(x)] {
                    '#'
                } else {
                    '.'
                },
            );
        }
        map.push('\n');
    }
    Grid::from_charmap(&map, |c| match c {
        '#' => Tile::new('#', Style::new().fg(COL_WALL).bg(COL_BG)),
        _ => Tile::new('·', Style::new().fg(COL_FLOOR).bg(COL_BG)),
    })
}

// ── Pathfinding (BFS) ─────────────────────────────────────────────────────────

/// First step of a shortest path from `from` to `to`, or `None` if blocked.
fn step_toward(walls: &[bool], from: Pos, to: Pos, blocked: &[Pos]) -> Option<Pos> {
    if from == to {
        return None;
    }
    let passable = |p: Pos| -> bool {
        if p.x >= WORLD_W || p.y >= WORLD_H {
            return false;
        }
        if walls[idx_pos(p)] {
            return false;
        }
        p == to || !blocked.contains(&p)
    };

    let mut visited = vec![false; usize::from(WORLD_W) * usize::from(WORLD_H)];
    let mut queue: VecDeque<(Pos, Pos)> = VecDeque::new();
    visited[idx_pos(from)] = true;
    for n in neighbors(from) {
        if passable(n) && !visited[idx_pos(n)] {
            visited[idx_pos(n)] = true;
            queue.push_back((n, n));
        }
    }
    while let Some((cur, first)) = queue.pop_front() {
        if cur == to {
            return Some(first);
        }
        for n in neighbors(cur) {
            if passable(n) && !visited[idx_pos(n)] {
                visited[idx_pos(n)] = true;
                queue.push_back((n, first));
            }
        }
    }
    None
}

fn neighbors(p: Pos) -> impl Iterator<Item = Pos> {
    [(0i16, -1i16), (0, 1), (-1, 0), (1, 0)]
        .into_iter()
        .map(move |(dx, dy)| Pos::new(p.x.wrapping_add_signed(dx), p.y.wrapping_add_signed(dy)))
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Dim a foreground color for "seen but not currently visible" fog.
const fn fog(color: Color) -> Color {
    match color {
        Color::Rgb { r, g, b } => Color::Rgb {
            r: r / 3,
            g: g / 3,
            b: b / 3,
        },
        other => other,
    }
}

fn draw<B: Backend>(term: &mut Terminal<B>, state: &GameState) {
    let size = term.size();

    // Terrain (layer 0) through the camera, with fog of war.
    term.layer(0);
    for (world, screen) in state.camera.cells() {
        let i = idx_pos(world);
        if state.visible[i] {
            let base = state.terrain.get(world.x, world.y);
            term.put_styled(screen.x, screen.y, base.glyph(), base.style());
        } else if state.seen[i] {
            let base = state.terrain.get(world.x, world.y);
            let style = Style::new().fg(fog(base.style().foreground())).bg(COL_BG);
            term.put_styled(screen.x, screen.y, base.glyph(), style);
        } else {
            term.put_styled(screen.x, screen.y, ' ', Style::new().bg(COL_BG));
        }
    }

    // Entities (layer 1): monsters then the player, only where visible.
    term.layer(1);
    for m in &state.monsters {
        if m.alive
            && state.visible[idx_pos(m.pos)]
            && let Some(s) = state.camera.world_to_screen(m.pos)
        {
            term.put_styled(s.x, s.y, 'g', Style::new().fg(COL_MONSTER).bg(COL_BG));
        }
    }
    if let Some(s) = state.camera.world_to_screen(state.player) {
        term.put_styled(s.x, s.y, '@', Style::new().fg(COL_PLAYER).bg(COL_BG));
    }

    // UI (layer 2): top status bar and bottom hint bar.
    term.layer(2);
    let top = format!(
        " HP {:>2}/{}   Turn {:<4}   Pos {:>2},{:<2}   Foes {}",
        state.hp,
        PLAYER_MAX_HP,
        state.turn,
        state.player.x,
        state.player.y,
        state.alive_monsters(),
    );
    bar(term, 0, size.width, &top);
    let hint = if state.dead {
        " You died.   [R] new dungeon   [Q] quit "
    } else {
        " Move: WASD / arrows   Bump to attack   [Q] quit "
    };
    bar(term, size.height.saturating_sub(1), size.width, hint);

    if state.dead {
        let msg = "  YOU DIED  ";
        #[allow(clippy::cast_possible_truncation)]
        let x = size.width.saturating_sub(msg.len() as u16) / 2;
        let y = size.height / 2;
        put_str(
            term,
            x,
            y,
            msg,
            Style::new().fg(Color::BRIGHT_WHITE).bg(COL_MONSTER),
            size.width,
        );
    }
}

/// Fill row `y` with `text` on a status-bar background across `width`.
fn bar<B: Backend>(term: &mut Terminal<B>, y: u16, width: u16, text: &str) {
    for x in 0..width {
        term.put_styled(x, y, ' ', Style::new().bg(COL_UI_BG));
    }
    put_str(
        term,
        0,
        y,
        text,
        Style::new().fg(COL_UI_FG).bg(COL_UI_BG),
        width,
    );
}

/// Draw `text` one ASCII cell per column starting at `(x, y)`, clipped at
/// `max_x`. Avoids the wrapping behaviour of `print` for fixed-width bars.
fn put_str<B: Backend>(
    term: &mut Terminal<B>,
    x: u16,
    y: u16,
    text: &str,
    style: Style,
    max_x: u16,
) {
    for (cx, ch) in (x..max_x).zip(text.chars()) {
        term.put_styled(cx, y, ch, style);
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut GameState) -> bool {
    // Keep the viewport in sync with the terminal, then follow the player.
    state.camera.set_viewport(viewport(term.size()));
    state.camera.center_on(state.player);

    draw(term, state);
    term.present().expect("present failed");

    match next_action(term) {
        Action::MoveUp => state.player_move(0, -1),
        Action::MoveDown => state.player_move(0, 1),
        Action::MoveLeft => state.player_move(-1, 0),
        Action::MoveRight => state.player_move(1, 0),
        Action::Quit => return false,
        Action::Confirm | Action::Interact if state.dead => {
            #[cfg(not(target_arch = "wasm32"))]
            let mut rng = Lcg::from_time();
            #[cfg(target_arch = "wasm32")]
            let mut rng = Lcg::new(u64::from(state.turn) + 1);
            *state = GameState::with_rng(term, &mut rng);
        }
        _ => {}
    }
    true
}

retroglyph_examples::rg_run!(GameState, GameState::new, tick);
