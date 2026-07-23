//! 11: Sokoban
//!
//! Tier 3's pilot game: pure grid-and-core logic, no
//! external dependencies, no field-of-view or pathfinding needed -- unlike the roguelike planned
//! after it, sokoban's rules (push a box onto a goal, don't push it into a wall or another box)
//! are a handful of `match` arms over a [`Grid::from_charmap`]-built level, not an algorithm.
//! This is also the first example to compose a whole small game loop out of pieces every earlier
//! example proved individually: `02_colors`' fg/bg vocabulary distinguishes walls, floors, goals,
//! and boxes; `03_keyboard`'s arrow-key handling drives movement; `05_layout_grid`'s manual
//! `Rect` math lays out the play field next to a status pane; `08_animation`'s `Tween`-driven
//! [`Terminal::put_offset`] slides the player (and any box it pushes) one cell at a time instead
//! of snapping, generalized here from one axis to two; and `09_widgets_dashboard`'s
//! `retroglyph-widgets` usage (`Panel`, `split_h`/`split_v`) frames the status pane.
//!
//! Sliding is a visual-only nicety, the same graceful degradation `08_animation` documents for
//! `put_offset`: the software backend renders the true in-between position; crossterm and
//! headless silently ignore the offset and redraw only the final cell, so a push looks like a
//! discrete hop there instead of a slide -- no fallback code needed, and no gameplay logic
//! depends on the animation ever finishing (a move's board state is applied instantly; the tween
//! only decides how the *next present* draws it).
//!
//! ```sh
//! cargo run --example 11_sokoban --features crossterm
//! cargo run --example 11_sokoban --features software
//! cargo run --example 11_sokoban  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Arrow keys move (and push); `u` undoes the last move, `r` resets the level; `q`/`Escape` quits.

use std::time::Duration;

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{
    AnsiColor, Backend, Color, Easing, Frame, Grid, Rect, Style, Terminal, Tile, Tween,
};
use retroglyph_examples::Example;
use retroglyph_widgets::{Constraint, Panel, Widget, split_h, split_v};

/// The level, hand-designed to be solvable with the two boxes pushed one at a time: `#` wall,
/// `.` floor, `o` goal, `$` a box (on plain floor), `@` the player's start. Both `o` cells are
/// reachable straight-up pushes from directly below, so the intended solve is: walk to just
/// below the left box, push up, walk around to just below the right box, push up.
const LEVEL: &str = "\
###########
#.........#
#..o...o..#
#..$...$..#
#....@....#
#.........#
###########";

/// Cell width/height in pixels of the software backend's default embedded font (see
/// `crates/font/src/lib.rs`'s `FONT` constant) -- the scale `put_offset`'s `dx`/`dy`
/// pixel units are relative to.
const CELL_W_PX: f32 = 8.0;
const CELL_H_PX: f32 = 16.0;

/// How long a single step (walk or push) takes to slide, on backends that render it. Short
/// enough to read as a snappy step rather than a lingering animation.
const MOVE_DURATION: Duration = Duration::from_millis(90);

/// A grid position, in cells.
type Cell = (u16, u16);

/// One moving entity's logical position plus the per-axis tweens that animate its on-screen
/// draw position toward it. The two are deliberately decoupled: `pos` is the authoritative game
/// state (used for collision, win-checking, undo), updated the instant a move is legal; `x`/`y`
/// only affect what a `present()` draws in between, and finishing late costs nothing.
struct Slot {
    pos: Cell,
    x: Tween,
    y: Tween,
}

impl Slot {
    fn at(pos: Cell) -> Self {
        let (x, y) = (f32::from(pos.0), f32::from(pos.1));
        Self {
            pos,
            x: Tween::new(x, x)
                .duration(MOVE_DURATION)
                .easing(Easing::EaseOutQuad),
            y: Tween::new(y, y)
                .duration(MOVE_DURATION)
                .easing(Easing::EaseOutQuad),
        }
    }

    /// Instantly teleports to `pos` with no slide -- used by reset, where snapping back to the
    /// start is the point, not another animated step.
    fn snap(&mut self, pos: Cell) {
        *self = Self::at(pos);
    }

    /// Moves to `pos`, animated: the tween retargets from wherever it currently is (mid-flight
    /// or at rest), per [`Tween::retarget`]'s own doc comment, so a rapid run of keypresses never
    /// visibly snaps between steps.
    fn slide_to(&mut self, pos: Cell) {
        self.pos = pos;
        self.x.retarget(f32::from(pos.0));
        self.y.retarget(f32::from(pos.1));
    }

    fn advance(&mut self, dt: Duration) {
        self.x.update(dt);
        self.y.update(dt);
    }

    /// Current draw position: the target cell (so a fresh present after `is_finished` lands
    /// exactly on-cell) plus a sub-cell pixel offset for whichever axis is still animating.
    fn draw_pos(&self) -> (u16, u16, i16, i16) {
        let (vx, vy) = (self.x.value(), self.y.value());
        let (cx, cy) = (vx.floor(), vy.floor());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (cell_x, cell_y) = (cx as u16, cy as u16);
        #[allow(clippy::cast_possible_truncation)]
        let dx = ((vx - cx) * CELL_W_PX) as i16;
        #[allow(clippy::cast_possible_truncation)]
        let dy = ((vy - cy) * CELL_H_PX) as i16;
        (cell_x, cell_y, dx, dy)
    }
}

/// A snapshot of board state, pushed before every legal move so `u` can restore it.
struct Snapshot {
    player: Cell,
    boxes: Vec<Cell>,
}

/// State for the sokoban example.
pub struct Sokoban {
    /// The static level: walls and goals, built once via [`Grid::from_charmap`] and blitted
    /// unchanged every frame. Boxes and the player are drawn on top, separately, because they
    /// move; baking them into this grid would mean rebuilding it every step instead of just
    /// updating two small `Vec`s.
    level: Grid,
    width: u16,
    height: u16,
    goals: Vec<Cell>,
    player: Slot,
    boxes: Vec<Slot>,
    history: Vec<Snapshot>,
    moves: u32,
    won: bool,
}

impl Default for Sokoban {
    fn default() -> Self {
        let mut goals = Vec::new();
        let mut box_starts = Vec::new();
        let mut player_start = (0u16, 0u16);
        let mut width = 0u16;
        let mut height = 0u16;
        for (y, line) in LEVEL.lines().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let y = y as u16;
            height = height.max(y + 1);
            for (x, ch) in line.chars().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let x = x as u16;
                width = width.max(x + 1);
                match ch {
                    'o' => goals.push((x, y)),
                    '$' => box_starts.push((x, y)),
                    '@' => player_start = (x, y),
                    _ => {}
                }
            }
        }

        let wall_style = Style::new().fg(Color::Ansi(AnsiColor::White));
        let floor_style = Style::new().fg(Color::Ansi(AnsiColor::BrightBlack));
        let goal_style = Style::new().fg(Color::Ansi(AnsiColor::BrightYellow));
        let level = Grid::from_charmap(LEVEL, |c| match c {
            '#' => Tile::new('#', wall_style),
            'o' => Tile::new('o', goal_style),
            _ => Tile::new('.', floor_style),
        });

        Self {
            level,
            width,
            height,
            goals,
            player: Slot::at(player_start),
            boxes: box_starts.into_iter().map(Slot::at).collect(),
            history: Vec::new(),
            moves: 0,
            won: false,
        }
    }
}

impl Sokoban {
    fn is_wall(&self, x: i32, y: i32) -> bool {
        let (Ok(ux), Ok(uy)) = (u16::try_from(x), u16::try_from(y)) else {
            return true; // negative: off the top/left edge, treated as a wall
        };
        ux >= self.width || uy >= self.height || self.level.get(ux, uy).glyph() == '#'
    }

    fn box_at(&self, cell: Cell) -> Option<usize> {
        self.boxes.iter().position(|b| b.pos == cell)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            player: self.player.pos,
            boxes: self.boxes.iter().map(|b| b.pos).collect(),
        }
    }

    fn check_win(&mut self) {
        self.won = self.boxes.iter().all(|b| self.goals.contains(&b.pos));
    }

    /// Attempts one step in `(dx, dy)` (a unit vector: one of the four arrow directions). A
    /// no-op, silently, if the destination is a wall, or holds a box with nothing but a wall or
    /// another box behind it -- sokoban's whole rule set is these two checks.
    fn try_move(&mut self, dx: i32, dy: i32) {
        if self.won {
            return;
        }
        let (px, py) = self.player.pos;
        let (nx, ny) = (i32::from(px) + dx, i32::from(py) + dy);
        if self.is_wall(nx, ny) {
            return;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let target = (nx as u16, ny as u16);

        let pushed_box = match self.box_at(target) {
            Some(box_idx) => {
                let (bx, by) = (nx + dx, ny + dy);
                if self.is_wall(bx, by) {
                    return;
                }
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let box_target = (bx as u16, by as u16);
                if self.box_at(box_target).is_some() {
                    return;
                }
                Some((box_idx, box_target))
            }
            None => None,
        };

        self.history.push(self.snapshot());
        if let Some((box_idx, box_target)) = pushed_box {
            self.boxes[box_idx].slide_to(box_target);
        }
        self.player.slide_to(target);
        self.moves += 1;
        self.check_win();
    }

    fn undo(&mut self) {
        let Some(snap) = self.history.pop() else {
            return;
        };
        self.player.snap(snap.player);
        for (slot, pos) in self.boxes.iter_mut().zip(snap.boxes) {
            slot.snap(pos);
        }
        self.moves = self.moves.saturating_sub(1);
        self.check_win();
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Drains pending input: arrows move/push, `u` undoes, `r` resets, `q`/`Escape` quits.
    ///
    /// Gated on [`KeyEvent::is_down`](retroglyph_core::event::KeyEvent::is_down) -- see
    /// `09_widgets_dashboard.rs`'s `handle_events` doc comment for why: without it, a backend
    /// reporting both press and release as separate events would move twice per key tap.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if key.is_down() => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Up => self.try_move(0, -1),
                    KeyCode::Down => self.try_move(0, 1),
                    KeyCode::Left => self.try_move(-1, 0),
                    KeyCode::Right => self.try_move(1, 0),
                    KeyCode::Char('u') => self.undo(),
                    KeyCode::Char('r') => self.reset(),
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let area = Rect::new(0, 0, 50, 25);
        let rows = split_v(area, &[Constraint::Fixed(1), Constraint::Fill(1)]);
        let (title_area, body_area) = (rows[0], rows[1]);

        term.print(
            title_area.left() + 1,
            title_area.top(),
            "Sokoban -- arrows move/push, u undoes, r resets, q/Escape quits",
        );

        let cols = split_h(body_area, &[Constraint::Fixed(24), Constraint::Fill(1)]);
        let (play_area, status_area) = (cols[0], cols[1]);

        let level_x = play_area.left() + 2;
        let level_y = play_area.top() + 2;
        retroglyph_widgets::blit_into(term, &self.level, level_x, level_y);

        for b in &self.boxes {
            let on_goal = self.goals.contains(&b.pos);
            let (cx, cy, dx, dy) = b.draw_pos();
            let color = if on_goal {
                AnsiColor::BrightGreen
            } else {
                AnsiColor::BrightRed
            };
            term.reset_style().fg(Color::Ansi(color)).bg(Color::Default);
            let glyph = if on_goal { '*' } else { '$' };
            term.put_offset(level_x + cx, level_y + cy, dx, dy, glyph);
        }

        let (px, py, pdx, pdy) = self.player.draw_pos();
        term.reset_style()
            .fg(Color::Ansi(AnsiColor::BrightCyan))
            .bg(Color::Default);
        term.put_offset(level_x + px, level_y + py, pdx, pdy, '@');
        term.reset_style();

        Panel::new().title("Status").render(status_area, term);
        let inner_x = status_area.left() + 2;
        let mut y = status_area.top() + 1;
        term.print(inner_x, y, &format!("Moves: {}", self.moves));
        y += 2;
        term.print(inner_x, y, "Arrows: move / push");
        y += 1;
        term.print(inner_x, y, "u: undo");
        y += 1;
        term.print(inner_x, y, "r: reset level");
        y += 1;
        term.print(inner_x, y, "q / Esc: quit");
        if self.won {
            y += 2;
            term.reset_style().fg(Color::Ansi(AnsiColor::BrightGreen));
            term.print(inner_x, y, "*** Solved! ***");
            term.reset_style();
        }

        term.present().ok();
    }
}

impl Example for Sokoban {
    const NAME: &'static str = "11_sokoban";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.player.advance(frame.delta);
        for b in &mut self.boxes {
            b.advance(frame.delta);
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(Sokoban);
