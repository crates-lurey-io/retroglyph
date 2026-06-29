#![allow(dead_code, unreachable_pub)]
//! Battle simulation: board state, unit types, turn engine, and replay log.
//!
//! No rendering. Pure game logic that both the software and crossterm
//! render paths consume identically.

// ── Factions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Rebel,
    Empire,
}

impl Faction {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rebel => "REBEL",
            Self::Empire => "EMPIRE",
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::Rebel => Self::Empire,
            Self::Empire => Self::Rebel,
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

    pub const fn attack_range(self) -> u32 {
        match self {
            Self::Trooper => 2,
            Self::Driver => 3,
        }
    }

    pub const fn move_range(self) -> u32 {
        match self {
            Self::Trooper => 2,
            Self::Driver => 3,
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
    /// Remaining figures in the unit (strength).
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
    TurnStart {
        turn: u32,
        faction: Faction,
        card: Card,
    },
}

impl GameEvent {
    /// Short human-readable description for the event log sidebar.
    pub fn description(&self, units: &[Unit]) -> String {
        match self {
            Self::Move { unit_id, from, to } => {
                let u = find_unit(units, *unit_id);
                format!(
                    "{} [{},{}] → [{},{}]",
                    u.map_or('?', |u| u.kind.glyph()),
                    from.0,
                    from.1,
                    to.0,
                    to.1,
                )
            }
            Self::Attack {
                attacker_id,
                target_id,
                hits,
                eliminated,
            } => {
                let attacker = find_unit(units, *attacker_id);
                let target = find_unit(units, *target_id);
                let elim = if *eliminated { " (eliminated)" } else { "" };
                format!(
                    "{} hits {} for {} hit(s){}",
                    attacker.map_or('?', |u| u.kind.glyph()),
                    target.map_or('?', |u| u.kind.glyph()),
                    hits,
                    elim,
                )
            }
            Self::TurnStart {
                turn,
                faction,
                card,
            } => {
                format!("Turn {}: {} plays {}", turn, faction.name(), card.name)
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
    /// Cards in each player's hand at this moment.
    pub rebel_hand: Vec<Card>,
    pub empire_hand: Vec<Card>,
}

// ── Pre-baked replay ──────────────────────────────────────────────────────────

// ── Scenario builder helper ───────────────────────────────────────────────────

/// Accumulates replay steps, cloning the current hands into each snapshot.
struct ScenarioBuilder {
    steps: Vec<ReplayStep>,
    rebel_hand: Vec<Card>,
    empire_hand: Vec<Card>,
}

impl ScenarioBuilder {
    const fn new(rebel_hand: Vec<Card>, empire_hand: Vec<Card>) -> Self {
        Self {
            steps: Vec::new(),
            rebel_hand,
            empire_hand,
        }
    }

    fn push(&mut self, event: GameEvent, units: &[Unit]) {
        self.steps.push(ReplayStep {
            event,
            units: units.to_vec(),
            rebel_hand: self.rebel_hand.clone(),
            empire_hand: self.empire_hand.clone(),
        });
    }
}

// ── Initial board ─────────────────────────────────────────────────────────────

fn initial_units() -> Vec<Unit> {
    vec![
        Unit::new(0, Faction::Rebel, UnitKind::Trooper, (1, 1), 4),
        Unit::new(1, Faction::Rebel, UnitKind::Trooper, (1, 2), 4),
        Unit::new(2, Faction::Rebel, UnitKind::Trooper, (1, 4), 4),
        Unit::new(3, Faction::Rebel, UnitKind::Trooper, (1, 5), 3),
        Unit::new(4, Faction::Rebel, UnitKind::Trooper, (1, 6), 3),
        Unit::new(5, Faction::Rebel, UnitKind::Trooper, (3, 5), 3),
        Unit::new(6, Faction::Rebel, UnitKind::Trooper, (3, 6), 3),
        Unit::new(7, Faction::Rebel, UnitKind::Driver, (1, 0), 2),
        Unit::new(8, Faction::Empire, UnitKind::Trooper, (5, 2), 4),
        Unit::new(9, Faction::Empire, UnitKind::Trooper, (5, 4), 2),
        Unit::new(10, Faction::Empire, UnitKind::Trooper, (6, 5), 1),
        Unit::new(11, Faction::Empire, UnitKind::Trooper, (6, 6), 3),
        Unit::new(12, Faction::Empire, UnitKind::Trooper, (7, 5), 3),
        Unit::new(13, Faction::Empire, UnitKind::Trooper, (7, 6), 3),
        Unit::new(14, Faction::Empire, UnitKind::Trooper, (8, 5), 3),
        Unit::new(15, Faction::Empire, UnitKind::Trooper, (8, 6), 3),
    ]
}

// ── Turn scripts ──────────────────────────────────────────────────────────────
//
// Each function mutates `units` and pushes events into `b`.
// Empire units are at indices [8..] in the vec (id == index + 0 for rebels,
// id == index + 8 for empire from index 0).

fn play_turns_1_2(b: &mut ScenarioBuilder, units: &mut [Unit]) {
    // Turn 1 – Empire Recon Probe: unit 10 probes forward.
    b.push(
        GameEvent::TurnStart {
            turn: 1,
            faction: Faction::Empire,
            card: Card::new("Recon Probe"),
        },
        units,
    );
    units[2].pos = (5, 3); // unit id 10, empire index 2
    b.push(
        GameEvent::Move {
            unit_id: 10,
            from: (6, 5),
            to: (5, 3),
        },
        units,
    );

    // Turn 2 – Rebel Sector Assault: unit 0 advances, unit 8 eliminates unit 10.
    b.push(
        GameEvent::TurnStart {
            turn: 2,
            faction: Faction::Rebel,
            card: Card::new("Sector Assault"),
        },
        units,
    );
    units[0].pos = (3, 2);
    b.push(
        GameEvent::Move {
            unit_id: 0,
            from: (1, 1),
            to: (3, 2),
        },
        units,
    );
    units[2].strength = 0; // unit 10 eliminated
    b.push(
        GameEvent::Attack {
            attacker_id: 8,
            target_id: 10,
            hits: 1,
            eliminated: true,
        },
        units,
    );
}

fn play_turns_3_4(b: &mut ScenarioBuilder, units: &mut [Unit]) {
    // Turn 3 – Empire Forward Command: unit 9 advances.
    b.push(
        GameEvent::TurnStart {
            turn: 3,
            faction: Faction::Empire,
            card: Card::new("Forward Command"),
        },
        units,
    );
    units[1].pos = (4, 3); // unit id 9, empire index 1
    b.push(
        GameEvent::Move {
            unit_id: 9,
            from: (5, 4),
            to: (4, 3),
        },
        units,
    );

    // Turn 4 – Rebel General Advance: unit 1 advances.
    b.push(
        GameEvent::TurnStart {
            turn: 4,
            faction: Faction::Rebel,
            card: Card::new("General Advance"),
        },
        units,
    );
    units[1].pos = (3, 3); // rebel unit id 1
    b.push(
        GameEvent::Move {
            unit_id: 1,
            from: (1, 2),
            to: (3, 3),
        },
        units,
    );
}

fn play_turns_5_6(b: &mut ScenarioBuilder, units: &mut [Unit]) {
    // Turn 5 – Empire Imperial Ambush: unit 11 damages unit 8.
    b.push(
        GameEvent::TurnStart {
            turn: 5,
            faction: Faction::Empire,
            card: Card::special("Imperial Ambush"),
        },
        units,
    );
    units[0].strength = units[0].strength.saturating_sub(2); // unit 8, empire index 0
    b.push(
        GameEvent::Attack {
            attacker_id: 11,
            target_id: 8,
            hits: 2,
            eliminated: false,
        },
        units,
    );

    // Turn 6 – Empire Recon Probe (final step in the screenshot).
    b.push(
        GameEvent::TurnStart {
            turn: 6,
            faction: Faction::Empire,
            card: Card::new("Recon Probe"),
        },
        units,
    );
    b.push(
        GameEvent::Attack {
            attacker_id: 8,
            target_id: 9,
            hits: 1,
            eliminated: true,
        },
        units,
    );
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Build a fixed scenario inspired by Battle for Hoth.
///
/// Hex layout uses odd-r offset. The board is ~9 cols × 7 rows.
/// Rebels occupy the left flank, Empire the right.
pub fn build_scenario() -> (Vec<Unit>, Vec<ReplayStep>) {
    let units_start = initial_units();
    let mut units = units_start.clone();

    // Only the empire units (indices 8..) are mutated by the turn scripts.
    // We pass a slice starting at index 8 so empire index 0 == unit id 8.
    let (rebel_units, empire_units) = units.split_at_mut(8);
    let _ = rebel_units; // rebel positions mutated via full-vec index below
    let _ = empire_units;

    let mut b = ScenarioBuilder::new(
        vec![
            Card::new("Speeder Strike"),
            Card::new("Sector Assault"),
            Card::new("General Advance"),
            Card::new("Recon Probe"),
        ],
        vec![
            Card::new("Recon Probe"),
            Card::new("Forward Command"),
            Card::special("Imperial Ambush"),
        ],
    );

    play_turns_1_2(&mut b, &mut units);
    play_turns_3_4(&mut b, &mut units);
    play_turns_5_6(&mut b, &mut units);

    (units_start, b.steps)
}
