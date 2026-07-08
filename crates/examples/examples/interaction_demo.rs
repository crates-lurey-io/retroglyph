//! Focus/hit-test/pointer demo for `retroglyph-widgets`'s `interact` module.
//!
//! A small control panel (buttons, a toggle, a drag slider) next to a
//! scrollable track list and a live event log. Every interactive element
//! goes through a single [`Interaction<Id>`] instead of the hand-rolled
//! `PointerState` + `hitboxes: Vec<(Rect, HitTarget)>` + manual tap/drag
//! math `responsive_game_ui` uses: [`HitTester`] replaces the manual hitbox
//! vec and reverse-scan, [`FocusRing`] replaces nothing (that example has no
//! keyboard focus concept at all), and [`Response`] replaces the ad hoc
//! per-target `match` blocks in that example's `handle_action`.
//!
//! What each control demonstrates:
//!
//! - **New / Save / Delete** buttons ([`Sense::click`]) — hover, press, and
//!   click, plus keyboard activation: they're also operable with Tab to
//!   focus and Enter/Space to activate, with no extra code.
//! - **Mute** toggle — the same [`Sense::click`] shape applied to a
//!   different visual (a checkbox instead of a bracketed label).
//! - **Volume** slider ([`Sense::drag`]) — a drag gesture past the
//!   threshold, read back via [`Interaction::pointer`]'s live position.
//! - **Track list** — [`Sense::scroll`] on the container feeds
//!   [`Response::scroll_delta`] straight into
//!   [`ListState::scroll_by`], while each row senses hover/click without
//!   being individually Tab-focusable (`Sense::HOVER | Sense::CLICK`,
//!   skipping [`Sense::FOCUSABLE`]) — a container can hold focus while its
//!   children stay mouse-only.
//! - The **Focused**/**Pointer** debug lines and the event log read
//!   [`Interaction::focus`]/[`Interaction::pointer`] directly, and the
//!   arrow keys change meaning based on which id currently has focus (Up/
//!   Down move the list selection only while the list is focused; Left/
//!   Right nudge the volume only while the slider is focused) — the same
//!   focus state gates both mouse and keyboard routing.
//!
//! Every `Response` here is resolved one frame after the input that
//! produced it, per [`Interaction`]'s documented frame lifecycle — at this
//! poll rate that's imperceptible, but it's why `tick` below calls
//! `begin_frame`/`handle_event`/`draw`/`end_frame` in exactly that order.
//!
//! [`Interaction<Id>`]: retroglyph_widgets::Interaction
//! [`Interaction`]: retroglyph_widgets::Interaction
//! [`Interaction::focus`]: retroglyph_widgets::Interaction::focus
//! [`Interaction::pointer`]: retroglyph_widgets::Interaction::pointer
//! [`HitTester`]: retroglyph_widgets::HitTester
//! [`FocusRing`]: retroglyph_widgets::FocusRing
//! [`Response`]: retroglyph_widgets::Response
//! [`Response::scroll_delta`]: retroglyph_widgets::Response::scroll_delta
//! [`Sense::click`]: retroglyph_widgets::Sense::click
//! [`Sense::drag`]: retroglyph_widgets::Sense::drag
//! [`Sense::scroll`]: retroglyph_widgets::Sense::scroll
//! [`Sense::FOCUSABLE`]: retroglyph_widgets::Sense::FOCUSABLE
//! [`ListState::scroll_by`]: retroglyph_widgets::ListState::scroll_by
//!
//! # Controls
//!
//! - Tab / Shift+Tab — move focus between New, Save, Delete, Mute, Volume,
//!   and the track list
//! - Enter / Space — activate the focused button/toggle
//! - Up / Down — move the track selection, while the list is focused
//! - Left / Right — nudge the volume, while the slider is focused
//! - Mouse: click buttons/rows, drag the volume bar, scroll the track list
//! - Q / Escape — quit
//!
//! # Run
//!
//! ```sh
//! cargo run --example interaction_demo --features crossterm
//! cargo run --example interaction_demo --features software-default-font
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::collections::VecDeque;
use std::time::Duration;

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Color, Line, Rect, Size, Span, Style, Terminal};
use retroglyph_widgets::{
    Constraint, Interaction, ListState, Response, Sense, gauge, log, panel, split_h, split_v,
};

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
const ACCENT: Color = Color::Rgb {
    r: 90,
    g: 170,
    b: 250,
};
const HOVER_BG: Color = Color::Rgb {
    r: 40,
    g: 44,
    b: 64,
};
const PRESS_BG: Color = Color::Rgb {
    r: 60,
    g: 110,
    b: 170,
};
const DIM: Color = Color::Rgb {
    r: 110,
    g: 112,
    b: 130,
};

// ── Data ──────────────────────────────────────────────────────────────────────

struct Track {
    name: &'static str,
    duration: &'static str,
}

// More entries than fit in most terminal heights, so the track list actually
// demonstrates ListState-driven scrolling (both via mouse wheel and
// Up/Down), not just a static, always-fully-visible list.
const TRACKS: &[Track] = &[
    Track {
        name: "Sunrise Over Static",
        duration: "3:41",
    },
    Track {
        name: "Glass Corridor",
        duration: "4:12",
    },
    Track {
        name: "Nine Tenths Silence",
        duration: "2:58",
    },
    Track {
        name: "Low Orbit",
        duration: "5:03",
    },
    Track {
        name: "Paper Weather",
        duration: "3:27",
    },
    Track {
        name: "Vacant Frequencies",
        duration: "4:45",
    },
    Track {
        name: "Halflight",
        duration: "3:12",
    },
    Track {
        name: "The Long Answer",
        duration: "6:08",
    },
    Track {
        name: "Slow Static",
        duration: "3:55",
    },
    Track {
        name: "Nocturne for Modems",
        duration: "4:33",
    },
    Track {
        name: "Rust and Ceremony",
        duration: "2:41",
    },
    Track {
        name: "Everything, Eventually",
        duration: "5:19",
    },
    Track {
        name: "Fog Index",
        duration: "3:03",
    },
    Track {
        name: "Terminal Velocity",
        duration: "4:01",
    },
    Track {
        name: "Copper Wire Lullaby",
        duration: "3:38",
    },
    Track {
        name: "Departure Gate Four",
        duration: "4:27",
    },
    Track {
        name: "Salt Marsh Radio",
        duration: "2:52",
    },
    Track {
        name: "Undertow",
        duration: "5:41",
    },
    Track {
        name: "Between Two Clocks",
        duration: "3:16",
    },
    Track {
        name: "A Room With No Exit",
        duration: "4:09",
    },
    Track {
        name: "Faint Signal",
        duration: "2:35",
    },
    Track {
        name: "Winter Circuit",
        duration: "5:57",
    },
    Track {
        name: "The Last Elevator",
        duration: "3:44",
    },
    Track {
        name: "Static Bloom",
        duration: "4:18",
    },
    Track {
        name: "Origami Weather",
        duration: "3:02",
    },
    Track {
        name: "Blue Hour Traffic",
        duration: "4:52",
    },
    Track {
        name: "Nothing But Weather",
        duration: "2:47",
    },
    Track {
        name: "Empty Platform",
        duration: "5:11",
    },
    Track {
        name: "The Long Way Home",
        duration: "3:29",
    },
    Track {
        name: "Analog Ghost",
        duration: "4:36",
    },
    Track {
        name: "Loose Change",
        duration: "2:58",
    },
    Track {
        name: "Perimeter Walk",
        duration: "5:04",
    },
    Track {
        name: "Halfway to Nowhere",
        duration: "3:21",
    },
    Track {
        name: "The Quiet Engine",
        duration: "4:44",
    },
    Track {
        name: "Overcast Anthem",
        duration: "3:07",
    },
    Track {
        name: "Borrowed Time",
        duration: "5:26",
    },
    Track {
        name: "Signal to Noise",
        duration: "2:39",
    },
    Track {
        name: "Late Checkout",
        duration: "4:15",
    },
    Track {
        name: "Paper Boats",
        duration: "3:53",
    },
    Track {
        name: "Interference Pattern",
        duration: "5:33",
    },
    Track {
        name: "Afterimage",
        duration: "2:44",
    },
    Track {
        name: "The Waiting Room",
        duration: "4:08",
    },
    Track {
        name: "Common Ground",
        duration: "3:31",
    },
    Track {
        name: "Distant Static",
        duration: "5:02",
    },
];

const LOG_CAPACITY: usize = 8;
const CONTROLS_WIDTH: u16 = 30;
const LOG_HEIGHT: u16 = 10;
const POLL_MS: u64 = 60;

// ── Widget identity ─────────────────────────────────────────────────────────

/// Every interactive element's id, shared by [`HitTester`](retroglyph_widgets::HitTester)
/// and [`FocusRing`](retroglyph_widgets::FocusRing) via [`Interaction`](retroglyph_widgets::Interaction).
///
/// An exhaustive, app-owned enum rather than a hash: `Track(usize)` carries
/// its row index directly, so `{:?}`-printing an id (or matching on it in
/// `handle`/`draw`) always says exactly which widget it is, with no
/// possibility of two unrelated widgets colliding onto the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Id {
    New,
    Save,
    Delete,
    Mute,
    Volume,
    TrackList,
    Track(usize),
}

fn id_label(id: Id) -> String {
    match id {
        Id::New => "New".to_owned(),
        Id::Save => "Save".to_owned(),
        Id::Delete => "Delete".to_owned(),
        Id::Mute => "Mute".to_owned(),
        Id::Volume => "Volume".to_owned(),
        Id::TrackList => "Track List".to_owned(),
        Id::Track(i) => TRACKS
            .get(i)
            .map_or_else(|| "Track".to_owned(), |t| format!("Track \"{}\"", t.name)),
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

struct AppState {
    interaction: Interaction<Id>,
    new_count: u32,
    save_count: u32,
    delete_count: u32,
    muted: bool,
    volume: i32,
    list_state: ListState,
    log: VecDeque<Line>,
}

fn init<B: Backend>(_term: &mut Terminal<B>) -> AppState {
    let mut list_state = ListState::new();
    list_state.select_first(TRACKS.len());

    let mut interaction = Interaction::new();
    // Focus the track list by default so Up/Down/scroll work immediately,
    // without requiring a first Tab press -- demonstrates driving focus
    // programmatically via `focus_mut()` rather than only through Tab.
    interaction.focus_mut().request(Id::TrackList);

    AppState {
        interaction,
        new_count: 0,
        save_count: 0,
        delete_count: 0,
        muted: false,
        volume: 65,
        list_state,
        log: VecDeque::with_capacity(LOG_CAPACITY),
    }
}

fn push_log(state: &mut AppState, color: Color, text: String) {
    if state.log.len() == LOG_CAPACITY {
        state.log.pop_front();
    }
    state.log.push_back(Line::from(vec![Span::styled(
        text,
        Style::new().fg(color).bg(PANEL_BG),
    )]));
}

// ── Layout ────────────────────────────────────────────────────────────────────

/// Every rect `draw` needs, computed once from the terminal size. Pure and
/// deterministic (no `state` dependency), so tests recompute the exact same
/// rects `draw` used instead of hardcoding coordinates.
struct Layout {
    title: Rect,
    footer: Rect,
    controls: Rect,
    list: Rect,
    log: Rect,
    new_row: Rect,
    save_row: Rect,
    delete_row: Rect,
    mute_row: Rect,
    volume_row: Rect,
    focused_row: Rect,
    pointer_row: Rect,
}

fn layout(size: Size) -> Layout {
    let screen = Rect::new(0, 0, size.width, size.height);
    let [title, body, footer] = take3(&split_v(
        screen,
        &[Constraint::Fixed(1), Constraint::Fill, Constraint::Fixed(1)],
    ));
    let [controls, right] = take2(&split_h(
        body,
        &[Constraint::Fixed(CONTROLS_WIDTH), Constraint::Fill],
    ));
    let [list, log] = take2(&split_v(
        right,
        &[Constraint::Fill, Constraint::Fixed(LOG_HEIGHT)],
    ));

    let rows = split_v(
        inset(controls),
        &[
            Constraint::Fixed(1), // 0: new
            Constraint::Fixed(1), // 1: save
            Constraint::Fixed(1), // 2: delete
            Constraint::Fixed(1), // 3: spacer
            Constraint::Fixed(1), // 4: mute
            Constraint::Fixed(1), // 5: spacer
            Constraint::Fixed(1), // 6: volume
            Constraint::Fixed(1), // 7: spacer
            Constraint::Fixed(1), // 8: focused debug line
            Constraint::Fixed(1), // 9: pointer debug line
            Constraint::Fill,
        ],
    );

    Layout {
        title,
        footer,
        controls,
        list,
        log,
        new_row: rows[0],
        save_row: rows[1],
        delete_row: rows[2],
        mute_row: rows[4],
        volume_row: rows[6],
        focused_row: rows[8],
        pointer_row: rows[9],
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw<B: Backend>(term: &mut Terminal<B>, state: &mut AppState) {
    let l = layout(term.size());

    for y in 0..term.size().height {
        for x in 0..term.size().width {
            term.put_styled(x, y, ' ', Style::new().bg(BG));
        }
    }

    draw_bar(
        term,
        l.title,
        " retroglyph — interaction demo (hover / click / drag / focus / scroll)",
    );
    draw_bar(
        term,
        l.footer,
        " Tab/Shift+Tab: focus   Enter/Space: activate   click/drag/scroll: mouse   Q: quit",
    );

    draw_controls(term, &l, state);
    draw_track_list(term, l.list, state);
    draw_event_log(term, l.log, state);
}

fn draw_bar<B: Backend>(term: &mut Terminal<B>, area: Rect, text: &str) {
    let style = Style::new().fg(FG).bg(TITLE_BG);
    fill_row(term, area, style);
    print_at(term, area, text, style);
}

fn draw_controls<B: Backend>(term: &mut Terminal<B>, l: &Layout, state: &mut AppState) {
    panel_bg(term, l.controls, "CONTROLS", false);

    let new_label = format!("New ({})", state.new_count);
    let r = draw_button(term, l.new_row, Id::New, &new_label, state);
    if r.clicked() {
        state.new_count += 1;
        push_log(state, ACCENT, "created a new item".to_owned());
    }

    let save_label = format!("Save ({})", state.save_count);
    let r = draw_button(term, l.save_row, Id::Save, &save_label, state);
    if r.clicked() {
        state.save_count += 1;
        push_log(state, ACCENT, "saved".to_owned());
    }

    let delete_label = format!("Delete ({})", state.delete_count);
    let r = draw_button(term, l.delete_row, Id::Delete, &delete_label, state);
    if r.clicked() {
        state.delete_count += 1;
        push_log(state, Color::RED, "deleted".to_owned());
    }

    let r = draw_toggle(term, l.mute_row, state);
    if r.clicked() {
        state.muted = !state.muted;
        push_log(state, ACCENT, format!("mute -> {}", state.muted));
    }

    let r = draw_volume(term, l.volume_row, state);
    if r.released() {
        push_log(state, ACCENT, format!("volume -> {}%", state.volume));
    }

    let focus_text = state.interaction.focus().focused().map_or_else(
        || "Focused: -".to_owned(),
        |id| format!("Focused: {}", id_label(id)),
    );
    print_at(
        term,
        l.focused_row,
        &focus_text,
        Style::new().fg(DIM).bg(PANEL_BG),
    );

    let pointer_text = state.interaction.pointer().pos().map_or_else(
        || "Pointer: -".to_owned(),
        |p| format!("Pointer: {},{}", p.x, p.y),
    );
    print_at(
        term,
        l.pointer_row,
        &pointer_text,
        Style::new().fg(DIM).bg(PANEL_BG),
    );
}

fn draw_button<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    id: Id,
    label: &str,
    state: &mut AppState,
) -> Response {
    let response = state.interaction.interact(area, id, Sense::click());
    let row = focus_gutter(term, area, response.focused());
    let bg = if response.pressed() {
        PRESS_BG
    } else if response.hovered() {
        HOVER_BG
    } else {
        PANEL_BG
    };
    let style = Style::new().fg(FG).bg(bg);
    fill_row(term, row, style);
    print_at(term, row, &format!("[ {label} ]"), style);
    response
}

fn draw_toggle<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut AppState) -> Response {
    let response = state.interaction.interact(area, Id::Mute, Sense::click());
    let row = focus_gutter(term, area, response.focused());
    let bg = if response.pressed() {
        PRESS_BG
    } else if response.hovered() {
        HOVER_BG
    } else {
        PANEL_BG
    };
    let style = Style::new().fg(FG).bg(bg);
    fill_row(term, row, style);
    let mark = if state.muted { "[x]" } else { "[ ]" };
    print_at(term, row, &format!("{mark} Mute"), style);
    response
}

/// Drag (or click-and-hold) anywhere on the bar to set the volume
/// proportionally to the pointer's x position, like a seek bar.
fn draw_volume<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut AppState) -> Response {
    let response = state.interaction.interact(area, Id::Volume, Sense::drag());
    let row = focus_gutter(term, area, response.focused());

    if (response.pressed() || response.dragging())
        && let Some(pos) = state.interaction.pointer().pos()
    {
        let rel = pos.x.saturating_sub(row.left());
        let ratio = f32::from(rel) / f32::from(row.width().max(1));
        state.volume = (ratio.clamp(0.0, 1.0) * 100.0).round() as i32;
    }

    gauge(term, row, "vol", state.volume as f32 / 100.0);
    response
}

fn draw_track_list<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut AppState) {
    let focused = state.interaction.focus().is_focused(Id::TrackList);
    panel_bg(term, area, "TRACKS", focused);
    let inner = inset(area);
    if inner.width() == 0 || inner.height() == 0 {
        return;
    }

    // Sensing the container with FOCUSABLE (Tab-reachable) but each row
    // below with only HOVER | CLICK (not FOCUSABLE): a container can hold
    // keyboard focus while its children stay mouse/tap-only, without every
    // row cluttering the Tab order.
    let list_response =
        state
            .interaction
            .interact(inner, Id::TrackList, Sense::scroll() | Sense::FOCUSABLE);
    let visible_rows = inner.height() as usize;
    if list_response.scroll_delta() == 0 {
        // Only auto-follow the selection (e.g. after Up/Down or a row
        // click) on frames where the wheel didn't just move the view --
        // ensure_visible is idempotent and safe to call every such frame,
        // but calling it unconditionally would immediately snap a free
        // wheel-scroll straight back to the selected row.
        state.list_state.ensure_visible(visible_rows);
    } else {
        state.list_state.scroll_by(list_response.scroll_delta());
    }
    let offset = state.list_state.offset();

    for (i, track) in TRACKS.iter().enumerate().skip(offset).take(visible_rows) {
        let y = inner.top() + (i - offset) as u16;
        let row = Rect::new(inner.left(), y, inner.width(), 1);
        let response = state
            .interaction
            .interact(row, Id::Track(i), Sense::HOVER | Sense::CLICK);
        if response.clicked() {
            state.list_state.select(Some(i));
            // Rows themselves aren't Sense::FOCUSABLE (they'd clutter the Tab
            // order -- see the module docs), so clicking one doesn't
            // automatically focus anything the way a FOCUSABLE widget's own
            // click does. Move focus to the container by hand instead, so
            // Up/Down keep working immediately after a click without
            // requiring an extra Tab press first.
            state.interaction.focus_mut().request(Id::TrackList);
            push_log(state, ACCENT, format!("selected \"{}\"", track.name));
        }

        let selected = state.list_state.selected() == Some(i);
        let bg = if selected {
            PRESS_BG
        } else if response.hovered() {
            HOVER_BG
        } else {
            PANEL_BG
        };
        let style = Style::new().fg(FG).bg(bg);
        fill_row(term, row, style);
        print_at(
            term,
            row,
            &format!("{:<24} {}", track.name, track.duration),
            style,
        );
    }
}

fn draw_event_log<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &AppState) {
    panel_bg(term, area, "EVENTS", false);
    let inner = inset(area);
    let lines: Vec<Line> = state.log.iter().cloned().collect();
    log(term, inner, &lines, 0);
}

// ── Small drawing helpers ─────────────────────────────────────────────────────

fn panel_bg<B: Backend>(term: &mut Terminal<B>, area: Rect, title: &str, focused: bool) {
    if area.width() < 2 || area.height() < 2 {
        return;
    }
    let border = if focused { ACCENT } else { BORDER };
    panel(
        term,
        area,
        Some(title),
        Style::new().fg(border).bg(BG),
        Style::new().bg(PANEL_BG),
    );
}

/// The interior of a panel (inside its one-cell border).
const fn inset(area: Rect) -> Rect {
    Rect::new(
        area.left() + 1,
        area.top() + 1,
        area.width().saturating_sub(2),
        area.height().saturating_sub(2),
    )
}

/// Draws a one-cell focus indicator at `area`'s left edge and returns the
/// remaining rect for the widget's own content -- shared by every
/// [`Sense::FOCUSABLE`] control in the controls panel so focus always reads
/// the same way regardless of what else a widget's [`Response`] changes
/// about its own background.
fn focus_gutter<B: Backend>(term: &mut Terminal<B>, area: Rect, focused: bool) -> Rect {
    let (ch, fg) = if focused {
        ('›', ACCENT)
    } else {
        (' ', PANEL_BG)
    };
    term.put_styled(
        area.left(),
        area.top(),
        ch,
        Style::new().fg(fg).bg(PANEL_BG),
    );
    Rect::new(
        area.left() + 1,
        area.top(),
        area.width().saturating_sub(1),
        area.height(),
    )
}

fn fill_row<B: Backend>(term: &mut Terminal<B>, area: Rect, style: Style) {
    for x in area.left()..area.right() {
        term.put_styled(x, area.top(), ' ', style);
    }
}

fn print_at<B: Backend>(term: &mut Terminal<B>, area: Rect, text: &str, style: Style) {
    for (i, ch) in text.chars().enumerate() {
        let x = area.left() + i as u16;
        if x >= area.right() {
            break;
        }
        term.put_styled(x, area.top(), ch, style);
    }
}

fn take2(v: &[Rect]) -> [Rect; 2] {
    [v[0], v[1]]
}

fn take3(v: &[Rect]) -> [Rect; 3] {
    [v[0], v[1], v[2]]
}

// ── Loop ────────────────────────────────────────────────────────────────────

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut AppState) -> bool {
    // See the `Interaction` frame lifecycle docs: begin_frame (resolve
    // against last frame's registrations) -> handle_event* -> draw
    // (register this frame's) -> end_frame, in that order.
    state.interaction.begin_frame();

    if let Some(event) = term.poll(Duration::from_millis(POLL_MS)) {
        if !handle(state, &event) {
            return false;
        }
        for event in term.drain_events() {
            if !handle(state, &event) {
                return false;
            }
        }
    }

    draw(term, state);
    term.present().expect("present failed");
    state.interaction.end_frame();
    true
}

/// Apply one input event. Returns `false` to quit.
fn handle(state: &mut AppState, event: &Event) -> bool {
    // Arrow keys mean different things depending on what currently holds
    // focus -- the same `FocusRing` state that gates Tab/Enter also gates
    // plain arrow-key routing, so there's exactly one source of truth for
    // "what does input go to right now."
    if let Event::Key(k) = event
        && k.is_down()
    {
        match k.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Escape => return false,
            KeyCode::Up if state.interaction.focus().is_focused(Id::TrackList) => {
                state.list_state.select_previous(TRACKS.len());
            }
            KeyCode::Down if state.interaction.focus().is_focused(Id::TrackList) => {
                state.list_state.select_next(TRACKS.len());
            }
            KeyCode::Left if state.interaction.focus().is_focused(Id::Volume) => {
                state.volume = (state.volume - 5).max(0);
            }
            KeyCode::Right if state.interaction.focus().is_focused(Id::Volume) => {
                state.volume = (state.volume + 5).min(100);
            }
            _ => {}
        }
    }

    let before = state.interaction.focus().focused();
    state.interaction.handle_event(event);
    let after = state.interaction.focus().focused();
    if after != before
        && let Some(id) = after
    {
        push_log(state, DIM, format!("focus -> {}", id_label(id)));
    }

    true
}

retroglyph_examples::rg_run!(AppState, init, tick);

#[cfg(test)]
mod tests {
    use retroglyph_core::event::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use retroglyph_core::{Headless, Pos};

    use super::*;

    const SIZE: Size = Size {
        width: 80,
        height: 30,
    };

    fn init_state() -> AppState {
        let mut term = Terminal::new(Headless::new(SIZE.width, SIZE.height));
        init(&mut term)
    }

    /// Runs one full frame (`begin_frame` -> `handle_event`* -> `draw` ->
    /// `end_frame`), mirroring `tick` exactly. Per [`Interaction`]'s
    /// documented lifecycle, a `Response` resolves against input fed to a
    /// *previous* `frame` call, not the one it's returned from -- so seeing
    /// a click take effect is always: one `frame` to register the target,
    /// one to deliver the click events (not yet visible), one more with no
    /// new events to observe it resolved.
    fn frame(state: &mut AppState, events: &[Event]) {
        state.interaction.begin_frame();
        for event in events {
            handle(state, event);
        }
        let mut term = Terminal::new(Headless::new(SIZE.width, SIZE.height));
        draw(&mut term, state);
        term.present().unwrap();
        state.interaction.end_frame();
    }

    fn mouse_at(kind: MouseEventKind, pos: Pos) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            position: pos,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// Smoke test: renders without panicking and the panel titles land
    /// somewhere on screen.
    #[test]
    fn renders_headless() {
        let mut state = init_state();
        frame(&mut state, &[]);
        let mut term = Terminal::new(Headless::new(SIZE.width, SIZE.height));
        draw(&mut term, &mut state);
        term.present().unwrap();
        let view = term.backend().format_view();
        assert!(view.contains("CONTROLS"));
        assert!(view.contains("TRACKS"));
        assert!(view.contains("EVENTS"));
    }

    #[test]
    fn clicking_new_button_increments_its_counter() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers New's hit rect

        let l = layout(SIZE);
        let pos = Pos::new(l.new_row.left() + 2, l.new_row.top());
        let click = [
            mouse_at(MouseEventKind::Down(MouseButton::Left), pos),
            mouse_at(MouseEventKind::Up(MouseButton::Left), pos),
        ];

        // frame 2: delivers the click; resolves against frame 1's rect, but
        // not until the *next* frame's begin_frame (see the `frame` helper's
        // doc comment) -- not visible yet.
        frame(&mut state, &click);
        assert_eq!(state.new_count, 0);

        // frame 3: no new input, just resolving frame 2's click.
        frame(&mut state, &[]);
        assert_eq!(state.new_count, 1);
    }

    #[test]
    fn tab_cycles_focus_new_save_delete_mute_volume_list() {
        let mut state = init_state();
        assert_eq!(state.interaction.focus().focused(), Some(Id::TrackList));

        frame(&mut state, &[]); // establishes the focus order from this frame's draw

        let expect = [
            Id::New,
            Id::Save,
            Id::Delete,
            Id::Mute,
            Id::Volume,
            Id::TrackList,
        ];
        for want in expect {
            frame(&mut state, &[key(KeyCode::Tab)]);
            assert_eq!(state.interaction.focus().focused(), Some(want));
        }
    }

    #[test]
    fn enter_activates_the_focused_button_without_any_pointer_input() {
        let mut state = init_state();
        frame(&mut state, &[]); // establishes focus order
        frame(&mut state, &[key(KeyCode::Tab)]); // -> New
        assert_eq!(state.interaction.focus().focused(), Some(Id::New));

        frame(&mut state, &[key(KeyCode::Enter)]);
        assert_eq!(state.new_count, 1);
    }

    #[test]
    fn up_down_only_move_the_list_selection_while_it_is_focused() {
        let mut state = init_state(); // TrackList focused by default
        frame(&mut state, &[]);
        frame(&mut state, &[key(KeyCode::Down)]);
        assert_eq!(state.list_state.selected(), Some(1));

        // Move focus elsewhere (one Tab: TrackList -> New); Down should no
        // longer move the selection.
        frame(&mut state, &[key(KeyCode::Tab)]);
        assert_eq!(state.interaction.focus().focused(), Some(Id::New));
        frame(&mut state, &[key(KeyCode::Down)]);
        assert_eq!(state.list_state.selected(), Some(1)); // unchanged
    }

    #[test]
    fn left_right_nudge_volume_only_while_it_is_focused() {
        let mut state = init_state();
        let starting = state.volume;
        frame(&mut state, &[key(KeyCode::Right)]); // TrackList focused, not Volume
        assert_eq!(state.volume, starting);

        frame(&mut state, &[]);
        for _ in 0..5 {
            frame(&mut state, &[key(KeyCode::Tab)]); // New, Save, Delete, Mute, Volume
        }
        assert_eq!(state.interaction.focus().focused(), Some(Id::Volume));

        frame(&mut state, &[key(KeyCode::Right)]);
        assert_eq!(state.volume, starting + 5);
        frame(&mut state, &[key(KeyCode::Left), key(KeyCode::Left)]);
        assert_eq!(state.volume, starting - 5);
    }

    #[test]
    fn dragging_the_volume_bar_sets_it_from_the_pointer_position() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers the volume bar's hit rect

        let l = layout(SIZE);
        let row = Rect::new(
            l.volume_row.left() + 1, // account for the focus gutter
            l.volume_row.top(),
            l.volume_row.width().saturating_sub(1),
            1,
        );
        let near_full = Pos::new(row.left() + row.width() - 1, row.top());

        // frame 2: delivers the press; `pressed()`/`dragging()` (and so the
        // volume update inside `draw_volume`) aren't visible until frame 3.
        frame(
            &mut state,
            &[mouse_at(MouseEventKind::Down(MouseButton::Left), near_full)],
        );
        assert_eq!(state.volume, 65);

        // frame 3: resolves the press against frame 2's (correctly
        // positioned) rect.
        frame(&mut state, &[]);
        assert!(
            state.volume > 90,
            "expected near-max volume, got {}",
            state.volume
        );
    }

    #[test]
    fn clicking_a_track_selects_it_and_logs_the_event() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers track row rects

        let l = layout(SIZE);
        let inner = inset(l.list);
        let second_row = Pos::new(inner.left(), inner.top() + 1);
        let click = [
            mouse_at(MouseEventKind::Down(MouseButton::Left), second_row),
            mouse_at(MouseEventKind::Up(MouseButton::Left), second_row),
        ];
        frame(&mut state, &click); // frame 2: delivers the click, not yet resolved
        assert_eq!(state.list_state.selected(), Some(0));

        frame(&mut state, &[]); // frame 3: resolves it
        assert_eq!(state.list_state.selected(), Some(1));
        assert!(
            state
                .log
                .back()
                .is_some_and(|line| line.spans.iter().any(|s| s.content.contains("selected"))),
        );
    }

    /// Regression test for a real bug: clicking a track row (which isn't
    /// itself `Sense::FOCUSABLE`, see the module docs) used to leave
    /// keyboard focus wherever it was before the click, so Up/Down did
    /// nothing until the user tabbed all the way around back to the list.
    #[test]
    fn clicking_a_track_focuses_the_list_so_arrow_keys_work_immediately() {
        let mut state = init_state();
        frame(&mut state, &[]); // establishes the focus order
        // Move focus away from the list first, so a passing test can't be
        // hiding behind init()'s default focus.
        frame(&mut state, &[key(KeyCode::Tab)]);
        assert_ne!(state.interaction.focus().focused(), Some(Id::TrackList));

        let l = layout(SIZE);
        let inner = inset(l.list);
        let second_row = Pos::new(inner.left(), inner.top() + 1);
        let click = [
            mouse_at(MouseEventKind::Down(MouseButton::Left), second_row),
            mouse_at(MouseEventKind::Up(MouseButton::Left), second_row),
        ];
        frame(&mut state, &click);
        frame(&mut state, &[]); // resolves the click
        assert_eq!(state.interaction.focus().focused(), Some(Id::TrackList));

        frame(&mut state, &[key(KeyCode::Down)]);
        assert_eq!(state.list_state.selected(), Some(2)); // moved from row 1
    }

    #[test]
    fn scrolling_the_track_list_advances_its_offset() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers the list container's hit rect

        let l = layout(SIZE);
        let inner = inset(l.list);
        let mid = Pos::new(inner.left(), inner.top());

        frame(&mut state, &[mouse_at(MouseEventKind::ScrollDown, mid)]); // frame 2: not yet resolved
        assert_eq!(state.list_state.offset(), 0);

        frame(&mut state, &[]); // frame 3: resolves the scroll
        assert!(state.list_state.offset() > 0);
    }
}
