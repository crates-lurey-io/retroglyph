//! Single-level dungeon crawler demo.
//!
//! A minimal but playable roguelike that exercises:
//! - Multi-layer rendering (`terrain`, `entities`, `UI`)
//! - Field of view (FOV) with persistent fog-of-war
//! - BFS monster pathfinding
//! - Combat (bump-to-attack)
//! - Action and timestep utilities
//!
//! # Controls
//!
//! - Arrow keys / WASD — move player
//! - R — restart on death screen
//! - Q / Escape — quit
//!
//! # Run
//!
//! ```sh
//! cargo run --example roguelike_dungeon --features crossterm
//! cargo run --example roguelike_dungeon --features software-default-font
//! ```

mod util;

use std::collections::VecDeque;

use retroglyph::color::Color;
use retroglyph::style::Style;
use retroglyph::{Backend, Pos, Terminal};
use util::action::{Action, next_action};

// ── Map ───────────────────────────────────────────────────────────────────────

/// Field of view radius.
const FOV_RADIUS: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Wall,
    Floor,
}

/// Raw level data. `#` = wall, `.` = floor, `@` = player start, `M` = monster.
const LEVEL: &str = concat!(
    "########################################\n",
    "#......................................#\n",
    "#......................................#\n",
    "#..........###...............###.......#\n",
    "#..........#.#...............###.......#\n",
    "#..........#.#.........................#\n",
    "####.......#.#..............############\n",
    "#...........#.#.............############\n",
    "#...........#.#.....................M..#\n",
    "#......@....#.#....................M...#\n",
    "#...........###...................M....#\n",
    "#..................M..................#\n",
    "#..........#############################\n",
    "#..........#...........................#\n",
    "#..........#...........................#\n",
    "#..........#...........................#\n",
    "#..........#############################\n",
    "#..........#...........................#\n",
    "#..........#...........................#\n",
    "########################################",
);

struct Map {
    cells: Vec<Cell>,
    width: u16,
    height: u16,
}

impl Map {
    fn from_str(raw: &str) -> Self {
        // Compute width from the first row (up to \n).
        #[allow(clippy::cast_possible_truncation)]
        let width = raw.chars().take_while(|&c| c != '\n').count() as u16;
        let cells: Vec<Cell> = raw
            .chars()
            .filter(|c| *c != '\n')
            .map(|c| match c {
                '#' => Cell::Wall,
                _ => Cell::Floor,
            })
            .collect();
        #[allow(clippy::cast_possible_truncation)]
        let height = cells.len() as u16 / width;
        Self {
            cells,
            width,
            height,
        }
    }

    fn parse_entities(raw: &str) -> (Pos, Vec<Pos>) {
        #[allow(clippy::cast_possible_truncation)]
        let width = raw.chars().take_while(|&c| c != '\n').count() as u16;
        let mut player = Pos::new(1, 1);
        let mut monsters = Vec::new();
        for (i, ch) in raw.chars().filter(|c| *c != '\n').enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % width as usize) as u16;
            #[allow(clippy::cast_possible_truncation)]
            let y = (i / width as usize) as u16;
            match ch {
                '@' => player = Pos::new(x, y),
                'M' => monsters.push(Pos::new(x, y)),
                _ => {}
            }
        }
        (player, monsters)
    }

    fn cell(&self, x: u16, y: u16) -> Cell {
        self.cells[y as usize * self.width as usize + x as usize]
    }

    fn is_wall(&self, p: Pos) -> bool {
        self.cell(p.x, p.y) == Cell::Wall
    }

    #[allow(clippy::unused_self)]
    const fn in_bounds(&self, p: Pos) -> bool {
        p.x < self.width && p.y < self.height
    }
}

// ── Entity state ──────────────────────────────────────────────────────────────

struct Player {
    pos: Pos,
    hp: i32,
    max_hp: i32,
}

struct Monster {
    pos: Pos,
    hp: i32,
    alive: bool,
    name: &'static str,
}

const MONSTER_MAX_HP: i32 = 3;
const PLAYER_MAX_HP: i32 = 10;

// ── Game state ────────────────────────────────────────────────────────────────

struct GameState {
    map: Map,
    player: Player,
    monsters: Vec<Monster>,
    /// For each cell: is it currently visible?
    visible: Vec<bool>,
    /// For each cell: has it ever been seen?
    seen: Vec<bool>,
    /// Turn counter.
    turn: u32,
    /// Messages from the last few turns (oldest first).
    messages: VecDeque<String>,
    /// Is the player dead (waiting for R or Q)?
    dead: bool,
}

impl GameState {
    fn new() -> Self {
        let map = Map::from_str(LEVEL);
        let (player_pos, monster_positions) = Map::parse_entities(LEVEL);
        let cell_count = map.width as usize * map.height as usize;

        let mut state = Self {
            map,
            player: Player {
                pos: player_pos,
                hp: PLAYER_MAX_HP,
                max_hp: PLAYER_MAX_HP,
            },
            monsters: monster_positions
                .iter()
                .map(|&p| Monster {
                    pos: p,
                    hp: MONSTER_MAX_HP,
                    alive: true,
                    name: "Goblin",
                })
                .collect(),
            visible: vec![false; cell_count],
            seen: vec![false; cell_count],
            turn: 0,
            messages: VecDeque::with_capacity(3),
            dead: false,
        };
        state.add_message("You enter the dungeon...".to_string());
        compute_fov(
            &state.map,
            state.player.pos,
            &mut state.visible,
            &mut state.seen,
        );
        state
    }

    fn add_message(&mut self, msg: String) {
        if self.messages.len() >= 3 {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
    }

    /// Move the player by `(dx, dy)`. Handles wall collision, monster combat,
    /// then advances monster AI.
    fn player_move(&mut self, dx: i16, dy: i16) {
        if self.dead {
            return;
        }

        let new_x = self.player.pos.x.wrapping_add_signed(dx);
        let new_y = self.player.pos.y.wrapping_add_signed(dy);
        let dest = Pos::new(new_x, new_y);

        if !self.map.in_bounds(dest) || self.map.is_wall(dest) {
            return;
        }

        // Check for monster at destination.
        let monster_idx = self.monsters.iter().position(|m| m.alive && m.pos == dest);

        if let Some(idx) = monster_idx {
            self.monsters[idx].hp -= 3;
            self.add_message(format!("You strike the {}.", self.monsters[idx].name));
            if self.monsters[idx].hp <= 0 {
                self.monsters[idx].alive = false;
                self.add_message(format!("You slew the {}.", self.monsters[idx].name));
            }
            // Don't move; bump-attack ends the player's movement.
        } else {
            self.player.pos = dest;
        }

        self.monster_turn();
        self.turn = self.turn.wrapping_add(1);
    }

    /// Each living monster takes one step toward the player (`BFS`).
    /// Monsters outside `FOV` don't move (they are "asleep").
    fn monster_turn(&mut self) {
        let monster_count = self.monsters.len();
        for i in 0..monster_count {
            if !self.monsters[i].alive {
                continue;
            }
            let mpos = self.monsters[i].pos;
            // Only move if visible to the player.
            if !self.visible(mpos) {
                continue;
            }
            // Build a list of blocked positions (other living monsters + walls).
            let blocked: Vec<Pos> = self
                .monsters
                .iter()
                .enumerate()
                .filter(|(j, m)| *j != i && m.alive)
                .map(|(_, m)| m.pos)
                .collect();

            let step = step_toward(&self.map, mpos, self.player.pos, &blocked[..]);

            if let Some(next) = step {
                if next == self.player.pos {
                    self.player.hp -= 1;
                    self.add_message("A Goblin claws you!".to_string());
                } else {
                    self.monsters[i].pos = next;
                }
            }
        }

        // Recompute visibility after all entities moved.
        compute_fov(
            &self.map,
            self.player.pos,
            &mut self.visible,
            &mut self.seen,
        );

        if self.player.hp <= 0 {
            self.dead = true;
            self.add_message("You die...".to_string());
        }
    }

    fn restart(&mut self) {
        *self = Self::new();
    }

    fn visible(&self, p: Pos) -> bool {
        self.visible[p.y as usize * self.map.width as usize + p.x as usize]
    }
}

// ── Field of View (raycasting) ────────────────────────────────────────────────

/// Simple raycasting `FOV`: cast rays in 1-degree increments, marking
/// visible cells until a wall is hit.
fn compute_fov(map: &Map, origin: Pos, visible: &mut [bool], seen: &mut [bool]) {
    visible.fill(false);

    // Mark the player's cell.
    let idx = |x: u16, y: u16| y as usize * map.width as usize + x as usize;

    if map.in_bounds(origin) {
        visible[idx(origin.x, origin.y)] = true;
        seen[idx(origin.x, origin.y)] = true;
    }

    // Cast 360 rays (1° each).
    for deg in 0..360 {
        let rad = f64::from(deg).to_radians();
        let cos = rad.cos();
        let sin = rad.sin();

        // Walk along the ray.
        for step in 1..=FOV_RADIUS {
            let step_f = f64::from(step);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let cx = (cos.mul_add(step_f, f64::from(origin.x))).round() as u16;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let cy = (sin.mul_add(step_f, f64::from(origin.y))).round() as u16;

            if !map.in_bounds(Pos::new(cx, cy)) {
                break;
            }
            let i = idx(cx, cy);
            visible[i] = true;
            seen[i] = true;

            if map.cell(cx, cy) == Cell::Wall {
                break;
            }
        }
    }
}

// ── Pathfinding (BFS) ─────────────────────────────────────────────────────────

/// BFS from `from` to `to`. Returns the first step toward `to`, or `None`
/// if no path exists or the target is unreachable.
fn step_toward(map: &Map, from: Pos, to: Pos, blocked: &[Pos]) -> Option<Pos> {
    if from == to {
        return None;
    }

    let is_blocked = |p: Pos| -> bool {
        if !map.in_bounds(p) {
            return true;
        }
        if map.is_wall(p) {
            return true;
        }
        // Allow the target cell itself to be walkable.
        if p == to {
            return false;
        }
        blocked.contains(&p)
    };

    // Fast path: if `to` isn't walkable, give up.
    if is_blocked(to) {
        return None;
    }

    let idx = |p: Pos| p.y as usize * map.width as usize + p.x as usize;
    let mut visited = vec![false; map.width as usize * map.height as usize];
    // Queue: (current position, first step from `from`).
    let mut queue = VecDeque::new();

    visited[idx(from)] = true;

    // Enqueue all walkable neighbors of `from`.
    for (dx, dy) in &[(0i16, -1), (0, 1), (-1, 0), (1, 0)] {
        let nx = from.x.wrapping_add_signed(*dx);
        let ny = from.y.wrapping_add_signed(*dy);
        let n = Pos::new(nx, ny);
        if !is_blocked(n) && !visited[idx(n)] {
            visited[idx(n)] = true;
            queue.push_back((n, n));
        }
    }

    while let Some((current, first)) = queue.pop_front() {
        if current == to {
            return Some(first);
        }
        for (dx, dy) in &[(0i16, -1), (0, 1), (-1, 0), (1, 0)] {
            let nx = current.x.wrapping_add_signed(*dx);
            let ny = current.y.wrapping_add_signed(*dy);
            let n = Pos::new(nx, ny);
            if !is_blocked(n) && !visited[idx(n)] {
                visited[idx(n)] = true;
                queue.push_back((n, first));
            }
        }
    }
    None
}

// ── Rendering ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn draw<B: Backend>(term: &mut Terminal<B>, state: &GameState) {
    let mw = state.map.width;
    let mh = state.map.height;
    let cell_at = |x: u16, y: u16| y as usize * mw as usize + x as usize;

    // Author on separate layers (terrain / entities / UI). Terminal::present
    // composites them for cell backends, so this renders identically on
    // crossterm and software (ADR 015 Decision 1).

    // Layer 0: terrain.
    term.layer(0);
    for y in 0..mh {
        for x in 0..mw {
            let idx = cell_at(x, y);
            let is_visible = state.visible[idx];
            let is_seen = state.seen[idx];

            match state.map.cell(x, y) {
                Cell::Wall => {
                    let fg = if is_visible {
                        Color::Rgb {
                            r: 100,
                            g: 90,
                            b: 110,
                        }
                    } else if is_seen {
                        Color::Rgb {
                            r: 40,
                            g: 35,
                            b: 45,
                        }
                    } else {
                        Color::Rgb { r: 5, g: 5, b: 10 }
                    };
                    term.put_styled(x, y, '█', Style::new().fg(fg));
                }
                Cell::Floor => {
                    if is_visible {
                        term.put_styled(
                            x,
                            y,
                            '·',
                            Style::new().fg(Color::Rgb {
                                r: 70,
                                g: 65,
                                b: 80,
                            }),
                        );
                    } else if is_seen {
                        term.put_styled(
                            x,
                            y,
                            '·',
                            Style::new().fg(Color::Rgb {
                                r: 30,
                                g: 28,
                                b: 35,
                            }),
                        );
                    }
                }
            }
        }
    }

    // Layer 1: entities.
    term.layer(1);
    // Player.
    if state.visible[cell_at(state.player.pos.x, state.player.pos.y)] {
        term.put_styled(
            state.player.pos.x,
            state.player.pos.y,
            '@',
            Style::new().fg(Color::Rgb {
                r: 220,
                g: 220,
                b: 255,
            }),
        );
    }

    // Monsters.
    for m in &state.monsters {
        if !m.alive {
            continue;
        }
        let midx = cell_at(m.pos.x, m.pos.y);
        if state.visible[midx] {
            term.put_styled(
                m.pos.x,
                m.pos.y,
                'M',
                Style::new().fg(Color::Rgb {
                    r: 200,
                    g: 80,
                    b: 80,
                }),
            );
        }
    }

    // Layer 2: UI.
    term.layer(2);
    // Top bar.
    let top_bg = Color::Rgb {
        r: 25,
        g: 25,
        b: 40,
    };
    for x in 0..mw {
        term.put_styled(x, 0, ' ', Style::new().bg(top_bg));
    }
    let hp_text = if state.dead {
        "DEAD".to_string()
    } else {
        format!(
            "HP: {}/{}  Turn: {}",
            state.player.hp, state.player.max_hp, state.turn
        )
    };
    let hp_style = Style::new().fg(Color::BRIGHT_WHITE).bg(top_bg);
    for (i, ch) in hp_text.chars().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        term.put_styled(i as u16, 0, ch, hp_style);
    }

    // Death overlay.
    if state.dead {
        let death_msg = "You died.";
        let restart_msg = "[R] restart  [Q] quit";
        #[allow(clippy::cast_possible_truncation)]
        let cx = mw.saturating_sub(death_msg.len() as u16) / 2;
        let cy = mh / 2;
        for (i, ch) in death_msg.chars().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            term.put_styled(cx + i as u16, cy, ch, Style::new().fg(Color::BRIGHT_RED));
        }
        #[allow(clippy::cast_possible_truncation)]
        let rx = mw.saturating_sub(restart_msg.len() as u16) / 2;
        for (i, ch) in restart_msg.chars().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            term.put_styled(
                rx + i as u16,
                cy + 1,
                ch,
                Style::new().fg(Color::Rgb {
                    r: 180,
                    g: 180,
                    b: 140,
                }),
            );
        }
    }

    // Message log (bottom of screen).
    let msg_bg = Color::Rgb {
        r: 20,
        g: 20,
        b: 30,
    };
    for x in 0..mw {
        term.put_styled(x, mh - 4, ' ', Style::new().bg(msg_bg));
        term.put_styled(x, mh - 3, ' ', Style::new().bg(msg_bg));
        term.put_styled(x, mh - 2, ' ', Style::new().bg(msg_bg));
        term.put_styled(x, mh - 1, ' ', Style::new().bg(msg_bg));
    }
    let msg_style = Style::new()
        .fg(Color::Rgb {
            r: 180,
            g: 180,
            b: 140,
        })
        .bg(msg_bg);
    for (mi, msg) in state.messages.iter().enumerate() {
        for (i, ch) in msg.chars().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            term.put_styled(i as u16, mh - 4 + mi as u16, ch, msg_style);
        }
    }

    // Backend glyph fallback: on crossterm, `█` and `·` may render poorly.
    // We accept that; most modern terminals handle them.
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn init<B: Backend>(_term: &mut Terminal<B>) -> GameState {
    GameState::new()
}

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut GameState) -> bool {
    draw(term, state);
    term.present().expect("present failed");

    // Handle input.
    let mut handled = false;
    loop {
        let action = next_action(term);
        match action {
            Action::None => break,
            Action::Quit => {
                if state.dead {
                    // On death screen, Q quits entirely.
                    return false;
                }
                return false;
            }
            Action::Confirm => {
                if state.dead {
                    // On death screen, Enter also restarts.
                    state.restart();
                    handled = true;
                }
            }
            Action::MoveUp if !state.dead => {
                state.player_move(0, -1);
                handled = true;
            }
            Action::MoveDown if !state.dead => {
                state.player_move(0, 1);
                handled = true;
            }
            Action::MoveLeft if !state.dead => {
                state.player_move(-1, 0);
                handled = true;
            }
            Action::MoveRight if !state.dead => {
                state.player_move(1, 0);
                handled = true;
            }
            _ => {
                // Interact = nothing; Cancel = nothing
            }
        }
        if handled {
            break;
        }

        // Also check raw KeyCode for R (restart on death).
        for event in term.drain_events() {
            if let retroglyph::event::Event::Key(k) = event {
                if state.dead && matches!(k.code, retroglyph::event::KeyCode::Char('r' | 'R')) {
                    state.restart();
                    handled = true;
                    break;
                }
                if !state.dead && matches!(k.code, retroglyph::event::KeyCode::Char('r' | 'R')) {
                    state.restart();
                    handled = true;
                    break;
                }
            }
        }
        if !handled {
            break;
        }
    }

    true
}

rg_run!(GameState, init, tick);
