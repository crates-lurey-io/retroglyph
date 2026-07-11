//! 09: Widgets dashboard
//!
//! The first `retroglyph-widgets` showcase: [`Table`] (a scrollable service list with a
//! [`ListState`]-driven highlighted row), [`Gauge`] (load-colored bars for CPU/memory),
//! [`Sparkline`] (a recent-history graph), [`BoxStyle`] (a bordered legend box, rendered into a
//! standalone [`Grid`] and blitted in), [`split_h`]/[`split_v`] (the whole layout), and [`Theme`]
//! (every color in this example comes from [`Theme::DARK`], not a hand-picked one-off).
//! `retroglyph-widgets` is backend-generic -- nothing here is software/crossterm/headless-
//! specific -- which is the payoff of deferring it past Tier 1 rather than being blocked on it
//! (see this crate's ADR 019 doc comment for the full staging rationale).
//!
//! ```sh
//! cargo run --example 09_widgets_dashboard --features crossterm
//! cargo run --example 09_widgets_dashboard --features software
//! cargo run --example 09_widgets_dashboard  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Up/Down arrows move the table's selection; `q` or `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Frame, Rect, Style, Terminal};
use retroglyph_examples::Example;
use retroglyph_widgets::{
    BoxStyle, Constraint, Gauge, ListState, Sides, Sparkline, StatefulWidget, Table, Theme, Widget,
    blit_into, split_h, split_v,
};

/// Fixed service list the table displays: `(name, status)`.
const SERVICES: [(&str, &str); 6] = [
    ("api-gateway", "OK"),
    ("auth", "OK"),
    ("billing", "WARN"),
    ("search", "OK"),
    ("worker-queue", "DOWN"),
    ("cache", "OK"),
];

/// Synthetic CPU load history the sparkline graphs, oldest first.
const CPU_HISTORY: [f32; 16] = [
    0.20, 0.24, 0.31, 0.28, 0.35, 0.42, 0.39, 0.55, 0.61, 0.58, 0.47, 0.52, 0.60, 0.66, 0.62, 0.58,
];

/// State for the dashboard example: which table row is selected, and the two gauge ratios.
pub struct Dashboard {
    theme: Theme,
    table_state: ListState,
    cpu: f32,
    mem: f32,
}

impl Default for Dashboard {
    fn default() -> Self {
        let mut table_state = ListState::new();
        table_state.select(Some(0));
        Self {
            theme: Theme::DARK,
            table_state,
            cpu: 0.58,
            mem: 0.41,
        }
    }
}

impl Dashboard {
    /// Drains pending input: Up/Down move the table selection; `q`/`Escape` quits.
    ///
    /// Gated on [`KeyEvent::is_down`](retroglyph_core::event::KeyEvent::is_down) -- see
    /// `07_sprites_tileset.rs`'s `handle_events` doc comment for why: without it, a backend that
    /// reports both press and release as separate events would move the selection twice per key
    /// tap.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if key.is_down() => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Down => self.table_state.select_next(SERVICES.len()),
                    KeyCode::Up => self.table_state.select_previous(SERVICES.len()),
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws this frame and presents it.
    fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let area = Rect::new(0, 0, 50, 25);
        let rows = split_v(area, &[Constraint::Fixed(1), Constraint::Fill]);
        let (title_area, body_area) = (rows[0], rows[1]);

        term.reset_style().fg(self.theme.accent);
        term.print(
            title_area.left() + 1,
            title_area.top(),
            "retroglyph dashboard -- Up/Down selects, q/Escape quits",
        );
        term.reset_style();

        let cols = split_h(body_area, &[Constraint::Percent(60), Constraint::Fill]);
        let (left, right) = (cols[0], cols[1]);

        let headers = ["Service", "Status"];
        let widths = [18u16, 8u16];
        let table_rows: Vec<Vec<String>> = SERVICES
            .iter()
            .map(|&(name, status)| vec![name.to_owned(), status.to_owned()])
            .collect();
        Table::new(&headers, &widths, &table_rows).render(left, term, &mut self.table_state);

        let right_rows = split_v(
            right,
            &[
                Constraint::Fixed(1),
                Constraint::Fixed(1),
                Constraint::Fixed(1),
                Constraint::Fixed(1),
                Constraint::Fill,
            ],
        );
        Gauge::new("CPU", self.cpu).render(right_rows[0], term);
        Gauge::new("MEM", self.mem).render(right_rows[1], term);
        term.reset_style().fg(self.theme.dim);
        term.print(right_rows[2].left(), right_rows[2].top(), "History:");
        term.reset_style();
        Sparkline::new(&CPU_HISTORY).render(right_rows[3], term);

        let legend = BoxStyle::new(Style::new().fg(self.theme.fg).bg(self.theme.panel_bg))
            .padding(Sides::symmetric(0, 1))
            .border(true)
            .render("Legend\nOK / WARN / DOWN");
        blit_into(term, &legend, right_rows[4].left(), right_rows[4].top());

        term.present().ok();
    }
}

impl Example for Dashboard {
    const NAME: &'static str = "09_widgets_dashboard";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(Dashboard);
