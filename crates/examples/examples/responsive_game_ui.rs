//! Responsive mobile-strategy "kingdom map" UI mockup.
//!
//! A free-to-play-style overworld screen (think the map view of one of
//! those "build your kingdom" mobile games), built to reflow across wildly
//! different surfaces: a narrow SSH session, a full desktop terminal, a
//! resizable native window, and a phone browser tab. Nothing here is a real
//! game: taps, buttons, and the nav bar are all interactive, but no state
//! persists any gameplay meaning. The point is the *shell*: layout
//! breakpoints, tap/click hit-testing, and small "appy" transitions layered
//! on top of plain glyphs.
//!
//! # Layout breakpoints
//!
//! - `width <  46` (phone-portrait): icon-only nav, abbreviated resource
//!   counters, tile detail opens as a bottom sheet.
//! - `46 <= width < 90` (phone-landscape, or a normal terminal): labeled
//!   nav, full resource counters, still a bottom sheet.
//! - `width >= 90` (desktop): a persistent right-hand detail sidebar
//!   replaces the bottom sheet.
//! - `height < 16` (short terminals) collapse the nav bar and event banner
//!   to single rows regardless of width.
//!
//! # Controls
//!
//! - Arrow keys / WASD: move the map cursor (scrolls the camera)
//! - Enter / Space: select the tile under the cursor
//! - 1-5 / Tab / Shift+Tab: switch the bottom-nav tab
//! - Escape: close the detail sheet or settings panel
//! - Mouse / tap: click nav tabs, resource icons, the event banner, map
//!   tiles, and every button in the detail panel / tab content
//! - Q: quit
//!
//! # Run
//!
//! ```sh
//! cargo run --example responsive_game_ui --features crossterm
//! cargo run --example responsive_game_ui --features default-font
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]

use std::time::Duration;

use retroglyph_core::event::{Event, KeyCode, MouseButton, MouseEventKind};
use retroglyph_core::{Backend, Camera, Color, Pos, Rect, Size, Style, Terminal};
use retroglyph_examples::util::lcg::Lcg;
use retroglyph_examples::util::timestep::Stopwatch;
use retroglyph_widgets::{Constraint, gauge, panel, split_h, split_v, truncate};

// ── Breakpoints ───────────────────────────────────────────────────────────────

/// Below this width, the nav bar drops labels and numbers get abbreviated.
const BP_XS: u16 = 46;
/// At or above this width, a persistent sidebar replaces the bottom sheet.
const BP_WIDE: u16 = 90;
/// Below this height, chrome rows collapse to a single line.
const BP_SHORT: u16 = 16;

// ── Palette ───────────────────────────────────────────────────────────────────

const BG: Color = Color::Rgb {
    r: 14,
    g: 15,
    b: 22,
};
const PANEL_BG: Color = Color::Rgb {
    r: 20,
    g: 22,
    b: 32,
};
const CHROME_BG: Color = Color::Rgb {
    r: 26,
    g: 22,
    b: 40,
};
const BORDER: Color = Color::Rgb {
    r: 90,
    g: 80,
    b: 120,
};
const FG: Color = Color::Rgb {
    r: 220,
    g: 218,
    b: 232,
};
const DIM_FG: Color = Color::Rgb {
    r: 130,
    g: 128,
    b: 148,
};
const ACCENT: Color = Color::Rgb {
    r: 250,
    g: 200,
    b: 90,
};
const GOOD: Color = Color::Rgb {
    r: 110,
    g: 210,
    b: 140,
};
const BAD: Color = Color::Rgb {
    r: 230,
    g: 90,
    b: 100,
};

// ── Terrain ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Terrain {
    Plains,
    Forest,
    Mountain,
    Sea,
    River,
    Desert,
}

impl Terrain {
    const fn glyph(self) -> char {
        match self {
            Self::Plains => '.',
            Self::Forest => '♣',
            Self::Mountain => '^',
            Self::Sea => '~',
            Self::River => '≈',
            Self::Desert => ':',
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::Plains => Color::Rgb {
                r: 90,
                g: 110,
                b: 80,
            },
            Self::Forest => Color::Rgb {
                r: 70,
                g: 140,
                b: 90,
            },
            Self::Mountain => Color::Rgb {
                r: 170,
                g: 165,
                b: 175,
            },
            Self::Sea => Color::Rgb {
                r: 70,
                g: 110,
                b: 190,
            },
            Self::River => Color::Rgb {
                r: 90,
                g: 170,
                b: 210,
            },
            Self::Desert => Color::Rgb {
                r: 200,
                g: 175,
                b: 100,
            },
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Plains => "Plains",
            Self::Forest => "Forest",
            Self::Mountain => "Mountains",
            Self::Sea => "Open sea",
            Self::River => "River",
            Self::Desert => "Desert",
        }
    }
}

const WORLD_W: u16 = 34;
const WORLD_H: u16 = 20;

/// Generate a deterministic, purely decorative terrain field: a couple of
/// sine-wave "ridges" for elevation/moisture, jittered by a seeded LCG, plus
/// two carved rivers. Not a real world-gen algorithm, just enough texture to
/// make the map read as a place rather than a random grid.
fn generate_world() -> Vec<Terrain> {
    let mut rng = Lcg::new(0x0051_0570);
    let mut tiles = Vec::with_capacity(usize::from(WORLD_W) * usize::from(WORLD_H));
    for y in 0..WORLD_H {
        for x in 0..WORLD_W {
            let fx = f32::from(x) / f32::from(WORLD_W);
            let fy = f32::from(y) / f32::from(WORLD_H);
            let ridge = (fx * 6.0).sin() * (fy * 4.0).cos();
            let swell = (fx.mul_add(3.0, fy * 5.0)).sin().mul_add(0.5, ridge + 1.5);
            let elevation = swell / 3.0;
            let moisture = f32::midpoint(fx.mul_add(4.0, 1.7).cos() * (fy * 3.0).sin(), 1.0);
            let jitter = (rng.next() % 1000) as f32 / 1000.0;
            let v = elevation.mul_add(0.7, jitter * 0.3);
            let terrain = if v > 0.78 {
                Terrain::Mountain
            } else if v < 0.22 {
                Terrain::Sea
            } else if moisture > 0.7 && v < 0.55 {
                Terrain::Forest
            } else if moisture < 0.3 && v > 0.4 {
                Terrain::Desert
            } else {
                Terrain::Plains
            };
            tiles.push(terrain);
        }
    }
    for &x0 in &[10.0_f32, 22.0_f32] {
        for y in 0..WORLD_H {
            let x = (f32::from(y) * 0.5).sin().mul_add(3.0, x0).round();
            if x < 0.0 || x >= f32::from(WORLD_W) {
                continue;
            }
            let idx = usize::from(y) * usize::from(WORLD_W) + x as usize;
            if tiles[idx] != Terrain::Mountain {
                tiles[idx] = Terrain::River;
            }
        }
    }
    tiles
}

fn terrain_at(world: &[Terrain], pos: Pos) -> Terrain {
    world[usize::from(pos.y) * usize::from(WORLD_W) + usize::from(pos.x)]
}

// ── Points of interest ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum PoiKind {
    HomeCapital,
    RivalCapital,
    GoldMine,
    TimberCamp,
    Farmland,
    Ruins,
    Outpost,
}

impl PoiKind {
    const fn glyph(self) -> char {
        match self {
            // U+25C6 isn't in the software backend's CP437 font; the card-suit
            // diamond (U+2666) is, and reads the same at a glance.
            Self::HomeCapital | Self::RivalCapital => '♦',
            Self::GoldMine => '$',
            Self::TimberCamp => '♠',
            Self::Farmland => '☼', // sun -- CP437 has no wheat/farm glyph
            Self::Ruins => '◘',    // inverse bullet -- CP437 has no dagger
            Self::Outpost => '▲',
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::HomeCapital => ACCENT,
            Self::RivalCapital => BAD,
            Self::GoldMine => Color::Rgb {
                r: 240,
                g: 200,
                b: 80,
            },
            Self::TimberCamp => Color::Rgb {
                r: 120,
                g: 190,
                b: 110,
            },
            Self::Farmland => Color::Rgb {
                r: 200,
                g: 170,
                b: 90,
            },
            Self::Ruins => Color::Rgb {
                r: 190,
                g: 110,
                b: 220,
            },
            Self::Outpost => Color::Rgb {
                r: 230,
                g: 130,
                b: 90,
            },
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::HomeCapital => "Home capital",
            Self::RivalCapital => "Rival stronghold",
            Self::GoldMine => "Gold mine",
            Self::TimberCamp => "Timber camp",
            Self::Farmland => "Farmland",
            Self::Ruins => "Ancient ruins",
            Self::Outpost => "Watch outpost",
        }
    }
}

struct Poi {
    pos: Pos,
    name: &'static str,
    kind: PoiKind,
}

fn generate_pois() -> Vec<Poi> {
    vec![
        Poi {
            pos: Pos::new(8, 11),
            name: "Highspire",
            kind: PoiKind::HomeCapital,
        },
        Poi {
            pos: Pos::new(28, 4),
            name: "Blackmoor Hold",
            kind: PoiKind::RivalCapital,
        },
        Poi {
            pos: Pos::new(14, 5),
            name: "Glitterdeep",
            kind: PoiKind::GoldMine,
        },
        Poi {
            pos: Pos::new(5, 15),
            name: "Oakenfell",
            kind: PoiKind::TimberCamp,
        },
        Poi {
            pos: Pos::new(12, 16),
            name: "Millbrook",
            kind: PoiKind::Farmland,
        },
        Poi {
            pos: Pos::new(25, 15),
            name: "Sunken Keep",
            kind: PoiKind::Ruins,
        },
        Poi {
            pos: Pos::new(19, 9),
            name: "Greywatch",
            kind: PoiKind::Outpost,
        },
    ]
}

/// Chebyshev distance from the home capital, in tiles.
fn distance_from_home(pois: &[Poi], tile: Pos) -> i32 {
    let home = pois
        .iter()
        .find(|p| p.kind == PoiKind::HomeCapital)
        .map_or(Pos::new(0, 0), |p| p.pos);
    let dx = i32::from(tile.x) - i32::from(home.x);
    let dy = i32::from(tile.y) - i32::from(home.y);
    dx.abs().max(dy.abs())
}

const EXPLORED_RADIUS: i32 = 8;
const SIGHTED_RADIUS: i32 = 13;

// ── Nav tabs ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Map,
    Army,
    Build,
    Quests,
    Chat,
}

impl Tab {
    const ALL: [Self; 5] = [Self::Map, Self::Army, Self::Build, Self::Quests, Self::Chat];

    const fn icon(self) -> char {
        match self {
            // Every glyph below is chosen from the CP437 set the software
            // backend's bitmap font actually contains (see
            // `retroglyph_software::bitmap_font::unicode_to_cp437`) -- anything
            // outside it silently renders as a solid block there instead of
            // the intended icon.
            Self::Map => '◙',    // inverse white circle
            Self::Army => '♂',   // male sign
            Self::Build => '■',  // black square
            Self::Quests => '☺', // smiley
            Self::Chat => '♪',   // eighth note
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Map => "Map",
            Self::Army => "Army",
            Self::Build => "Build",
            Self::Quests => "Quests",
            Self::Chat => "Chat",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Map => 0,
            Self::Army => 1,
            Self::Build => 2,
            Self::Quests => 3,
            Self::Chat => 4,
        }
    }
}

// ── Resources ─────────────────────────────────────────────────────────────────

struct ResourceSlot {
    icon: char,
    name: &'static str,
    color: Color,
    value: u32,
    /// Animated display value, tweened toward `value` each frame so taps
    /// feel like a little "ka-ching" count-up instead of an instant jump.
    display: f32,
}

fn abbreviate(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", f64::from(n) / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", f64::from(n) / 1000.0)
    } else {
        n.to_string()
    }
}

// ── Misc content: army / build / quests ──────────────────────────────────────

struct Unit {
    name: &'static str,
    count: u32,
    power: f32,
}

struct BuildItem {
    name: &'static str,
    level: u32,
    seed: f32,
}

struct Quest {
    name: &'static str,
    progress: f32,
    claimed: bool,
}

// ── Transient UI feedback ─────────────────────────────────────────────────────

struct FloatingText {
    x: f32,
    y: f32,
    text: String,
    color: Color,
    born: f64,
}

struct Toast {
    text: String,
    born: f64,
}

// ── Hit testing ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum HitTarget {
    Tab(Tab),
    ResourceIcon(usize),
    SettingsGear,
    AlertBell,
    EventBanner,
    CloseSheet,
    SheetAction(u8),
    SettingToggle(u8),
    QuestClaim(usize),
    BuildBoost(usize),
    ArmyRow(usize),
}

// ── State ─────────────────────────────────────────────────────────────────────

struct State {
    watch: Stopwatch,
    time: f64,

    world: Vec<Terrain>,
    pois: Vec<Poi>,
    camera: Camera,
    cursor: Pos,
    selected: Option<Pos>,
    last_map_rect: Option<Rect>,

    tab: Tab,
    tab_anim: f32,

    resources: Vec<ResourceSlot>,

    event_banner: bool,
    banner_pulse_mul: f32,

    floating: Vec<FloatingText>,
    toast: Option<Toast>,

    settings_open: bool,
    settings_anim: f32,
    settings_toggles: [bool; 3],

    sheet_anim: f32,

    units: Vec<Unit>,
    build_queue: Vec<BuildItem>,
    quests: Vec<Quest>,

    hitboxes: Vec<(Rect, HitTarget)>,
    notifications: u32,
}

impl State {
    fn new() -> Self {
        let world = generate_world();
        let pois = generate_pois();
        let home = pois
            .iter()
            .find(|p| p.kind == PoiKind::HomeCapital)
            .map_or(Pos::new(0, 0), |p| p.pos);

        let mut camera = Camera::new(
            Rect::new(0, 0, 20, 10),
            Size {
                width: WORLD_W,
                height: WORLD_H,
            },
        );
        camera.center_on(home);

        Self {
            watch: Stopwatch::new(),
            time: 0.0,
            world,
            pois,
            camera,
            cursor: home,
            selected: None,
            last_map_rect: None,
            tab: Tab::Map,
            tab_anim: 0.0,
            resources: vec![
                ResourceSlot {
                    icon: '$',
                    name: "gold",
                    color: ACCENT,
                    value: 12_450,
                    display: 12_450.0,
                },
                ResourceSlot {
                    icon: '♦',
                    name: "gems",
                    color: Color::Rgb {
                        r: 120,
                        g: 210,
                        b: 230,
                    },
                    value: 340,
                    display: 340.0,
                },
                ResourceSlot {
                    icon: '♠',
                    name: "wood",
                    color: Color::Rgb {
                        r: 150,
                        g: 190,
                        b: 110,
                    },
                    value: 8_900,
                    display: 8_900.0,
                },
                ResourceSlot {
                    icon: '☼',
                    name: "food",
                    color: Color::Rgb {
                        r: 230,
                        g: 170,
                        b: 110,
                    },
                    value: 5_120,
                    display: 5_120.0,
                },
            ],
            event_banner: true,
            banner_pulse_mul: 1.0,
            floating: Vec::new(),
            toast: None,
            settings_open: false,
            settings_anim: 0.0,
            settings_toggles: [true, true, false],
            sheet_anim: 0.0,
            units: vec![
                Unit {
                    name: "Spearmen",
                    count: 240,
                    power: 0.4,
                },
                Unit {
                    name: "Archers",
                    count: 120,
                    power: 0.55,
                },
                Unit {
                    name: "Cavalry",
                    count: 40,
                    power: 0.7,
                },
                Unit {
                    name: "Siege engines",
                    count: 6,
                    power: 0.9,
                },
            ],
            build_queue: vec![
                BuildItem {
                    name: "Keep",
                    level: 4,
                    seed: 0.1,
                },
                BuildItem {
                    name: "Barracks",
                    level: 2,
                    seed: 0.4,
                },
                BuildItem {
                    name: "Granary",
                    level: 3,
                    seed: 0.7,
                },
            ],
            quests: vec![
                Quest {
                    name: "Scout the ruins",
                    progress: 1.0,
                    claimed: false,
                },
                Quest {
                    name: "Train 100 troops",
                    progress: 0.62,
                    claimed: false,
                },
                Quest {
                    name: "Collect 500 gold",
                    progress: 1.0,
                    claimed: false,
                },
                Quest {
                    name: "Upgrade the keep",
                    progress: 0.2,
                    claimed: false,
                },
            ],
            hitboxes: Vec::new(),
            notifications: 3,
        }
    }

    /// Animation speed multiplier: instant when "reduced motion" is on.
    const fn motion_rate(&self) -> f32 {
        if self.settings_toggles[2] { 60.0 } else { 8.0 }
    }

    fn advance(&mut self, dt: f64) {
        self.time += dt;
        let dt = dt as f32;
        let rate = self.motion_rate();
        let lerp_amt = (dt * rate).min(1.0);

        let target = self.tab.index() as f32;
        self.tab_anim = (target - self.tab_anim).mul_add(lerp_amt, self.tab_anim);

        let sheet_target = bool_to_f32(self.selected.is_some());
        self.sheet_anim = (sheet_target - self.sheet_anim).mul_add(lerp_amt, self.sheet_anim);

        let settings_target = bool_to_f32(self.settings_open);
        self.settings_anim =
            (settings_target - self.settings_anim).mul_add(lerp_amt, self.settings_anim);

        self.banner_pulse_mul = 0.5_f32.mul_add((self.time * 2.2).sin() as f32, 0.5);

        for r in &mut self.resources {
            r.display = (r.value as f32 - r.display).mul_add((dt * 6.0).min(1.0), r.display);
        }

        let now = self.time;
        self.floating.retain(|f| now - f.born < 1.1);
        for f in &mut self.floating {
            f.y = dt.mul_add(-6.0, f.y);
        }
        if let Some(t) = &self.toast
            && now - t.born > 2.6
        {
            self.toast = None;
        }
    }

    fn bump_resource(&mut self, name: &str, amount: i64) {
        if let Some(r) = self.resources.iter_mut().find(|r| r.name == name) {
            r.value = (i64::from(r.value) + amount).max(0) as u32;
        }
    }

    fn poi_at(&self, pos: Pos) -> Option<&Poi> {
        self.pois.iter().find(|p| p.pos == pos)
    }

    fn push_toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            born: self.time,
        });
    }

    fn push_float(&mut self, x: f32, y: f32, text: impl Into<String>, color: Color) {
        self.floating.push(FloatingText {
            x,
            y,
            text: text.into(),
            color,
            born: self.time,
        });
    }
}

const fn bool_to_f32(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

// ── Layout helpers ────────────────────────────────────────────────────────────

struct Chrome {
    wide: bool,
    xs: bool,
    short: bool,
}

const fn classify(size: Size) -> Chrome {
    Chrome {
        wide: size.width >= BP_WIDE,
        xs: size.width < BP_XS,
        short: size.height < BP_SHORT,
    }
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw<B: Backend>(term: &mut Terminal<B>, state: &mut State) {
    let size = term.size();
    let chrome = classify(size);
    state.hitboxes.clear();

    let screen = Rect::new(0, 0, size.width, size.height);
    for y in 0..size.height {
        for x in 0..size.width {
            term.put_styled(x, y, ' ', Style::new().bg(BG));
        }
    }

    let banner_h = u16::from(state.event_banner);
    let nav_h = if chrome.xs || chrome.short { 1 } else { 2 };
    let [title_area, banner_area, body_area, nav_area] = take4(&split_v(
        screen,
        &[
            Constraint::Fixed(1),
            Constraint::Fixed(banner_h),
            Constraint::Fill,
            Constraint::Fixed(nav_h),
        ],
    ));

    draw_title_bar(term, title_area, state, &chrome);
    if state.event_banner {
        draw_event_banner(term, banner_area, state);
    }

    let (main_area, sidebar_area) = if chrome.wide {
        let [m, s] = take2(&split_h(
            body_area,
            &[Constraint::Fill, Constraint::Fixed(30)],
        ));
        (m, Some(s))
    } else {
        (body_area, None)
    };

    match state.tab {
        Tab::Map => draw_map(term, main_area, state),
        Tab::Army => draw_army(term, main_area, state),
        Tab::Build => draw_build(term, main_area, state),
        Tab::Quests => draw_quests(term, main_area, state),
        Tab::Chat => draw_chat(term, main_area),
    }
    if !matches!(state.tab, Tab::Map) {
        state.last_map_rect = None;
    }

    if let Some(sidebar) = sidebar_area {
        draw_detail_panel(term, sidebar, state, false);
    } else if state.sheet_anim > 0.01 {
        let max_h = (main_area.height() / 2).clamp(4, 11);
        let h = (f32::from(max_h) * ease_out(state.sheet_anim)).round() as u16;
        if h > 0 && h <= main_area.height() {
            let sheet = Rect::new(
                main_area.left(),
                main_area.bottom() - h,
                main_area.width(),
                h,
            );
            draw_detail_panel(term, sheet, state, true);
        }
    }

    draw_nav_bar(term, nav_area, state, &chrome);
    draw_floating(term, state);
    draw_toast(term, screen, state);
    if state.settings_anim > 0.01 {
        draw_settings(term, screen, state, &chrome);
    }
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - t).mul_add(-(1.0 - t), 1.0)
}

fn take2(v: &[Rect]) -> [Rect; 2] {
    [v[0], v[1]]
}
fn take4(v: &[Rect]) -> [Rect; 4] {
    [v[0], v[1], v[2], v[3]]
}

// ── Title bar ─────────────────────────────────────────────────────────────────

fn draw_title_bar<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    state: &mut State,
    chrome: &Chrome,
) {
    for x in area.left()..area.right() {
        term.put_styled(x, area.top(), ' ', Style::new().bg(CHROME_BG));
    }
    let y = area.top();
    let mut x = area.left() + 1;

    if chrome.xs {
        // Not enough room for the kingdom name; the resource pills on the
        // right take priority.
    } else {
        let name = "Highspire";
        term.reset_style().fg(ACCENT).bg(CHROME_BG);
        term.print(x, y, name);
        x += name.chars().count() as u16 + 2;
    }

    // Right-aligned: resource pills, then gear + bell. Every glyph here comes
    // from the CP437 set the software backend's bitmap font actually
    // contains -- anything outside it silently renders as a solid block
    // there instead of the intended icon (see `Tab::icon`'s comment).
    let bell_text = format!("‼{}", state.notifications.min(9));
    let bell_w = bell_text.chars().count() as u16;
    let gear_w = 1u16;
    let mut labels: Vec<(String, Color)> = Vec::new();
    for r in &state.resources {
        let val = if chrome.xs {
            abbreviate(r.display.round().max(0.0) as u32)
        } else {
            format!("{}", r.display.round().max(0.0) as u32)
        };
        labels.push((format!("{} {}", r.icon, val), r.color));
    }
    let content_w: u16 = labels
        .iter()
        .map(|(s, _)| s.chars().count() as u16 + 2)
        .sum::<u16>()
        + bell_w
        + gear_w
        + 2;
    let mut rx = area.right().saturating_sub(content_w).max(x);

    for (i, (text, color)) in labels.iter().enumerate() {
        let w = text.chars().count() as u16;
        let rect = Rect::new(rx, y, w, 1);
        term.reset_style().fg(*color).bg(CHROME_BG);
        term.print(rx, y, text);
        state.hitboxes.push((rect, HitTarget::ResourceIcon(i)));
        rx += w + 2;
    }

    let bell_rect = Rect::new(rx, y, bell_w, 1);
    let bell_color = if state.notifications > 0 { BAD } else { DIM_FG };
    term.reset_style().fg(bell_color).bg(CHROME_BG);
    term.print(rx, y, &bell_text);
    state.hitboxes.push((bell_rect, HitTarget::AlertBell));
    rx += bell_w + 1;

    let gear_rect = Rect::new(rx, y, gear_w, 1);
    term.reset_style().fg(FG).bg(CHROME_BG);
    term.print(rx, y, "≡");
    state.hitboxes.push((gear_rect, HitTarget::SettingsGear));

    term.reset_style();
}

// ── Event banner ──────────────────────────────────────────────────────────────

fn draw_event_banner<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State) {
    if area.height() == 0 {
        return;
    }
    let pulse = state.banner_pulse_mul;
    let bg = Color::lerp(
        Color::Rgb {
            r: 80,
            g: 20,
            b: 30,
        },
        Color::Rgb {
            r: 150,
            g: 40,
            b: 50,
        },
        pulse,
    );
    let text = " \u{203C} War council: raid detected in 04:12 - tap to prepare  (tap to dismiss) ";
    let clipped = truncate(text, area.width_usize());
    for x in area.left()..area.right() {
        term.put_styled(x, area.top(), ' ', Style::new().bg(bg));
    }
    term.reset_style().fg(Color::BRIGHT_WHITE).bg(bg);
    term.print(area.left(), area.top(), &clipped);
    term.reset_style();
    state.hitboxes.push((area, HitTarget::EventBanner));
}

// ── Map tab ───────────────────────────────────────────────────────────────────

fn draw_map<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State) {
    if area.width() < 2 || area.height() < 2 {
        return;
    }
    state.camera.set_viewport(area);
    state.last_map_rect = Some(area);

    for (world_pos, screen_pos) in state.camera.cells() {
        let dist = distance_from_home(&state.pois, world_pos);
        let explored = dist <= EXPLORED_RADIUS;
        let poi = state.poi_at(world_pos);

        let (glyph, color) = if let Some(p) = poi {
            if explored || dist <= SIGHTED_RADIUS {
                (
                    p.kind.glyph(),
                    if explored {
                        p.kind.color()
                    } else {
                        p.kind.color().darken(0.55)
                    },
                )
            } else {
                ('?', DIM_FG)
            }
        } else if explored {
            let t = terrain_at(&state.world, world_pos);
            (t.glyph(), t.color())
        } else if dist <= SIGHTED_RADIUS {
            (
                '\u{00B7}',
                Color::Rgb {
                    r: 40,
                    g: 40,
                    b: 52,
                },
            )
        } else {
            (' ', BG)
        };

        let is_selected = state.selected == Some(world_pos);
        let is_cursor = state.cursor == world_pos;
        let style = if is_selected {
            let flash = 0.5_f32.mul_add((state.time * 6.0).sin() as f32, 0.5);
            Style::new()
                .fg(Color::lerp(color, Color::BRIGHT_WHITE, 0.3))
                .bg(Color::lerp(PANEL_BG, ACCENT, 0.25 * flash))
        } else if is_cursor {
            Style::new().fg(color).bg(Color::Rgb {
                r: 40,
                g: 44,
                b: 60,
            })
        } else {
            Style::new().fg(color).bg(BG)
        };
        term.put_styled(screen_pos.x, screen_pos.y, glyph, style);
    }

    // Name labels for explored POIs, if there's room (never on tiny screens).
    if area.width() >= 30 {
        for p in &state.pois {
            let dist = distance_from_home(&state.pois, p.pos);
            if dist > EXPLORED_RADIUS && state.selected != Some(p.pos) {
                continue;
            }
            if let Some(screen) = state.camera.world_to_screen(p.pos) {
                let label_y = screen.y + 1;
                if label_y >= area.bottom() {
                    continue;
                }
                let max_w = (area.right().saturating_sub(screen.x)) as usize;
                let label = truncate(p.name, max_w);
                term.reset_style().fg(DIM_FG).bg(BG);
                term.print(screen.x, label_y, &label);
            }
        }
    }
    term.reset_style();

    // Compass hint, bottom-right corner, when there's spare room.
    if area.width() >= 20 && area.height() >= 6 {
        let hint = "arrows: pan   enter: select";
        let hint = truncate(hint, area.width_usize());
        let x = area.right().saturating_sub(hint.chars().count() as u16 + 1);
        let y = area.bottom() - 1;
        term.reset_style().fg(DIM_FG).bg(BG);
        term.print(x, y, &hint);
        term.reset_style();
    }
}

// ── Army / Build / Quests / Chat tabs ────────────────────────────────────────

fn draw_army<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State) {
    panel_bg(term, area, "GARRISON");
    let inner = inset(area);
    for (i, u) in state.units.iter().enumerate() {
        let y = inner.top() + i as u16 * 2;
        if y + 1 >= inner.bottom() {
            break;
        }
        let row = Rect::new(inner.left(), y, inner.width(), 1);
        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(
            row.left(),
            row.top(),
            &format!("{:<16}x{}", u.name, u.count),
        );
        let bar_row = Rect::new(inner.left(), y + 1, inner.width(), 1);
        gauge(term, bar_row, "pwr", u.power);
        state.hitboxes.push((
            Rect::new(inner.left(), y, inner.width(), 2),
            HitTarget::ArmyRow(i),
        ));
    }
    term.reset_style();
}

fn draw_build<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State) {
    panel_bg(term, area, "BUILD QUEUE");
    let inner = inset(area);
    for (i, b) in state.build_queue.iter().enumerate() {
        let y = inner.top() + i as u16 * 2;
        if y + 1 >= inner.bottom() {
            break;
        }
        let progress = ((state.time as f32).mul_add(0.05, b.seed) % 1.0).clamp(0.0, 1.0);
        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y,
            &format!("{:<12} Lv.{}  [>> boost]", b.name, b.level),
        );
        let bar_row = Rect::new(inner.left(), y + 1, inner.width().saturating_sub(0), 1);
        gauge(term, bar_row, "up ", progress);
        let boost_x = inner.left() + 12 + 8;
        let boost_w = 8u16.min(inner.right().saturating_sub(boost_x));
        state
            .hitboxes
            .push((Rect::new(boost_x, y, boost_w, 1), HitTarget::BuildBoost(i)));
    }
    term.reset_style();
}

fn draw_quests<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State) {
    panel_bg(term, area, "QUESTS");
    let inner = inset(area);
    for (i, q) in state.quests.iter().enumerate() {
        let y = inner.top() + i as u16 * 2;
        if y + 1 >= inner.bottom() {
            break;
        }
        let status = if q.claimed {
            "done"
        } else if q.progress >= 1.0 {
            "claim"
        } else {
            ""
        };
        let color = if q.claimed {
            DIM_FG
        } else if q.progress >= 1.0 {
            GOOD
        } else {
            FG
        };
        term.reset_style().fg(color).bg(PANEL_BG);
        let label = truncate(q.name, inner.width_usize().saturating_sub(8));
        term.print(inner.left(), y, &label);
        if !status.is_empty() {
            let sx = inner.right().saturating_sub(status.chars().count() as u16);
            term.print(sx, y, status);
        }
        let bar_row = Rect::new(inner.left(), y + 1, inner.width(), 1);
        gauge(term, bar_row, "", q.progress);
        if q.progress >= 1.0 && !q.claimed {
            state.hitboxes.push((
                Rect::new(inner.left(), y, inner.width(), 2),
                HitTarget::QuestClaim(i),
            ));
        }
    }
    term.reset_style();
}

fn draw_chat<B: Backend>(term: &mut Terminal<B>, area: Rect) {
    panel_bg(term, area, "ALLIANCE CHAT");
    let inner = inset(area);
    let lines = [
        "Marcher: anyone free to help raid the ruins?",
        "Aelra: pushed my keep to level 5",
        "Dorn: reinforcements sent to the north wall",
        "System: alliance war starts in 2 days",
    ];
    for (i, line) in lines.iter().enumerate() {
        let y = inner.top() + i as u16;
        if y >= inner.bottom() {
            break;
        }
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(inner.left(), y, &truncate(line, inner.width_usize()));
    }
    if inner.height() > 0 {
        let y = inner.bottom() - 1;
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y,
            &truncate("[ type a message... ]", inner.width_usize()),
        );
    }
    term.reset_style();
}

// ── Detail panel (sidebar or bottom sheet) ───────────────────────────────────

fn draw_detail_panel<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    state: &mut State,
    is_sheet: bool,
) {
    if area.width() < 2 || area.height() < 1 {
        return;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(PANEL_BG));
        }
    }
    draw_box_top_shadow(term, area);
    if is_sheet && area.width() > 3 {
        let x = area.right() - 3;
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(x, area.top(), "[x]");
        state
            .hitboxes
            .push((Rect::new(x, area.top(), 3, 1), HitTarget::CloseSheet));
    }

    let inner = Rect::new(
        area.left() + 1,
        area.top() + (u16::from(is_sheet)),
        area.width().saturating_sub(2),
        area.height().saturating_sub(1 + u16::from(is_sheet)),
    );
    if inner.width() == 0 || inner.height() == 0 {
        return;
    }

    let Some(sel) = state.selected else {
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            inner.top(),
            &truncate("Tap a tile to inspect it.", inner.width_usize()),
        );
        return;
    };

    let poi = state.poi_at(sel);
    let terrain = terrain_at(&state.world, sel);
    let title = poi.map_or_else(|| terrain.name(), |p| p.name);
    let subtitle = poi.map_or_else(|| terrain.name(), |p| p.kind.label());
    let color = poi.map_or_else(|| terrain.color(), |p| p.kind.color());

    term.reset_style().fg(color).bg(PANEL_BG);
    term.print(
        inner.left(),
        inner.top(),
        &truncate(title, inner.width_usize()),
    );
    if inner.height() > 1 {
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            inner.top() + 1,
            &truncate(subtitle, inner.width_usize()),
        );
    }

    if inner.height() > 3 {
        let dist = distance_from_home(&state.pois, sel);
        let coord = format!("({}, {}) \u{2022} {} tiles from home", sel.x, sel.y, dist);
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            inner.top() + 2,
            &truncate(&coord, inner.width_usize()),
        );
    }

    // Action buttons.
    if inner.height() > 4 {
        let y = inner.top() + 4;
        let actions: &[(&str, u8)] = &[("Scout", 0), ("Attack", 1), ("Upgrade", 2)];
        let mut x = inner.left();
        for &(label, id) in actions {
            let text = format!("[ {label} ]");
            let w = text.chars().count() as u16;
            if x + w > inner.right() {
                break;
            }
            term.reset_style().fg(FG).bg(Color::Rgb {
                r: 40,
                g: 40,
                b: 56,
            });
            term.print(x, y, &text);
            state
                .hitboxes
                .push((Rect::new(x, y, w, 1), HitTarget::SheetAction(id)));
            x += w + 1;
        }
    }
    term.reset_style();
}

fn draw_box_top_shadow<B: Backend>(term: &mut Terminal<B>, area: Rect) {
    for x in area.left()..area.right() {
        term.put_styled(
            x,
            area.top(),
            '\u{2500}',
            Style::new().fg(BORDER).bg(PANEL_BG),
        );
    }
}

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

const fn inset(area: Rect) -> Rect {
    Rect::new(
        area.left() + 1,
        area.top() + 1,
        area.width().saturating_sub(2),
        area.height().saturating_sub(2),
    )
}

// ── Nav bar ───────────────────────────────────────────────────────────────────

fn draw_nav_bar<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    state: &mut State,
    chrome: &Chrome,
) {
    if area.width() == 0 || area.height() == 0 {
        return;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(CHROME_BG));
        }
    }

    let n = Tab::ALL.len() as u16;
    let slot_w = area.width() / n;
    let extra = area.width() % n;

    // Sliding active-tab indicator, one row tall, on the top edge of the bar.
    let indicator_x = area.left() + (state.tab_anim * f32::from(slot_w)).round() as u16;
    for x in indicator_x..(indicator_x + slot_w).min(area.right()) {
        term.put_styled(
            x,
            area.top(),
            '\u{2580}',
            Style::new().fg(ACCENT).bg(CHROME_BG),
        );
    }

    let mut x = area.left();
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let w = slot_w + u16::from((i as u16) < extra);
        let rect = Rect::new(x, area.top(), w, area.height());
        let active = state.tab == tab;
        let fg = if active { ACCENT } else { DIM_FG };
        let label_y = if area.height() >= 2 {
            area.top() + 1
        } else {
            area.top()
        };
        let icon_y = area.top();

        term.reset_style().fg(fg).bg(CHROME_BG);
        let icon_x = x + w / 2;
        term.put(icon_x, icon_y, tab.icon());
        if area.height() >= 2 && !chrome.xs {
            let label = tab.label();
            let lx = x + w.saturating_sub(label.chars().count() as u16) / 2;
            term.print(lx, label_y, label);
        }
        if tab == Tab::Chat && state.notifications > 0 {
            term.reset_style().fg(BAD).bg(CHROME_BG);
            term.put(icon_x + 1, icon_y, '\u{2022}');
        }
        state.hitboxes.push((rect, HitTarget::Tab(tab)));
        x += w;
    }
    term.reset_style();
}

// ── Floating text + toast ────────────────────────────────────────────────────

fn draw_floating<B: Backend>(term: &mut Terminal<B>, state: &State) {
    for f in &state.floating {
        let age = (state.time - f.born) as f32;
        let alpha = (1.0 - age / 1.1).clamp(0.0, 1.0);
        let color = Color::lerp(BG, f.color, alpha);
        let x = f.x.round().max(0.0) as u16;
        let y = f.y.round().max(0.0) as u16;
        term.reset_style().fg(color).bg(BG);
        term.print(x, y, &f.text);
    }
    term.reset_style();
}

fn draw_toast<B: Backend>(term: &mut Terminal<B>, screen: Rect, state: &State) {
    let Some(toast) = &state.toast else { return };
    let age = (state.time - toast.born) as f32;
    let slide_in = ease_out((age / 0.25).min(1.0));
    let fade = if age > 2.2 {
        (1.0 - (age - 2.2) / 0.4).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let text = format!(" \u{266B} {} ", toast.text);
    let w = (text.chars().count() as u16).min(screen.width());
    let x = screen.left() + (screen.width().saturating_sub(w)) / 2;
    let target_y = screen.top() + 1;
    let y = target_y.saturating_sub(1) + (slide_in.round() as u16).min(1);
    if y >= screen.bottom() {
        return;
    }
    let bg = Color::lerp(
        BG,
        Color::Rgb {
            r: 40,
            g: 46,
            b: 70,
        },
        fade,
    );
    let fg = Color::lerp(BG, Color::BRIGHT_WHITE, fade);
    term.reset_style().fg(fg).bg(bg);
    term.print(x, y, &text);
    term.reset_style();
}

// ── Settings overlay ──────────────────────────────────────────────────────────

fn draw_settings<B: Backend>(
    term: &mut Terminal<B>,
    screen: Rect,
    state: &mut State,
    chrome: &Chrome,
) {
    let t = ease_out(state.settings_anim);
    let full_h = 8u16.min(screen.height());
    let h = (f32::from(full_h) * t).round() as u16;
    if h == 0 {
        return;
    }
    let w = if chrome.wide { 36 } else { screen.width() };
    let area = if chrome.wide {
        Rect::new(
            screen.left() + (screen.width().saturating_sub(w)) / 2,
            screen.top() + (screen.height().saturating_sub(full_h)) / 2,
            w,
            h,
        )
    } else {
        Rect::new(screen.left(), screen.bottom().saturating_sub(h), w, h)
    };

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(
                x,
                y,
                ' ',
                Style::new().bg(Color::Rgb {
                    r: 24,
                    g: 22,
                    b: 36,
                }),
            );
        }
    }
    draw_box_top_shadow(term, area);
    if area.height() < 2 {
        return;
    }
    term.reset_style().fg(FG).bg(Color::Rgb {
        r: 24,
        g: 22,
        b: 36,
    });
    term.print(area.left() + 1, area.top() + 1, "SETTINGS");

    let toggles: &[(&str, u8)] = &[("Sound", 0), ("Notifications", 1), ("Reduced motion", 2)];
    for (i, &(label, id)) in toggles.iter().enumerate() {
        let y = area.top() + 3 + i as u16;
        if y >= area.bottom() {
            break;
        }
        let on = state.settings_toggles[usize::from(id)];
        let value = if on { "[x]" } else { "[ ]" };
        term.reset_style().fg(FG).bg(Color::Rgb {
            r: 24,
            g: 22,
            b: 36,
        });
        term.print(area.left() + 1, y, &format!("{value} {label}"));
        state.hitboxes.push((
            Rect::new(area.left() + 1, y, area.width().saturating_sub(2), 1),
            HitTarget::SettingToggle(id),
        ));
    }
    term.reset_style();
}

// ── Input handling ────────────────────────────────────────────────────────────

fn handle_click(state: &mut State, pos: Pos) -> bool {
    if state.settings_open {
        if let Some(&(_, target)) = state
            .hitboxes
            .iter()
            .find(|(r, t)| r.contains_pos(pos) && matches!(t, HitTarget::SettingToggle(_)))
        {
            if let HitTarget::SettingToggle(id) = target {
                state.settings_toggles[usize::from(id)] ^= true;
            }
            return true;
        }
        // Tapping outside the panel closes it; tapping the gear (still
        // registered underneath) is handled by the generic lookup below.
        state.settings_open = false;
        return true;
    }

    // Copy to avoid holding an immutable borrow of `state.hitboxes` across
    // the mutating calls below.
    let hit = state
        .hitboxes
        .iter()
        .find(|(r, _)| r.contains_pos(pos))
        .map(|(_, t)| *t);

    if let Some(target) = hit {
        match target {
            HitTarget::Tab(tab) => state.tab = tab,
            HitTarget::ResourceIcon(i) => {
                if let Some(r) = state.resources.get(i) {
                    let (x, y, color) = (r_screen_x(state, i), 0.0, r.color);
                    state.push_float(x, y, "+", color);
                }
            }
            HitTarget::SettingsGear => state.settings_open = true,
            HitTarget::AlertBell => {
                if state.notifications > 0 {
                    state.notifications -= 1;
                    state.push_toast("Notification cleared");
                }
            }
            HitTarget::EventBanner => {
                state.event_banner = false;
                state.push_toast("Reinforcements requested!");
            }
            HitTarget::CloseSheet => state.selected = None,
            HitTarget::SheetAction(id) => match id {
                0 => state.push_toast("Scouts dispatched"),
                1 => state.push_toast("Not enough troops nearby"),
                2 => state.push_toast("Upgrade queued (VIP skips the wait)"),
                _ => {}
            },
            HitTarget::SettingToggle(id) => state.settings_toggles[usize::from(id)] ^= true,
            HitTarget::QuestClaim(i) => {
                if let Some(name) = state.quests.get_mut(i).map(|q| {
                    q.claimed = true;
                    q.name
                }) {
                    state.bump_resource("gold", 50);
                    state.push_toast(format!("Reward claimed: {name}"));
                    state.push_float(10.0, 0.0, "+50", ACCENT);
                }
            }
            HitTarget::BuildBoost(i) => {
                let name = state.build_queue.get(i).map_or("that", |b| b.name);
                state.push_toast(format!("Not enough gems to boost {name}!"));
            }
            HitTarget::ArmyRow(i) => {
                if let Some(u) = state.units.get(i) {
                    state.push_toast(format!("{} selected", u.name));
                }
            }
        }
        return true;
    }

    if matches!(state.tab, Tab::Map)
        && let Some(map_rect) = state.last_map_rect
        && map_rect.contains_pos(pos)
        && let Some(world) = state.camera.screen_to_world(pos)
    {
        state.selected = Some(world);
        state.cursor = world;
        return true;
    }
    false
}

/// Rough on-screen x for a resource icon's floating "+", for the tap-feedback
/// animation. Not pixel perfect (the title bar's exact position depends on
/// text width) -- close enough for a cosmetic effect.
const fn r_screen_x(_state: &State, _i: usize) -> f32 {
    2.0
}

fn move_cursor(state: &mut State, dx: i16, dy: i16) {
    let nx = i32::from(state.cursor.x) + i32::from(dx);
    let ny = i32::from(state.cursor.y) + i32::from(dy);
    if nx < 0 || ny < 0 || nx >= i32::from(WORLD_W) || ny >= i32::from(WORLD_H) {
        return;
    }
    state.cursor = Pos::new(nx as u16, ny as u16);
    state.camera.center_on(state.cursor);
}

/// Applies one input event. Returns `false` to quit.
fn handle(state: &mut State, event: &Event) -> bool {
    match event {
        Event::Key(k) if k.is_down() => match k.code {
            KeyCode::Escape => {
                if state.settings_open {
                    state.settings_open = false;
                } else if state.selected.is_some() {
                    state.selected = None;
                } else {
                    return false;
                }
            }
            KeyCode::Char('q' | 'Q') => return false,
            KeyCode::Up | KeyCode::Char('w' | 'W') if matches!(state.tab, Tab::Map) => {
                move_cursor(state, 0, -1);
            }
            KeyCode::Down | KeyCode::Char('s' | 'S') if matches!(state.tab, Tab::Map) => {
                move_cursor(state, 0, 1);
            }
            KeyCode::Left | KeyCode::Char('a' | 'A') if matches!(state.tab, Tab::Map) => {
                move_cursor(state, -1, 0);
            }
            KeyCode::Right | KeyCode::Char('d' | 'D') if matches!(state.tab, Tab::Map) => {
                move_cursor(state, 1, 0);
            }
            KeyCode::Enter | KeyCode::Char(' ') if matches!(state.tab, Tab::Map) => {
                state.selected = Some(state.cursor);
            }
            KeyCode::Tab => {
                let i = (state.tab.index() + 1) % Tab::ALL.len();
                state.tab = Tab::ALL[i];
            }
            KeyCode::BackTab => {
                let i = (state.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len();
                state.tab = Tab::ALL[i];
            }
            KeyCode::Char(c @ '1'..='5') => {
                let i = (c as u8 - b'1') as usize;
                if let Some(&tab) = Tab::ALL.get(i) {
                    state.tab = tab;
                }
            }
            _ => {}
        },
        Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
            handle_click(state, m.position);
        }
        Event::Close => return false,
        _ => {}
    }
    true
}

// ── Loop ──────────────────────────────────────────────────────────────────────

fn init<B: Backend>(_term: &mut Terminal<B>) -> State {
    State::new()
}

fn tick<B: Backend>(term: &mut Terminal<B>, state: &mut State) -> bool {
    let dt = state.watch.lap();
    state.advance(dt.as_secs_f64());

    draw(term, state);
    term.present().expect("present failed");

    if let Some(event) = term.poll(Duration::from_millis(40)) {
        if !handle(state, &event) {
            return false;
        }
        let pending: Vec<Event> = term.drain_events().collect();
        for event in pending {
            if !handle(state, &event) {
                return false;
            }
        }
    }
    true
}

retroglyph_examples::rg_run!(State, init, tick);

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::Headless;

    fn render_at(width: u16, height: u16) -> String {
        let mut term = Terminal::new(Headless::new(width, height));
        let mut state = State::new();
        draw(&mut term, &mut state);
        term.present().unwrap();
        term.backend().format_view()
    }

    #[test]
    fn renders_narrow_phone_size() {
        let view = render_at(40, 24);
        // Below `BP_XS` the title bar drops the kingdom name and the nav bar
        // drops its labels, but the view should still render at the right
        // size without panicking.
        assert_eq!(view.lines().count(), 24);
        // The title bar (row 0) drops the kingdom name at this width, even
        // though the map below may still label the capital by name.
        let title_row = view.lines().next().unwrap();
        assert!(!title_row.contains("Highspire"));
    }

    #[test]
    fn renders_wide_desktop_size() {
        let view = render_at(120, 40);
        // The sidebar's placeholder copy should show up when nothing's
        // selected yet, since a wide layout always keeps it visible.
        assert!(view.contains("inspect"));
    }

    #[test]
    fn click_on_map_selects_a_tile() {
        let mut term = Terminal::new(Headless::new(100, 30));
        let mut state = State::new();
        draw(&mut term, &mut state);
        term.present().unwrap();
        let rect = state.last_map_rect.expect("map tab draws a map rect");
        let clicked = handle_click(&mut state, Pos::new(rect.left() + 3, rect.top() + 3));
        assert!(clicked);
        assert!(state.selected.is_some());
    }

    #[test]
    fn tapping_nav_switches_tabs() {
        let mut term = Terminal::new(Headless::new(80, 24));
        let mut state = State::new();
        draw(&mut term, &mut state);
        term.present().unwrap();
        let (rect, _) = *state
            .hitboxes
            .iter()
            .find(|(_, t)| matches!(t, HitTarget::Tab(Tab::Quests)))
            .expect("quests tab hitbox registered");
        handle_click(&mut state, Pos::new(rect.left(), rect.top()));
        assert!(matches!(state.tab, Tab::Quests));
    }

    #[test]
    fn claiming_a_finished_quest_bumps_gold_and_marks_it_claimed() {
        let mut state = State::new();
        state.tab = Tab::Quests;
        let mut term = Terminal::new(Headless::new(80, 24));
        draw(&mut term, &mut state);
        term.present().unwrap();
        let before = state
            .resources
            .iter()
            .find(|r| r.name == "gold")
            .unwrap()
            .value;
        let (rect, _) = *state
            .hitboxes
            .iter()
            .find(|(_, t)| matches!(t, HitTarget::QuestClaim(0)))
            .expect("first quest is already complete and claimable");
        handle_click(&mut state, Pos::new(rect.left(), rect.top()));
        assert!(state.quests[0].claimed);
        let after = state
            .resources
            .iter()
            .find(|r| r.name == "gold")
            .unwrap()
            .value;
        assert!(after > before);
    }
}
