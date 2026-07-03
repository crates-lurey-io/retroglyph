//! Sokoban puzzle demo.
//!
//! Validates core input, grid rendering, and game-loop APIs in a real puzzle
//! context. Push all boxes (`■`) onto the goals (`·`) to advance to the next
//! level. Wraps back to level 0 after the last.
//!
//! # Controls
//!
//! - Arrow keys / WASD — move player
//! - Enter — restart current level
//! - Q / Escape — quit
//!
//! # Run
//!
//! ```sh
//! cargo run --example sokoban --features crossterm
//! cargo run --example sokoban --features software-default-font
//! ```

use retroglyph_examples::util::action::{Action, next_action};

use retroglyph_core::color::Color;
use retroglyph_core::style::Style;
use retroglyph_core::{Backend, Pos, Terminal};

// ── Level data ────────────────────────────────────────────────────────────────

/// Raw level strings in standard Sokoban (XSB) notation.
///
/// Cell characters:
/// - `#`  wall
/// - ` `  floor
/// - `@`  player on floor
/// - `+`  player on goal
/// - `$`  box on floor
/// - `*`  box on goal
/// - `.`  goal (empty)
///
/// Levels 1-3 from the canonical Thinking Rabbit "Original" collection (1982).
/// Rows may vary in length; the parser pads each row to the level width with floor.
static LEVELS: &[&str] = &[
    // Level 1 — Original #1 (Thinking Rabbit, 1982)
    // Solution exists in ~97 moves.
    r"    #####
    #   #
    #$  #
  ###  $##
  #  $ $ #
### # ## #   ######
#   # ## #####  ..#
# $  $          ..#
##### ### #@##  ..#
    #     #########
    #######",
    // Level 2 — Original #2 (Thinking Rabbit, 1982)
    r"############
#..  #     ###
#..  # $  $  #
#..  #$####  #
#..    @ ##  #
#..  # #  $ ##
###### ##$ $ #
  # $  $ $ $ #
  #    #     #
  ############",
    // Level 3 — Original #3 (Thinking Rabbit, 1982)
    r"        ########
        #     @#
        # $#$ ##
        # $  $#
        ##$ $ #
######### $ # ###
#....  ## $  $  #
##...    $  $   #
#....  ##########
########",
];

// ── Cell type ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Wall,
    Floor,
    Goal,
}

// ── State ─────────────────────────────────────────────────────────────────────

struct GameState {
    cells: Vec<Cell>,
    width: u16,
    height: u16,
    player: Pos,
    boxes: Vec<Pos>,
    level: usize,
    moves: u32,
    /// Frames remaining on the win overlay before advancing to the next level.
    win_frames: u32,
}

impl GameState {
    /// Parse level `n` into a fresh [`GameState`].
    fn load_level(n: usize) -> Self {
        let src = LEVELS[n % LEVELS.len()];
        let lines: Vec<&str> = src.lines().collect();
        #[allow(clippy::cast_possible_truncation)]
        let height = lines.len() as u16;
        #[allow(clippy::cast_possible_truncation)]
        let width = lines.iter().map(|l| l.len()).max().unwrap_or(0) as u16;

        let mut cells = vec![Cell::Floor; usize::from(width) * usize::from(height)];
        let mut player = Pos::new(0, 0);
        let mut boxes: Vec<Pos> = Vec::new();

        for (row, line) in lines.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let x = col as u16;
                #[allow(clippy::cast_possible_truncation)]
                let y = row as u16;
                let idx = usize::from(y) * usize::from(width) + usize::from(x);
                match ch {
                    '#' => cells[idx] = Cell::Wall,
                    '.' => cells[idx] = Cell::Goal,
                    '@' => {
                        cells[idx] = Cell::Floor;
                        player = Pos::new(x, y);
                    }
                    '+' => {
                        cells[idx] = Cell::Goal;
                        player = Pos::new(x, y);
                    }
                    '$' => {
                        cells[idx] = Cell::Floor;
                        boxes.push(Pos::new(x, y));
                    }
                    '*' => {
                        cells[idx] = Cell::Goal;
                        boxes.push(Pos::new(x, y));
                    }
                    _ => cells[idx] = Cell::Floor,
                }
            }
        }

        Self {
            cells,
            width,
            height,
            player,
            boxes,
            level: n % LEVELS.len(),
            moves: 0,
            win_frames: 0,
        }
    }

    /// Restart the current level from scratch.
    fn restart(&mut self) {
        *self = Self::load_level(self.level);
    }

    /// Load the next level (wraps to 0 after the last).
    fn advance_level(&mut self) {
        *self = Self::load_level((self.level + 1) % LEVELS.len());
    }

    fn cell_at(&self, x: u16, y: u16) -> Cell {
        if x >= self.width || y >= self.height {
            return Cell::Wall;
        }
        self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)]
    }

    fn box_at(&self, x: u16, y: u16) -> Option<usize> {
        self.boxes.iter().position(|b| b.x == x && b.y == y)
    }

    /// Attempt to move the player by `(dx, dy)`. Standard Sokoban push logic.
    ///
    /// Wrapping arithmetic on `u16` is intentional: out-of-range coordinates
    /// (e.g. column 65535 when moving left from column 0) are caught by
    /// `cell_at`, which returns `Cell::Wall` for any coordinate outside the
    /// level bounds, so movement is blocked correctly.
    fn try_move(&mut self, dx: i16, dy: i16) {
        let nx = self.player.x.wrapping_add_signed(dx);
        let ny = self.player.y.wrapping_add_signed(dy);

        if self.cell_at(nx, ny) == Cell::Wall {
            return;
        }

        if let Some(bi) = self.box_at(nx, ny) {
            let bx = nx.wrapping_add_signed(dx);
            let by = ny.wrapping_add_signed(dy);

            if self.cell_at(bx, by) == Cell::Wall || self.box_at(bx, by).is_some() {
                return;
            }
            self.boxes[bi] = Pos::new(bx, by);
        }

        self.player = Pos::new(nx, ny);
        self.moves += 1;
    }

    /// Returns `true` when every box is on a goal.
    fn is_solved(&self) -> bool {
        self.boxes
            .iter()
            .all(|b| self.cell_at(b.x, b.y) == Cell::Goal)
    }
}

// ── Init ──────────────────────────────────────────────────────────────────────

fn init<B: Backend>(_term: &mut Terminal<B>) -> GameState {
    GameState::load_level(0)
}

// ── Colors ────────────────────────────────────────────────────────────────────

const COLOR_WALL: Color = Color::Rgb {
    r: 80,
    g: 70,
    b: 90,
};
const COLOR_GOAL: Color = Color::Rgb {
    r: 120,
    g: 100,
    b: 60,
};
const COLOR_BOX: Color = Color::Rgb {
    r: 180,
    g: 140,
    b: 60,
};
const COLOR_BOX_ON_GOAL: Color = Color::Rgb {
    r: 80,
    g: 200,
    b: 80,
};
const COLOR_PLAYER: Color = Color::Rgb {
    r: 200,
    g: 200,
    b: 255,
};
const COLOR_BG: Color = Color::Rgb {
    r: 15,
    g: 14,
    b: 18,
};
const COLOR_STATUS: Color = Color::Rgb {
    r: 160,
    g: 160,
    b: 180,
};
const COLOR_WIN: Color = Color::Rgb {
    r: 80,
    g: 220,
    b: 120,
};

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw<B: Backend>(term: &mut Terminal<B>, state: &GameState) {
    let size = term.size();

    // Center the level, leaving one row for the status bar below.
    let ox = size.width.saturating_sub(state.width) / 2;
    let oy = size.height.saturating_sub(state.height + 2) / 2;

    // Background fill.
    for y in 0..size.height {
        for x in 0..size.width {
            term.put_styled(x, y, ' ', Style::new().bg(COLOR_BG));
        }
    }

    // Grid cells.
    for row in 0..state.height {
        for col in 0..state.width {
            let sx = ox + col;
            let sy = oy + row;
            if sx >= size.width || sy >= size.height {
                continue;
            }
            match state.cell_at(col, row) {
                Cell::Wall => {
                    term.put_styled(sx, sy, '█', Style::new().fg(COLOR_WALL).bg(COLOR_BG));
                }
                Cell::Floor => {
                    // Already filled with BG above; nothing extra to draw.
                }
                Cell::Goal => {
                    term.put_styled(sx, sy, '·', Style::new().fg(COLOR_GOAL).bg(COLOR_BG));
                }
            }
        }
    }

    // Boxes.
    for b in &state.boxes {
        let sx = ox + b.x;
        let sy = oy + b.y;
        if sx < size.width && sy < size.height {
            let on_goal = state.cell_at(b.x, b.y) == Cell::Goal;
            let fg = if on_goal {
                COLOR_BOX_ON_GOAL
            } else {
                COLOR_BOX
            };
            term.put_styled(sx, sy, '■', Style::new().fg(fg).bg(COLOR_BG));
        }
    }

    // Player.
    {
        let sx = ox + state.player.x;
        let sy = oy + state.player.y;
        if sx < size.width && sy < size.height {
            term.put_styled(sx, sy, '@', Style::new().fg(COLOR_PLAYER).bg(COLOR_BG));
        }
    }

    // Status bar.
    let status_y = oy + state.height + 1;
    if status_y < size.height {
        let status = format!(
            "Level {}  Moves: {}  [Enter=restart  Q=quit]",
            state.level + 1,
            state.moves,
        );
        #[allow(clippy::cast_possible_truncation)]
        let sx = size.width.saturating_sub(status.len() as u16) / 2;
        term.fg(COLOR_STATUS);
        term.bg(COLOR_BG);
        term.print(sx, status_y, &status);
        term.reset_style();
    }

    // Win overlay.
    if state.win_frames > 0 {
        let msg = "  Solved! Loading next level...  ";
        #[allow(clippy::cast_possible_truncation)]
        let mx = size.width.saturating_sub(msg.len() as u16) / 2;
        let my = oy + state.height / 2;
        if my < size.height {
            term.fg(COLOR_WIN);
            term.bg(COLOR_BG);
            term.print(mx, my, msg);
            term.reset_style();
        }
    }
}

// ── Tick ──────────────────────────────────────────────────────────────────────

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut GameState) -> bool {
    // Win animation: count down, then advance on the final frame.
    if state.win_frames > 0 {
        state.win_frames -= 1;
        draw(term, state);
        term.present().expect("present failed");
        if state.win_frames == 0 {
            state.advance_level();
        }
        return true;
    }

    draw(term, state);
    term.present().expect("present failed");

    match next_action(term) {
        Action::MoveUp => state.try_move(0, -1),
        Action::MoveDown => state.try_move(0, 1),
        Action::MoveLeft => state.try_move(-1, 0),
        Action::MoveRight => state.try_move(1, 0),
        Action::Confirm => state.restart(),
        Action::Quit => return false,
        _ => {}
    }

    if state.is_solved() {
        // Show the win overlay for ~60 frames before advancing.
        state.win_frames = 60;
    }

    true
}

// ── Entry point ───────────────────────────────────────────────────────────────

retroglyph_examples::rg_run!(GameState, init, tick);
