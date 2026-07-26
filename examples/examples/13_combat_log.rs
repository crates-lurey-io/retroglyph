//! 13: Combat log
//!
//! Four `retroglyph-widgets` widgets none of the earlier examples exercise: [`StatBar`] (a
//! health readout), [`Log`] (a scrolled-back message tail), [`Scrollbar`] (its track and thumb),
//! and [`Modal`] (a centered end-of-game dialog). A turn-based fight against a fixed-stat
//! goblin, deterministic on purpose: every attack does the same damage, so every run, and every
//! snapshot, plays out identically. `Scrollbar`, `Log`, `Modal`, and a stat-bar widget are the
//! kind of thing a real game built on this library needs, and libraries like ratatui treat them
//! as must-have gallery entries.
//!
//! ```sh
//! cargo run --example 13_combat_log --features crossterm
//! cargo run --example 13_combat_log --features software
//! cargo run --example 13_combat_log  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `a` attacks. `Up`/`Down` scroll the log. `r` resets after the fight ends. `q` or
//! `Escape` quits at any time.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::text::Line;
use retroglyph_core::{AnsiColor, Backend, Color, Frame, Rect, Style, Surface, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{Log, Modal, Scrollbar, StatBar, Widget};

const PLAYER_MAX_HP: u32 = 30;
const ENEMY_MAX_HP: u32 = 40;
const PLAYER_DAMAGE: u32 = 7;
const ENEMY_DAMAGE: u32 = 5;

/// State for the combat-log example.
pub struct CombatLog {
    player_hp: u32,
    enemy_hp: u32,
    log: Vec<Line>,
    log_offset: usize,
    over: bool,
}

impl Default for CombatLog {
    fn default() -> Self {
        Self {
            player_hp: PLAYER_MAX_HP,
            enemy_hp: ENEMY_MAX_HP,
            log: vec![Line::raw("A goblin blocks your path!")],
            log_offset: 0,
            over: false,
        }
    }
}

impl CombatLog {
    /// One turn: the player strikes for a fixed amount; if the goblin survives, it retaliates
    /// for its own fixed amount. Both damage values are constants (no RNG), so every playthrough
    /// -- and this example's own snapshot -- is identical.
    fn attack(&mut self) {
        if self.over {
            return;
        }
        self.enemy_hp = self.enemy_hp.saturating_sub(PLAYER_DAMAGE);
        self.log.push(Line::raw(format!(
            "You strike the goblin for {PLAYER_DAMAGE}."
        )));

        if self.enemy_hp == 0 {
            self.log.push(Line::raw("The goblin falls. You win!"));
            self.over = true;
            return;
        }

        self.player_hp = self.player_hp.saturating_sub(ENEMY_DAMAGE);
        self.log.push(Line::raw(format!(
            "The goblin claws you for {ENEMY_DAMAGE}."
        )));
        if self.player_hp == 0 {
            self.log.push(Line::raw("You have fallen. Game over."));
            self.over = true;
        }
    }

    fn scroll(&mut self, delta: i32) {
        let max = self.log.len().saturating_sub(1);
        self.log_offset = self
            .log_offset
            .saturating_add_signed(delta as isize)
            .min(max);
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if key.is_down() => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Char('a') => self.attack(),
                    KeyCode::Up => self.scroll(1),
                    KeyCode::Down => self.scroll(-1),
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
        // Kept short on purpose: this is a 50-column grid, and `Surface::print` wraps text
        // that overflows the width onto the next row -- which would otherwise stomp on the
        // stat bars printed right below.
        {
            let mut surface = term.surface();
            surface.print(
                (1, 0),
                "a: attack  Up/Down: scroll  r: reset  q/Esc: quit",
                Style::default(),
            );
        }

        let term_area = term.area();
        StatBar::new("You  ", self.player_hp, PLAYER_MAX_HP).render(
            Rect::new(1, 1, 46, 1),
            &mut Surface::new(term.grid_mut(), term_area, 0),
        );
        StatBar::new("Gob. ", self.enemy_hp, ENEMY_MAX_HP).render(
            Rect::new(1, 2, 46, 1),
            &mut Surface::new(term.grid_mut(), term_area, 0),
        );

        let log_area = Rect::new(1, 4, 47, 20);
        Log::new(&self.log)
            .offset(self.log_offset)
            .render(log_area, &mut Surface::new(term.grid_mut(), term_area, 0));
        Scrollbar::new(self.log.len(), log_area.height_usize())
            .offset(self.log_offset)
            .track_style(Style::new().fg(Color::Ansi(AnsiColor::BrightBlack)))
            .thumb_style(Style::new().bg(Color::Ansi(AnsiColor::BrightBlack)))
            .render(
                Rect::new(48, 4, 1, 20),
                &mut Surface::new(term.grid_mut(), term_area, 0),
            );

        if self.over {
            let title = if self.enemy_hp == 0 {
                "You win!"
            } else {
                "Game over"
            };
            let inner = Modal::new(30, 6)
                .title(title)
                .border_style(Style::new().fg(Color::Ansi(AnsiColor::BrightYellow)))
                .render(
                    Rect::new(0, 0, 50, 25),
                    &mut Surface::new(term.grid_mut(), term_area, 0),
                );
            let mut surface = term.surface();
            surface.print(
                (inner.left(), inner.top()),
                &format!(
                    "You: {}/{PLAYER_MAX_HP}  Goblin: {}/{ENEMY_MAX_HP}",
                    self.player_hp, self.enemy_hp
                ),
                Style::default(),
            );
            surface.print(
                (inner.left(), inner.top() + 2),
                "r: reset",
                Style::default(),
            );
            surface.print(
                (inner.left(), inner.top() + 3),
                "q / Esc: quit",
                Style::default(),
            );
        }
    }
}

impl Example for CombatLog {
    const NAME: &'static str = "13_combat_log";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(CombatLog);
