//! 15: Outpost dashboard
//!
//! A larger, composed dashboard: a pannable outpost map, animated stat readouts, and a detail
//! panel that opens as a bottom sheet on narrow terminals or a persistent sidebar on wide ones.
//! Bigger than the other examples on purpose, because the point is how the pieces compose
//! rather than any single capability.
//!
//! Four stat readouts ([`Tween`]-driven, retargeted on a fixed schedule, deterministic, no RNG)
//! count up or down toward a new value instead of jumping, the same easing `08_animation` uses
//! for motion applied to numeric text instead. Every tappable control (nav tabs, header buttons,
//! sheet actions) is sized to WCAG 2.2's SC 2.5.8 minimum touch target (at least 24x24 CSS px;
//! this example enforces at least 6 columns by 3 rows, comfortably above that on every backend's
//! worst-case cell size, see `touch_targets_meet_minimums`), shows a pressed state while held,
//! and cancels if the pointer slides off before release. Every mouse action also has a
//! keyboard-equivalent path (Tab, number keys, arrows, Enter), so nothing here is mouse-only.
//!
//! Below [`BP_WIDE`] the outpost detail opens as a bottom sheet over the map; at or above it, a
//! persistent sidebar replaces the sheet entirely. That's a real layout decision, not just the
//! same shapes resized (`14_resize` covers resizing). [`Camera`], reused from `12_dungeon_scroll`,
//! pans a world (see [`WORLD_W`]/[`WORLD_H`]) large enough that it doesn't already fit in any
//! reasonable terminal or window; `Camera` itself has no notion of zoom, only pan, so this
//! exercises scrolling, not scaling. `REPAIR`/`INSPECT` spawn a floating `+N` that rises and
//! fades over about a second, the same per-frame lerp/tween idea as the stat tiles applied to a
//! one-shot effect instead of a continuous readout.
//!
//! ```sh
//! cargo run --example 15_outpost_dashboard --features crossterm
//! cargo run --example 15_outpost_dashboard --features software
//! cargo run --example 15_outpost_dashboard  # headless fallback, prints a few frames to stdout
//! ```
//!
//! # Keys
//!
//! - Tap/click: nav tabs, header buttons, map tiles, sheet/sidebar actions
//! - Drag on the map (or scroll wheel): pan the camera; only a handful of world tiles hold a
//!   structure, so most of the map is empty ground
//! - Arrow keys: move the map cursor (also pans); Enter/Space: select the tile under it
//! - Tab/Shift+Tab or 1-2: switch tabs
//! - Escape: close an open sheet/settings panel, then quit; Q: quit

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use retroglyph_core::app::Frame;
use retroglyph_core::backend::Backend;
use retroglyph_core::color::{Color, Style};
use retroglyph_core::event::{Event, KeyCode, MouseButton, MouseEventKind};
use retroglyph_core::grid::{HasSize, Pos, Rect, Size};
use retroglyph_core::surface::Surface;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;
use retroglyph_ui::align::Align;
use retroglyph_ui::animate::{Easing, Tween};
use retroglyph_ui::camera::Camera;
use retroglyph_ui::layout::{Constraint, Layout, split_h, split_v};
use retroglyph_ui::text::{draw_clipped, truncate};

// ── Breakpoints ────────────────────────────────────────────────────────────

/// At or above this width, a persistent sidebar replaces the bottom sheet.
const BP_WIDE: u16 = 84;
/// Below this height, chrome compresses and touch minimums are waived (a terminal this short is
/// keyboard-first by definition).
const BP_SHORT: u16 = 16;

// ── Touch target minimums (WCAG 2.2 SC 2.5.8 AA, translated to cells; see module docs) ─────

/// Minimum interactive target width, in cells. `pub` so `touch_targets_meet_minimums` (in the
/// test file included via `#[path]`) can check against the same constant this module enforces.
pub const MIN_TARGET_W: u16 = 6;
/// Minimum interactive target height, in cells; see [`MIN_TARGET_W`].
pub const MIN_TARGET_H: u16 = 3;

// ── Palette ──────────────────────────────────────────────────────────────────

const BG: Color = Color::Rgb {
    r: 16,
    g: 17,
    b: 24,
};
const PANEL_BG: Color = Color::Rgb {
    r: 22,
    g: 24,
    b: 34,
};
const CHROME_BG: Color = Color::Rgb {
    r: 27,
    g: 24,
    b: 40,
};
const BUTTON_BG: Color = Color::Rgb {
    r: 42,
    g: 40,
    b: 62,
};
const BORDER: Color = Color::Rgb {
    r: 88,
    g: 78,
    b: 118,
};
const FG: Color = Color::Rgb {
    r: 218,
    g: 216,
    b: 230,
};
const DIM_FG: Color = Color::Rgb {
    r: 128,
    g: 126,
    b: 146,
};
const ACCENT: Color = Color::Rgb {
    r: 248,
    g: 198,
    b: 90,
};
const GOOD: Color = Color::Rgb {
    r: 108,
    g: 208,
    b: 138,
};
const BAD: Color = Color::Rgb {
    r: 228,
    g: 92,
    b: 100,
};

// ── Outpost structures ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Structure {
    Watchtower,
    Barracks,
    Well,
    Wall,
}

impl Structure {
    const fn glyph(self) -> char {
        match self {
            Self::Watchtower => '▲',
            Self::Barracks => '■',
            Self::Well => 'o',
            Self::Wall => '#',
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::Watchtower => ACCENT,
            Self::Barracks => Color::Rgb {
                r: 150,
                g: 170,
                b: 210,
            },
            Self::Well => Color::Rgb {
                r: 110,
                g: 190,
                b: 210,
            },
            Self::Wall => BORDER,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Watchtower => "Watchtower",
            Self::Barracks => "Barracks",
            Self::Well => "Well",
            Self::Wall => "Perimeter wall",
        }
    }

    const fn detail(self) -> &'static str {
        match self {
            Self::Watchtower => "Line of sight: 12 tiles. Fully staffed.",
            Self::Barracks => "Houses the garrison. Bunks: 24/30.",
            Self::Well => "Fresh water. Yield steady.",
            Self::Wall => "Structural integrity nominal.",
        }
    }
}

/// World width, in cells -- deliberately much larger than any reasonable terminal or window, so
/// panning always has somewhere to go (see the module doc comment for the 22x14 draft that
/// didn't).
pub const WORLD_W: u16 = 140;
/// World height, in cells; see [`WORLD_W`].
pub const WORLD_H: u16 = 80;

const _: () = assert!(
    WORLD_W > 120 && WORLD_H > 60,
    "world must stay comfortably larger than any real terminal/window or panning has nowhere \
     to go -- see the module doc comment for the 22x14 draft that shipped this exact bug"
);

struct Building {
    pos: Pos,
    kind: Structure,
}

/// Hand-placed (no RNG, so every run and every snapshot is identical) structures scattered across
/// the world -- four clusters far enough apart that panning between them is obviously necessary,
/// plus a perimeter wall ringing the home cluster.
fn outpost_layout() -> Vec<Building> {
    vec![
        // Home cluster, near the world's center.
        Building {
            pos: Pos::new(70, 40),
            kind: Structure::Watchtower,
        },
        Building {
            pos: Pos::new(66, 42),
            kind: Structure::Barracks,
        },
        Building {
            pos: Pos::new(74, 43),
            kind: Structure::Well,
        },
        Building {
            pos: Pos::new(63, 37),
            kind: Structure::Wall,
        },
        Building {
            pos: Pos::new(78, 37),
            kind: Structure::Wall,
        },
        Building {
            pos: Pos::new(63, 45),
            kind: Structure::Wall,
        },
        Building {
            pos: Pos::new(78, 45),
            kind: Structure::Wall,
        },
        // Outlying watch posts, each a full screen or more away from the home cluster.
        Building {
            pos: Pos::new(12, 8),
            kind: Structure::Watchtower,
        },
        Building {
            pos: Pos::new(15, 12),
            kind: Structure::Wall,
        },
        Building {
            pos: Pos::new(128, 10),
            kind: Structure::Watchtower,
        },
        Building {
            pos: Pos::new(124, 14),
            kind: Structure::Well,
        },
        Building {
            pos: Pos::new(20, 70),
            kind: Structure::Barracks,
        },
        Building {
            pos: Pos::new(24, 74),
            kind: Structure::Wall,
        },
        Building {
            pos: Pos::new(120, 72),
            kind: Structure::Watchtower,
        },
        Building {
            pos: Pos::new(116, 68),
            kind: Structure::Wall,
        },
    ]
}

fn building_at(buildings: &[Building], pos: Pos) -> Option<&Building> {
    buildings.iter().find(|b| b.pos == pos)
}

/// Sparse, purely decorative ground texture: a scrub patch on a deterministic (RNG-free)
/// fraction of empty tiles, so panning across open ground reads as "more world," not a repeating
/// blank field.
const fn is_scrub(pos: Pos) -> bool {
    (pos.x as u32)
        .wrapping_mul(31)
        .wrapping_add((pos.y as u32).wrapping_mul(17))
        .is_multiple_of(23)
}

// ── Tabs ─────────────────────────────────────────────────────────────────────

/// Which top-level screen is active. `pub` (not `pub(crate)`) only so the test module included
/// via `#[path]` can name it; not otherwise meant as public API.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    /// The pannable outpost map plus its detail sidebar/sheet.
    Overview,
    /// A couple of toggles, to prove the settings-panel/modal pattern.
    Settings,
}

impl Tab {
    const ALL: [Self; 2] = [Self::Overview, Self::Settings];

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Settings => "Settings",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Settings => 1,
        }
    }
}

// ── Animated stat tiles ──────────────────────────────────────────────────────

/// One live-ish readout: a [`Tween`] retargeted on a fixed schedule rather than jumping straight
/// to the new value, so the number visibly counts toward it -- the same continuous-interpolation
/// idea `08_animation` uses for motion, applied to text.
struct Stat {
    label: &'static str,
    unit: &'static str,
    value: Tween,
    color: Color,
    schedule: [f32; 4],
    next: usize,
}

impl Stat {
    const fn new(
        label: &'static str,
        unit: &'static str,
        start: f32,
        schedule: [f32; 4],
        color: Color,
    ) -> Self {
        Self {
            label,
            unit,
            value: Tween::new(start, start)
                .duration(std::time::Duration::from_millis(900))
                .easing(Easing::EaseInOutCubic),
            color,
            schedule,
            next: 0,
        }
    }

    fn retarget_next(&mut self) {
        let target = self.schedule[self.next % self.schedule.len()];
        self.next += 1;
        self.value.retarget(target);
    }
}

// ── Pointer + hit testing ────────────────────────────────────────────────────

/// One registered tap/click target, rebuilt every `draw()` call. `pub` for the same
/// test-visibility reason as [`Tab`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitTarget {
    /// A nav-bar tab.
    Tab(Tab),
    /// The header's settings-gear button.
    SettingsGear,
    /// The header's notification-bell button.
    AlertBell,
    /// The detail sheet/sidebar's close button.
    SheetClose,
    /// The detail panel's "repair" action.
    Repair,
    /// The detail panel's "inspect" action.
    Inspect,
    /// One settings toggle, by index.
    ToggleSetting(usize),
}

struct PointerState {
    start: Pos,
    last: Pos,
    on_map: bool,
    pressed: Option<Rect>,
    dragging: bool,
}

/// A one-shot `+N`-style acknowledgement that rises and fades over about a second -- the
/// animated feedback `REPAIR`/`INSPECT` give, with no economy behind it (see the module doc
/// comment). Screen-space, not world-space: it's chrome, not part of the map.
struct FloatingText {
    x: f32,
    y: f32,
    text: String,
    color: Color,
    born: f64,
}

/// How long a [`FloatingText`] stays visible before being dropped.
const FLOATING_TEXT_LIFETIME: f64 = 1.1;
/// How fast a [`FloatingText`] rises, in cells per second.
const FLOATING_TEXT_RISE: f32 = 6.0;

// ── State ────────────────────────────────────────────────────────────────────

/// State for the outpost dashboard example: animated stats, camera/pan state, active tab, and
/// the current frame's hit-test targets (rebuilt every draw).
pub struct OutpostDashboard {
    time: f64,
    retarget_every: f64,
    next_retarget_at: f64,

    buildings: Vec<Building>,
    camera: Camera,
    /// World position the camera is centered on.
    pub cam_center: Pos,
    /// World position of the keyboard cursor.
    pub cursor: Pos,
    /// World position of the currently-selected building, if any.
    pub selected: Option<Pos>,
    /// The map's screen [`Rect`] from the most recent draw, for pointer picking.
    pub last_map_rect: Option<Rect>,
    pointer: Option<PointerState>,

    /// The active top-level tab.
    pub tab: Tab,
    stats: Vec<Stat>,
    notifications: u32,
    settings_toggles: [bool; 2],
    /// Whether the narrow-layout bottom sheet is open.
    pub sheet_open: bool,

    /// Every registered tap/click target from the most recent draw.
    pub hitboxes: Vec<(Rect, HitTarget)>,

    floating: Vec<FloatingText>,
}

impl Default for OutpostDashboard {
    fn default() -> Self {
        let camera = Camera::new(Rect::new(0, 0, 10, 6), Size::new(WORLD_W, WORLD_H));
        Self {
            time: 0.0,
            retarget_every: 2.4,
            next_retarget_at: 2.4,
            buildings: outpost_layout(),
            camera,
            cam_center: Pos::new(70, 40),
            cursor: Pos::new(70, 40),
            selected: None,
            last_map_rect: None,
            pointer: None,
            tab: Tab::Overview,
            stats: vec![
                Stat::new("REQ/S", "", 120.0, [420.0, 95.0, 310.0, 180.0], ACCENT),
                Stat::new("P99", "ms", 40.0, [180.0, 60.0, 25.0, 90.0], BAD),
                Stat::new("ERR", "%", 0.4, [2.8, 0.1, 1.2, 0.3], GOOD),
                Stat::new(
                    "CONN",
                    "",
                    812.0,
                    [1450.0, 640.0, 990.0, 730.0],
                    Color::Rgb {
                        r: 130,
                        g: 190,
                        b: 230,
                    },
                ),
            ],
            notifications: 2,
            settings_toggles: [true, false],
            sheet_open: false,
            hitboxes: Vec::new(),
            floating: Vec::new(),
        }
    }
}

impl OutpostDashboard {
    /// The camera's current visible world rectangle, for tests confirming a drag/pan sequence
    /// actually moved the viewport rather than being clamped right back to the origin.
    #[must_use]
    pub const fn camera_origin(&self) -> Pos {
        self.camera.origin()
    }

    /// How many rising/fading `REPAIR`/`INSPECT` acknowledgements are currently live, for tests
    /// confirming one was actually spawned without depending on [`FloatingText`]'s private
    /// fields.
    #[must_use]
    pub const fn floating_count(&self) -> usize {
        self.floating.len()
    }

    /// Pans the camera by a screen-cell delta via [`Camera::scroll_by`], then syncs `cam_center`
    /// to the result so the unconditional `center_on(cam_center)` in [`Self::draw_map`] (there to
    /// pick up viewport resizes every frame) reapplies the same origin instead of undoing the
    /// pan: `scroll_by` clamps `origin` directly, one clamp with no dead zone, unlike clamping a
    /// tracked center to `[0, world)` and re-deriving the origin through `center_on` every frame.
    fn pan_by(&mut self, dx: i32, dy: i32) {
        self.camera.scroll_by(dx, dy);
        let origin = self.camera.origin();
        let vp = self.camera.viewport();
        self.cam_center = Pos::new(
            (u32::from(origin.x) + u32::from(vp.width()) / 2).min(u32::from(WORLD_W) - 1) as u16,
            (u32::from(origin.y) + u32::from(vp.height()) / 2).min(u32::from(WORLD_H) - 1) as u16,
        );
    }

    fn hit_at(&self, pos: Pos) -> Option<(Rect, HitTarget)> {
        self.hitboxes
            .iter()
            .rev()
            .find(|(r, _)| r.contains_pos(pos))
            .copied()
    }

    fn advance(&mut self, dt: std::time::Duration) {
        self.time += dt.as_secs_f64();
        for stat in &mut self.stats {
            stat.value.update(dt);
        }
        if self.time >= self.next_retarget_at {
            self.next_retarget_at = self.time + self.retarget_every;
            for stat in &mut self.stats {
                stat.retarget_next();
            }
        }

        let now = self.time;
        self.floating
            .retain(|f| now - f.born < FLOATING_TEXT_LIFETIME);
        let rise = FLOATING_TEXT_RISE * dt.as_secs_f32();
        for f in &mut self.floating {
            f.y -= rise;
        }
    }

    /// Spawns a rising, fading acknowledgement at `at` (screen coordinates), one row above so it
    /// doesn't visually fuse with whatever label triggered it on the first frame.
    fn push_float(&mut self, at: Pos, text: impl Into<String>, color: Color) {
        self.floating.push(FloatingText {
            x: f32::from(at.x),
            y: f32::from(at.y) - 1.0,
            text: text.into(),
            color,
            born: self.time,
        });
    }

    // ── Input ────────────────────────────────────────────────────────────────

    fn move_cursor(&mut self, dx: i32, dy: i32) {
        let nx = (i32::from(self.cursor.x) + dx).clamp(0, i32::from(WORLD_W) - 1);
        let ny = (i32::from(self.cursor.y) + dy).clamp(0, i32::from(WORLD_H) - 1);
        self.cursor = Pos::new(nx as u16, ny as u16);
        self.cam_center = self.cursor;
    }

    fn activate(&mut self, target: HitTarget, at: Pos) {
        match target {
            HitTarget::Tab(tab) => self.tab = tab,
            HitTarget::SettingsGear => self.tab = Tab::Settings,
            HitTarget::AlertBell => self.notifications = self.notifications.saturating_sub(1),
            HitTarget::SheetClose => {
                self.selected = None;
                self.sheet_open = false;
            }
            // Purely cosmetic acknowledgement -- no economy to simulate, just the rising/fading
            // `+N` feedback itself.
            HitTarget::Repair => self.push_float(at, "+5", GOOD),
            HitTarget::Inspect => self.push_float(at, "+1", FG),
            HitTarget::ToggleSetting(i) => {
                if let Some(t) = self.settings_toggles.get_mut(i) {
                    *t = !*t;
                }
            }
        }
    }

    fn on_pointer_down(&mut self, pos: Pos) {
        let hit = self.hit_at(pos);
        let on_map = hit.is_none()
            && self.tab == Tab::Overview
            && self.last_map_rect.is_some_and(|r| r.contains_pos(pos));
        self.pointer = Some(PointerState {
            start: pos,
            last: pos,
            on_map,
            pressed: hit.map(|(r, _)| r),
            dragging: false,
        });
    }

    fn on_pointer_move(&mut self, pos: Pos) {
        let Some(p) = self.pointer.as_mut() else {
            return;
        };
        if pos != p.start {
            p.dragging = true;
        }
        let (dx, dy) = (
            i32::from(pos.x) - i32::from(p.last.x),
            i32::from(pos.y) - i32::from(p.last.y),
        );
        let on_map = p.on_map;
        p.last = pos;
        if on_map && (dx != 0 || dy != 0) {
            self.pan_by(-dx, -dy);
        }
    }

    fn on_pointer_up(&mut self, pos: Pos) {
        let Some(p) = self.pointer.take() else { return };
        if p.on_map {
            if !p.dragging
                && let Some(map_rect) = self.last_map_rect
                && map_rect.contains_pos(pos)
                && let Some(world) = self.camera.screen_to_world(pos)
            {
                self.selected = Some(world);
                self.cursor = world;
                self.sheet_open = true;
            }
            return;
        }
        // Slide-off cancel: only activate if release lands on the same control the press
        // started on.
        if let (Some((r1, _)), Some((r2, target))) = (self.hit_at(p.start), self.hit_at(pos))
            && r1 == r2
        {
            self.activate(target, pos);
        }
    }

    fn on_scroll(&mut self, pos: Pos, dy: i32) {
        if self.tab == Tab::Overview
            && self.hit_at(pos).is_none()
            && self.last_map_rect.is_some_and(|r| r.contains_pos(pos))
        {
            self.pan_by(0, dy * 2);
        }
    }

    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(k) if k.is_down() => match k.code {
                    KeyCode::Escape => {
                        if self.sheet_open || self.selected.is_some() {
                            self.selected = None;
                            self.sheet_open = false;
                        } else {
                            return false;
                        }
                    }
                    KeyCode::Char('q' | 'Q') => return false,
                    KeyCode::Up if self.tab == Tab::Overview => self.move_cursor(0, -1),
                    KeyCode::Down if self.tab == Tab::Overview => self.move_cursor(0, 1),
                    KeyCode::Left if self.tab == Tab::Overview => self.move_cursor(-1, 0),
                    KeyCode::Right if self.tab == Tab::Overview => self.move_cursor(1, 0),
                    KeyCode::Enter | KeyCode::Char(' ') if self.tab == Tab::Overview => {
                        self.selected = Some(self.cursor);
                        self.sheet_open = true;
                    }
                    KeyCode::Tab => self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()],
                    KeyCode::BackTab => {
                        self.tab =
                            Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()];
                    }
                    KeyCode::Char(c @ ('1' | '2')) => {
                        let i = (c as u8 - b'1') as usize;
                        if let Some(&tab) = Tab::ALL.get(i) {
                            self.tab = tab;
                        }
                    }
                    _ => {}
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => self.on_pointer_down(m.position),
                    MouseEventKind::Moved => self.on_pointer_move(m.position),
                    MouseEventKind::Up(MouseButton::Left) => self.on_pointer_up(m.position),
                    MouseEventKind::Scroll { dy, .. } if dy > 0.0 => {
                        self.on_scroll(m.position, -1);
                    }
                    MouseEventKind::Scroll { dy, .. } if dy < 0.0 => {
                        self.on_scroll(m.position, 1);
                    }
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    // ── Drawing ────────────────────────────────────────────────────────────

    fn draw_button(
        surface: &mut Surface<'_>,
        hitboxes: &mut Vec<(Rect, HitTarget)>,
        pointer: Option<&PointerState>,
        rect: Rect,
        label: &str,
        fg: Color,
        target: HitTarget,
    ) {
        if rect.width() == 0 || rect.height() == 0 {
            return;
        }
        let pressed = pointer.is_some_and(|p| p.pressed == Some(rect) && rect.contains_pos(p.last));
        let bg = if pressed {
            Color::lerp(BUTTON_BG, fg, 0.35)
        } else {
            BUTTON_BG
        };
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                surface.put((x, y), ' ', Style::new().bg(bg));
            }
        }
        let ty = rect.top() + rect.height() / 2;
        let style = Style::new().fg(fg).bg(bg);
        let _ = draw_clipped(
            surface,
            (rect.left() + 1, ty),
            rect.width().saturating_sub(2),
            label,
            Align::Center,
            style,
        );
        hitboxes.push((rect, target));
    }

    fn draw_header(&mut self, surface: &mut Surface<'_>, area: Rect) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                surface.put((x, y), ' ', Style::new().bg(CHROME_BG));
            }
        }
        surface.print(
            (area.left() + 1, area.top()),
            "Outpost 7 -- Ridgeline Watch",
            Style::new().fg(ACCENT).bg(CHROME_BG),
        );

        if area.height() < 4 {
            return;
        }

        // Four animated stat tiles, side by side.
        let cols = Layout::horizontal([Constraint::Fill(1); 4])
            .spacing(1)
            .split(Rect::new(
                area.left(),
                area.top() + 1,
                area.width().saturating_sub(2 * MIN_TARGET_W + 2),
                3,
            ));
        for (col, stat) in cols.iter().zip(&self.stats) {
            if col.width() < 4 {
                continue;
            }
            let value = stat.value.value();
            let text = if stat.unit.is_empty() {
                format!("{} {value:.0}", stat.label)
            } else {
                format!("{} {value:.1}{}", stat.label, stat.unit)
            };
            surface.print(
                (col.left(), col.top() + 1),
                truncate(&text, col.width_usize()),
                Style::new().fg(stat.color).bg(CHROME_BG),
            );
        }

        let gear = Rect::new(
            area.right().saturating_sub(MIN_TARGET_W),
            area.top() + 1,
            MIN_TARGET_W,
            MIN_TARGET_H,
        );
        Self::draw_button(
            surface,
            &mut self.hitboxes,
            self.pointer.as_ref(),
            gear,
            "SET",
            FG,
            HitTarget::SettingsGear,
        );
        let bell_text = format!("!{}", self.notifications.min(9));
        let bell = Rect::new(
            gear.left().saturating_sub(MIN_TARGET_W + 1),
            area.top() + 1,
            MIN_TARGET_W,
            MIN_TARGET_H,
        );
        let bell_color = if self.notifications > 0 { BAD } else { DIM_FG };
        Self::draw_button(
            surface,
            &mut self.hitboxes,
            self.pointer.as_ref(),
            bell,
            &bell_text,
            bell_color,
            HitTarget::AlertBell,
        );
    }

    fn draw_map(&mut self, surface: &mut Surface<'_>, area: Rect) {
        if area.width() < 2 || area.height() < 2 {
            return;
        }
        self.camera.set_viewport(area);
        self.camera.center_on(self.cam_center);
        self.last_map_rect = Some(area);

        for (world_pos, screen_pos) in self.camera.cells() {
            let building = building_at(&self.buildings, world_pos);
            let (glyph, color) = building.map_or_else(
                || {
                    if is_scrub(world_pos) {
                        (
                            ',',
                            Color::Rgb {
                                r: 90,
                                g: 100,
                                b: 70,
                            },
                        )
                    } else {
                        (
                            '.',
                            Color::Rgb {
                                r: 60,
                                g: 66,
                                b: 54,
                            },
                        )
                    }
                },
                |b| (b.kind.glyph(), b.kind.color()),
            );
            let is_selected = self.selected == Some(world_pos);
            let is_cursor = self.cursor == world_pos;
            let style = if is_selected {
                Style::new()
                    .fg(Color::lerp(color, Color::BRIGHT_WHITE, 0.3))
                    .bg(Color::lerp(PANEL_BG, ACCENT, 0.2))
            } else if is_cursor {
                Style::new().fg(color).bg(Color::Rgb {
                    r: 38,
                    g: 42,
                    b: 58,
                })
            } else {
                Style::new().fg(color).bg(BG)
            };
            surface.put((screen_pos.x, screen_pos.y), glyph, style);
        }
        if area.width() >= 26 && area.height() >= 4 {
            let _ = draw_clipped(
                surface,
                (area.left(), area.bottom() - 1),
                area.width().saturating_sub(1),
                "drag: pan   tap: select",
                Align::Right,
                Style::new().fg(DIM_FG).bg(BG),
            );
        }
    }

    fn draw_detail_panel(&mut self, surface: &mut Surface<'_>, area: Rect, is_sheet: bool) {
        if area.width() < 8 || area.height() < 4 {
            return;
        }
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                surface.put((x, y), ' ', Style::new().bg(PANEL_BG));
            }
        }
        for x in area.left()..area.right() {
            surface.put((x, area.top()), '-', Style::new().fg(BORDER).bg(PANEL_BG));
        }
        let inner = Rect::new(
            area.left() + 1,
            area.top() + 1,
            area.width().saturating_sub(2),
            area.height().saturating_sub(2),
        );

        let Some(sel) = self.selected else {
            surface.print(
                (inner.left(), inner.top() + 1),
                truncate("Select a tile to inspect it.", inner.width_usize()),
                Style::new().fg(DIM_FG).bg(PANEL_BG),
            );
            return;
        };

        if is_sheet {
            let close = Rect::new(
                inner.right().saturating_sub(MIN_TARGET_W),
                inner.top(),
                MIN_TARGET_W,
                MIN_TARGET_H,
            );
            Self::draw_button(
                surface,
                &mut self.hitboxes,
                self.pointer.as_ref(),
                close,
                "x",
                DIM_FG,
                HitTarget::SheetClose,
            );
        }

        let building = building_at(&self.buildings, sel);
        let title = building.map_or("Open ground", |b| b.kind.label());
        let detail = building.map_or("Empty. Good for expansion.", |b| b.kind.detail());
        let color = building.map_or(FG, |b| b.kind.color());
        let text_w = inner
            .width()
            .saturating_sub(if is_sheet { MIN_TARGET_W + 1 } else { 0 })
            as usize;

        surface.print(
            (inner.left(), inner.top()),
            truncate(title, text_w),
            Style::new().fg(color).bg(PANEL_BG),
        );
        let dim_style = Style::new().fg(DIM_FG).bg(PANEL_BG);
        surface.print(
            (inner.left(), inner.top() + 1),
            truncate(&format!("({}, {})", sel.x, sel.y), text_w),
            dim_style,
        );
        surface.print(
            (inner.left(), inner.top() + 2),
            truncate(detail, text_w),
            dim_style,
        );

        let y = inner.top() + 4;
        if y + 3 <= inner.bottom() + 1 {
            let bw = (inner.width().saturating_sub(1)) / 2;
            let repair = Rect::new(inner.left(), y, bw, 3);
            let inspect = Rect::new(inner.left() + bw + 1, y, bw, 3);
            Self::draw_button(
                surface,
                &mut self.hitboxes,
                self.pointer.as_ref(),
                repair,
                "REPAIR",
                GOOD,
                HitTarget::Repair,
            );
            Self::draw_button(
                surface,
                &mut self.hitboxes,
                self.pointer.as_ref(),
                inspect,
                "INSPECT",
                FG,
                HitTarget::Inspect,
            );
        }
    }

    fn draw_settings(&mut self, surface: &mut Surface<'_>, area: Rect) {
        if area.width() < 4 || area.height() < 4 {
            return;
        }
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                surface.put((x, y), ' ', Style::new().bg(PANEL_BG));
            }
        }
        surface.print(
            (area.left() + 1, area.top() + 1),
            "Settings",
            Style::new().fg(FG).bg(PANEL_BG),
        );

        let labels = ["Sound", "Notifications"];
        let toggles = self.settings_toggles;
        let mut y = area.top() + 3;
        for (i, label) in labels.iter().enumerate() {
            if y + 3 > area.bottom() {
                break;
            }
            let on = toggles[i];
            let text = format!("{} {label}", if on { "[x]" } else { "[ ]" });
            let btn = Rect::new(
                area.left() + 1,
                y,
                area.width().saturating_sub(2).max(MIN_TARGET_W),
                3,
            );
            let color = if on { GOOD } else { DIM_FG };
            Self::draw_button(
                surface,
                &mut self.hitboxes,
                self.pointer.as_ref(),
                btn,
                &text,
                color,
                HitTarget::ToggleSetting(i),
            );
            y += 4;
        }
    }

    fn draw_nav_bar(&mut self, surface: &mut Surface<'_>, area: Rect) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                surface.put((x, y), ' ', Style::new().bg(CHROME_BG));
            }
        }
        let n = Tab::ALL.len() as u16;
        let slot_w = area.width() / n;
        let extra = area.width() % n;
        let mut x = area.left();
        for (i, tab) in Tab::ALL.into_iter().enumerate() {
            let w = slot_w + u16::from((i as u16) < extra);
            let rect = Rect::new(x, area.top(), w, area.height());
            let active = self.tab == tab;
            let pressed = self
                .pointer
                .as_ref()
                .is_some_and(|p| p.pressed == Some(rect) && rect.contains_pos(p.last));
            let bg = if pressed { BUTTON_BG } else { CHROME_BG };
            let fg = if active { ACCENT } else { DIM_FG };
            for yy in rect.top()..rect.bottom() {
                for xx in rect.left()..rect.right() {
                    surface.put((xx, yy), ' ', Style::new().bg(bg));
                }
            }
            let label = tab.label();
            let ly = rect.top() + rect.height() / 2;
            let _ = draw_clipped(
                surface,
                (x, ly),
                w,
                label,
                Align::Center,
                Style::new().fg(fg).bg(bg),
            );
            self.hitboxes.push((rect, HitTarget::Tab(tab)));
            x += w;
        }
    }

    /// Lays out and draws the whole screen from `term.size()`, rebuilding [`Self::hitboxes`].
    /// `pub` (not `pub(crate)`) only so tests can call it directly to set up specific layout
    /// states before asserting on `hitboxes`.
    pub fn draw<B: Backend>(&mut self, term: &mut Terminal<B>) {
        let size = term.size();
        let mut surface = term.surface();
        self.hitboxes.clear();
        let screen = Rect::new(0, 0, size.width(), size.height());
        for y in 0..size.height() {
            for x in 0..size.width() {
                surface.put((x, y), ' ', Style::new().bg(BG));
            }
        }

        let short = size.height() < BP_SHORT;
        let header_h = if short { 1 } else { 4 };
        let nav_h = if short { 1 } else { 3 };
        let rows = split_v(
            screen,
            &[
                Constraint::Fixed(header_h),
                Constraint::Fill(1),
                Constraint::Fixed(nav_h),
            ],
        );
        let (header_area, body_area, nav_area) = (rows[0], rows[1], rows[2]);

        self.draw_header(&mut surface, header_area);

        let wide = size.width() >= BP_WIDE;
        let (main_area, sidebar_area) = if wide && self.tab == Tab::Overview {
            let cols = split_h(body_area, &[Constraint::Fill(1), Constraint::Fixed(30)]);
            (cols[0], Some(cols[1]))
        } else {
            (body_area, None)
        };

        match self.tab {
            Tab::Overview => self.draw_map(&mut surface, main_area),
            Tab::Settings => self.draw_settings(&mut surface, main_area),
        }

        if self.tab == Tab::Overview {
            if let Some(sidebar) = sidebar_area {
                self.draw_detail_panel(&mut surface, sidebar, false);
            } else if self.sheet_open {
                let h = 9u16.min(main_area.height().saturating_sub(2));
                if h > 0 {
                    let sheet = Rect::new(
                        main_area.left(),
                        main_area.bottom() - h,
                        main_area.width(),
                        h,
                    );
                    self.draw_detail_panel(&mut surface, sheet, true);
                }
            }
        }

        self.draw_nav_bar(&mut surface, nav_area);
        self.draw_floating(&mut surface);
    }

    fn draw_floating(&self, surface: &mut Surface<'_>) {
        for f in &self.floating {
            let age = self.time - f.born;
            let alpha = (1.0 - age / FLOATING_TEXT_LIFETIME).clamp(0.0, 1.0) as f32;
            let color = Color::lerp(BG, f.color, alpha);
            let x = f.x.round().max(0.0) as u16;
            let y = f.y.round().max(0.0) as u16;
            surface.print((x, y), &f.text, Style::new().fg(color).bg(BG));
        }
    }
}

impl Example for OutpostDashboard {
    const NAME: &'static str = "15_outpost_dashboard";

    // Fill the whole browser viewport on the wasm backends (software Canvas2D and GL WebGL2)
    // instead of rendering at a fixed 50x25 grid wherever it lands on the page: this is a
    // flagship, app-like dashboard meant to be the whole page (see the module doc comment's
    // "genuine responsiveness" bullet), and a fixed small grid leaves most of a phone screen
    // black -- see `Example::fill_viewport`. Applies to every windowed backend's wasm build alike,
    // software, gl and wgpu.
    #[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
    fn fill_viewport() -> bool {
        true
    }

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.advance(frame.delta);
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(OutpostDashboard);
