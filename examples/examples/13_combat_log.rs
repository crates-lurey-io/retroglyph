//! 13: Combat log
//!
//! Four `retroglyph-ui` widgets none of the earlier examples exercise: [`StatBar`] (a
//! health readout), [`Log`] (a scrolled-back message tail), [`Scrollbar`] (its track and thumb),
//! and [`Modal`] (a centered end-of-game dialog). A turn-based fight against a fixed-stat
//! goblin, deterministic on purpose: every attack does the same damage, so every run, and every
//! snapshot, plays out identically. `Scrollbar`, `Log`, `Modal`, and a stat-bar widget are the
//! kind of thing a real game built on this library needs, and libraries like ratatui treat them
//! as must-have gallery entries.
//!
//! The log also scrolls by mouse wheel, via [`ScrollState`] and [`Interaction`]: wheel input
//! over the log resolves to a `Response`, and [`ScrollState::apply`] feeds its
//! `scroll_delta` straight into momentum, instead of the log re-deriving wheel handling from
//! raw `MouseEventKind::Scroll` events itself (retroglyph#605). `Up`/`Down` still move the log
//! by exactly one message, instantly, via [`ScrollState::set_offset`] -- only the wheel path
//! gets momentum.
//!
//! `Scrollbar` also implements `AnimatedWidget`, ticking a `ScrollState`'s physics against
//! `total_len - visible_len` before drawing -- the right bound for a top-anchored, `List`-style
//! window. `Log`'s own offset means something different (how far back from the newest message,
//! unbounded by the viewport), so that bound doesn't fit it: this example ticks `scroll`
//! directly against the log's own bound instead of going through `AnimatedWidget` here.
//!
//! ```sh
//! cargo run --example 13_combat_log --features crossterm
//! cargo run --example 13_combat_log --features software
//! cargo run --example 13_combat_log  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `a` attacks. `Up`/`Down`, or the mouse wheel over the log, scroll it. `r` resets after
//! the fight ends. `q` or `Escape` quits at any time.

use retroglyph::app::Frame;
use retroglyph::backend::Backend;
use retroglyph::color::{AnsiColor, Color, Style};
use retroglyph::event::{Event, KeyCode};
use retroglyph::grid::Rect;
use retroglyph::surface::Surface;
use retroglyph::terminal::Terminal;
use retroglyph::text::Line;
use retroglyph::ui::{Interaction, Log, Modal, ScrollState, Scrollbar, Sense, StatBar, Widget};
use retroglyph_examples::Example;

const PLAYER_MAX_HP: u32 = 30;
const ENEMY_MAX_HP: u32 = 40;
const PLAYER_DAMAGE: u32 = 7;
const ENEMY_DAMAGE: u32 = 5;

/// Identifies the scrollable log region for [`Interaction`]'s hit-testing. The only widget in
/// this example that senses anything, so a single variant is enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Id {
    Log,
}

/// State for the combat-log example.
pub struct CombatLog {
    player_hp: u32,
    enemy_hp: u32,
    log: Vec<Line>,
    scroll: ScrollState,
    interaction: Interaction<Id>,
    over: bool,
}

impl Default for CombatLog {
    fn default() -> Self {
        Self {
            player_hp: PLAYER_MAX_HP,
            enemy_hp: ENEMY_MAX_HP,
            log: vec![Line::raw("A goblin blocks your path!")],
            scroll: ScrollState::new(),
            interaction: Interaction::new(),
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

    /// The highest offset [`Log::offset`] usefully reaches: scrolling back further than the log
    /// is long shows fewer (or zero) messages (see `Log`'s own doc comment), a legitimate but
    /// useless state to land in via a key press or a wheel notch, so both cap here instead.
    #[allow(clippy::cast_precision_loss)] // combat logs stay well under 2^24 messages
    const fn max_log_offset(&self) -> f32 {
        self.log.len().saturating_sub(1) as f32
    }

    /// Moves the log by exactly `delta` messages, instantly (no momentum): `ScrollState::set_offset`
    /// zeroes velocity, so a key press always lands exactly where pressed, cutting off any wheel
    /// fling still decaying from before.
    #[allow(clippy::cast_precision_loss)] // see max_log_offset
    fn scroll(&mut self, delta: i32) {
        let max = self.max_log_offset();
        let current = self.scroll.integer_offset() as f32;
        self.scroll.set_offset(current + delta as f32, max);
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            let _ = self.interaction.handle_event(&event);
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

    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) {
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
        StatBar::new("You  ", self.player_hp, PLAYER_MAX_HP)
            .render(&mut Surface::new(term.grid_mut(), term_area, 0).scope(Rect::new(1, 1, 46, 1)));
        StatBar::new("Gob. ", self.enemy_hp, ENEMY_MAX_HP)
            .render(&mut Surface::new(term.grid_mut(), term_area, 0).scope(Rect::new(1, 2, 46, 1)));

        let log_area = Rect::new(1, 4, 47, 20);

        // `Sense::scroll()` reaches this rect regardless of what's drawn on top of it (see
        // `Interaction::interact`'s own doc comment on `scroll_delta`), so this feeds wheel
        // input into `scroll`'s momentum the same way a real scrollable widget would, instead of
        // matching raw `MouseEventKind::Scroll` events by hand.
        let response = self
            .interaction
            .interact(log_area, Id::Log, Sense::scroll());
        self.scroll.apply(&response);

        // Not `AnimatedWidget::render`: see this module's doc comment for why `Log`'s own bound
        // doesn't match `Scrollbar`'s built-in `total_len - visible_len` one.
        self.scroll.tick(frame.delta, self.max_log_offset());
        let log_offset = self.scroll.integer_offset();

        Log::new(&self.log)
            .offset(log_offset)
            .render(&mut Surface::new(term.grid_mut(), term_area, 0).scope(log_area));
        let scrollbar = Scrollbar::new(self.log.len(), log_area.height_usize())
            .offset(log_offset)
            .track_style(Style::new().fg(Color::Ansi(AnsiColor::BrightBlack)))
            .thumb_style(Style::new().bg(Color::Ansi(AnsiColor::BrightBlack)));
        // UFCS: `Scrollbar` also implements `AnimatedWidget` (not used here, see this module's
        // doc comment), so a bare `.render(...)` call is ambiguous between the two traits.
        Widget::render(
            &scrollbar,
            &mut Surface::new(term.grid_mut(), term_area, 0).scope(Rect::new(48, 4, 1, 20)),
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

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        self.interaction.begin_frame();
        if !self.handle_events(term) {
            self.interaction.end_frame();
            return false;
        }
        self.draw(term, frame);
        self.interaction.end_frame();
        true
    }
}

retroglyph_examples::example_main!(CombatLog);
