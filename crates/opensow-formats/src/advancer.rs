//! Advancer building actions, decoded from `State of War.exe` (docs/advancer-actions.md).
//!
//! The Advancer (unit kind 40) takes one of three orders on a building or turret: *destroy* (hit points go down, the
//! building changes hands when they reach zero), *repair* and *upgrade* (unlock the next production slot). An order
//! stores a mode, the target cell centre and the target id in the Advancer's top (`[top+0xfc]` 1/2/3, `+0xf4/+0xf8`,
//! `+0x100`; setters `0x46e5c0/0x46e620/0x46e680`) and flies there. Once the Advancer sits on that cell and the target is
//! still valid, every beam shot (`0x46f440`) creates an action event (code 0x15/0x13/0x14) executed by `0x40f590`.

use crate::kinds;
use crate::level::Level;

/// The three Advancer orders; the numbers are the action-event codes (`[event+0x2658]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    /// Repair a damaged building or turret (event code `0x13`).
    Repair = 0x13,
    /// Upgrade a building's production line (event code `0x14`).
    Upgrade = 0x14,
    /// Destroy: shoot a building down to capture it, or a turret or headquarters to remove it (event code `0x15`).
    Destroy = 0x15,
}

/// Maximum hit points of a building kind (constructor `0x40a330`, jump table `0x40afb8`): turrets 25-29, every
/// building 100-108 (headquarters included) 13536, everything else (kinds 109-113 included: their table index is past
/// the `cmp edi, 0x53` bound at `0x40a988`, so `ja 0x40a9f4`) 50.
pub fn building_max_hp(kind: i32) -> i32 {
    match kind {
        25 => 900,
        26 => 1250,
        27 => 1540,
        28 => 2100,
        29 => 2600,
        100..=108 => 13536,
        _ => 50,
    }
}

/// A new building starts with 1 hit point and a pending heal of its maximum (`[bld+0x6c]`), paid out by the building
/// update (`0x40de60`) at up to this many points per tick.
pub const HEAL_PER_TICK: i32 = 150;

/// Whether an object kind is one of the five turrets (25..=29).
pub fn is_turret(kind: i32) -> bool {
    kinds::TURRETS.contains(&kind)
}

/// Ticks between two beam shots of one Advancer (60 ticks/s, so 6 shots per second).
///
/// The Advancer's top holds a "cooldown" member at `[top+0x104]` (class built by `0x47c630`, called from the top's
/// constructor `0x470048`, which also stores `10` in `[top+0x124]`). Its reload field `[+0x20]` is `10` and its spread
/// `[+0x24]` is `0`; nothing in the 0x46xxxx-0x47xxxx Advancer code writes either again, so the period does not depend on
/// the level, the action (repair / upgrade / destroy), the owner or any unit state.
pub const BEAM_PERIOD: u32 = 10;

/// Cooldown member of the Advancer's top (`[top+0x104]`, object fields in brackets): the shot gate of `0x46f210`.
///
/// Per tick `0x46f210` calls [`BeamTimer::tick`] (`0x47c6b0`) *first* and then, only when the state is idle (`0x46f29d`
/// compares `[top+0x144]` with 0), the unit hovers on the target pixel and the target is still valid, fires: [`fire`]
/// (`0x47c5f0`) followed by the state store 6 (`0x46f454`). In state 6 the counter runs down by one per tick *whether or not
/// the unit still hovers* and the state returns to 0 when it reaches 0, in the same tick - so a shot every
/// [`BEAM_PERIOD`] ticks, exactly, as long as the conditions hold.
///
/// [`fire`]: BeamTimer::fire
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BeamTimer {
    /// `[+0x04]` enabled flag (set by `0x47c630`).
    pub enabled: bool,
    /// `[+0x40]` state: 0 idle, 2 just fired (`0x47c5f0`), 6 reloading (stored by `0x46f454`), 5 and 1 are other users' states.
    pub state: i32,
    /// `[+0x1c]` reload counter.
    pub counter: i32,
    /// `[+0x20]` reload value, 10.
    pub reload: i32,
}

impl Default for BeamTimer {
    /// The state after `0x47c630`.
    fn default() -> Self {
        BeamTimer { enabled: true, state: 0, counter: 0, reload: BEAM_PERIOD as i32 }
    }
}

impl BeamTimer {
    /// `0x47c6b0`, once per tick. A counter above `2 * reload` is first clamped to `reload`; state 5 counts down but
    /// stops at 0 (no state change); state 6 counts down and returns to state 0 when the counter reaches 0 (the counter
    /// is zeroed); every other state does nothing.
    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }
        if self.counter > 2 * self.reload {
            self.counter = self.reload;
        }
        match self.state {
            5 => self.counter = (self.counter - 1).max(0),
            6 => {
                self.counter -= 1;
                if self.counter <= 0 {
                    self.counter = 0;
                    self.state = 0;
                }
            }
            _ => {}
        }
    }

    /// The gate `[top+0x144] == 0` of `0x46f29d` / `0x46f36a`.
    pub fn ready(&self) -> bool {
        self.state == 0
    }

    /// `0x47c5f0` (+ the state store 6 of `0x46f454`): `counter = reload - spread/2 + random(spread)` with spread 0,
    /// where `random(0)` is 0 without touching the generator (`0x437b10`), so the counter is just `reload`.
    pub fn fire(&mut self) {
        if self.enabled {
            self.counter = self.reload;
        }
        self.state = 6;
    }
}

/// The per-player Advancer strength `[0x5baaac + player*4]` (`0x41d7a3..0x41d7cd`): level header words 31 (player 0) and 32
/// (player 1), usually 10 and ranging 6..15 per mission. The loader reads the level id (header word 1) just before the
/// 0x170-byte block (header words 2..): with an id other than 0 - every shipped mission and map - the words are used **as they
/// are**; only id 0 (the empty template, no mission) multiplies them by 4 (`shl eax, 2`). The zeroed default is 10 (`0x41d094`).
pub fn rate(level: &Level, player: usize) -> i32 {
    let word = level.header.get(31 + player).copied().unwrap_or(10);
    if level.header.get(1).copied().unwrap_or(0) == 0 {
        word * 4
    } else {
        word
    }
}

/// Hit points a destroy (`-`) or repair (`+`) event moves: `2 * rate` (`0x40ffe9`, `0x4102df`).
pub fn hp_per_event(rate: i32) -> i32 {
    2 * rate
}

/// Upgrade points one upgrade event adds (`0x40e3d0`: `[bld+0x148] += rate`); the next slot unlocks when they reach that
/// slot's target (2000 ground, 4000 air; `production::slot_target`).
pub fn upgrade_per_event(rate: i32) -> i32 {
    rate
}

/// The pointer the exe shows while an Advancer is selected and the mouse is over a building (`0x417b8c..0x417d0a`), which
/// is also the order a click gives (`0x46b8b0` destroy, `0x46b9f0` repair, `0x46bb30` upgrade):
/// - a building that is not the player's (enemy *or* neutral): destroy (red arrow), a click starts the capture;
/// - the player's headquarters (kind 100): nothing (the plain move pointer);
/// - turrets (kinds 25-29) and, in repair mode (`[pointer+0x1f0]`, toggled by a right click on an own building),
///   every own building: repair if damaged, else upgrade if a slot is still locked, else nothing;
/// - any other own building otherwise: upgrade if a slot is locked, else repair if damaged, else nothing.
pub fn pointer_over_building(own: bool, kind: i32, damaged: bool, slot_locked: bool, repair_mode: bool) -> crate::cursor::Pointer {
    use crate::cursor::Pointer;
    if !own {
        return Pointer::Destroy;
    }
    if kind == kinds::HEADQUARTERS {
        return Pointer::Move;
    }
    let repair_first = is_turret(kind) || repair_mode;
    match (repair_first, damaged, slot_locked) {
        (true, true, _) | (false, true, false) => Pointer::Repair,
        (_, _, true) => Pointer::Upgrade,
        _ => Pointer::Move,
    }
}

/// The action a click gives for that pointer.
pub fn action_for_pointer(p: crate::cursor::Pointer) -> Option<Action> {
    use crate::cursor::Pointer;
    match p {
        Pointer::Destroy => Some(Action::Destroy),
        Pointer::Repair => Some(Action::Repair),
        Pointer::Upgrade => Some(Action::Upgrade),
        _ => None,
    }
}

/// The shot an Advancer fires is a bullet of type = the action code (19 repair, 20 upgrade, 21 destroy; constructor cases
/// `0x412408`, `0x4123aa`, `0x412333`): speed 60 (one path step per tick) from the hovering Advancer straight down to the
/// building's cell, and the action happens when it lands (the impact function `0x40f590`).
pub const BEAM_SPEED: u32 = 60;

/// Sparks one beam bullet carries (`[bullet+0x268c]`): 5 blue (repair), 2 yellow (upgrade), 12 red in a column (destroy).
pub fn fragment_count(a: Action) -> usize {
    match a {
        Action::Repair => 5,
        Action::Upgrade => 2,
        Action::Destroy => 12,
    }
}

/// Offset and sprite frame of fragment `i` (`rand(n)` is the visual generator `0x437cd0`): repair `x = rand(30) - 15`,
/// `y = rand(20) - 10`, frame `rand(3)` of `bodky_b`; upgrade `x`, `y = rand(20) - 10`, frame 0; destroy `(0, -i)`, frame
/// `rand(2)` of `bodky_r`.
pub fn fragment(a: Action, i: usize, rand: &mut impl FnMut(u32) -> u32) -> (i32, i32, u16) {
    match a {
        Action::Repair => {
            let x = rand(30) as i32 - 15;
            let y = rand(20) as i32 - 10;
            (x, y, rand(3) as u16)
        }
        Action::Upgrade => {
            let x = rand(20) as i32 - 10;
            let y = rand(20) as i32 - 10;
            (x, y, 0)
        }
        Action::Destroy => (0, -(i as i32), rand(2) as u16),
    }
}

/// `bodky_b` / `bodky_y` / `bodky_r` effect arrays (`extras\bodky_*`).
pub const BODKY_B: u32 = 0x5e6bd0;
/// The `extras\bodky_y` effect array.
pub const BODKY_Y: u32 = 0x5e6c20;
/// The `extras\bodky_r` effect array.
pub const BODKY_R: u32 = 0x5e6b80;

/// Who owns a destroyed building afterwards (`0x43ab40` with the owner argument 0): the player with the largest
/// damage tally `[bld+0x7c + player*4]` among those who are not the current owner. `None` means nobody dealt damage
/// (single player: the exe then gives it to player 1, `0x43ac2f..0x43ac38`; with remote players a random one). Turrets and headquarters are removed instead of captured.
pub fn capture_winner(tally: [i32; 3], owner: usize) -> Option<usize> {
    let mut best = 0;
    let mut winner = None;
    for p in 0..3 {
        if tally[p] > best && p != owner {
            best = tally[p];
            winner = Some(p);
        }
    }
    winner
}

/// Whether destroying a building of this kind removes it (turrets and the HQ) instead of handing it over.
pub fn removed_when_destroyed(kind: i32) -> bool {
    is_turret(kind) || kind == kinds::HEADQUARTERS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beam_timer_fires_every_ten_ticks() {
        let mut t = BeamTimer::default();
        let mut shots = Vec::new();
        for tick in 0..60 {
            t.tick();
            if t.ready() {
                t.fire();
                shots.push(tick);
            }
        }
        assert_eq!(shots, vec![0, 10, 20, 30, 40, 50]);
    }

    #[test]
    fn beam_timer_states() {
        let mut t = BeamTimer::default();
        t.fire();
        assert_eq!((t.state, t.counter), (6, 10));
        for _ in 0..9 {
            t.tick();
            assert!(!t.ready());
        }
        t.tick();
        assert_eq!((t.state, t.counter), (0, 0));
        // oversized counter is clamped to the reload value first
        t.state = 6;
        t.counter = 99;
        t.tick();
        assert_eq!(t.counter, 9);
        // state 5 stops at 0 and keeps the state, other states do nothing
        t.state = 5;
        t.counter = 1;
        t.tick();
        t.tick();
        assert_eq!((t.state, t.counter), (5, 0));
        t.state = 2;
        t.counter = 7;
        t.tick();
        assert_eq!((t.state, t.counter), (2, 7));
        let mut off = BeamTimer { enabled: false, state: 6, counter: 3, reload: 10 };
        off.tick();
        assert_eq!(off.counter, 3);
    }

    #[test]
    fn hit_points_by_kind() {
        assert_eq!(building_max_hp(27), 1540);
        assert_eq!(building_max_hp(100), 13536);
        assert_eq!(building_max_hp(103), 13536);
        assert_eq!(building_max_hp(109), 50);
        assert_eq!(building_max_hp(113), 50);
    }

    #[test]
    fn the_biggest_foreign_tally_takes_the_building() {
        assert_eq!(capture_winner([500, 0, 0], 2), Some(0));
        assert_eq!(capture_winner([500, 900, 100], 1), Some(0));
        assert_eq!(capture_winner([0, 0, 0], 2), None);
        assert_eq!(capture_winner([0, 300, 0], 1), None);
    }

    #[test]
    fn the_pointer_over_a_building_follows_the_exes_cases() {
        use crate::cursor::Pointer::*;
        // not own (enemy or neutral): destroy, whatever the state
        assert_eq!(pointer_over_building(false, 103, false, false, false), Destroy);
        assert_eq!(pointer_over_building(false, 100, true, true, true), Destroy);
        // own HQ: nothing
        assert_eq!(pointer_over_building(true, 100, true, true, false), Move);
        // own building: upgrade first, repair when no slot is locked, nothing when healthy and complete
        assert_eq!(pointer_over_building(true, 103, true, true, false), Upgrade);
        assert_eq!(pointer_over_building(true, 103, true, false, false), Repair);
        assert_eq!(pointer_over_building(true, 103, false, true, false), Upgrade);
        assert_eq!(pointer_over_building(true, 103, false, false, false), Move);
        // repair mode and turrets: repair first
        assert_eq!(pointer_over_building(true, 103, true, true, true), Repair);
        assert_eq!(pointer_over_building(true, 103, false, true, true), Upgrade);
        assert_eq!(pointer_over_building(true, 27, true, true, false), Repair);
        assert_eq!(pointer_over_building(true, 27, false, false, false), Move);
        assert_eq!(action_for_pointer(Destroy), Some(Action::Destroy));
        assert_eq!(action_for_pointer(Move), None);
    }

    #[test]
    fn event_sizes_follow_the_rate() {
        assert_eq!(hp_per_event(40), 80);
        assert_eq!(upgrade_per_event(40), 40);
    }
}
