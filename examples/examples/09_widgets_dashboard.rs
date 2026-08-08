//! 09: Widgets dashboard
//!
//! A `retroglyph-ui` showcase covering every widget in the crate: [`Table`] (a scrollable
//! service list with a [`ListState`]-driven highlighted row), [`Tabs`] (switches the right panel
//! between "Metrics" and "Alerts"), [`List`] (the Alerts panel, its own [`ListState`]-driven
//! highlighted item), [`Button`] (a "Ping" button on the Metrics panel, styled from an
//! [`Interaction`]-resolved `Response` the same way `10_widgets_interaction` demonstrates in
//! more depth), [`Gauge`] (load-colored bars for CPU/memory), [`Sparkline`] (a recent-history
//! graph), [`BoxStyle`] (a bordered legend box, rendered into a standalone [`Grid`] and blitted
//! in), [`split_h`]/[`split_v`] (the whole layout), [`Ui`] (pairing the frame's surface with
//! `Interaction`, via [`Interaction::frame`]), and [`Theme`] (every color in this example comes
//! from [`Theme::DARK`], not a hand-picked one-off). `retroglyph-ui` is backend-generic:
//! nothing here is software, crossterm, or headless specific.
//!
//! ```sh
//! cargo run --example 09_widgets_dashboard --features crossterm
//! cargo run --example 09_widgets_dashboard --features software
//! cargo run --example 09_widgets_dashboard  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: Left/Right switches the active tab. Up/Down moves whichever list the active tab shows
//! (the service table on "Metrics", the alert list on "Alerts"). On the Metrics tab, click (or
//! Tab to focus, then Enter/Space) the "Ping" button. `q` or `Escape` quits, or close the
//! window.

use retroglyph::app::Frame;
use retroglyph::backend::Backend;
use retroglyph::color::Style;
use retroglyph::event::{Event, KeyCode};
use retroglyph::grid::Rect;
use retroglyph::terminal::Terminal;
use retroglyph::ui::interact::Interaction;
use retroglyph::ui::layout::{Constraint, split_h, split_v};
use retroglyph::ui::state::ListState;
use retroglyph::ui::style::{BoxStyle, Sides};
use retroglyph::ui::theme::Theme;
use retroglyph::ui::ui::Ui;
use retroglyph::ui::widget::{Button, Gauge, List, Sparkline, Table, Tabs};
use retroglyph_examples::Example;

/// Identifies the dashboard's one interactive widget for [`Interaction`]'s hit-testing and focus
/// ring -- see `10_widgets_interaction` for a fuller demonstration of the same machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashId {
    PingButton,
}

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

/// The right panel's tab strip: index 0 shows the metrics/gauges panel, index 1 the alert list.
const TABS: [&str; 2] = ["Metrics", "Alerts"];

/// Fixed alert log the Alerts tab's [`List`] displays, newest last.
const ALERTS: [&str; 5] = [
    "cache: latency p99 above 200ms",
    "worker-queue: health check failed",
    "billing: retrying webhook delivery",
    "auth: certificate renewed",
    "search: reindex completed",
];

/// State for the dashboard example: which table row/alert/tab is selected, the two gauge ratios,
/// the [`Button`] interaction context, and how many times "Ping" has been clicked.
pub struct Dashboard {
    theme: Theme,
    table_state: ListState,
    alerts_state: ListState,
    selected_tab: usize,
    cpu: f32,
    mem: f32,
    interaction: Interaction<DashId>,
    pings: u32,
}

impl Default for Dashboard {
    fn default() -> Self {
        let mut table_state = ListState::new();
        table_state.select(Some(0));
        Self {
            theme: Theme::DARK,
            table_state,
            alerts_state: ListState::new(),
            selected_tab: 0,
            cpu: 0.58,
            mem: 0.41,
            interaction: Interaction::new(),
            pings: 0,
        }
    }
}

/// Handles one already-drained event: Left/Right switch the active tab; Up/Down move whichever
/// list the active tab shows; `q`/`Escape` quits.
///
/// Gated on [`KeyEvent::is_down`](retroglyph::event::KeyEvent::is_down) -- see
/// `07_sprites_tileset.rs`'s `handle_events` doc comment for why: without it, a backend that
/// reports both press and release as separate events would move the selection twice per key tap.
fn handle_event(
    event: &Event,
    selected_tab: &mut usize,
    table_state: &mut ListState,
    alerts_state: &mut ListState,
) -> bool {
    match event {
        Event::Key(key) if key.is_down() => match key.code {
            KeyCode::Char('q') | KeyCode::Escape => return false,
            KeyCode::Left => *selected_tab = selected_tab.saturating_sub(1),
            KeyCode::Right => *selected_tab = (*selected_tab + 1).min(TABS.len() - 1),
            KeyCode::Down if *selected_tab == 0 => table_state.select_next(SERVICES.len()),
            KeyCode::Up if *selected_tab == 0 => table_state.select_previous(SERVICES.len()),
            KeyCode::Down if *selected_tab == 1 => alerts_state.select_next(ALERTS.len()),
            KeyCode::Up if *selected_tab == 1 => alerts_state.select_previous(ALERTS.len()),
            _ => {}
        },
        Event::Close => return false,
        _ => {}
    }
    true
}

/// Draws the "Metrics" tab's content: CPU/MEM gauges, a recent-history sparkline, the status
/// legend, and a "Ping" [`Button`] -- the whole right panel before [`Tabs`]/[`List`] were added,
/// plus the [`Button`] this dashboard now also showcases.
fn draw_metrics(
    ui: &mut Ui<'_, '_, DashId>,
    area: Rect,
    theme: Theme,
    cpu: f32,
    mem: f32,
    pings: &mut u32,
) {
    let rows = split_v(
        area,
        &[
            Constraint::Fixed(1),
            Constraint::Fixed(1),
            Constraint::Fixed(1),
            Constraint::Fixed(1),
            Constraint::Fill(1),
        ],
    );
    ui.draw(rows[0], &Gauge::new("CPU", cpu));
    ui.draw(rows[1], &Gauge::new("MEM", mem));
    let history_style = Style::new().fg(theme.dim);
    ui.surface()
        .print((rows[2].left(), rows[2].top()), "History:", history_style);
    ui.draw(rows[3], &Sparkline::new(&CPU_HISTORY));

    let legend = BoxStyle::new(Style::new().fg(theme.fg).bg(theme.panel_bg))
        .padding(Sides::symmetric(0, 1))
        .border(true)
        .render("Legend\nOK / WARN / DOWN");
    ui.surface().blit(&legend, rows[4].left(), rows[4].top());

    draw_ping_button(
        ui,
        Rect::new(rows[4].left(), rows[4].top() + 5, 8, 1),
        theme,
        pings,
    );
}

/// Draws the "Ping" [`Button`] and applies its click to `pings`. `ui.show` resolves the click
/// and draws the button from the one `rect`; the caller only needs `response.clicked()` for the
/// counter below -- see `10_widgets_interaction`'s `draw_button` for the same pattern applied to
/// three buttons.
fn draw_ping_button(ui: &mut Ui<'_, '_, DashId>, rect: Rect, theme: Theme, pings: &mut u32) {
    let button = Button::new("Ping")
        .style(Style::new().fg(theme.fg).bg(theme.panel_bg))
        .hovered_style(Style::new().fg(theme.fg).bg(theme.hover_bg))
        .pressed_style(Style::new().fg(theme.fg).bg(theme.press_bg))
        .focused_style(Style::new().fg(theme.accent).bg(theme.panel_bg));
    if ui.show(rect, DashId::PingButton, &button).clicked() {
        *pings += 1;
    }

    let pings_style = Style::new().fg(theme.dim);
    ui.surface().print(
        (rect.left() + rect.width() + 1, rect.top()),
        &format!("Pings: {pings}"),
        pings_style,
    );
}

/// Draws one frame.
#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut Ui<'_, '_, DashId>,
    theme: Theme,
    table_state: &mut ListState,
    alerts_state: &mut ListState,
    selected_tab: usize,
    cpu: f32,
    mem: f32,
    pings: &mut u32,
) {
    let area = Rect::new(0, 0, 50, 25);
    let rows = split_v(area, &[Constraint::Fixed(1), Constraint::Fill(1)]);
    let (title_area, body_area) = (rows[0], rows[1]);

    let title_style = Style::new().fg(theme.accent);
    ui.surface().print(
        (title_area.left() + 1, title_area.top()),
        "retroglyph dashboard -- tabs/select, q/Esc quits",
        title_style,
    );

    let cols = split_h(body_area, &[Constraint::Percent(60), Constraint::Fill(1)]);
    let (left, right) = (cols[0], cols[1]);

    let headers = ["Service", "Status"];
    let widths = [18u16, 8u16];
    let table_rows: Vec<[&str; 2]> = SERVICES
        .iter()
        .map(|&(name, status)| <[&str; 2]>::from((name, status)))
        .collect();
    let table_rows: Vec<&[&str]> = table_rows.iter().map(<[&str; 2]>::as_slice).collect();
    ui.draw_stateful(
        left,
        &Table::new(&headers, &widths, &table_rows),
        table_state,
    );

    let right_rows = split_v(
        right,
        &[
            Constraint::Fixed(1),
            Constraint::Fixed(1),
            Constraint::Fill(1),
        ],
    );
    let (tabs_area, panel_area) = (right_rows[0], right_rows[2]);

    let tabs = Tabs::new(&TABS)
        .select(Some(selected_tab))
        .style(Style::new().fg(theme.dim))
        .selected_style(Style::new().fg(theme.accent).bg(theme.panel_bg));
    ui.draw(tabs_area, &tabs);

    if selected_tab == 0 {
        draw_metrics(ui, panel_area, theme, cpu, mem, pings);
    } else {
        let list = List::new(&ALERTS)
            .item_style(Style::new().fg(theme.fg))
            .selected_style(Style::new().fg(theme.bg).bg(theme.accent));
        ui.draw_stateful(panel_area, &list, alerts_state);
    }
}

impl Example for Dashboard {
    const NAME: &'static str = "09_widgets_dashboard";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> bool {
        let events: Vec<Event> = term.drain_events().collect();
        let mut surface = term.surface();
        let Self {
            interaction,
            theme,
            table_state,
            alerts_state,
            selected_tab,
            cpu,
            mem,
            pings,
        } = self;

        let mut keep_going = true;
        interaction.frame(&mut surface, |ui| {
            for event in &events {
                let _ = ui.interaction().handle_event(event);
                if !handle_event(event, selected_tab, table_state, alerts_state) {
                    keep_going = false;
                }
            }
            if keep_going {
                draw(
                    ui,
                    *theme,
                    table_state,
                    alerts_state,
                    *selected_tab,
                    *cpu,
                    *mem,
                    pings,
                );
            }
        });
        keep_going
    }
}

retroglyph_examples::example_main!(Dashboard);
