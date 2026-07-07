//! Responsive, touch-first mobile-strategy "kingdom map" UI mockup.
//!
//! A free-to-play-style overworld screen (think the map view of one of
//! those "build your kingdom" mobile games), built to reflow across wildly
//! different surfaces: a narrow SSH session, a full desktop terminal, a
//! resizable native window, and a phone browser tab. Nothing here is a real
//! game: every button, tab, and tile is interactive, but no state persists
//! any gameplay meaning. The point is the *shell*: layout breakpoints,
//! touch-friendly hit targets, pan-by-drag map navigation, and small "appy"
//! transitions layered on top of plain glyphs.
//!
//! # Touch target sizing (accessibility)
//!
//! Interactive targets follow the published guidance:
//!
//! - WCAG 2.2 SC 2.5.8 Target Size (Minimum, level AA): at least 24x24 CSS
//!   px, or undersized targets spaced so 24 px circles centered on each
//!   don't intersect a neighbor.
//! - WCAG 2.2 SC 2.5.5 Target Size (Enhanced, level AAA) and Apple's HIG:
//!   44x44 px/pt. Material Design: 48x48 dp, with ~8 dp between targets.
//!
//! Cells are the unit here, so those translate via the smallest cell any
//! deployed backend renders. On the docs site the worst case is the
//! wasm-headless `<pre>` on a ~390 px phone: a cell is roughly 7.6x8.8 CSS
//! px (xterm.js is ~8x17; the canvas backend is far larger). The policy,
//! enforced by the `touch_targets_meet_minimums` test:
//!
//! - every target is at least 6 columns x 3 rows (~45x26 px worst case:
//!   above AA on both axes, above AAA on width);
//! - no two targets overlap, and adjacent targets either exceed the
//!   minimum on both axes or are separated by at least one blank cell
//!   (~8 px, matching Material's spacing guidance);
//! - the map itself is one large pan/tap surface rather than per-tile
//!   targets, so tile selection precision is content, not chrome;
//! - terminals under 16 rows compress the chrome below these minimums;
//!   that layout is keyboard-first by definition (a 12-row terminal is not
//!   a touchscreen).
//!
//! # Layout breakpoints
//!
//! - `width <  46` (phone-portrait): two resource pills, icon-only nav.
//! - `46 <= width < 90` (phone-landscape, or a normal terminal): labeled
//!   nav, all resource pills; tile detail opens as a bottom sheet.
//! - `width >= 90` (desktop): a persistent right-hand detail sidebar
//!   replaces the bottom sheet.
//! - `height < 16` (short terminals): chrome collapses to single rows.
//!
//! # Controls
//!
//! - Tap / click: everything (nav tabs, pills, banner, buttons, map tiles)
//! - Drag on the map (touch or mouse): pan the camera
//! - Scroll wheel over the map: pan vertically
//! - Arrow keys / WASD: move the map cursor (scrolls the camera)
//! - Enter / Space: select the tile under the cursor
//! - 1-5 / Tab / Shift+Tab: switch the bottom-nav tab
//! - Escape: close the topmost modal/sheet, then quit
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

/// Below this width, only two resource pills fit and the nav drops labels.
const BP_XS: u16 = 46;
/// At or above this width, a persistent sidebar replaces the bottom sheet.
const BP_WIDE: u16 = 90;
/// Below this height, chrome rows collapse and touch minimums are waived
/// (see the touch-target section of the module docs).
const BP_SHORT: u16 = 16;

// ── Touch target minimums (see module docs for the derivation) ───────────────

/// Minimum interactive target width, in cells.
const MIN_TARGET_W: u16 = 6;
/// Minimum interactive target height, in cells.
const MIN_TARGET_H: u16 = 3;

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
const BUTTON_BG: Color = Color::Rgb {
    r: 44,
    g: 42,
    b: 66,
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
const GEM: Color = Color::Rgb {
    r: 120,
    g: 210,
    b: 230,
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
    /// Map-marker glyph. Every glyph is chosen from the CP437 set the
    /// software backend's bitmap font actually contains (see
    /// `retroglyph_software::bitmap_font::unicode_to_cp437`); anything
    /// outside it silently renders as a solid block there.
    const fn glyph(self) -> char {
        match self {
            Self::HomeCapital | Self::RivalCapital => '♦',
            Self::GoldMine => '$',
            Self::TimberCamp => '♣',
            Self::Farmland => '☼',
            Self::Ruins => '◘',
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

    /// One line of fake intel for the detail panel.
    const fn flavor(self) -> &'static str {
        match self {
            Self::HomeCapital => "Your seat of power. The keep is warm.",
            Self::RivalCapital => "Garrison ~2,400. Walls: level 9.",
            Self::GoldMine => "Yield: 120 gold/hr while held.",
            Self::TimberCamp => "Yield: 90 wood/hr while held.",
            Self::Farmland => "Yield: 150 food/hr while held.",
            Self::Ruins => "Unexplored. Rumors of relics below.",
            Self::Outpost => "Extends march range in this region.",
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Map,
    Army,
    Build,
    Quests,
    Chat,
}

impl Tab {
    const ALL: [Self; 5] = [Self::Map, Self::Army, Self::Build, Self::Quests, Self::Chat];

    /// CP437-safe nav icons; see the note on [`PoiKind::glyph`].
    const fn icon(self) -> char {
        match self {
            Self::Map => '◙',
            Self::Army => '♂',
            Self::Build => '■',
            Self::Quests => '☺',
            Self::Chat => '♪',
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
    /// Animated display value, tweened toward `value` each frame so payouts
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

fn format_mmss(secs: f32) -> String {
    let s = secs.max(0.0).round() as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

// ── Tab content: army / build / quests / chat ────────────────────────────────

struct Unit {
    name: &'static str,
    count: u32,
    power: f32,
}

struct BuildItem {
    name: &'static str,
    level: u32,
    remaining: f32,
    total: f32,
}

struct Quest {
    name: &'static str,
    progress: f32,
    reward: u32,
    claimed: bool,
}

/// Scripted alliance-chat lines that "arrive" over time.
const CHAT_SCRIPT: &[&str] = &[
    "Marcher: anyone free to raid the ruins?",
    "Aelra: pushed my keep to level 5!",
    "Dorn: reinforcements sent north",
    "System: alliance war starts in 2 days",
    "Aelra: who keeps farming my tiles >:(",
    "Marcher: gg, rally filled in 30s",
];

const GEM_PACKS: &[(u32, &str, &str, bool)] = &[
    (240, "Pouch of Gems", "$1.99", false),
    (1_300, "Chest of Gems", "$9.99", true),
    (2_800, "Vault of Gems", "$19.99", false),
];

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SheetAction {
    Scout,
    March,
    Harvest,
    EnterCity,
}

impl SheetAction {
    const fn label(self) -> &'static str {
        match self {
            Self::Scout => "SCOUT",
            Self::March => "MARCH",
            Self::Harvest => "HARVEST",
            Self::EnterCity => "ENTER CITY",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HitTarget {
    Tab(Tab),
    ResourcePill(usize),
    AlertBell,
    SettingsGear,
    EventBanner,
    SheetClose,
    SheetAction(SheetAction),
    TrainUnit(usize),
    BoostBuild(usize),
    HireBuilder,
    ClaimQuest(usize),
    ChatInput,
    BuyPack(usize),
    ToggleSetting(usize),
    ModalClose,
    ConfirmMarch,
    CancelModal,
    /// Non-activating surface that swallows taps (a sheet/modal body), so a
    /// press on it neither pans the map nor reaches controls underneath.
    /// Exempt from the touch-target minimums: it's a scrim, not a control.
    Inert,
    /// Like [`Inert`](Self::Inert), but tapping it closes the open modal
    /// (the "tap outside to dismiss" idiom). Also exempt from minimums.
    Scrim,
}

#[cfg(test)]
impl HitTarget {
    /// Targets exempt from the size/spacing minimums (backdrops, not controls).
    const fn is_surface(self) -> bool {
        matches!(self, Self::Inert | Self::Scrim)
    }
}

// ── Modals ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Modal {
    Shop,
    Settings,
    /// Confirm a march on the named target.
    ConfirmMarch(&'static str),
}

// ── Pointer state ─────────────────────────────────────────────────────────────

/// One in-flight press (finger or left button), from down to up.
struct PointerState {
    start: Pos,
    last: Pos,
    /// Whether the press started on the bare map (no control under it), so
    /// movement pans the camera.
    on_map: bool,
    /// The control rect under the press, for the pressed visual and for
    /// slide-off cancel (release outside the rect doesn't activate).
    pressed: Option<Rect>,
    /// Set once the pointer leaves its starting cell; an up after dragging
    /// is a pan, not a tap.
    dragging: bool,
}

// ── State ─────────────────────────────────────────────────────────────────────

struct State {
    watch: Stopwatch,
    time: f64,

    world: Vec<Terrain>,
    pois: Vec<Poi>,
    camera: Camera,
    cam_center: Pos,
    cursor: Pos,
    selected: Option<Pos>,
    last_map_rect: Option<Rect>,
    pointer: Option<PointerState>,

    tab: Tab,
    tab_anim: f32,

    resources: Vec<ResourceSlot>,

    event_banner: bool,
    banner_secs: f32,

    floating: Vec<FloatingText>,
    toast: Option<Toast>,

    modal: Option<Modal>,
    modal_anim: f32,
    settings_toggles: [bool; 3],

    sheet_anim: f32,

    units: Vec<Unit>,
    build_queue: Vec<BuildItem>,
    quests: Vec<Quest>,

    chat_log: Vec<&'static str>,
    next_chat_at: f64,
    chat_unread: u32,
    notifications: u32,

    hitboxes: Vec<(Rect, HitTarget)>,
}

impl State {
    fn new() -> Self {
        let world = generate_world();
        let pois = generate_pois();
        let home = pois
            .iter()
            .find(|p| p.kind == PoiKind::HomeCapital)
            .map_or(Pos::new(0, 0), |p| p.pos);

        let camera = Camera::new(
            Rect::new(0, 0, 20, 10),
            Size {
                width: WORLD_W,
                height: WORLD_H,
            },
        );

        let mut state = Self {
            watch: Stopwatch::new(),
            time: 0.0,
            world,
            pois,
            camera,
            cam_center: home,
            cursor: home,
            selected: None,
            last_map_rect: None,
            pointer: None,
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
                    color: GEM,
                    value: 340,
                    display: 340.0,
                },
                ResourceSlot {
                    icon: '♣',
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
            banner_secs: 252.0,
            floating: Vec::new(),
            toast: None,
            modal: None,
            modal_anim: 0.0,
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
                    remaining: 147.0,
                    total: 240.0,
                },
                BuildItem {
                    name: "Barracks",
                    level: 2,
                    remaining: 28.0,
                    total: 90.0,
                },
            ],
            quests: vec![
                Quest {
                    name: "Scout the ruins",
                    progress: 1.0,
                    reward: 50,
                    claimed: false,
                },
                Quest {
                    name: "Train 100 troops",
                    progress: 0.62,
                    reward: 80,
                    claimed: false,
                },
                Quest {
                    name: "Collect 500 gold",
                    progress: 1.0,
                    reward: 40,
                    claimed: false,
                },
                Quest {
                    name: "Upgrade the keep",
                    progress: 0.2,
                    reward: 120,
                    claimed: false,
                },
            ],
            chat_log: vec![CHAT_SCRIPT[0], CHAT_SCRIPT[1]],
            next_chat_at: 9.0,
            chat_unread: 2,
            notifications: 3,
            hitboxes: Vec::new(),
        };
        state.push_toast("Welcome back, Commander");
        state
    }

    /// Animation speed multiplier: near-instant when "reduced motion" is on.
    const fn motion_rate(&self) -> f32 {
        if self.settings_toggles[2] { 60.0 } else { 8.0 }
    }

    /// Total army power, for the header readout. Recomputed so training
    /// troops visibly moves the number.
    fn army_power(&self) -> u32 {
        self.units
            .iter()
            .map(|u| (u.power * 100.0) as u32 * u.count)
            .sum()
    }

    fn quests_claimable(&self) -> u32 {
        self.quests
            .iter()
            .filter(|q| q.progress >= 1.0 && !q.claimed)
            .count() as u32
    }

    fn advance(&mut self, dt: f64) {
        self.time += dt;
        let dt = dt as f32;
        let rate = self.motion_rate();
        let lerp_amt = (dt * rate).min(1.0);

        let target = self.tab.index() as f32;
        self.tab_anim = (target - self.tab_anim).mul_add(lerp_amt, self.tab_anim);

        let sheet_target = f32::from(u8::from(self.selected.is_some()));
        self.sheet_anim = (sheet_target - self.sheet_anim).mul_add(lerp_amt, self.sheet_anim);

        let modal_target = f32::from(u8::from(self.modal.is_some()));
        self.modal_anim = (modal_target - self.modal_anim).mul_add(lerp_amt, self.modal_anim);

        if self.event_banner {
            self.banner_secs -= dt;
            if self.banner_secs <= 0.0 {
                self.banner_secs = 252.0; // the raid is always 4:12 away
            }
        }

        // Build timers tick in real time; a finished job levels up and
        // immediately queues the next (longer) one, mobile-game style.
        let mut finished: Option<(String, u32)> = None;
        for b in &mut self.build_queue {
            b.remaining -= dt;
            if b.remaining <= 0.0 {
                b.level += 1;
                b.total *= 1.4;
                b.remaining = b.total;
                finished = Some((b.name.to_owned(), b.level));
            }
        }
        if let Some((name, level)) = finished {
            self.push_toast(format!("{name} reached Lv.{level}!"));
            self.notifications += 1;
        }

        // Scripted chat feed.
        if self.time >= self.next_chat_at {
            let idx = self.chat_log.len() % CHAT_SCRIPT.len();
            self.chat_log.push(CHAT_SCRIPT[idx]);
            self.next_chat_at = self.time + 8.5;
            if self.tab != Tab::Chat {
                self.chat_unread += 1;
            }
        }

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

    fn resource_mut(&mut self, name: &str) -> Option<&mut ResourceSlot> {
        self.resources.iter_mut().find(|r| r.name == name)
    }

    fn bump_resource(&mut self, name: &str, amount: i64) {
        if let Some(r) = self.resource_mut(name) {
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

    fn push_float(&mut self, at: Pos, text: impl Into<String>, color: Color) {
        self.floating.push(FloatingText {
            x: f32::from(at.x),
            y: f32::from(at.y),
            text: text.into(),
            color,
            born: self.time,
        });
    }

    /// Topmost interactive target under `pos`. Hitboxes are pushed in draw
    /// order (chrome first, overlays last), so the *last* match is the one
    /// visually on top.
    fn hit_at(&self, pos: Pos) -> Option<(Rect, HitTarget)> {
        self.hitboxes
            .iter()
            .rev()
            .find(|(r, _)| r.contains_pos(pos))
            .copied()
    }

    /// Pan the camera center by `(dx, dy)` cells, clamped to the world.
    fn pan_by(&mut self, dx: i32, dy: i32) {
        let x = (i32::from(self.cam_center.x) + dx).clamp(0, i32::from(WORLD_W) - 1);
        let y = (i32::from(self.cam_center.y) + dy).clamp(0, i32::from(WORLD_H) - 1);
        self.cam_center = Pos::new(x as u16, y as u16);
    }
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

    let header_h = if chrome.short { 1 } else { 4 };
    let banner_h = if state.event_banner {
        if chrome.short { 1 } else { 3 }
    } else {
        0
    };
    let nav_h = if chrome.short {
        1
    } else if chrome.xs {
        3
    } else {
        4
    };
    let [header_area, banner_area, body_area, nav_area] = take4(&split_v(
        screen,
        &[
            Constraint::Fixed(header_h),
            Constraint::Fixed(banner_h),
            Constraint::Fill,
            Constraint::Fixed(nav_h),
        ],
    ));

    draw_header(term, header_area, state, &chrome);
    if state.event_banner {
        draw_event_banner(term, banner_area, state, &chrome);
    }

    let (main_area, sidebar_area) = if chrome.wide {
        let [m, s] = take2(&split_h(
            body_area,
            &[Constraint::Fill, Constraint::Fixed(32)],
        ));
        (m, Some(s))
    } else {
        (body_area, None)
    };

    match state.tab {
        Tab::Map => draw_map(term, main_area, state),
        Tab::Army => draw_army(term, main_area, state, &chrome),
        Tab::Build => draw_build(term, main_area, state, &chrome),
        Tab::Quests => draw_quests(term, main_area, state, &chrome),
        Tab::Chat => draw_chat(term, main_area, state, &chrome),
    }
    if !matches!(state.tab, Tab::Map) {
        state.last_map_rect = None;
    }

    if let Some(sidebar) = sidebar_area {
        draw_detail_panel(term, sidebar, state, false, &chrome);
    } else if state.sheet_anim > 0.01 && matches!(state.tab, Tab::Map) {
        let full_h = sheet_height(state).min(main_area.height().saturating_sub(2));
        let h = (f32::from(full_h) * ease_out(state.sheet_anim)).round() as u16;
        if h > 0 && h <= main_area.height() {
            let sheet = Rect::new(
                main_area.left(),
                main_area.bottom() - h,
                main_area.width(),
                h,
            );
            draw_detail_panel(term, sheet, state, true, &chrome);
        }
    }

    draw_nav_bar(term, nav_area, state, &chrome);
    if state.modal_anim > 0.01 {
        // Everything under an open modal is inert (the scrim swallows
        // presses), so its hitboxes are dropped outright: the modal's
        // targets are the only reachable ones, and the overlap invariant
        // in `touch_targets_meet_minimums` stays meaningful.
        let chrome_targets = state.hitboxes.len();
        draw_modal(term, screen, state, &chrome);
        if state.modal.is_some() {
            state.hitboxes.drain(..chrome_targets);
        }
    }
    draw_floating(term, state);
    draw_toast(term, screen, state);
}

/// Draw one touch-sized button and register its hitbox. The whole `rect` is
/// tappable; the label is centered. Shows a pressed state while the pointer
/// is held on it.
fn draw_button<B: Backend>(
    term: &mut Terminal<B>,
    state: &mut State,
    rect: Rect,
    label: &str,
    fg: Color,
    target: HitTarget,
) {
    if rect.width() == 0 || rect.height() == 0 {
        return;
    }
    let pressed = state
        .pointer
        .as_ref()
        .is_some_and(|p| p.pressed == Some(rect) && rect.contains_pos(p.last));
    let bg = if pressed {
        Color::lerp(BUTTON_BG, fg, 0.35)
    } else {
        BUTTON_BG
    };
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            term.put_styled(x, y, ' ', Style::new().bg(bg));
        }
    }
    let text = truncate(label, rect.width_usize().saturating_sub(2));
    let tx = rect.left() + (rect.width().saturating_sub(text.chars().count() as u16)) / 2;
    let ty = rect.top() + rect.height() / 2;
    term.reset_style().fg(fg).bg(bg);
    term.print(tx, ty, &text);
    term.reset_style();
    state.hitboxes.push((rect, target));
}

// ── Header ────────────────────────────────────────────────────────────────────

fn draw_header<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State, chrome: &Chrome) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(CHROME_BG));
        }
    }

    // Row 0: kingdom name + army power (informational, not tappable).
    let title = if chrome.xs {
        "Highspire"
    } else {
        "Highspire  Lv.12"
    };
    term.reset_style().fg(ACCENT).bg(CHROME_BG);
    term.print(area.left() + 1, area.top(), title);
    let power = format!("PWR {}", abbreviate(state.army_power()));
    let px = area
        .right()
        .saturating_sub(power.chars().count() as u16 + 1);
    term.reset_style().fg(DIM_FG).bg(CHROME_BG);
    term.print(px, area.top(), &power);

    if area.height() < 4 {
        term.reset_style();
        return; // short chrome: keyboard-first, no pill row
    }

    // Rows 1-3: resource pills (3 rows tall) + bell + gear, all touch-sized.
    let pill_row = Rect::new(area.left(), area.top() + 1, area.width(), 3);
    let show = if area.width() < 56 { 2 } else { 4 };
    let mut x = pill_row.left() + 1;
    for i in 0..show {
        let r = &state.resources[i];
        let value = abbreviate(r.display.round().max(0.0) as u32);
        // The gems pill doubles as the shop button; hint that with a "+".
        let text = if r.name == "gems" {
            format!("{} {value} +", r.icon)
        } else {
            format!("{} {value}", r.icon)
        };
        let w = (text.chars().count() as u16 + 2).max(MIN_TARGET_W);
        let rect = Rect::new(x, pill_row.top(), w, MIN_TARGET_H);
        if rect.right() + 13 > pill_row.right() {
            break; // keep room for bell + gear
        }
        let color = r.color;
        draw_pill(term, state, rect, &text, color, HitTarget::ResourcePill(i));
        x = rect.right() + 1;
    }

    let gear = Rect::new(
        pill_row.right().saturating_sub(MIN_TARGET_W),
        pill_row.top(),
        MIN_TARGET_W,
        MIN_TARGET_H,
    );
    draw_pill(term, state, gear, "≡", FG, HitTarget::SettingsGear);
    let bell_text = format!("‼{}", state.notifications.min(9));
    let bell = Rect::new(
        gear.left().saturating_sub(MIN_TARGET_W + 1),
        pill_row.top(),
        MIN_TARGET_W,
        MIN_TARGET_H,
    );
    let bell_color = if state.notifications > 0 { BAD } else { DIM_FG };
    draw_pill(
        term,
        state,
        bell,
        &bell_text,
        bell_color,
        HitTarget::AlertBell,
    );
    term.reset_style();
}

/// A pill is a button with the chrome background treatment.
fn draw_pill<B: Backend>(
    term: &mut Terminal<B>,
    state: &mut State,
    rect: Rect,
    text: &str,
    fg: Color,
    target: HitTarget,
) {
    let pressed = state
        .pointer
        .as_ref()
        .is_some_and(|p| p.pressed == Some(rect) && rect.contains_pos(p.last));
    let bg = if pressed {
        Color::lerp(BUTTON_BG, fg, 0.35)
    } else {
        Color::Rgb {
            r: 36,
            g: 32,
            b: 54,
        }
    };
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            term.put_styled(x, y, ' ', Style::new().bg(bg));
        }
    }
    let clipped = truncate(text, rect.width_usize().saturating_sub(1));
    let tx = rect.left() + (rect.width().saturating_sub(clipped.chars().count() as u16)) / 2;
    let ty = rect.top() + rect.height() / 2;
    term.reset_style().fg(fg).bg(bg);
    term.print(tx, ty, &clipped);
    term.reset_style();
    state.hitboxes.push((rect, target));
}

// ── Event banner ──────────────────────────────────────────────────────────────

fn draw_event_banner<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    state: &mut State,
    chrome: &Chrome,
) {
    if area.height() == 0 {
        return;
    }
    let pulse = 0.5_f32.mul_add((state.time * 2.2).sin() as f32, 0.5);
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
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(bg));
        }
    }
    let countdown = format_mmss(state.banner_secs);
    let line = format!("‼ WAR RALLY - raid lands in {countdown}");
    let line = truncate(&line, area.width_usize().saturating_sub(2));
    let y = area.top() + area.height() / 2;
    let x = area.left() + (area.width().saturating_sub(line.chars().count() as u16)) / 2;
    term.reset_style().fg(Color::BRIGHT_WHITE).bg(bg);
    term.print(x, y, &line);
    if !chrome.short && area.height() >= 3 {
        let hint = "tap to prepare";
        let hx = area.left() + (area.width().saturating_sub(hint.chars().count() as u16)) / 2;
        term.reset_style()
            .fg(Color::Rgb {
                r: 240,
                g: 170,
                b: 170,
            })
            .bg(bg);
        term.print(hx, y + 1, hint);
    }
    term.reset_style();
    state.hitboxes.push((area, HitTarget::EventBanner));
}

// ── Map tab ───────────────────────────────────────────────────────────────────

fn draw_map<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State) {
    if area.width() < 2 || area.height() < 2 {
        return;
    }
    state.camera.set_viewport(area);
    state.camera.center_on(state.cam_center);
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
                '·',
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

    // Name labels for explored POIs, when there's room.
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

    if area.width() >= 26 && area.height() >= 6 {
        let hint = "drag: pan   tap: select";
        let hint = truncate(hint, area.width_usize());
        let x = area.right().saturating_sub(hint.chars().count() as u16 + 1);
        let y = area.bottom() - 1;
        term.reset_style().fg(DIM_FG).bg(BG);
        term.print(x, y, &hint);
        term.reset_style();
    }
}

// ── Army / Build / Quests / Chat tabs ────────────────────────────────────────

/// Card stride: 3 content rows + 1 separating row (>= the 1-cell spacing
/// the touch guidelines ask for between adjacent targets).
const CARD_STRIDE: u16 = 4;

fn draw_army<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State, chrome: &Chrome) {
    panel_bg(term, area, "GARRISON");
    let inner = inset(area);
    let button_w = 11u16.min(inner.width() / 2);
    for i in 0..state.units.len() {
        let y = inner.top() + i as u16 * CARD_STRIDE;
        if y + 3 > inner.bottom() {
            break;
        }
        let (name, count, power) = {
            let u = &state.units[i];
            (u.name, u.count, u.power)
        };
        let text_w = inner.width().saturating_sub(button_w + 1);
        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y,
            &truncate(&format!("{name}  x{count}"), text_w as usize),
        );
        gauge(
            term,
            Rect::new(inner.left(), y + 1, text_w, 1),
            "pwr",
            power,
        );
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y + 2,
            &truncate("200 food per 10", text_w as usize),
        );
        let btn = Rect::new(inner.right().saturating_sub(button_w), y, button_w, 3);
        let label = if chrome.xs { "TRAIN" } else { "TRAIN +10" };
        draw_button(term, state, btn, label, GOOD, HitTarget::TrainUnit(i));
    }
    term.reset_style();
}

fn draw_build<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State, chrome: &Chrome) {
    panel_bg(term, area, "BUILD QUEUE");
    let inner = inset(area);
    let button_w = 11u16.min(inner.width() / 2);
    let mut y = inner.top();
    for i in 0..state.build_queue.len() {
        if y + 3 > inner.bottom() {
            break;
        }
        let (name, level, remaining, total) = {
            let b = &state.build_queue[i];
            (b.name, b.level, b.remaining, b.total)
        };
        let text_w = inner.width().saturating_sub(button_w + 1);
        term.reset_style().fg(FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y,
            &truncate(&format!("{name}  Lv.{level}"), text_w as usize),
        );
        let progress = (1.0 - remaining / total).clamp(0.0, 1.0);
        gauge(
            term,
            Rect::new(inner.left(), y + 1, text_w, 1),
            "",
            progress,
        );
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y + 2,
            &truncate(&format_mmss(remaining), text_w as usize),
        );
        // The classic mobile hook: boosting is free when nearly done,
        // otherwise it wants gems you don't have.
        let (label, color) = if remaining < 45.0 {
            ("FREE!", GOOD)
        } else {
            ("BOOST ♦", GEM)
        };
        let btn = Rect::new(inner.right().saturating_sub(button_w), y, button_w, 3);
        draw_button(term, state, btn, label, color, HitTarget::BoostBuild(i));
        y += CARD_STRIDE;
    }
    // Second-builder upsell, full width.
    if y + 3 <= inner.bottom() {
        let label = if chrome.xs {
            "HIRE 2ND BUILDER  450♦"
        } else {
            "HIRE A SECOND BUILDER  450♦"
        };
        let btn = Rect::new(inner.left(), y, inner.width(), 3);
        draw_button(term, state, btn, label, GEM, HitTarget::HireBuilder);
    }
    term.reset_style();
}

fn draw_quests<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    state: &mut State,
    _chrome: &Chrome,
) {
    panel_bg(term, area, "QUESTS");
    let inner = inset(area);
    let button_w = 11u16.min(inner.width() / 2);
    for i in 0..state.quests.len() {
        let y = inner.top() + i as u16 * CARD_STRIDE;
        if y + 3 > inner.bottom() {
            break;
        }
        let (name, progress, reward, claimed) = {
            let q = &state.quests[i];
            (q.name, q.progress, q.reward, q.claimed)
        };
        let claimable = progress >= 1.0 && !claimed;
        let text_w = inner.width().saturating_sub(button_w + 1);
        let color = if claimed {
            DIM_FG
        } else if claimable {
            GOOD
        } else {
            FG
        };
        term.reset_style().fg(color).bg(PANEL_BG);
        term.print(inner.left(), y, &truncate(name, text_w as usize));
        gauge(
            term,
            Rect::new(inner.left(), y + 1, text_w, 1),
            "",
            progress,
        );
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            y + 2,
            &truncate(&format!("reward: {reward} gold"), text_w as usize),
        );
        if claimable {
            let btn = Rect::new(inner.right().saturating_sub(button_w), y, button_w, 3);
            draw_button(term, state, btn, "CLAIM", ACCENT, HitTarget::ClaimQuest(i));
        } else if claimed {
            term.reset_style().fg(DIM_FG).bg(PANEL_BG);
            term.print(inner.right().saturating_sub(5), y + 1, "done");
        }
    }
    term.reset_style();
}

fn draw_chat<B: Backend>(term: &mut Terminal<B>, area: Rect, state: &mut State, _chrome: &Chrome) {
    panel_bg(term, area, "ALLIANCE CHAT");
    let inner = inset(area);
    if inner.height() < 4 {
        return;
    }
    // Input bar (bottom, touch-sized) first so the message list can clip
    // around it.
    let input = Rect::new(
        inner.left(),
        inner.bottom().saturating_sub(3),
        inner.width(),
        3,
    );
    let list_h = inner.height().saturating_sub(4);
    // Newest messages at the bottom of the list, like a real chat.
    let visible = usize::from(list_h);
    let start = state.chat_log.len().saturating_sub(visible);
    for (row, msg) in state.chat_log[start..].iter().enumerate() {
        let y = inner.top() + row as u16;
        if y >= input.top().saturating_sub(1) {
            break;
        }
        let color = if msg.starts_with("System:") {
            ACCENT
        } else {
            DIM_FG
        };
        term.reset_style().fg(color).bg(PANEL_BG);
        term.print(inner.left(), y, &truncate(msg, inner.width_usize()));
    }
    draw_button(
        term,
        state,
        input,
        "Tap to type...",
        DIM_FG,
        HitTarget::ChatInput,
    );
    term.reset_style();
}

// ── Detail panel (bottom sheet or sidebar) ───────────────────────────────────

/// Actions offered for the selected tile, driven by what's on it.
fn sheet_actions(state: &State, sel: Pos) -> Vec<(SheetAction, Color)> {
    let explored = distance_from_home(&state.pois, sel) <= EXPLORED_RADIUS;
    match state.poi_at(sel).map(|p| p.kind) {
        Some(PoiKind::HomeCapital) => vec![(SheetAction::EnterCity, ACCENT)],
        Some(PoiKind::RivalCapital | PoiKind::Ruins | PoiKind::Outpost) => {
            vec![(SheetAction::Scout, FG), (SheetAction::March, BAD)]
        }
        Some(PoiKind::GoldMine | PoiKind::TimberCamp | PoiKind::Farmland) if explored => {
            vec![(SheetAction::Scout, FG), (SheetAction::Harvest, GOOD)]
        }
        _ => vec![(SheetAction::Scout, FG)],
    }
}

/// Bottom-sheet height: top border + 3 header rows + button row + padding.
const fn sheet_height(_state: &State) -> u16 {
    9
}

fn draw_detail_panel<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    state: &mut State,
    is_sheet: bool,
    _chrome: &Chrome,
) {
    if area.width() < 8 || area.height() < 4 {
        return;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(PANEL_BG));
        }
    }
    for x in area.left()..area.right() {
        term.put_styled(x, area.top(), '─', Style::new().fg(BORDER).bg(PANEL_BG));
    }
    if is_sheet {
        // The sheet swallows presses so they don't pan the map underneath.
        state.hitboxes.push((area, HitTarget::Inert));
    }

    let inner = Rect::new(
        area.left() + 1,
        area.top() + 1,
        area.width().saturating_sub(2),
        area.height().saturating_sub(2),
    );

    let Some(sel) = state.selected else {
        term.reset_style().fg(DIM_FG).bg(PANEL_BG);
        term.print(
            inner.left(),
            inner.top() + 1,
            &truncate("Tap a map tile to inspect it.", inner.width_usize()),
        );
        return;
    };

    // Close button (sheet only), top-right, touch-sized.
    let close_w = MIN_TARGET_W;
    if is_sheet {
        let btn = Rect::new(
            inner.right().saturating_sub(close_w),
            inner.top(),
            close_w,
            MIN_TARGET_H,
        );
        draw_button(term, state, btn, "x", DIM_FG, HitTarget::SheetClose);
    }

    let poi = state.poi_at(sel);
    let terrain = terrain_at(&state.world, sel);
    let title = poi.map_or_else(|| terrain.name(), |p| p.name);
    let subtitle = poi.map_or_else(|| terrain.name(), |p| p.kind.label());
    let flavor = poi.map_or("Empty land. Good for farming, someday.", |p| {
        p.kind.flavor()
    });
    let color = poi.map_or_else(|| terrain.color(), |p| p.kind.color());
    let text_w = inner
        .width()
        .saturating_sub(if is_sheet { close_w + 1 } else { 0 }) as usize;

    term.reset_style().fg(color).bg(PANEL_BG);
    term.print(inner.left(), inner.top(), &truncate(title, text_w));
    term.reset_style().fg(DIM_FG).bg(PANEL_BG);
    let dist = distance_from_home(&state.pois, sel);
    let meta = format!("{subtitle} - ({}, {}) - {dist} tiles", sel.x, sel.y);
    term.print(inner.left(), inner.top() + 1, &truncate(&meta, text_w));
    term.print(inner.left(), inner.top() + 2, &truncate(flavor, text_w));

    // Action buttons: one row of up to 3 side-by-side in the sheet; stacked
    // in the sidebar (it's narrow but tall).
    let actions = sheet_actions(state, sel);
    if is_sheet {
        let y = inner.top() + 4;
        if y + 3 > inner.bottom() + 1 {
            return;
        }
        let n = actions.len() as u16;
        let gap = 1u16;
        let bw = (inner.width().saturating_sub(gap * (n - 1))) / n;
        let mut x = inner.left();
        for (action, color) in actions {
            let btn = Rect::new(x, y, bw, 3);
            draw_button(
                term,
                state,
                btn,
                action.label(),
                color,
                HitTarget::SheetAction(action),
            );
            x += bw + gap;
        }
    } else {
        let mut y = inner.top() + 4;
        for (action, color) in actions {
            if y + 3 > inner.bottom() + 1 {
                break;
            }
            let btn = Rect::new(inner.left(), y, inner.width(), 3);
            draw_button(
                term,
                state,
                btn,
                action.label(),
                color,
                HitTarget::SheetAction(action),
            );
            y += CARD_STRIDE;
        }
    }
    term.reset_style();
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

    // Sliding active-tab indicator along the top edge of the bar.
    let indicator_x = area.left() + (state.tab_anim * f32::from(slot_w)).round() as u16;
    for x in indicator_x..(indicator_x + slot_w).min(area.right()) {
        term.put_styled(x, area.top(), '▀', Style::new().fg(ACCENT).bg(CHROME_BG));
    }

    let mut x = area.left();
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let w = slot_w + u16::from((i as u16) < extra);
        let rect = Rect::new(x, area.top(), w, area.height());
        let active = state.tab == tab;
        let pressed = state
            .pointer
            .as_ref()
            .is_some_and(|p| p.pressed == Some(rect) && rect.contains_pos(p.last));
        if pressed {
            for y in rect.top()..rect.bottom() {
                for cx in rect.left()..rect.right() {
                    term.put_styled(cx, y, ' ', Style::new().bg(BUTTON_BG));
                }
            }
        }
        let bg = if pressed { BUTTON_BG } else { CHROME_BG };
        let fg = if active { ACCENT } else { DIM_FG };

        let icon_y = if area.height() >= 3 {
            area.top() + 1
        } else {
            area.top()
        };
        let icon_x = x + w / 2;
        term.reset_style().fg(fg).bg(bg);
        term.put(icon_x, icon_y, tab.icon());
        if area.height() >= 4 && !chrome.xs {
            let label = tab.label();
            let lx = x + w.saturating_sub(label.chars().count() as u16) / 2;
            term.print(lx, icon_y + 1, label);
        }

        // Badges: claimable quests, unread chat.
        let badge = match tab {
            Tab::Quests => state.quests_claimable(),
            Tab::Chat => state.chat_unread,
            _ => 0,
        };
        if badge > 0 {
            term.reset_style().fg(Color::BRIGHT_WHITE).bg(BAD);
            term.print(icon_x + 1, icon_y, &badge.min(9).to_string());
        }
        state.hitboxes.push((rect, HitTarget::Tab(tab)));
        x += w;
    }
    term.reset_style();
}

// ── Modals ────────────────────────────────────────────────────────────────────

fn draw_modal<B: Backend>(
    term: &mut Terminal<B>,
    screen: Rect,
    state: &mut State,
    chrome: &Chrome,
) {
    let Some(modal) = state.modal else { return };
    let (title, full_h) = match modal {
        Modal::Shop => ("GEM SHOP", 5 + GEM_PACKS.len() as u16 * CARD_STRIDE),
        Modal::Settings => ("SETTINGS", 5 + 3 * CARD_STRIDE),
        Modal::ConfirmMarch(_) => ("CONFIRM MARCH", 10),
    };
    let full_h = full_h.min(screen.height());
    let h = (f32::from(full_h) * ease_out(state.modal_anim)).round() as u16;
    if h == 0 {
        return;
    }
    let w = if chrome.wide {
        48.min(screen.width())
    } else {
        screen.width()
    };
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

    // Tap outside to dismiss; also blocks everything underneath.
    state.hitboxes.push((screen, HitTarget::Scrim));

    let bg = Color::Rgb {
        r: 24,
        g: 22,
        b: 36,
    };
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(bg));
        }
    }
    for x in area.left()..area.right() {
        term.put_styled(x, area.top(), '─', Style::new().fg(BORDER).bg(bg));
    }
    state.hitboxes.push((area, HitTarget::Inert));
    if area.height() < 5 {
        return; // still animating open
    }

    let inner = Rect::new(
        area.left() + 1,
        area.top() + 1,
        area.width().saturating_sub(2),
        area.height().saturating_sub(2),
    );
    term.reset_style().fg(FG).bg(bg);
    term.print(inner.left(), inner.top(), title);
    let close = Rect::new(
        inner.right().saturating_sub(MIN_TARGET_W),
        inner.top(),
        MIN_TARGET_W,
        MIN_TARGET_H,
    );
    draw_button(term, state, close, "x", DIM_FG, HitTarget::ModalClose);

    let content_top = inner.top() + 3;
    match modal {
        Modal::Shop => {
            let packs: Vec<(usize, String, bool)> = GEM_PACKS
                .iter()
                .enumerate()
                .map(|(i, (gems, name, price, best))| {
                    (i, format!("{gems}♦  {name}  {price}"), *best)
                })
                .collect();
            let mut y = content_top;
            for (i, label, best) in packs {
                if y + 3 > inner.bottom() + 1 {
                    break;
                }
                let btn = Rect::new(inner.left(), y, inner.width(), 3);
                let color = if best { ACCENT } else { GEM };
                let label = if best {
                    format!("{label}  *BEST VALUE*")
                } else {
                    label
                };
                draw_button(term, state, btn, &label, color, HitTarget::BuyPack(i));
                y += CARD_STRIDE;
            }
        }
        Modal::Settings => {
            let labels = ["Sound", "Notifications", "Reduced motion"];
            let toggles = state.settings_toggles;
            let mut y = content_top;
            for (i, label) in labels.iter().enumerate() {
                if y + 3 > inner.bottom() + 1 {
                    break;
                }
                let on = toggles[i];
                let text = format!("{} {label}", if on { "[x]" } else { "[ ]" });
                let btn = Rect::new(inner.left(), y, inner.width(), 3);
                let color = if on { GOOD } else { DIM_FG };
                draw_button(term, state, btn, &text, color, HitTarget::ToggleSetting(i));
                y += CARD_STRIDE;
            }
        }
        Modal::ConfirmMarch(target) => {
            term.reset_style().fg(DIM_FG).bg(bg);
            term.print(
                inner.left(),
                content_top,
                &truncate(
                    &format!("March on {target}? Your army will be away."),
                    inner.width_usize(),
                ),
            );
            let y = content_top + 2;
            if y + 3 <= inner.bottom() + 1 {
                let gap = 1u16;
                let bw = (inner.width() - gap) / 2;
                let march = Rect::new(inner.left(), y, bw, 3);
                let cancel = Rect::new(inner.left() + bw + gap, y, bw, 3);
                draw_button(term, state, march, "MARCH!", BAD, HitTarget::ConfirmMarch);
                draw_button(
                    term,
                    state,
                    cancel,
                    "Cancel",
                    DIM_FG,
                    HitTarget::CancelModal,
                );
            }
        }
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

    let text = format!(" ♫ {} ", toast.text);
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

// ── Panel helpers ─────────────────────────────────────────────────────────────

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

// ── Activation (what a completed tap does) ───────────────────────────────────

fn activate(state: &mut State, target: HitTarget, at: Pos) {
    match target {
        HitTarget::Tab(tab) => {
            state.tab = tab;
            if tab == Tab::Chat {
                state.chat_unread = 0;
            }
        }
        HitTarget::ResourcePill(i) => {
            if state.resources.get(i).is_some_and(|r| r.name == "gems") {
                state.modal = Some(Modal::Shop);
            } else if let Some(r) = state.resources.get(i) {
                let color = r.color;
                state.push_float(at, "+", color);
            }
        }
        HitTarget::AlertBell => {
            if state.notifications > 0 {
                state.notifications -= 1;
                state.push_toast("Notification cleared");
            } else {
                state.push_toast("All caught up");
            }
        }
        HitTarget::SettingsGear => state.modal = Some(Modal::Settings),
        HitTarget::EventBanner => {
            state.event_banner = false;
            state.push_toast("Reinforcements requested!");
        }
        HitTarget::SheetClose => state.selected = None,
        HitTarget::SheetAction(action) => {
            let name = state
                .selected
                .and_then(|sel| state.poi_at(sel))
                .map_or("the wilds", |p| p.name);
            match action {
                SheetAction::Scout => state.push_toast(format!("Scouts dispatched to {name}")),
                SheetAction::March => state.modal = Some(Modal::ConfirmMarch(name)),
                SheetAction::Harvest => {
                    state.bump_resource("gold", 25);
                    state.push_float(at, "+25", ACCENT);
                    state.push_toast(format!("Harvested {name}"));
                }
                SheetAction::EnterCity => {
                    state.push_toast("The city view is another mockup away");
                }
            }
        }
        HitTarget::TrainUnit(i) => {
            let food = state.resource_mut("food").map_or(0, |r| r.value);
            if food >= 200 {
                state.bump_resource("food", -200);
                if let Some(u) = state.units.get_mut(i) {
                    u.count += 10;
                }
                state.push_float(at, "+10", GOOD);
            } else {
                state.push_toast("Not enough food!");
            }
        }
        HitTarget::BoostBuild(i) => {
            let Some(b) = state.build_queue.get_mut(i) else {
                return;
            };
            if b.remaining < 45.0 {
                b.level += 1;
                b.total *= 1.4;
                b.remaining = b.total;
                let msg = format!("{} boosted to Lv.{}!", b.name, b.level);
                state.push_toast(msg);
            } else {
                let gems = (b.remaining / 10.0).ceil() as u32;
                state.push_toast(format!("Need {gems}♦ to finish now"));
            }
        }
        HitTarget::HireBuilder => {
            state.push_toast("Not enough gems! (conveniently)");
            state.modal = Some(Modal::Shop);
        }
        HitTarget::ClaimQuest(i) => {
            if let Some((name, reward)) = state.quests.get_mut(i).map(|q| {
                q.claimed = true;
                (q.name, q.reward)
            }) {
                state.bump_resource("gold", i64::from(reward));
                state.push_float(at, format!("+{reward}"), ACCENT);
                state.push_toast(format!("Reward claimed: {name}"));
            }
        }
        HitTarget::ChatInput => state.push_toast("Typing is out of scope for this mock"),
        HitTarget::BuyPack(i) => {
            if let Some((gems, ..)) = GEM_PACKS.get(i) {
                state.bump_resource("gems", i64::from(*gems));
                state.push_float(at, format!("+{gems}"), GEM);
                state.push_toast("Purchase simulated. Your wallet is safe.");
                state.modal = None;
            }
        }
        HitTarget::ToggleSetting(i) => {
            if let Some(t) = state.settings_toggles.get_mut(usize::from(i as u8)) {
                *t = !*t;
            }
        }
        HitTarget::ModalClose | HitTarget::CancelModal | HitTarget::Scrim => state.modal = None,
        HitTarget::ConfirmMarch => {
            state.modal = None;
            state.push_toast("March underway - eta 12:00");
            state.notifications += 1;
        }
        HitTarget::Inert => {}
    }
}

// ── Pointer handling (tap vs drag) ────────────────────────────────────────────

fn on_pointer_down(state: &mut State, pos: Pos) {
    let hit = state.hit_at(pos);
    let on_map = hit.is_none()
        && matches!(state.tab, Tab::Map)
        && state.last_map_rect.is_some_and(|r| r.contains_pos(pos));
    state.pointer = Some(PointerState {
        start: pos,
        last: pos,
        on_map,
        pressed: hit.map(|(r, _)| r),
        dragging: false,
    });
}

fn on_pointer_move(state: &mut State, pos: Pos) {
    let Some(p) = state.pointer.as_mut() else {
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
        // Content follows the finger: dragging right pans the camera left.
        state.pan_by(-dx, -dy);
    }
}

fn on_pointer_up(state: &mut State, pos: Pos) {
    let Some(p) = state.pointer.take() else {
        return;
    };
    if p.on_map {
        if !p.dragging
            && let Some(map_rect) = state.last_map_rect
            && map_rect.contains_pos(pos)
            && let Some(world) = state.camera.screen_to_world(pos)
        {
            state.selected = Some(world);
            state.cursor = world;
        }
        return;
    }
    // Button semantics: activate only when the release lands on the same
    // control the press started on (slide off to cancel).
    if let (Some((r1, _)), Some((r2, target))) = (state.hit_at(p.start), state.hit_at(pos))
        && r1 == r2
    {
        activate(state, target, pos);
    }
}

fn on_scroll(state: &mut State, pos: Pos, dy: i32) {
    // Scroll pans the map when the wheel is over it and nothing covers it.
    if matches!(state.tab, Tab::Map)
        && state.hit_at(pos).is_none()
        && state.last_map_rect.is_some_and(|r| r.contains_pos(pos))
    {
        state.pan_by(0, dy * 2);
    }
}

// ── Keyboard handling ─────────────────────────────────────────────────────────

fn move_cursor(state: &mut State, dx: i16, dy: i16) {
    let nx = i32::from(state.cursor.x) + i32::from(dx);
    let ny = i32::from(state.cursor.y) + i32::from(dy);
    if nx < 0 || ny < 0 || nx >= i32::from(WORLD_W) || ny >= i32::from(WORLD_H) {
        return;
    }
    state.cursor = Pos::new(nx as u16, ny as u16);
    state.cam_center = state.cursor;
}

/// Applies one input event. Returns `false` to quit.
fn handle(state: &mut State, event: &Event) -> bool {
    match event {
        Event::Key(k) if k.is_down() => match k.code {
            KeyCode::Escape => {
                if state.modal.is_some() {
                    state.modal = None;
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
                    if tab == Tab::Chat {
                        state.chat_unread = 0;
                    }
                }
            }
            _ => {}
        },
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Down(MouseButton::Left) => on_pointer_down(state, m.position),
            MouseEventKind::Moved => on_pointer_move(state, m.position),
            MouseEventKind::Up(MouseButton::Left) => on_pointer_up(state, m.position),
            MouseEventKind::ScrollUp => on_scroll(state, m.position, -1),
            MouseEventKind::ScrollDown => on_scroll(state, m.position, 1),
            _ => {}
        },
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

    fn draw_at(state: &mut State, width: u16, height: u16) {
        let mut term = Terminal::new(Headless::new(width, height));
        draw(&mut term, state);
        term.present().unwrap();
    }

    /// Simulate a tap: pointer down then up at the same cell.
    fn tap(state: &mut State, x: u16, y: u16) {
        let pos = Pos::new(x, y);
        handle(
            state,
            &Event::Mouse(retroglyph_core::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                position: pos,
                pixel_position: None,
                modifiers: retroglyph_core::event::KeyModifiers::NONE,
            }),
        );
        handle(
            state,
            &Event::Mouse(retroglyph_core::event::MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                position: pos,
                pixel_position: None,
                modifiers: retroglyph_core::event::KeyModifiers::NONE,
            }),
        );
    }

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
        Event::Mouse(retroglyph_core::event::MouseEvent {
            kind,
            position: Pos::new(x, y),
            pixel_position: None,
            modifiers: retroglyph_core::event::KeyModifiers::NONE,
        })
    }

    fn find_target(state: &State, want: HitTarget) -> Rect {
        state
            .hitboxes
            .iter()
            .find(|(_, t)| *t == want)
            .map(|(r, _)| *r)
            .unwrap_or_else(|| panic!("target {want:?} not registered"))
    }

    /// Every interactive target meets the WCAG-derived minimums from the
    /// module docs (>= 6x3 cells, no overlaps) across sizes, tabs, and
    /// overlay states. Surfaces (scrims/sheet bodies) are exempt.
    #[test]
    fn touch_targets_meet_minimums() {
        let sizes = [(40u16, 26u16), (46, 24), (50, 25), (80, 30), (120, 40)];
        for (w, h) in sizes {
            for setup in 0..8u8 {
                let mut state = State::new();
                match setup {
                    0 => {}
                    1 => state.tab = Tab::Army,
                    2 => state.tab = Tab::Build,
                    3 => state.tab = Tab::Quests,
                    4 => state.tab = Tab::Chat,
                    5 => {
                        state.selected = Some(Pos::new(8, 11));
                        state.sheet_anim = 1.0;
                    }
                    6 => {
                        state.modal = Some(Modal::Shop);
                        state.modal_anim = 1.0;
                    }
                    7 => {
                        state.modal = Some(Modal::Settings);
                        state.modal_anim = 1.0;
                    }
                    _ => unreachable!(),
                }
                draw_at(&mut state, w, h);
                let targets: Vec<&(Rect, HitTarget)> = state
                    .hitboxes
                    .iter()
                    .filter(|(_, t)| !t.is_surface())
                    .collect();
                for (rect, target) in &targets {
                    assert!(
                        rect.width() >= MIN_TARGET_W && rect.height() >= MIN_TARGET_H,
                        "{target:?} is {}x{} at {w}x{h} (setup {setup}); \
                         minimum is {MIN_TARGET_W}x{MIN_TARGET_H}",
                        rect.width(),
                        rect.height(),
                    );
                }
                for (i, (a, ta)) in targets.iter().enumerate() {
                    for (b, tb) in targets.iter().skip(i + 1) {
                        assert!(
                            !a.overlaps(*b),
                            "{ta:?} at {a:?} overlaps {tb:?} at {b:?} ({w}x{h}, setup {setup})",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn tap_selects_a_tile_and_opens_the_sheet() {
        let mut state = State::new();
        draw_at(&mut state, 50, 25);
        let map = state.last_map_rect.expect("map rect");
        tap(&mut state, map.left() + 3, map.top() + 3);
        assert!(state.selected.is_some());
    }

    #[test]
    fn drag_pans_the_camera_without_selecting() {
        let mut state = State::new();
        draw_at(&mut state, 50, 25);
        let map = state.last_map_rect.expect("map rect");
        let before = state.cam_center;
        let (x, y) = (map.left() + 5, map.top() + 5);
        handle(
            &mut state,
            &mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        );
        handle(&mut state, &mouse(MouseEventKind::Moved, x + 3, y + 2));
        handle(
            &mut state,
            &mouse(MouseEventKind::Up(MouseButton::Left), x + 3, y + 2),
        );
        assert_ne!(state.cam_center, before, "drag should pan");
        assert!(state.selected.is_none(), "drag must not select a tile");
    }

    #[test]
    fn scroll_pans_vertically() {
        let mut state = State::new();
        // Start away from the top edge so an upward pan is visible.
        state.cam_center = Pos::new(17, 10);
        draw_at(&mut state, 50, 25);
        let map = state.last_map_rect.expect("map rect");
        let before = state.cam_center;
        handle(
            &mut state,
            &mouse(MouseEventKind::ScrollUp, map.left() + 5, map.top() + 5),
        );
        assert_ne!(state.cam_center.y, before.y);
    }

    #[test]
    fn nav_tap_switches_tabs_and_clears_chat_badge() {
        let mut state = State::new();
        state.chat_unread = 3;
        draw_at(&mut state, 80, 30);
        let rect = find_target(&state, HitTarget::Tab(Tab::Chat));
        tap(&mut state, rect.left() + 1, rect.top() + 1);
        assert!(matches!(state.tab, Tab::Chat));
        assert_eq!(state.chat_unread, 0);
    }

    #[test]
    fn gems_pill_opens_shop_and_scrim_closes_it() {
        let mut state = State::new();
        draw_at(&mut state, 80, 30);
        let rect = find_target(&state, HitTarget::ResourcePill(1));
        tap(&mut state, rect.left() + 1, rect.top());
        assert!(matches!(state.modal, Some(Modal::Shop)));

        // Redraw with the modal open, then tap outside it (the scrim).
        state.modal_anim = 1.0;
        draw_at(&mut state, 80, 30);
        let scrim = find_target(&state, HitTarget::Scrim);
        // Top-left corner is outside the centered/bottom modal box.
        tap(&mut state, scrim.left(), scrim.top());
        assert!(state.modal.is_none());
    }

    #[test]
    fn buying_a_pack_adds_gems() {
        let mut state = State::new();
        state.modal = Some(Modal::Shop);
        state.modal_anim = 1.0;
        draw_at(&mut state, 80, 30);
        let before = state.resources[1].value;
        let rect = find_target(&state, HitTarget::BuyPack(0));
        tap(&mut state, rect.left() + 1, rect.top() + 1);
        assert_eq!(state.resources[1].value, before + GEM_PACKS[0].0);
        assert!(state.modal.is_none());
    }

    #[test]
    fn march_flow_confirms_through_the_modal() {
        let mut state = State::new();
        // Select the rival capital, which offers a MARCH action.
        state.selected = Some(Pos::new(28, 4));
        state.sheet_anim = 1.0;
        draw_at(&mut state, 50, 25);
        let rect = find_target(&state, HitTarget::SheetAction(SheetAction::March));
        tap(&mut state, rect.left() + 1, rect.top() + 1);
        assert!(matches!(state.modal, Some(Modal::ConfirmMarch(_))));

        state.modal_anim = 1.0;
        draw_at(&mut state, 50, 25);
        let go = find_target(&state, HitTarget::ConfirmMarch);
        tap(&mut state, go.left() + 1, go.top() + 1);
        assert!(state.modal.is_none());
        assert!(
            state
                .toast
                .as_ref()
                .is_some_and(|t| t.text.contains("March"))
        );
    }

    #[test]
    fn claiming_a_quest_pays_gold_and_clears_the_badge() {
        let mut state = State::new();
        state.tab = Tab::Quests;
        let badge_before = state.quests_claimable();
        draw_at(&mut state, 80, 30);
        let gold_before = state.resources[0].value;
        let rect = find_target(&state, HitTarget::ClaimQuest(0));
        tap(&mut state, rect.left() + 1, rect.top() + 1);
        assert!(state.quests[0].claimed);
        assert!(state.resources[0].value > gold_before);
        assert_eq!(state.quests_claimable(), badge_before - 1);
    }

    #[test]
    fn training_spends_food_and_grows_the_army() {
        let mut state = State::new();
        state.tab = Tab::Army;
        draw_at(&mut state, 80, 30);
        let food_before = state.resources[3].value;
        let count_before = state.units[0].count;
        let rect = find_target(&state, HitTarget::TrainUnit(0));
        tap(&mut state, rect.left() + 1, rect.top() + 1);
        assert_eq!(state.units[0].count, count_before + 10);
        assert_eq!(state.resources[3].value, food_before - 200);
    }

    #[test]
    fn slide_off_a_button_cancels_activation() {
        let mut state = State::new();
        draw_at(&mut state, 80, 30);
        let rect = find_target(&state, HitTarget::Tab(Tab::Quests));
        // Press on the tab, slide off it (out of the nav bar), release.
        handle(
            &mut state,
            &mouse(
                MouseEventKind::Down(MouseButton::Left),
                rect.left() + 1,
                rect.top() + 1,
            ),
        );
        handle(
            &mut state,
            &mouse(
                MouseEventKind::Moved,
                rect.left() + 1,
                rect.top().saturating_sub(2),
            ),
        );
        handle(
            &mut state,
            &mouse(
                MouseEventKind::Up(MouseButton::Left),
                rect.left() + 1,
                rect.top().saturating_sub(2),
            ),
        );
        assert!(matches!(state.tab, Tab::Map), "slide-off must not activate");
    }

    #[test]
    fn wide_layout_renders_the_sidebar() {
        let mut state = State::new();
        let mut term = Terminal::new(Headless::new(120, 40));
        draw(&mut term, &mut state);
        term.present().unwrap();
        let view = term.backend().format_view();
        assert!(view.contains("inspect"));
    }

    #[test]
    fn short_layout_still_renders_and_registers_nav() {
        let mut state = State::new();
        draw_at(&mut state, 60, 12);
        // Touch minimums are waived below BP_SHORT, but the nav must still
        // be tappable at all.
        assert!(
            state
                .hitboxes
                .iter()
                .any(|(_, t)| matches!(t, HitTarget::Tab(_)))
        );
    }
}
