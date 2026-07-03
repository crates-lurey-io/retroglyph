//! `bashtop`-style system-monitor demo (simulated data).
//!
//! A busy, full-screen, time-driven dashboard: labeled CPU gauges, an aggregate
//! CPU sparkline, a memory gauge, scrolling network sparklines, and a process
//! table with keyboard selection. All metrics are faked from a seeded LCG so the
//! demo is dep-free and deterministic per seed.
//!
//! This is the second UI-heavy demo (after the scrolling roguelike). Its real
//! job is to answer a design question — do immediate-mode draw functions carry a
//! multi-panel UI, or does a `Widget` trait earn its keep? It also exercises the
//! new [`split_v`]/[`split_h`] layout splitter, [`FrameClock`] as the first
//! genuinely wall-clock-driven demo, and cell-diff cost under a full redraw.
//!
//! [`split_v`]: util::layout::split_v
//! [`split_h`]: util::layout::split_h
//!
//! # Controls
//!
//! - Up / Down (or W/S) — move the process-table selection
//! - Q / Escape — quit
//!
//! # Run
//!
//! ```sh
//! cargo run --example dashboard --features crossterm
//! cargo run --example dashboard --features software-default-font
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

mod util;

use std::collections::VecDeque;

use retroglyph::{Backend, Color, FrameClock, Rect, Style, Terminal};
use util::action::{Action, event_to_action};
use util::draw::{gauge, panel, print_line, sparkline, table};
use util::layout::{Constraint, split_h, split_v};
use util::lcg::Lcg;
use util::timestep::Stopwatch;

// ── Tuning ────────────────────────────────────────────────────────────────────

const SIM_HZ: u32 = 8;
const CORES: usize = 4;
const HISTORY: usize = 120;
/// Crossterm blocks up to this long in `poll`, which paces the loop (~16 fps)
/// without a busy-loop. The software backend's `poll` is non-blocking, so
/// `requestAnimationFrame` / vsync paces it instead.
const POLL_MS: u64 = 60;

// ── Colors ──────────────────────────────────────────────────────────────────

const BG: Color = Color::Rgb {
    r: 16,
    g: 16,
    b: 24,
};
const PANEL_BG: Color = Color::Rgb {
    r: 22,
    g: 22,
    b: 32,
};
const BORDER: Color = Color::Rgb {
    r: 70,
    g: 74,
    b: 96,
};
const TITLE_BG: Color = Color::Rgb {
    r: 30,
    g: 32,
    b: 48,
};
const FG: Color = Color::Rgb {
    r: 190,
    g: 192,
    b: 208,
};

// ── State ─────────────────────────────────────────────────────────────────────

struct Proc {
    pid: u16,
    name: &'static str,
    cpu: f32,
    mem: f32,
}

const PROC_NAMES: &[&str] = &[
    "kernel_task",
    "WindowServer",
    "retroglyph",
    "cargo",
    "rust-analyzer",
    "zsh",
    "ghostty",
    "firefox",
    "Spotify",
    "node",
    "docker",
    "postgres",
];

struct Dashboard {
    clock: FrameClock,
    watch: Stopwatch,
    cpu: [f32; CORES],
    cpu_history: VecDeque<f32>,
    mem_used: f32,
    net_rx: VecDeque<f32>,
    net_tx: VecDeque<f32>,
    procs: Vec<Proc>,
    selected: usize,
    rng: Lcg,
}

impl Dashboard {
    fn new<B: Backend>(_term: &mut Terminal<B>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut rng = Lcg::from_time();
        #[cfg(target_arch = "wasm32")]
        let mut rng = Lcg::new(0x00C0_FFEE);

        let procs = PROC_NAMES
            .iter()
            .enumerate()
            .map(|(i, &name)| Proc {
                #[allow(clippy::cast_possible_truncation)]
                pid: 100 + i as u16 * 7,
                name,
                cpu: frand(&mut rng),
                mem: frand(&mut rng) * 0.4,
            })
            .collect();

        let mut d = Self {
            clock: FrameClock::new(SIM_HZ),
            watch: Stopwatch::new(),
            cpu: [0.3; CORES],
            cpu_history: VecDeque::with_capacity(HISTORY),
            mem_used: 0.4,
            net_rx: VecDeque::with_capacity(HISTORY),
            net_tx: VecDeque::with_capacity(HISTORY),
            procs,
            selected: 0,
            rng,
        };
        // Pre-fill history so the sparklines aren't empty on the first frame.
        for _ in 0..HISTORY {
            d.simulate();
        }
        d
    }

    /// Advance the simulation by one fixed logic step.
    fn simulate(&mut self) {
        for c in &mut self.cpu {
            *c = walk(&mut self.rng, *c, 0.18);
        }
        #[allow(clippy::cast_precision_loss)]
        let avg = self.cpu.iter().sum::<f32>() / CORES as f32;
        push_capped(&mut self.cpu_history, avg);

        // Memory drifts slowly.
        self.mem_used = walk(&mut self.rng, self.mem_used, 0.04);

        // Network is bursty: usually low, occasional spikes.
        push_capped(&mut self.net_rx, burst(&mut self.rng, 0.15));
        push_capped(&mut self.net_tx, burst(&mut self.rng, 0.10));

        // Jitter process metrics, then re-sort hottest-first.
        for p in &mut self.procs {
            p.cpu = walk(&mut self.rng, p.cpu, 0.25);
            p.mem = walk(&mut self.rng, p.mem, 0.05);
        }
        self.procs.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Keep the selection within the (stable-length) list.
        self.selected = self.selected.min(self.procs.len().saturating_sub(1));
    }

    const fn move_selection(&mut self, down: bool) {
        let n = self.procs.len();
        if n == 0 {
            return;
        }
        self.selected = if down {
            (self.selected + 1) % n
        } else {
            (self.selected + n - 1) % n
        };
    }
}

// ── Simulation helpers ──────────────────────────────────────────────────────

/// Uniform random `f32` in `0.0..1.0`.
fn frand(rng: &mut Lcg) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        (rng.next() % 10_000) as f32 / 10_000.0
    }
}

/// One clamped random-walk step of magnitude `mag` around `v`.
fn walk(rng: &mut Lcg, v: f32, mag: f32) -> f32 {
    (frand(rng) - 0.5).mul_add(mag, v).clamp(0.0, 1.0)
}

/// Bursty value: mostly near `base`, with an occasional spike toward 1.0.
fn burst(rng: &mut Lcg, base: f32) -> f32 {
    if frand(rng) < 0.12 {
        frand(rng).mul_add(0.8, base).clamp(0.0, 1.0)
    } else {
        (base * frand(rng) * 1.5).clamp(0.0, 1.0)
    }
}

fn push_capped(buf: &mut VecDeque<f32>, v: f32) {
    if buf.len() == HISTORY {
        buf.pop_front();
    }
    buf.push_back(v);
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw<B: Backend>(term: &mut Terminal<B>, state: &Dashboard) {
    let size = term.size();
    let screen = Rect::new(0, 0, size.width.into(), size.height.into());

    // Background wash so panel gaps use the app background, not the terminal's.
    for y in 0..size.height {
        for x in 0..size.width {
            term.put_styled(x, y, ' ', Style::new().bg(BG));
        }
    }

    let [title, body, footer] = take3(&split_v(
        screen,
        &[Constraint::Fixed(1), Constraint::Fill, Constraint::Fixed(1)],
    ));

    draw_bar(term, title, " retroglyph — system monitor (simulated)");
    draw_bar(
        term,
        footer,
        " Up/Down: select process    Q: quit    (fake data, deterministic per seed)",
    );

    let [left, right] = take2(&split_h(body, &[Constraint::Percent(52), Constraint::Fill]));

    draw_left(term, left, state);
    draw_table(term, right, state);
}

fn draw_left<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &Dashboard) {
    // A trailing Fill pane soaks up leftover height as background wash, so the
    // single-row sparklines don't leave a cavernous empty NET box.
    let [cpu_area, mem_area, net_area, _rest] = take4(&split_v(
        area,
        &[
            Constraint::Fixed(CORES as u16 + 3), // cores + aggregate + borders
            Constraint::Fixed(3),
            Constraint::Fixed(4), // rx + tx sparklines + borders
            Constraint::Fill,
        ],
    ));

    // CPU panel: one gauge per core, then an aggregate sparkline.
    panel_bg(term, cpu_area, "CPU");
    let inner = inset(cpu_area);
    for (i, &load) in state.cpu.iter().enumerate() {
        let row = Rect::new(
            inner.left(),
            inner.top() + i as u16,
            inner.width().into(),
            1,
        );
        let label = format!("c{i}");
        gauge(term, row, &label, load);
    }
    let spark_row = Rect::new(
        inner.left(),
        inner.top() + CORES as u16,
        inner.width().into(),
        1,
    );
    let hist: Vec<f32> = state.cpu_history.iter().copied().collect();
    sparkline(term, spark_row, &hist);

    // Memory gauge.
    panel_bg(term, mem_area, "MEM");
    let minner = inset(mem_area);
    gauge(
        term,
        Rect::new(minner.left(), minner.top(), minner.width().into(), 1),
        "used",
        state.mem_used,
    );

    // Network sparklines (rx over tx).
    panel_bg(term, net_area, "NET");
    let ninner = inset(net_area);
    if ninner.height() >= 2 {
        let rx: Vec<f32> = state.net_rx.iter().copied().collect();
        let tx: Vec<f32> = state.net_tx.iter().copied().collect();
        label(term, ninner.left(), ninner.top(), "rx");
        sparkline(
            term,
            Rect::new(
                ninner.left() + 3,
                ninner.top(),
                ninner.width().saturating_sub(3).into(),
                1,
            ),
            &rx,
        );
        label(term, ninner.left(), ninner.top() + 1, "tx");
        sparkline(
            term,
            Rect::new(
                ninner.left() + 3,
                ninner.top() + 1,
                ninner.width().saturating_sub(3).into(),
                1,
            ),
            &tx,
        );
    }
}

fn draw_table<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &Dashboard) {
    panel_bg(term, area, "PROCESSES");
    let inner = inset(area);
    let headers = ["PID", "NAME", "CPU%", "MEM%"];
    let widths = [5, 14, 5, 5];
    let rows: Vec<Vec<String>> = state
        .procs
        .iter()
        .map(|p| {
            vec![
                p.pid.to_string(),
                p.name.to_string(),
                format!("{:.0}", p.cpu * 100.0),
                format!("{:.0}", p.mem * 100.0),
            ]
        })
        .collect();
    table(term, inner, &headers, &widths, &rows, state.selected);
}

// ── Small drawing helpers ─────────────────────────────────────────────────────

/// Draw a bordered panel filled with the app panel background.
fn panel_bg<B: Backend>(term: &mut Terminal<B>, area: Rect, title: &str) {
    if area.width() < 2 || area.height() < 2 {
        return;
    }
    panel(
        term,
        area,
        Some(title),
        Style::new().fg(BORDER).bg(BG),
        Style::new().bg(PANEL_BG),
    );
}

/// The interior of a panel (inside its one-cell border).
fn inset(area: Rect) -> Rect {
    Rect::new(
        area.left() + 1,
        area.top() + 1,
        area.width().saturating_sub(2).into(),
        area.height().saturating_sub(2).into(),
    )
}

/// Fill a one-row bar with `text` on the title-bar background.
fn draw_bar<B: Backend>(term: &mut Terminal<B>, area: Rect, text: &str) {
    let y = area.top();
    for x in area.left()..area.right() {
        term.put_styled(x, y, ' ', Style::new().bg(TITLE_BG));
    }
    let line = retroglyph::text::Line::from(retroglyph::text::Span::styled(
        text,
        Style::new().fg(FG).bg(TITLE_BG),
    ));
    print_line(term, area.top_left(), &line, area.width());
}

fn label<B: Backend>(term: &mut Terminal<B>, x: u16, y: u16, text: &str) {
    for (cx, ch) in (x..).zip(text.chars()) {
        term.put_styled(cx, y, ch, Style::new().fg(FG).bg(PANEL_BG));
    }
}

/// Pull a fixed-size array out of a splitter result (panics if too short — the
/// splitter always returns one rect per constraint, so this is infallible here).
fn take3(v: &[Rect]) -> [Rect; 3] {
    [v[0], v[1], v[2]]
}

fn take2(v: &[Rect]) -> [Rect; 2] {
    [v[0], v[1]]
}

fn take4(v: &[Rect]) -> [Rect; 4] {
    [v[0], v[1], v[2], v[3]]
}

// ── Loop ────────────────────────────────────────────────────────────────────

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut Dashboard) -> bool {
    // Feed real elapsed time into the fixed-timestep clock, then drain whole
    // 8 Hz logic steps. This keeps the sim cadence identical on every backend
    // regardless of render rate.
    state.clock.advance(state.watch.lap());
    while state.clock.tick() {
        state.simulate();
    }

    draw(term, state);
    term.present().expect("present failed");

    // Pace the loop via poll's timeout. Crossterm blocks up to POLL_MS (no busy
    // loop); the software backend returns immediately and lets rAF/vsync pace.
    if let Some(event) = term.poll(std::time::Duration::from_millis(POLL_MS)) {
        if !handle(state, &event) {
            return false;
        }
        for event in term.drain_events() {
            if !handle(state, &event) {
                return false;
            }
        }
    }
    true
}

/// Apply one input event. Returns `false` to quit.
fn handle(state: &mut Dashboard, event: &retroglyph::event::Event) -> bool {
    match event_to_action(event) {
        Action::MoveUp => state.move_selection(false),
        Action::MoveDown => state.move_selection(true),
        Action::Quit => return false,
        _ => {}
    }
    true
}

rg_run!(Dashboard, Dashboard::new, tick);

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph::backend::Headless;

    /// Render one frame headlessly and assert the panels and title landed where
    /// the layout splitter should place them. Doubles as a smoke test that the
    /// widgets draw without panicking at this size.
    #[test]
    fn renders_panels_headless() {
        let mut term = Terminal::new(Headless::new(80, 30));
        let mut d = Dashboard::new(&mut term);
        draw(&mut term, &d);
        term.present().unwrap();
        let view = term.backend().format_view();
        let lines: Vec<&str> = view.lines().collect();

        // Title bar on row 0, footer hint on the last row. (format_view renders
        // spaces as '·', so match single words only.)
        assert!(lines[0].contains("monitor"));
        assert!(lines[29].contains("quit"));
        // All three left-column panels and the table are titled.
        assert!(view.contains("CPU"));
        assert!(view.contains("MEM"));
        assert!(view.contains("NET"));
        assert!(view.contains("PROCESSES"));
        // Selection stays in range as the sim reshuffles rows.
        for _ in 0..HISTORY {
            d.simulate();
        }
        assert!(d.selected < d.procs.len());
    }
}
