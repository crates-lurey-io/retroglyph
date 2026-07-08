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
//!   [`ListState::scroll_by`], while each row senses hover/click (plus
//!   [`Sense::SECONDARY_CLICK`] for right-click-to-favorite) without being
//!   individually Tab-focusable (skipping [`Sense::FOCUSABLE`]) — a
//!   container can hold focus while its children stay mouse-only. Clicking
//!   a row also hand-moves focus to the container via
//!   [`Interaction::focus_mut`] (rows aren't `FOCUSABLE`, so `interact`'s
//!   own click-focuses-me behavior doesn't apply to them), so Up/Down keep
//!   working immediately after a click instead of needing a Tab first.
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
//! [`Sense::SECONDARY_CLICK`]: retroglyph_widgets::Sense::SECONDARY_CLICK
//! [`Interaction::focus_mut`]: retroglyph_widgets::Interaction::focus_mut
//! [`ListState::scroll_by`]: retroglyph_widgets::ListState::scroll_by
//!
//! # Controls
//!
//! - Tab / Shift+Tab — move focus between New, Save, Delete, Mute, Volume,
//!   and the track list
//! - Enter / Space — activate the focused button/toggle
//! - Up / Down — move the track selection, while the list is focused
//! - `PageUp` / `PageDown` / Home / End — jump the track selection, while
//!   the list is focused
//! - Left / Right — nudge the volume, while the slider is focused
//! - Mouse: click buttons/rows, drag the volume bar, scroll the track list,
//!   right-click a track to favorite it
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

use retroglyph_core::event::{Event, KeyCode, KeyModifiers, SystemTheme};
use retroglyph_core::{Backend, Color, Line, Pos, Rect, Size, Span, Style, Terminal};
use retroglyph_widgets::{
    Constraint, Density, Interaction, ListState, Response, Sense, Shortcuts, Theme, log,
    offset_for_pos, panel, scrollbar, split_h, split_v,
};

// ── Breakpoints ───────────────────────────────────────────────────────────────

/// Below this width, [`Density::Compact`] kicks in automatically (unless
/// overridden by the 'c' shortcut -- see [`AppState::density_manual`]).
const BP_COMPACT_WIDTH: u16 = 60;
/// Below this height, likewise.
const BP_COMPACT_HEIGHT: u16 = 22;

// ── Data ──────────────────────────────────────────────────────────────────────

/// One track's fixed data. Kept separate from [`Track`] (which adds mutable
/// per-instance state like `favorite`) so the initial seed data can stay a
/// plain `const` slice; [`AppState::tracks`] is built from this once in
/// [`init`].
struct TrackSeed {
    name: &'static str,
    duration: &'static str,
}

/// A track row's live state: [`TrackSeed`]'s fixed data plus whatever an
/// interaction can change about it. Owned (`Vec<Track>` on [`AppState`],
/// not a `&'static` slice) so rows can be favorited and (eventually)
/// reordered without needing a parallel, index-keyed side table that would
/// go stale the moment the order changes.
#[derive(Clone)]
struct Track {
    name: &'static str,
    duration: &'static str,
    favorite: bool,
}

// More entries than fit in most terminal heights, so the track list actually
// demonstrates ListState-driven scrolling (both via mouse wheel and
// Up/Down), not just a static, always-fully-visible list.
const TRACK_SEED: &[TrackSeed] = &[
    TrackSeed {
        name: "Sunrise Over Static",
        duration: "3:41",
    },
    TrackSeed {
        name: "Glass Corridor",
        duration: "4:12",
    },
    TrackSeed {
        name: "Nine Tenths Silence",
        duration: "2:58",
    },
    TrackSeed {
        name: "Low Orbit",
        duration: "5:03",
    },
    TrackSeed {
        name: "Paper Weather",
        duration: "3:27",
    },
    TrackSeed {
        name: "Vacant Frequencies",
        duration: "4:45",
    },
    TrackSeed {
        name: "Halflight",
        duration: "3:12",
    },
    TrackSeed {
        name: "The Long Answer",
        duration: "6:08",
    },
    TrackSeed {
        name: "Slow Static",
        duration: "3:55",
    },
    TrackSeed {
        name: "Nocturne for Modems",
        duration: "4:33",
    },
    TrackSeed {
        name: "Rust and Ceremony",
        duration: "2:41",
    },
    TrackSeed {
        name: "Everything, Eventually",
        duration: "5:19",
    },
    TrackSeed {
        name: "Fog Index",
        duration: "3:03",
    },
    TrackSeed {
        name: "Terminal Velocity",
        duration: "4:01",
    },
    TrackSeed {
        name: "Copper Wire Lullaby",
        duration: "3:38",
    },
    TrackSeed {
        name: "Departure Gate Four",
        duration: "4:27",
    },
    TrackSeed {
        name: "Salt Marsh Radio",
        duration: "2:52",
    },
    TrackSeed {
        name: "Undertow",
        duration: "5:41",
    },
    TrackSeed {
        name: "Between Two Clocks",
        duration: "3:16",
    },
    TrackSeed {
        name: "A Room With No Exit",
        duration: "4:09",
    },
    TrackSeed {
        name: "Faint Signal",
        duration: "2:35",
    },
    TrackSeed {
        name: "Winter Circuit",
        duration: "5:57",
    },
    TrackSeed {
        name: "The Last Elevator",
        duration: "3:44",
    },
    TrackSeed {
        name: "Static Bloom",
        duration: "4:18",
    },
    TrackSeed {
        name: "Origami Weather",
        duration: "3:02",
    },
    TrackSeed {
        name: "Blue Hour Traffic",
        duration: "4:52",
    },
    TrackSeed {
        name: "Nothing But Weather",
        duration: "2:47",
    },
    TrackSeed {
        name: "Empty Platform",
        duration: "5:11",
    },
    TrackSeed {
        name: "The Long Way Home",
        duration: "3:29",
    },
    TrackSeed {
        name: "Analog Ghost",
        duration: "4:36",
    },
    TrackSeed {
        name: "Loose Change",
        duration: "2:58",
    },
    TrackSeed {
        name: "Perimeter Walk",
        duration: "5:04",
    },
    TrackSeed {
        name: "Halfway to Nowhere",
        duration: "3:21",
    },
    TrackSeed {
        name: "The Quiet Engine",
        duration: "4:44",
    },
    TrackSeed {
        name: "Overcast Anthem",
        duration: "3:07",
    },
    TrackSeed {
        name: "Borrowed Time",
        duration: "5:26",
    },
    TrackSeed {
        name: "Signal to Noise",
        duration: "2:39",
    },
    TrackSeed {
        name: "Late Checkout",
        duration: "4:15",
    },
    TrackSeed {
        name: "Paper Boats",
        duration: "3:53",
    },
    TrackSeed {
        name: "Interference Pattern",
        duration: "5:33",
    },
    TrackSeed {
        name: "Afterimage",
        duration: "2:44",
    },
    TrackSeed {
        name: "The Waiting Room",
        duration: "4:08",
    },
    TrackSeed {
        name: "Common Ground",
        duration: "3:31",
    },
    TrackSeed {
        name: "Distant Static",
        duration: "5:02",
    },
];

// Comfortably more than LOG_HEIGHT's visible rows, so the log panel's
// scrollbar/wheel-scroll actually has history to scroll through instead of
// always showing everything at once.
// ── Data ────────────────────────────────────────────────────────────────────

/// Log entry severity — not a pre-rendered line. Entries are re-rendered
/// every frame in `draw_event_log` so a theme change recolors every entry
/// immediately, rather than leaving stale dark rows when switching to
/// light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLevel {
    Accent,
    Dim,
    Red,
}

/// One stored log entry, rendered fresh each frame by `draw_event_log`.
#[derive(Debug, Clone)]
struct LogEntry {
    level: LogLevel,
    text: String,
}

// Comfortably more than LOG_HEIGHT's visible rows, so the log panel's
// scrollbar/wheel-scroll actually has history to scroll through instead of
// always showing everything at once.
const LOG_CAPACITY: usize = 60;
const CONTROLS_WIDTH: u16 = 30;
const LOG_HEIGHT: u16 = 10;
/// The event log's height in [`Density::Compact`], shrunk from `LOG_HEIGHT`
/// to give the track list more of a narrow terminal's scarce rows -- still
/// shown, just smaller, rather than hidden outright.
const COMPACT_LOG_HEIGHT: u16 = 4;
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
    TrackScrollbar,
    EventLog,
    EventLogScrollbar,
}

fn id_label(id: Id) -> String {
    match id {
        Id::New => "New".to_owned(),
        Id::Save => "Save".to_owned(),
        Id::Delete => "Delete".to_owned(),
        Id::Mute => "Mute".to_owned(),
        Id::Volume => "Volume".to_owned(),
        Id::TrackList => "Track List".to_owned(),
        // Rows aren't Sense::FOCUSABLE (see the module docs), so `focused()`
        // never actually resolves to `Track(i)` in practice -- this arm only
        // exists to keep the match exhaustive. TRACK_SEED (not the live,
        // reorderable `AppState::tracks`) is fine here for exactly that
        // reason: this function has no `state` access, and the fallback
        // path it serves doesn't depend on current track order.
        Id::Track(i) => TRACK_SEED
            .get(i)
            .map_or_else(|| "Track".to_owned(), |t| format!("Track \"{}\"", t.name)),
        Id::TrackScrollbar => "Track Scrollbar".to_owned(),
        Id::EventLog => "Event Log".to_owned(),
        Id::EventLogScrollbar => "Event Log Scrollbar".to_owned(),
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

// `muted`/`follow_selection`/`theme_manual`/`density_manual` are four
// independent flags, not states of one state machine (clippy's suggested
// fix) -- collapsing them into an enum would just be a bitfield with extra
// steps.
#[allow(clippy::struct_excessive_bools)]
struct AppState {
    interaction: Interaction<Id>,
    new_count: u32,
    save_count: u32,
    delete_count: u32,
    muted: bool,
    volume: i32,
    tracks: Vec<Track>,
    list_state: ListState,
    log: VecDeque<LogEntry>,
    /// Scroll position into `log`, in [`log`](retroglyph_widgets::log)'s own
    /// tail-anchored convention: `0` shows the newest messages, larger
    /// values scroll back through history. Deliberately the opposite
    /// direction from `list_state.offset()` -- see `draw_event_log`'s doc
    /// comment for how the two get reconciled for the scrollbar's shared,
    /// forward-only geometry math.
    log_scroll: usize,
    /// The in-progress drag-to-reorder gesture, if any -- see [`Reorder`].
    reorder: Option<Reorder>,
    /// Set whenever the track selection changes (Up/Down/PageUp/PageDown/
    /// Home/End, or clicking a row); consumed by `draw_track_list`, which
    /// calls `ensure_visible` only on the frame after this is set, then
    /// clears it.
    ///
    /// Without this, `ensure_visible` (idempotent and safe to call every
    /// frame *for its own purpose*) would fight a free mouse-wheel scroll
    /// on every frame that isn't itself a scroll event -- which, since
    /// wheel events are one-shot, is nearly every frame: the view would
    /// jump back to the selected row the instant the wheel stopped
    /// spinning, not just on the one frame a selection change needs to be
    /// followed.
    follow_selection: bool,
    /// The active color palette. Auto-detected once at startup and live-
    /// updated from [`Event::ThemeChanged`] (native/wasm windowed backend
    /// only -- see the event's own doc comment), unless
    /// [`theme_manual`](Self::theme_manual) is set.
    theme: Theme,
    /// `true` once the 't' shortcut has been used this session: further
    /// [`Event::ThemeChanged`] events are ignored, so a deliberate choice
    /// isn't clobbered by the system theme changing again later.
    theme_manual: bool,
    /// The active layout/touch-target density. Auto-detected from the
    /// terminal size every frame, unless
    /// [`density_manual`](Self::density_manual) is set.
    density: Density,
    /// `true` once the 'c' shortcut has been used this session: further
    /// automatic recomputation from terminal size is skipped, so a
    /// deliberate choice isn't clobbered by the next resize.
    density_manual: bool,
    /// Global keyboard shortcuts ('c'/'t'), resolved in [`handle`] against
    /// whatever currently holds focus -- see [`Shortcuts`]'s own docs for
    /// why this doesn't also carry Tab/Enter/Q/Escape or the list's
    /// arrow-key routing (those need more context than a flat action fits).
    shortcuts: Shortcuts<Id, Shortcut>,
}

/// What a global keyboard shortcut (as opposed to a widget's own
/// [`Sense`]d input) resolves to. See [`AppState::shortcuts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shortcut {
    /// 'c': toggle [`Density::Compact`]/[`Density::Relaxed`] and stop
    /// auto-detecting it from terminal size for the rest of the session.
    ToggleDensity,
    /// 't': toggle [`Theme::DARK`]/[`Theme::LIGHT`] and stop following
    /// [`Event::ThemeChanged`] for the rest of the session.
    ToggleTheme,
}

fn init<B: Backend>(term: &Terminal<B>) -> AppState {
    let tracks: Vec<Track> = TRACK_SEED
        .iter()
        .map(|seed| Track {
            name: seed.name,
            duration: seed.duration,
            favorite: false,
        })
        .collect();

    let mut list_state = ListState::new();
    list_state.select_first(tracks.len());

    let mut interaction = Interaction::new();
    // Focus the track list by default so Up/Down/scroll work immediately,
    // without requiring a first Tab press -- demonstrates driving focus
    // programmatically via `focus_mut()` rather than only through Tab.
    interaction.focus_mut().request(Id::TrackList);

    let mut shortcuts = Shortcuts::new();
    shortcuts.bind_global(
        KeyCode::Char('c'),
        KeyModifiers::NONE,
        Shortcut::ToggleDensity,
    );
    shortcuts.bind_global(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
        Shortcut::ToggleTheme,
    );

    AppState {
        interaction,
        new_count: 0,
        save_count: 0,
        delete_count: 0,
        muted: false,
        volume: 65,
        tracks,
        list_state,
        log: VecDeque::with_capacity(LOG_CAPACITY),
        log_scroll: 0,
        reorder: None,
        follow_selection: true,
        theme: Theme::DARK,
        theme_manual: false,
        density: density_for(term.size()),
        density_manual: false,
        shortcuts,
    }
}

/// The [`Density`] [`BP_COMPACT_WIDTH`]/[`BP_COMPACT_HEIGHT`] auto-select
/// for a given terminal size.
const fn density_for(size: Size) -> Density {
    if size.width < BP_COMPACT_WIDTH || size.height < BP_COMPACT_HEIGHT {
        Density::Compact
    } else {
        Density::Relaxed
    }
}

fn push_log(state: &mut AppState, level: LogLevel, text: String) {
    if state.log.len() == LOG_CAPACITY {
        state.log.pop_front();
    }
    state.log.push_back(LogEntry { level, text });
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
    /// `None` in [`Density::Compact`]: there's no vertical room to spare
    /// for debug text that's discoverable another way (the demo's own
    /// controls still work without it).
    focused_row: Option<Rect>,
    /// See [`focused_row`](Self::focused_row).
    pointer_row: Option<Rect>,
    /// One track-list row's height in cells at the active density --
    /// [`Density::min_target_size`]'s height, threaded through so
    /// `draw_track_list`/`draw_track_row` don't recompute it.
    track_row_height: u16,
}

fn layout(size: Size, density: Density) -> Layout {
    let screen = Rect::new(0, 0, size.width, size.height);
    let [title, body, footer] = take3(&split_v(
        screen,
        &[Constraint::Fixed(1), Constraint::Fill, Constraint::Fixed(1)],
    ));
    let row_h = density.min_target_size().height;

    match density {
        Density::Relaxed => {
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
                    Constraint::Fixed(row_h), // 0: new
                    Constraint::Fixed(row_h), // 1: save
                    Constraint::Fixed(row_h), // 2: delete
                    Constraint::Fixed(1),     // 3: spacer
                    Constraint::Fixed(row_h), // 4: mute
                    Constraint::Fixed(1),     // 5: spacer
                    Constraint::Fixed(row_h), // 6: volume
                    Constraint::Fixed(1),     // 7: spacer
                    Constraint::Fixed(1),     // 8: focused debug line
                    Constraint::Fixed(1),     // 9: pointer debug line
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
                focused_row: Some(rows[8]),
                pointer_row: Some(rows[9]),
                track_row_height: row_h,
            }
        }
        Density::Compact => {
            // Controls stacked full-width above the list, instead of a
            // fixed-width sidebar that would eat well over half a narrow
            // terminal's columns -- see the module docs' "Controls"
            // section. No spacer rows and no debug lines: a touch-target
            // row is already taller than `Relaxed`'s, so there's nothing
            // left to spare.
            const CONTROL_ROWS: u16 = 5; // new, save, delete, mute, volume
            let controls_h = row_h * CONTROL_ROWS + 2; // + top/bottom border
            let [controls, rest] = take2(&split_v(
                body,
                &[Constraint::Fixed(controls_h), Constraint::Fill],
            ));
            let [list, log] = take2(&split_v(
                rest,
                &[Constraint::Fill, Constraint::Fixed(COMPACT_LOG_HEIGHT)],
            ));

            let rows = split_v(
                inset(controls),
                &[
                    Constraint::Fixed(row_h), // 0: new
                    Constraint::Fixed(row_h), // 1: save
                    Constraint::Fixed(row_h), // 2: delete
                    Constraint::Fixed(row_h), // 3: mute
                    Constraint::Fixed(row_h), // 4: volume
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
                mute_row: rows[3],
                volume_row: rows[4],
                focused_row: None,
                pointer_row: None,
                track_row_height: row_h,
            }
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw<B: Backend>(term: &mut Terminal<B>, state: &mut AppState) {
    if !state.density_manual {
        state.density = density_for(term.size());
    }
    let l = layout(term.size(), state.density);

    for y in 0..term.size().height {
        for x in 0..term.size().width {
            term.put_styled(x, y, ' ', Style::new().bg(state.theme.bg));
        }
    }

    draw_bar(
        term,
        l.title,
        state.theme,
        " retroglyph — interaction demo (hover / click / drag / focus / scroll)",
    );
    draw_bar(
        term,
        l.footer,
        state.theme,
        " Tab/Shift+Tab: focus   Enter/Space: activate   click/drag/scroll/right-click: mouse   c/t: density/theme   Q: quit",
    );

    draw_controls(term, &l, state);
    draw_track_list(term, l.list, l.track_row_height, state);
    draw_event_log(term, l.log, state);
}

fn draw_bar<B: Backend>(term: &mut Terminal<B>, area: Rect, theme: Theme, text: &str) {
    let style = Style::new().fg(theme.fg).bg(theme.title_bg);
    fill_row(term, area, style);
    print_at(term, area, text, style);
}

fn draw_controls<B: Backend>(term: &mut Terminal<B>, l: &Layout, state: &mut AppState) {
    panel_bg(term, l.controls, state.theme, "CONTROLS", false);

    let new_label = format!("New ({})", state.new_count);
    let r = draw_button(term, l.new_row, Id::New, &new_label, state);
    if r.clicked() {
        state.new_count += 1;
        push_log(state, LogLevel::Accent, "created a new item".to_owned());
    }

    let save_label = format!("Save ({})", state.save_count);
    let r = draw_button(term, l.save_row, Id::Save, &save_label, state);
    if r.clicked() {
        state.save_count += 1;
        push_log(state, LogLevel::Accent, "saved".to_owned());
    }

    let delete_label = format!("Delete ({})", state.delete_count);
    let r = draw_button(term, l.delete_row, Id::Delete, &delete_label, state);
    if r.clicked() {
        state.delete_count += 1;
        push_log(state, LogLevel::Red, "deleted".to_owned());
    }

    let r = draw_toggle(term, l.mute_row, state);
    if r.clicked() {
        state.muted = !state.muted;
        push_log(state, LogLevel::Accent, format!("mute -> {}", state.muted));
    }

    let r = draw_volume(term, l.volume_row, state);
    if r.released() {
        push_log(
            state,
            LogLevel::Accent,
            format!("volume -> {}%", state.volume),
        );
    }

    // No room for these in Density::Compact -- see Layout::focused_row's
    // doc comment.
    if let Some(focused_row) = l.focused_row {
        let focus_text = state.interaction.focus().focused().map_or_else(
            || "Focused: -".to_owned(),
            |id| format!("Focused: {}", id_label(id)),
        );
        print_at(
            term,
            focused_row,
            &focus_text,
            Style::new().fg(state.theme.dim).bg(state.theme.panel_bg),
        );
    }

    if let Some(pointer_row) = l.pointer_row {
        let pointer_text = state.interaction.pointer().pos().map_or_else(
            || "Pointer: -".to_owned(),
            |p| format!("Pointer: {},{}", p.x, p.y),
        );
        print_at(
            term,
            pointer_row,
            &pointer_text,
            Style::new().fg(state.theme.dim).bg(state.theme.panel_bg),
        );
    }
}

fn draw_button<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    id: Id,
    label: &str,
    state: &mut AppState,
) -> Response {
    let response = state.interaction.interact(area, id, Sense::click());
    let row = focus_gutter(term, area, state.theme, response.focused());
    let bg = if response.pressed() {
        state.theme.press_bg
    } else if response.hovered() {
        state.theme.hover_bg
    } else {
        state.theme.panel_bg
    };
    let style = Style::new().fg(state.theme.fg).bg(bg);
    fill_row(term, row, style);
    print_at(term, row, &format!("[ {label} ]"), style);
    response
}

fn draw_toggle<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut AppState) -> Response {
    let response = state.interaction.interact(area, Id::Mute, Sense::click());
    let row = focus_gutter(term, area, state.theme, response.focused());
    let bg = if response.pressed() {
        state.theme.press_bg
    } else if response.hovered() {
        state.theme.hover_bg
    } else {
        state.theme.panel_bg
    };
    let style = Style::new().fg(state.theme.fg).bg(bg);
    fill_row(term, row, style);
    let mark = if state.muted { "[x]" } else { "[ ]" };
    print_at(term, row, &format!("{mark} Mute"), style);
    response
}

/// Drag (or click-and-hold) anywhere on the bar to set the volume
/// proportionally to the pointer's x position, like a seek bar.
fn draw_volume<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut AppState) -> Response {
    let response = state.interaction.interact(area, Id::Volume, Sense::drag());
    let row = focus_gutter(term, area, state.theme, response.focused());

    if (response.pressed() || response.dragging())
        && let Some(pos) = state.interaction.pointer().pos()
    {
        let rel = pos.x.saturating_sub(row.left());
        let ratio = f32::from(rel) / f32::from(row.width().max(1));
        state.volume = (ratio.clamp(0.0, 1.0) * 100.0).round() as i32;
    }

    // `gauge` hardcodes dark-theme colors for the label/empty-bar
    // background; draw the bar manually here with the current theme's
    // colors instead.
    let theme = state.theme;
    let pct = format!("{:>3}%", state.volume);
    let y = row.top() + row.height() / 2;
    let ratio = (state.volume as f32 / 100.0).clamp(0.0, 1.0);

    // Label
    let label_style = Style::new().fg(theme.fg).bg(theme.panel_bg);
    for (i, ch) in "vol".chars().enumerate() {
        term.put_styled(row.left() + i as u16, y, ch, label_style);
    }
    let bar_left = row.left() + 4;
    let bar_w = row.width().saturating_sub(4 + 1 + pct.len() as u16 + 1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let filled = (ratio * f32::from(bar_w)).round() as u16;

    // Filled bar (green→yellow→red ramp, theme-agnostic)
    for i in 0..bar_w {
        let t = if bar_w > 1 {
            f32::from(i) / f32::from(bar_w - 1)
        } else {
            0.0
        };
        let ch = if i < filled { '█' } else { '░' };
        let color = if i < filled {
            gauge_color(t)
        } else {
            theme.dim
        };
        term.put_styled(
            bar_left + i,
            y,
            ch,
            Style::new().fg(color).bg(theme.panel_bg),
        );
    }

    // % readout
    let pct_x = bar_left + bar_w + 1;
    for (i, ch) in pct.chars().enumerate() {
        let x = pct_x + i as u16;
        if x >= row.right() {
            break;
        }
        term.put_styled(x, y, ch, Style::new().fg(theme.fg).bg(theme.panel_bg));
    }

    response
}

/// Same green→yellow→red ramp as `meter_ramp`, but computed here so the
/// demo doesn't add a dependency on a widget whose own label colors are
/// baked dark. Uses `Color::lerp` (backed by `gem`) — the same underlying
/// lerp `meter_ramp` itself uses — rather than hand-rolling RGB
/// interpolation.
#[must_use]
fn gauge_color(t: f32) -> Color {
    #[allow(clippy::unusual_byte_groupings)]
    const GREEN: Color = Color::Rgb {
        r: 80,
        g: 200,
        b: 120,
    };
    #[allow(clippy::unusual_byte_groupings)]
    const YELLOW: Color = Color::Rgb {
        r: 220,
        g: 200,
        b: 90,
    };
    #[allow(clippy::unusual_byte_groupings)]
    const RED: Color = Color::Rgb {
        r: 220,
        g: 90,
        b: 90,
    };

    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        Color::lerp(GREEN, YELLOW, t * 2.0)
    } else {
        Color::lerp(YELLOW, RED, (t - 0.5) * 2.0)
    }
}

fn draw_track_list<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    row_height: u16,
    state: &mut AppState,
) {
    let focused = state.interaction.focus().is_focused(Id::TrackList);
    panel_bg(term, area, state.theme, "TRACKS", focused);
    let inner = inset(area);
    if inner.width() < 2 || inner.height() == 0 {
        return;
    }
    let (list_area, bar_area) = split_right(inner, 1);

    // Sensing the container with FOCUSABLE (Tab-reachable) but each row
    // below with only HOVER | CLICK (not FOCUSABLE): a container can hold
    // keyboard focus while its children stay mouse/tap-only, without every
    // row cluttering the Tab order.
    let list_response =
        state
            .interaction
            .interact(list_area, Id::TrackList, Sense::scroll() | Sense::FOCUSABLE);
    let row_height = row_height.max(1);
    let visible_rows = (list_area.height() / row_height) as usize;
    let track_count = state.tracks.len();
    let max_offset = track_count.saturating_sub(visible_rows);

    if list_response.scroll_delta() != 0 {
        state.list_state.scroll_by(list_response.scroll_delta());
        // ListState::scroll_by deliberately has no upper clamp (only the
        // caller knows the content length); without this, repeated
        // wheel-down scrolls the offset arbitrarily far past the last
        // page, leaving the list blank until scrolled all the way back.
        if state.list_state.offset() > max_offset {
            state.list_state.set_offset(max_offset);
        }
    } else if state.follow_selection {
        // Only follow the selection on the frame *after* it actually
        // changed (see `follow_selection`'s doc comment) -- not every
        // frame, which would fight a free wheel-scroll the instant it
        // stopped.
        state.list_state.ensure_visible(visible_rows);
        state.follow_selection = false;
    }

    let current_offset = state.list_state.offset();
    if let Some(new_offset) = draw_scrollbar_column(
        term,
        bar_area,
        Id::TrackScrollbar,
        &mut state.interaction,
        state.theme,
        ScrollGeometry {
            total_len: track_count,
            visible_len: visible_rows,
            forward_offset: current_offset,
        },
    ) {
        state.list_state.set_offset(new_offset);
    }
    let offset = state.list_state.offset();

    let window = TrackWindow {
        list_area,
        offset,
        visible_rows,
        track_count,
        row_height,
    };
    for i in offset..(offset + visible_rows).min(track_count) {
        let y = list_area.top() + (i - offset) as u16 * row_height;
        let row = Rect::new(list_area.left(), y, list_area.width(), row_height);
        draw_track_row(term, row, i, window, state);
    }
}

/// The currently visible slice of the track list, shared by every row's
/// [`draw_track_row`] call this frame -- just the loop variables
/// `draw_track_list` already has, grouped into one `Copy` struct so passing
/// them down doesn't blow past a reasonable argument count.
#[derive(Clone, Copy)]
struct TrackWindow {
    list_area: Rect,
    offset: usize,
    visible_rows: usize,
    track_count: usize,
    row_height: u16,
}

/// One track row: handles its own click/right-click/drag interaction and
/// draws itself. Split out of `draw_track_list` purely to keep that
/// function's own layout/scroll/scrollbar orchestration readable -- this
/// isn't a reusable widget, just a `for` loop body with somewhere to put
/// its local variables.
fn draw_track_row<B: Backend>(
    term: &mut Terminal<B>,
    row: Rect,
    i: usize,
    window: TrackWindow,
    state: &mut AppState,
) {
    let response = state.interaction.interact(
        row,
        Id::Track(i),
        Sense::HOVER | Sense::CLICK | Sense::SECONDARY_CLICK | Sense::DRAG,
    );

    if response.clicked() {
        state.list_state.select(Some(i));
        state.follow_selection = true;
        // Rows themselves aren't Sense::FOCUSABLE (they'd clutter the Tab
        // order -- see the module docs), so clicking one doesn't
        // automatically focus anything the way a FOCUSABLE widget's own
        // click does. Move focus to the container by hand instead, so
        // Up/Down keep working immediately after a click without requiring
        // an extra Tab press first.
        state.interaction.focus_mut().request(Id::TrackList);
        let name = state.tracks[i].name;
        push_log(state, LogLevel::Accent, format!("selected \"{name}\""));
    }
    if response.secondary_clicked() {
        state.tracks[i].favorite = !state.tracks[i].favorite;
        let name = state.tracks[i].name;
        let verb = if state.tracks[i].favorite {
            "favorited"
        } else {
            "unfavorited"
        };
        push_log(state, LogLevel::Accent, format!("{verb} \"{name}\""));
    }
    if response.dragging()
        && let Some(pos) = state.interaction.pointer().pos()
    {
        // Only the currently visible window is a valid drop target --
        // dragging past the top/bottom edge clamps rather than
        // auto-scrolling the list to reveal more rows. A real app might
        // want that; it's a reasonable amount of extra state (an
        // auto-scroll timer/velocity) for a demo already covering a lot of
        // ground.
        let target = drop_target_row(window, pos);
        state.reorder = Some(Reorder { from: i, target });
    }
    if response.released() && response.dragging() {
        if let Some(reorder) = state.reorder.take() {
            reorder_track(state, reorder.from, reorder.target);
        }
    } else if response.released() {
        // A plain release (no drag) still needs the marker cleared --
        // dragging() can be true on an earlier frame of the same gesture
        // and then this widget's `active` clears in end_frame, but a stale
        // `reorder` naming a row that's no longer the active drag would
        // otherwise linger and misdraw.
        state.reorder = None;
    }

    let is_dragged = state.reorder.as_ref().is_some_and(|r| r.from == i);
    let is_drop_target = state
        .reorder
        .as_ref()
        .is_some_and(|r| r.target == i && r.from != i);
    let track = &state.tracks[i];
    let selected = state.list_state.selected() == Some(i);
    let bg = if is_dragged {
        state.theme.border
    } else if is_drop_target {
        state.theme.accent
    } else if selected {
        state.theme.press_bg
    } else if response.hovered() {
        state.theme.hover_bg
    } else {
        state.theme.panel_bg
    };
    let style = Style::new().fg(state.theme.fg).bg(bg);
    let mark = if track.favorite { '*' } else { ' ' };
    fill_row(term, row, style);
    print_at(
        term,
        row,
        &format!("{mark}{:<24} {}", track.name, track.duration),
        style,
    );
}

/// In-progress drag-to-reorder gesture: dragging `from` currently wants to
/// land at `target` (both track indices, in the *pre-drop* array). Cleared
/// on release (successful or not) -- see `draw_track_list`'s row loop.
struct Reorder {
    from: usize,
    target: usize,
}

/// The track index a drop at cell-row `pos.y` would target, clamped to the
/// currently visible window. `pos.y` is divided by `window.row_height` to
/// get a row index first -- at `Density::Compact`'s taller rows, a drop
/// anywhere within a row's multiple cell-rows should still target that one
/// row, not whichever cell-row happens to be under the pointer.
fn drop_target_row(window: TrackWindow, pos: Pos) -> usize {
    let row_height = window.row_height.max(1);
    let rel_cells = pos.y.saturating_sub(window.list_area.top());
    let rel = usize::from(rel_cells / row_height);
    let rel = rel.min(window.visible_rows.saturating_sub(1));
    (window.offset + rel).min(window.track_count.saturating_sub(1))
}

/// Move the track at `from` so it lands just before whatever currently sits
/// at `target` -- see `draw_track_list`'s doc comment on `Reorder` for the
/// indices' meaning; see `drop_target_row` for how `target` is chosen from
/// a live pointer position.
///
/// Selection follows the moved track only when it *was* the moved one
/// (matched by its pre-move index): reordering a row that merely shifted
/// because something else moved past it doesn't retarget the selection.
/// Getting that fully right for every row would need remapping the whole
/// selection by identity rather than index, which is more bookkeeping than
/// this demo's simple `Option<usize>` selection is set up for.
fn reorder_track(state: &mut AppState, from: usize, target: usize) {
    if from == target || from >= state.tracks.len() {
        return;
    }
    let track = state.tracks.remove(from);
    let name = track.name;
    // `target` named a slot in the *pre-removal* array; removing `from`
    // shifts everything after it back by one, so a target past `from`
    // needs the same adjustment to still mean "insert before this row."
    let insert_at = if target > from { target - 1 } else { target }.min(state.tracks.len());
    state.tracks.insert(insert_at, track);

    if state.list_state.selected() == Some(from) {
        state.list_state.select(Some(insert_at));
        state.follow_selection = true;
    }
    push_log(
        state,
        LogLevel::Accent,
        format!("moved \"{name}\" to position {}", insert_at + 1),
    );
}

/// `log`'s `offset` counts backward from the tail (`0` = newest message),
/// the opposite direction from [`ListState`]'s forward, start-anchored
/// `offset()` -- both are the *correct* convention for what they each
/// model (a log defaults to "pinned to the newest line"; a list defaults to
/// "pinned to the first item"), so this isn't a bug to unify, just two
/// scroll positions that mean opposite things. [`thumb_geometry`]/[`offset_for_pos`]
/// only know the forward convention, so this function converts at the
/// boundary (`forward = max_scroll - log_scroll` going in,
/// `log_scroll = max_scroll - forward` coming back from a click/drag)
/// rather than teaching the scrollbar geometry a second direction.
fn draw_event_log<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut AppState) {
    let focused = state.interaction.focus().is_focused(Id::EventLog);
    panel_bg(term, area, state.theme, "EVENTS", focused);
    let inner = inset(area);
    if inner.width() < 2 || inner.height() == 0 {
        return;
    }
    let (log_area, bar_area) = split_right(inner, 1);

    let visible_len = log_area.height() as usize;
    let total_len = state.log.len();
    let max_scroll = total_len.saturating_sub(visible_len);

    let response =
        state
            .interaction
            .interact(log_area, Id::EventLog, Sense::scroll() | Sense::FOCUSABLE);
    if response.scroll_delta() != 0 {
        state.log_scroll = apply_signed(state.log_scroll, -response.scroll_delta(), max_scroll);
    }

    let forward = max_scroll.saturating_sub(state.log_scroll.min(max_scroll));
    if let Some(new_forward) = draw_scrollbar_column(
        term,
        bar_area,
        Id::EventLogScrollbar,
        &mut state.interaction,
        state.theme,
        ScrollGeometry {
            total_len,
            visible_len,
            forward_offset: forward,
        },
    ) {
        state.log_scroll = max_scroll.saturating_sub(new_forward.min(max_scroll));
    }

    let lines: Vec<Line> = state
        .log
        .iter()
        .map(|entry| {
            let color = log_level_color(entry.level, state.theme);
            Line::from(vec![Span::styled(
                entry.text.clone(),
                Style::new().fg(color).bg(state.theme.panel_bg),
            )])
        })
        .collect();
    log(term, log_area, &lines, state.log_scroll);
}

/// Maps a [`LogLevel`] to a [`Color`] for the current [`Theme`], so
/// switching themes recolors every log entry immediately rather than
/// leaving stale dark rows when switching to light.
const fn log_level_color(level: LogLevel, theme: Theme) -> Color {
    match level {
        LogLevel::Accent => theme.accent,
        LogLevel::Dim => theme.dim,
        LogLevel::Red => Color::RED,
    }
}

/// Add a signed `delta` to `current`, clamped to `0..=max`. Small enough
/// (and used in exactly one spot, `log_scroll`'s wheel handling) that it's
/// not worth pulling in as a `ListState`-style shared helper -- but written
/// with the same `try_from`/`clamp` idiom `ListState`'s own arithmetic uses
/// rather than an `as` cast, so it can't silently wrap on pathological
/// inputs either.
/// A generous upper bound for `log_scroll` usable outside `draw` (which
/// doesn't know the log panel's actual viewport height until layout runs).
/// See `draw_event_log`'s doc comment for the tighter, viewport-aware bound
/// `max_scroll` it re-clamps to on the next frame regardless.
fn log_scroll_max(state: &AppState) -> usize {
    state.log.len().saturating_sub(1)
}

fn apply_signed(current: usize, delta: i32, max: usize) -> usize {
    let current = i64::try_from(current).unwrap_or(i64::MAX);
    let max = i64::try_from(max).unwrap_or(i64::MAX);
    let next = (current + i64::from(delta)).clamp(0, max);
    usize::try_from(next).unwrap_or(0)
}

// ── Small drawing helpers ─────────────────────────────────────────────────────

fn panel_bg<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    theme: Theme,
    title: &str,
    focused: bool,
) {
    if area.width() < 2 || area.height() < 2 {
        return;
    }
    let border = if focused { theme.accent } else { theme.border };
    panel(
        term,
        area,
        Some(title),
        Style::new().fg(border).bg(theme.bg),
        Style::new().bg(theme.panel_bg),
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
fn focus_gutter<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    theme: Theme,
    focused: bool,
) -> Rect {
    let (ch, fg) = if focused {
        ('›', theme.accent)
    } else {
        (' ', theme.panel_bg)
    };
    let y = area.top() + area.height() / 2;
    term.put_styled(area.left(), y, ch, Style::new().fg(fg).bg(theme.panel_bg));
    Rect::new(
        area.left() + 1,
        area.top(),
        area.width().saturating_sub(1),
        area.height(),
    )
}

/// Fills every row of `area`, not just its top one -- at `Density::Compact`'s
/// taller touch-target rows, a one-row-tall fill would leave the rest of a
/// button/track row showing whatever was underneath.
fn fill_row<B: Backend>(term: &mut Terminal<B>, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', style);
        }
    }
}

/// Prints `text` on `area`'s vertically centered row. For a one-row-tall
/// `area` (every `Density::Relaxed` row), that's the same as `area.top()`;
/// for a taller one (`Density::Compact`'s touch targets), it centers the
/// label instead of pinning it to the first cell-row.
fn print_at<B: Backend>(term: &mut Terminal<B>, area: Rect, text: &str, style: Style) {
    let y = area.top() + area.height() / 2;
    for (i, ch) in text.chars().enumerate() {
        let x = area.left() + i as u16;
        if x >= area.right() {
            break;
        }
        term.put_styled(x, y, ch, style);
    }
}

fn take2(v: &[Rect]) -> [Rect; 2] {
    [v[0], v[1]]
}

fn take3(v: &[Rect]) -> [Rect; 3] {
    [v[0], v[1], v[2]]
}

/// Splits `cols` columns off the right edge of `area`, returning
/// `(remaining, split_off)`.
fn split_right(area: Rect, cols: u16) -> (Rect, Rect) {
    let cols = cols.min(area.width());
    let main_w = area.width() - cols;
    (
        Rect::new(area.left(), area.top(), main_w, area.height()),
        Rect::new(area.left() + main_w, area.top(), cols, area.height()),
    )
}

/// Draws a one-column vertical scrollbar at `bar_area` for a `total_len`-item
/// scrollable region with a `visible_len`-row viewport, and reports the
/// forward (`ListState`-style: `0` = start, larger = scrolled further in)
/// offset the user clicked/dragged it to this frame, if any.
///
/// Deliberately thin: [`retroglyph_widgets::thumb_geometry`]/[`offset_for_pos`]
/// already do the geometry, and [`scrollbar`] already does the drawing --
/// this just wires both into one [`Interaction::interact`] call with
/// [`Sense::DRAG`] (without [`Sense::FOCUSABLE`], so the thumb stays
/// mouse-only and doesn't clutter the Tab order, the same reasoning as
/// track rows). Callers own converting the returned forward offset into
/// whatever their content's actual offset convention is -- see
/// `draw_event_log` for a convention that isn't already forward (tail-
/// anchored, like `log`'s own `offset` parameter).
/// The three numbers [`thumb_geometry`]/[`offset_for_pos`]/[`scrollbar`]
/// need to describe a scroll position -- bundled into one `Copy` struct so
/// `draw_scrollbar_column` doesn't blow past a reasonable argument count.
#[derive(Clone, Copy)]
struct ScrollGeometry {
    total_len: usize,
    visible_len: usize,
    forward_offset: usize,
}

fn draw_scrollbar_column<B: Backend>(
    term: &mut Terminal<B>,
    bar_area: Rect,
    id: Id,
    interaction: &mut Interaction<Id>,
    theme: Theme,
    geometry: ScrollGeometry,
) -> Option<usize> {
    // No Sense::CLICK: that would also trigger interact()'s auto-focus-
    // request-on-click side effect, stealing keyboard focus away from
    // whatever container this scrollbar belongs to. pressed()/dragging()
    // don't need CLICK -- they're driven by `active`, which HOVER alone
    // already makes eligible.
    let response = interaction.interact(bar_area, id, Sense::HOVER | Sense::DRAG);

    let jumped_to = if (response.pressed() || response.dragging())
        && let Some(pos) = interaction.pointer().pos()
    {
        offset_for_pos(bar_area, geometry.total_len, geometry.visible_len, pos)
    } else {
        None
    };

    let track_style = Style::new().bg(theme.panel_bg);
    let thumb_style = if response.hovered() || response.dragging() {
        Style::new().bg(theme.accent)
    } else {
        Style::new().bg(theme.border)
    };
    scrollbar(
        term,
        bar_area,
        geometry.total_len,
        geometry.visible_len,
        geometry.forward_offset,
        track_style,
        thumb_style,
    );

    jumped_to
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

/// How many rows `PageUp`/`PageDown` move the track selection. Not tied to
/// the actual viewport height (which `handle` has no access to -- that's a
/// layout concern computed only inside `draw`): an approximate page is fine
/// for a keyboard shortcut, and [`ListState::ensure_visible`] scrolls
/// whatever lands into view either way.
const PAGE_SIZE: usize = 8;

/// Move the track selection by a page, clamped at the ends (unlike
/// [`ListState::select_next`]/[`select_previous`](ListState::select_previous),
/// which wrap).
fn page_selection(state: &mut AppState, direction: i32) {
    let len = state.tracks.len();
    if len == 0 {
        return;
    }
    let current = state.list_state.selected().unwrap_or(0);
    let delta = direction * i32::try_from(PAGE_SIZE).unwrap_or(i32::MAX);
    let last = i32::try_from(len - 1).unwrap_or(i32::MAX);
    let next = i32::try_from(current)
        .unwrap_or(0)
        .saturating_add(delta)
        .clamp(0, last);
    state.list_state.select(usize::try_from(next).ok());
    state.follow_selection = true;
}

/// Live theme updates from `Event::ThemeChanged` (winit native+wasm only —
/// the crossterm backend never emits this, so `state.theme` stays whatever
/// `init` seeded it with). No-op once `theme_manual` is set (the user
/// pressed 't').
#[allow(clippy::missing_const_for_fn)]
fn apply_theme_changed(state: &mut AppState, event: &Event) {
    if let Event::ThemeChanged(system_theme) = event
        && !state.theme_manual
    {
        state.theme = match system_theme {
            SystemTheme::Light => Theme::LIGHT,
            SystemTheme::Dark => Theme::DARK,
        };
    }
}

/// Global keyboard shortcuts ('c'/'t'), resolved via
/// [`Shortcuts`] against whatever currently holds focus — see the
/// type's own docs for why these two live in a registry while
/// Tab/Enter/Q/arrows stay direct `match` arms.
fn apply_shortcut(state: &mut AppState, event: &Event) {
    if let Some(shortcut) = state
        .shortcuts
        .resolve(event, state.interaction.focus().focused())
    {
        match shortcut {
            Shortcut::ToggleDensity => {
                state.density = match state.density {
                    Density::Compact => Density::Relaxed,
                    Density::Relaxed => Density::Compact,
                };
                state.density_manual = true;
            }
            Shortcut::ToggleTheme => {
                state.theme = if state.theme == Theme::DARK {
                    Theme::LIGHT
                } else {
                    Theme::DARK
                };
                state.theme_manual = true;
            }
        }
    }
}

/// Apply one input event. Returns `false` to quit.
fn handle(state: &mut AppState, event: &Event) -> bool {
    // The windowed (software) backend's close button doesn't exit on its
    // own -- retroglyph-window pushes Event::Close and leaves the decision
    // to the app, same as every other example's handle() (see
    // responsive_game_ui/sprite_stress/subpixel/tileset). Missing this arm
    // is why clicking the window's close button used to do nothing; only Q
    // worked.
    if *event == Event::Close {
        return false;
    }

    apply_theme_changed(state, event);
    apply_shortcut(state, event);

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
                state.list_state.select_previous(state.tracks.len());
                state.follow_selection = true;
            }
            KeyCode::Down if state.interaction.focus().is_focused(Id::TrackList) => {
                state.list_state.select_next(state.tracks.len());
                state.follow_selection = true;
            }
            KeyCode::PageUp if state.interaction.focus().is_focused(Id::TrackList) => {
                page_selection(state, -1);
            }
            KeyCode::PageDown if state.interaction.focus().is_focused(Id::TrackList) => {
                page_selection(state, 1);
            }
            KeyCode::Home if state.interaction.focus().is_focused(Id::TrackList) => {
                state.list_state.select_first(state.tracks.len());
                state.follow_selection = true;
            }
            KeyCode::End if state.interaction.focus().is_focused(Id::TrackList) => {
                state.list_state.select_last(state.tracks.len());
                state.follow_selection = true;
            }
            KeyCode::Left if state.interaction.focus().is_focused(Id::Volume) => {
                state.volume = (state.volume - 5).max(0);
            }
            KeyCode::Right if state.interaction.focus().is_focused(Id::Volume) => {
                state.volume = (state.volume + 5).min(100);
            }
            // log_scroll counts backward from the tail (see draw_event_log's
            // doc comment), so Up (toward older messages) *increases* it --
            // the opposite of the track list's Up, which decreases its
            // (forward-counted) offset. Bounded against `log.len() - 1`
            // rather than the exact viewport-aware max_scroll (unknown here
            // -- that's a layout concern computed only in `draw`, same
            // reasoning as `PAGE_SIZE`); draw_event_log's own wheel handling
            // re-clamps to the tighter, viewport-aware bound on the next
            // frame regardless.
            KeyCode::Up if state.interaction.focus().is_focused(Id::EventLog) => {
                state.log_scroll = apply_signed(state.log_scroll, 1, log_scroll_max(state));
            }
            KeyCode::Down if state.interaction.focus().is_focused(Id::EventLog) => {
                state.log_scroll = apply_signed(state.log_scroll, -1, log_scroll_max(state));
            }
            KeyCode::PageUp if state.interaction.focus().is_focused(Id::EventLog) => {
                state.log_scroll = apply_signed(
                    state.log_scroll,
                    i32::try_from(PAGE_SIZE).unwrap_or(i32::MAX),
                    log_scroll_max(state),
                );
            }
            KeyCode::PageDown if state.interaction.focus().is_focused(Id::EventLog) => {
                state.log_scroll = apply_signed(
                    state.log_scroll,
                    -i32::try_from(PAGE_SIZE).unwrap_or(i32::MAX),
                    log_scroll_max(state),
                );
            }
            KeyCode::Home if state.interaction.focus().is_focused(Id::EventLog) => {
                state.log_scroll = log_scroll_max(state); // oldest
            }
            KeyCode::End if state.interaction.focus().is_focused(Id::EventLog) => {
                state.log_scroll = 0; // newest
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
        push_log(state, LogLevel::Dim, format!("focus -> {}", id_label(id)));
    }

    true
}

// ── Entry points ────────────────────────────────────────────────────────────
//
// Hand-rolled dispatch instead of `rg_run!`: this demo's layout (the fixed
// CONTROLS_WIDTH/LOG_HEIGHT columns, the {:<24} track-name field) was
// designed and tested against DEMO_SIZE (also used by the test module
// below), but `rg_run!`'s software branch hardcodes a much narrower 50x25
// grid with no way to override it -- more than narrow enough to truncate
// track names and push the log panel off-screen in the live window. Every
// branch below is otherwise identical to what `rg_run!` itself expands to
// (see `util::mod`'s `rg_run!`/`__rg_wasm_headless_arm!`/
// `__rg_wasm_terminal_arm!`); only the software branch's `grid_size` call
// differs, which the macro has no parameter for.

/// The grid size this demo's layout is designed for -- drives both the
/// live software-backend window and the headless test harness below, so
/// there's exactly one place that has to agree with the layout constants
/// (`CONTROLS_WIDTH`, `LOG_HEIGHT`) instead of two drifting apart.
///
/// `allow(dead_code)`: only referenced by the `feature = "software"` `main`
/// below and by `#[cfg(test)]`, so a `--features crossterm` (only) build
/// sees neither use.
#[allow(dead_code)]
const DEMO_SIZE: Size = Size {
    width: 80,
    height: 30,
};

#[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
fn __init<B: Backend>(term: &mut Terminal<B>) -> AppState {
    init(term)
}
#[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
fn __tick<B: Backend>(term: &mut Terminal<B>, state: &mut AppState) -> bool {
    tick(term, state)
}

#[cfg(feature = "software")]
fn main() {
    #[cfg(target_arch = "wasm32")]
    ::console_error_panic_hook::set_once();

    let renderer = ::retroglyph_software::SoftwareBackendBuilder::new()
        .grid_size(DEMO_SIZE.width, DEMO_SIZE.height)
        .scale(2)
        .build()
        .expect("failed to initialize software backend")
        .run_headless();
    let config =
        ::retroglyph_window::winit::WindowConfig::fit(&renderer, env!("CARGO_BIN_NAME"), None);
    let app = retroglyph_examples::util::ClosureApp::new(__init, __tick);
    ::retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
}

#[cfg(all(feature = "software", target_arch = "wasm32"))]
#[allow(missing_docs)]
#[::wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_main() -> ::std::result::Result<(), ::wasm_bindgen::JsValue> {
    main();
    ::std::result::Result::Ok(())
}

#[cfg(all(feature = "crossterm", not(feature = "software")))]
fn main() -> ::std::result::Result<(), ::std::io::Error> {
    let app = retroglyph_examples::util::ClosureApp::new(__init, __tick);
    ::retroglyph_crossterm::Crossterm::run(app)
}

retroglyph_examples::__rg_wasm_headless_arm!(AppState, __init, __tick);
retroglyph_examples::__rg_wasm_terminal_arm!(AppState, __init, __tick);

#[cfg(not(any(
    feature = "crossterm",
    feature = "software",
    all(feature = "wasm-headless", target_arch = "wasm32"),
    all(feature = "wasm-terminal", target_arch = "wasm32"),
)))]
fn main() {
    retroglyph_examples::util::run_headless(__init, __tick);
}

#[cfg(test)]
mod tests {
    use retroglyph_core::event::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use retroglyph_core::{Headless, Pos};

    use super::*;

    const SIZE: Size = DEMO_SIZE;

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

    /// Regression test for a real bug: the windowed backend's close button
    /// sends `Event::Close` (retroglyph-window leaves quitting up to the
    /// app rather than exiting on its own), but `handle` never checked for
    /// it -- only Q/Escape worked, so clicking the window's close button
    /// silently did nothing.
    #[test]
    fn close_event_quits() {
        let mut state = init_state();
        assert!(!handle(&mut state, &Event::Close));
    }

    #[test]
    fn clicking_new_button_increments_its_counter() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers New's hit rect

        let l = layout(SIZE, Density::Relaxed);
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
    fn tab_cycles_focus_new_save_delete_mute_volume_list_log() {
        let mut state = init_state();
        assert_eq!(state.interaction.focus().focused(), Some(Id::TrackList));

        frame(&mut state, &[]); // establishes the focus order from this frame's draw

        let expect = [
            Id::EventLog,
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
        frame(&mut state, &[key(KeyCode::Tab), key(KeyCode::Tab)]); // TrackList -> EventLog -> New
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

        // Move focus elsewhere (one Tab: TrackList -> EventLog); Down should
        // no longer move the selection.
        frame(&mut state, &[key(KeyCode::Tab)]);
        assert_eq!(state.interaction.focus().focused(), Some(Id::EventLog));
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
        for _ in 0..6 {
            frame(&mut state, &[key(KeyCode::Tab)]); // EventLog, New, Save, Delete, Mute, Volume
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

        let l = layout(SIZE, Density::Relaxed);
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

        let l = layout(SIZE, Density::Relaxed);
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

        let l = layout(SIZE, Density::Relaxed);
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
    fn right_clicking_a_track_toggles_its_favorite() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers track row rects
        assert!(!state.tracks[1].favorite);

        let l = layout(SIZE, Density::Relaxed);
        let inner = inset(l.list);
        let second_row = Pos::new(inner.left(), inner.top() + 1);
        let right_click = [
            mouse_at(MouseEventKind::Down(MouseButton::Right), second_row),
            mouse_at(MouseEventKind::Up(MouseButton::Right), second_row),
        ];
        frame(&mut state, &right_click); // frame 2: delivers it, not yet resolved
        assert!(!state.tracks[1].favorite);

        frame(&mut state, &[]); // frame 3: resolves it
        assert!(state.tracks[1].favorite);
        // Left-click behavior on the same row is untouched by adding
        // SECONDARY_CLICK sensing.
        assert_eq!(state.list_state.selected(), Some(0));
    }

    #[test]
    fn page_down_and_end_jump_the_selection_without_wrapping() {
        let mut state = init_state(); // TrackList focused by default
        frame(&mut state, &[]);

        frame(&mut state, &[key(KeyCode::PageDown)]);
        assert_eq!(state.list_state.selected(), Some(PAGE_SIZE));

        frame(&mut state, &[key(KeyCode::End)]);
        assert_eq!(state.list_state.selected(), Some(state.tracks.len() - 1));

        // PageDown at the end clamps rather than wrapping back to the start.
        frame(&mut state, &[key(KeyCode::PageDown)]);
        assert_eq!(state.list_state.selected(), Some(state.tracks.len() - 1));

        frame(&mut state, &[key(KeyCode::Home)]);
        assert_eq!(state.list_state.selected(), Some(0));

        frame(&mut state, &[key(KeyCode::PageUp)]);
        assert_eq!(state.list_state.selected(), Some(0)); // clamps, doesn't wrap
    }

    #[test]
    fn scrolling_the_track_list_advances_its_offset() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers the list container's hit rect

        let l = layout(SIZE, Density::Relaxed);
        let inner = inset(l.list);
        let mid = Pos::new(inner.left(), inner.top());

        frame(&mut state, &[mouse_at(MouseEventKind::ScrollDown, mid)]); // frame 2: not yet resolved
        assert_eq!(state.list_state.offset(), 0);

        frame(&mut state, &[]); // frame 3: resolves the scroll
        assert!(state.list_state.offset() > 0);
    }

    /// Regression test for a real bug: `ensure_visible` used to run
    /// unconditionally on every frame that wasn't itself a scroll event,
    /// which is nearly every frame (wheel events are one-shot) -- so a free
    /// scroll would visibly move for exactly one frame and then snap
    /// straight back to the selected row the instant the wheel stopped.
    /// Fixed by only auto-following the selection on the frame after it
    /// actually changes (`AppState::follow_selection`).
    #[test]
    fn scrolling_stays_put_on_later_frames_with_no_further_input() {
        let mut state = init_state();
        frame(&mut state, &[]); // frame 1: registers the list container's hit rect

        let l = layout(SIZE, Density::Relaxed);
        let inner = inset(l.list);
        let mid = Pos::new(inner.left(), inner.top());

        frame(&mut state, &[mouse_at(MouseEventKind::ScrollDown, mid)]);
        frame(&mut state, &[]); // resolves the scroll
        let scrolled_offset = state.list_state.offset();
        assert!(scrolled_offset > 0);

        // Several more frames with no new input at all -- selection never
        // changed, so the view must not snap back to it.
        for _ in 0..5 {
            frame(&mut state, &[]);
            assert_eq!(state.list_state.offset(), scrolled_offset);
        }
    }

    /// Regression test for a real bug: `ListState::scroll_by` has no upper
    /// clamp by design (only the caller knows content length), and nothing
    /// here clamped it either -- so wheel-scrolling down past the last page
    /// pushed the offset arbitrarily far beyond `tracks.len()`, leaving the
    /// list blank until scrolled all the way back.
    #[test]
    fn scrolling_down_clamps_at_the_last_page() {
        let mut state = init_state();
        frame(&mut state, &[]);

        let l = layout(SIZE, Density::Relaxed);
        let inner = inset(l.list);
        let mid = Pos::new(inner.left(), inner.top());
        let visible_rows = inner.height() as usize;
        let max_offset = state.tracks.len().saturating_sub(visible_rows);

        // Scroll down far more than there is content for.
        for _ in 0..(state.tracks.len() + 10) {
            frame(&mut state, &[mouse_at(MouseEventKind::ScrollDown, mid)]);
            frame(&mut state, &[]); // resolve each one
        }

        assert_eq!(state.list_state.offset(), max_offset);
    }

    #[test]
    fn scrolling_the_event_log_moves_log_scroll_backward_through_history() {
        let mut state = init_state();
        // Build up more log history than fits in the viewport (the log
        // starts empty, and max_scroll is 0 -- and any scroll a no-op --
        // until there's more than a screenful of lines); each Tab logs a
        // "focus -> ..." line.
        for _ in 0..12 {
            frame(&mut state, &[key(KeyCode::Tab)]);
        }

        let l = layout(SIZE, Density::Relaxed);
        let inner = inset(l.log);
        let mid = Pos::new(inner.left(), inner.top());

        // Scrolling *up* (toward older messages) should *increase*
        // log_scroll -- the opposite sign relationship a list has, since
        // `log` counts backward from the tail (see draw_event_log's doc
        // comment).
        frame(&mut state, &[mouse_at(MouseEventKind::ScrollUp, mid)]); // not yet resolved
        assert_eq!(state.log_scroll, 0);

        frame(&mut state, &[]); // frame 3: resolves the scroll
        assert!(state.log_scroll > 0);
    }

    #[test]
    fn home_and_end_jump_the_log_to_the_oldest_and_newest_message() {
        let mut state = init_state();
        frame(&mut state, &[]); // establishes the focus order

        // A handful of Tabs each log a "focus -> ..." line (see `handle`),
        // building up enough log history for Home/End to mean something.
        // The focus ring has 7 stops, so 8 tabs lands back on EventLog
        // (the first stop from TrackList) with a full cycle of history
        // logged.
        for _ in 0..8 {
            frame(&mut state, &[key(KeyCode::Tab)]);
        }
        assert_eq!(state.interaction.focus().focused(), Some(Id::EventLog));

        frame(&mut state, &[key(KeyCode::Home)]);
        assert_eq!(state.log_scroll, log_scroll_max(&state));
        assert!(
            log_scroll_max(&state) > 0,
            "log needs history for this test to mean anything"
        );

        frame(&mut state, &[key(KeyCode::End)]);
        assert_eq!(state.log_scroll, 0);
    }
}
