#![allow(dead_code, unreachable_pub)]
//! Minimal battle simulation: board state, unit types, and a short replay log.
//!
//! Intentionally small — just enough to drive the UI demo.  No AI, no engine,
//! no complex turn scripts.  Three scripted steps showcase the event log and
//! replay controls.

// ── Factions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Blue,
    Red,
}

impl Faction {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Blue => "BLUE",
            Self::Red => "RED",
        }
    }
}

// ── Unit type ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Trooper,
    Driver,
}

impl UnitKind {
    pub const fn glyph(self) -> char {
        match self {
            Self::Trooper => 'T',
            Self::Driver => 'D',
        }
    }
}

// ── Unit ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Unit {
    pub id: u8,
    pub faction: Faction,
    pub kind: UnitKind,
    /// Hex axial coordinate (q, r).
    pub pos: (i32, i32),
    pub strength: u8,
    pub max_strength: u8,
}

impl Unit {
    pub const fn new(
        id: u8,
        faction: Faction,
        kind: UnitKind,
        pos: (i32, i32),
        strength: u8,
    ) -> Self {
        Self {
            id,
            faction,
            kind,
            pos,
            strength,
            max_strength: strength,
        }
    }

    pub const fn is_alive(&self) -> bool {
        self.strength > 0
    }
}

// ── Cards ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub name: &'static str,
    /// Visually highlighted (special action card).
    pub has_special: bool,
}

impl Card {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            has_special: false,
        }
    }

    pub const fn special(name: &'static str) -> Self {
        Self {
            name,
            has_special: true,
        }
    }
}

// ── Events ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum GameEvent {
    TurnStart {
        turn: u32,
        faction: Faction,
        card: Card,
    },
    Move {
        unit_id: u8,
        from: (i32, i32),
        to: (i32, i32),
    },
    Attack {
        attacker_id: u8,
        target_id: u8,
        hits: u8,
        eliminated: bool,
    },
}

impl GameEvent {
    /// Short human-readable description for the event log sidebar.
    pub fn description(&self, units: &[Unit]) -> String {
        match self {
            Self::TurnStart {
                turn,
                faction,
                card,
            } => {
                format!("Turn {}: {} plays {}", turn, faction.name(), card.name)
            }
            Self::Move { unit_id, from, to } => {
                let glyph = find_unit(units, *unit_id).map_or('?', |u| u.kind.glyph());
                format!("{glyph} [{},{}]→[{},{}]", from.0, from.1, to.0, to.1)
            }
            Self::Attack {
                attacker_id,
                target_id,
                hits,
                eliminated,
            } => {
                let ag = find_unit(units, *attacker_id).map_or('?', |u| u.kind.glyph());
                let tg = find_unit(units, *target_id).map_or('?', |u| u.kind.glyph());
                let suffix = if *eliminated { " — eliminated!" } else { "" };
                format!("{ag} hits {tg} for {hits}{suffix}")
            }
        }
    }
}

fn find_unit(units: &[Unit], id: u8) -> Option<&Unit> {
    units.iter().find(|u| u.id == id)
}

// ── Replay step ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReplayStep {
    pub event: GameEvent,
    /// Snapshot of all unit positions/strengths after this event.
    pub units: Vec<Unit>,
    pub blue_hand: Vec<Card>,
    pub red_hand: Vec<Card>,
}

// ── Scenario ──────────────────────────────────────────────────────────────────

fn initial_units() -> Vec<Unit> {
    vec![
        // Blue units — left flank
        Unit::new(0, Faction::Blue, UnitKind::Trooper, (1, 1), 4),
        Unit::new(1, Faction::Blue, UnitKind::Trooper, (1, 2), 4),
        Unit::new(2, Faction::Blue, UnitKind::Trooper, (1, 4), 4),
        Unit::new(3, Faction::Blue, UnitKind::Trooper, (1, 5), 3),
        Unit::new(4, Faction::Blue, UnitKind::Trooper, (1, 6), 3),
        Unit::new(5, Faction::Blue, UnitKind::Trooper, (3, 5), 3),
        Unit::new(6, Faction::Blue, UnitKind::Trooper, (3, 6), 3),
        Unit::new(7, Faction::Blue, UnitKind::Driver, (1, 0), 2),
        // Red units — right flank
        Unit::new(8, Faction::Red, UnitKind::Trooper, (5, 2), 4),
        Unit::new(9, Faction::Red, UnitKind::Trooper, (5, 4), 2),
        Unit::new(10, Faction::Red, UnitKind::Trooper, (6, 5), 1),
        Unit::new(11, Faction::Red, UnitKind::Trooper, (6, 6), 3),
        Unit::new(12, Faction::Red, UnitKind::Trooper, (7, 5), 3),
        Unit::new(13, Faction::Red, UnitKind::Trooper, (7, 6), 3),
        Unit::new(14, Faction::Red, UnitKind::Trooper, (8, 5), 3),
        Unit::new(15, Faction::Red, UnitKind::Trooper, (8, 6), 3),
    ]
}

/// Build a short fixed scenario: turn start → move → attack.
pub fn build_scenario() -> (Vec<Unit>, Vec<ReplayStep>) {
    let units = initial_units();

    let blue_hand = vec![
        Card::new("Advance"),
        Card::new("Assault"),
        Card::new("Scout"),
        Card::new("Regroup"),
    ];
    let red_hand = vec![
        Card::new("Advance"),
        Card::new("Assault"),
        Card::special("Blitz"),
    ];

    // Step 0: Blue opens with Advance.
    let step0 = ReplayStep {
        event: GameEvent::TurnStart {
            turn: 1,
            faction: Faction::Blue,
            card: Card::new("Advance"),
        },
        units: units.clone(),
        blue_hand: blue_hand.clone(),
        red_hand: red_hand.clone(),
    };

    // Step 1: Blue unit 0 moves from (1,1) to (3,1).
    let mut units1 = units.clone();
    units1[0].pos = (3, 1);
    let step1 = ReplayStep {
        event: GameEvent::Move {
            unit_id: 0,
            from: (1, 1),
            to: (3, 1),
        },
        units: units1.clone(),
        blue_hand: blue_hand.clone(),
        red_hand: red_hand.clone(),
    };

    // Step 2: Blue unit 0 attacks Red unit 8, eliminating it.
    let mut units2 = units1;
    units2[8].strength = 0;
    let step2 = ReplayStep {
        event: GameEvent::Attack {
            attacker_id: 0,
            target_id: 8,
            hits: 1,
            eliminated: true,
        },
        units: units2,
        blue_hand,
        red_hand,
    };

    (units, vec![step0, step1, step2])
}
