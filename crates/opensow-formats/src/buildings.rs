//! Building footprints, decoded from `State of War.exe` (footprint function `0x409760`, jump table `0x409910`, layout masks
//! `0x486b20`, constructor `0x40a330`).
//!
//! Every building and turret has a width/height in cells and a 25-entry layout mask. The mask is read with a row stride of
//! the width: entry `i` is cell `(i % w, i / w)` and `1` means the cell is blocked by the building (the top row is always
//! open: the sprite leans back). Turret placement (`0x408be0`) needs every footprint cell free of units, buildings and
//! blocked terrain, and the footprint expanded by two cells to touch a building of the same player.

/// `(width, height, layout)` of an object kind: turrets 25-29 and buildings 100-108.
pub fn footprint(kind: i32) -> Option<(i32, i32, usize)> {
    Some(match kind {
        25 => (1, 1, 8),
        26 | 27 => (1, 1, 9),
        28 | 29 => (1, 2, 10),
        100 => (4, 4, 0),
        101 => (3, 3, 1),
        102 => (5, 4, 2),
        103 => (5, 4, 3),
        104 => (3, 4, 4),
        105 => (3, 3, 5),
        106 => (3, 3, 6),
        107 => (3, 4, 7),
        108 => (4, 3, 11),
        _ => return None,
    })
}

/// Pixel offset `([b+0x1c], [b+0x20])` from the building's cell corner (`cell * 32`) to where its sprite hotspot is drawn
/// (draw `0x4092f0`: `x = [b+4]*32 + [b+0x1c]`, `y = [b+8]*32 + [b+0x20]`; set per kind by the constructor `0x40a330`, cases
/// `0x40a540..0x40a95b`). Horizontally it is the middle of the footprint, so art and footprint line up.
pub fn art_offset(kind: i32) -> (i32, i32) {
    match kind {
        25 => (16, 22),
        26 | 27 => (16, 20),
        28 | 29 => (16, 53),
        100 => (64, 48),
        101 => (48, 48),
        102 => (79, 44),
        103 => (80, 48),
        104 => (48, 51),
        105 => (48, 37),
        106 => (48, 39),
        107 => (48, 57),
        108 => (64, 43),
        _ => (16, 16),
    }
}

/// Frame ranges and animation speed of a building's art, from the constructor `0x40a330` (cases `0x40a540..0x40a95b`).
///
/// Each building keeps a state `[b+0x48]` and eight boundaries `[b+0x28..0x44]`; state `s` shows frames `bounds[s]..bounds[s+1]`.
/// Every kind uses the same states: 0 scaffold, 1 construction, 2 just finished (the three stages of the build-up, picked by
/// hit points in thirds while `[b+0x68]` is set), then 3 working (the idle loop), 4 damaged (hit points at most half), 5 destroyed,
/// 6 neutral. Only state 3 has more than one frame, except for the HQ (kind 100) whose art is entirely still. The loop advances
/// `[b+0x4c]` every `[b+0x58]` ticks (`0x40dfb6`, timer `[b+0x54]`) and wraps at the state's length; states of one frame are
/// painted once into the background instead of every frame (`0x4092f0` skips them, `0x40b140` tells them apart).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingAnim {
    /// State boundaries; state `s` covers `bounds[s]..bounds[s + 1]`.
    pub bounds: [u8; 8],
    /// Ticks (60 per second) per animation frame.
    pub period: u32,
}

/// The state numbers of [`BuildingAnim`].
pub const STATE_WORKING: usize = 3;
pub const STATE_DAMAGED: usize = 4;
pub const STATE_DESTROYED: usize = 5;
pub const STATE_NEUTRAL: usize = 6;

pub fn anim(kind: i32) -> Option<BuildingAnim> {
    let (damaged, period) = match kind {
        100 => return Some(BuildingAnim { bounds: [0, 1, 2, 3, 4, 5, 6, 7], period: 6000 }),
        101..=103 | 107 => (15, 2),
        104 => (25, 6),
        105 => (15, 6),
        106 => (15, 4),
        108 => (11, 4),
        // turrets (cases `0x40a8e0..0x40a95b`): six frames per base, one per state, nothing for the destroyed state
        25..=29 => return Some(BuildingAnim { bounds: [0, 1, 2, 3, 4, 5, 5, 6], period: 6 }),
        _ => return None,
    };
    Some(BuildingAnim { bounds: [0, 1, 2, 3, damaged, damaged + 1, damaged + 2, damaged + 3], period })
}

impl BuildingAnim {
    /// Frames of `state`: `(first, count)`.
    pub fn range(&self, state: usize) -> (usize, usize) {
        (self.bounds[state] as usize, (self.bounds[state + 1] - self.bounds[state]) as usize)
    }

    /// Frame of a working building `tick` ticks after the level started: the exe enters the state on tick 1 and then steps
    /// the loop once per `period` ticks.
    pub fn working_frame(&self, tick: u32) -> usize {
        let (first, n) = self.range(STATE_WORKING);
        first + (tick.saturating_sub(1) / self.period) as usize % n
    }

    /// The state of a finished, not destroyed building (`0x40e099`): damaged at half hit points or less, else working.
    pub fn state(hp: i32, max_hp: i32) -> usize {
        if hp <= max_hp / 2 { STATE_DAMAGED } else { STATE_WORKING }
    }

    /// Frame to draw for the given hit points at `tick`.
    pub fn frame(&self, hp: i32, max_hp: i32, tick: u32) -> usize {
        match Self::state(hp, max_hp) {
            STATE_WORKING => self.working_frame(tick),
            s => self.range(s).0,
        }
    }
}

/// The two parts of a turret: a static base (`builds\\{blue|green}\\sentry0N`, drawn like any building) and the gun top, which the
/// exe creates as a separate unit of kind 109-113 (`0x40e004`) once the turret has healed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurretArt {
    /// Base sprite stem (`[b+0x24]` = 9, 10, 11 minus 8, constructor cases `0x40a8e0..0x40a95b`).
    pub base: &'static str,
    /// Top sprite name inside `units\` (`[top+0x54]` of the weapon constructor, an index of `units::TOPS`).
    pub top: &'static str,
    /// The top's own vertical draw offset (`[top+0x60]`, added to the unit's y by the draw `0x479a80`).
    pub top_dy: i32,
}

/// Art of turret `kind` (25-29). Turret 1 (Guard Cannon) uses `tur_02`, turret 2 (A.A. Defence) the unit top `2AA`, 3 `tur_03`,
/// 4 `tur_01` and 5 `tur_05`: the sprite number is **not** the turret number.
pub fn turret_art(kind: i32) -> Option<TurretArt> {
    let (base, top, top_dy) = match kind {
        25 => ("sentry01", "tur_02", -11),
        26 => ("sentry02", "2AA", -2),
        27 => ("sentry02", "tur_03", -20),
        28 => ("sentry03", "tur_01", -17),
        29 => ("sentry03", "tur_05", -31),
        _ => return None,
    };
    Some(TurretArt { base, top, top_dy })
}

/// Map pixel where the turret's gun top (a unit) stands: the middle of the turret's first cell, or of its second cell for the
/// 1x2 turrets 28 and 29 (`0x40e00b..0x40e04b` creates the unit at `(x, y + 1)` for them).
pub fn turret_top_pos(kind: i32, cell: (i32, i32)) -> (i32, i32) {
    (cell.0 * 32 + 16, (cell.1 + i32::from(matches!(kind, 28 | 29))) * 32 + 16)
}

/// Where an Advancer hovers when it works on a building of this kind, as a cell offset from the building's cell: the exe
/// sends it to `x = cell.x + width/2` (`0x409ab0`) and `y = cell.y + r` (`0x409ac0`), `r` being the first blocked row of the
/// layout in the middle column (`width/2`) - the body of the building, not the open top row - or `height/2` when that column
/// has none; the 1x2 turrets (28, 29) and the walker factory (108) use row 1.
pub fn advancer_offset(kind: i32) -> (i32, i32) {
    let Some((w, h, layout)) = footprint(kind) else { return (0, 0) };
    let col = w / 2;
    if matches!(kind, 28 | 29 | 108) {
        return (col, 1);
    }
    let row = (0..h).find(|&r| LAYOUTS[layout][(r * w + col) as usize] == 1).unwrap_or(h / 2);
    (col, row)
}

/// The layout masks, in the exe's order: HQ, light, medium, heavy, air base, gold, lab, wind, turret 1, turrets 2-3, turrets 4-5, walker factory.
pub const LAYOUTS: [[i32; 25]; 12] = [
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];

/// The cells (offsets from the origin cell) a building of `kind` blocks.
pub fn blocked_cells(kind: i32) -> Vec<(i32, i32)> {
    let Some((w, h, layout)) = footprint(kind) else { return Vec::new() };
    (0..(w * h) as usize).filter(|&i| LAYOUTS[layout][i] == 1).map(|i| (i as i32 % w, i as i32 / w)).collect()
}

/// Whether a turret of `kind` may stand on `cell` (`0x408be0`): all footprint cells are free (`free`) and some cell within two
/// cells of the footprint belongs to a building of the player (`own_near`).
pub fn can_place(kind: i32, cell: (i32, i32), free: impl Fn(i32, i32) -> bool, own_near: impl Fn(i32, i32) -> bool) -> bool {
    let Some((w, h, _)) = footprint(kind) else { return false };
    let near = (cell.1 - 2..cell.1 + h + 2).any(|y| (cell.0 - 2..cell.0 + w + 2).any(|x| own_near(x, y)));
    near && (0..h).all(|dy| (0..w).all(|dx| free(cell.0 + dx, cell.1 + dy)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprints() {
        assert_eq!(footprint(102), Some((5, 4, 2)));
        assert_eq!(blocked_cells(26), vec![(0, 0)]);
        assert_eq!(blocked_cells(101).len(), 5);
        assert_eq!(blocked_cells(100).len(), 12);
        assert!(blocked_cells(100).iter().all(|&(_, y)| y >= 1));
    }

    #[test]
    fn animation_frames() {
        let air = anim(104).unwrap();
        assert_eq!(air.range(STATE_WORKING), (3, 22));
        assert_eq!(air.range(STATE_NEUTRAL), (27, 1));
        assert_eq!(air.working_frame(0), 3);
        assert_eq!(air.working_frame(6), 3);
        assert_eq!(air.working_frame(7), 4);
        assert_eq!(air.working_frame(1 + 6 * 22), 3); // wraps
        assert_eq!(air.frame(10, 100, 50), 25); // damaged
        assert_eq!(anim(100).unwrap().range(STATE_WORKING).1, 1);
        assert!(anim(99).is_none());
        // turrets: one frame per state, damaged below half
        let t = anim(26).unwrap();
        assert_eq!(t.range(STATE_WORKING), (3, 1));
        assert_eq!(t.frame(10, 100, 7), 4);
    }

    #[test]
    fn turret_parts() {
        // the sprite number is not the turret number: Guard Cannon (25) = tur_02, A.A. Defence (26) = the unit top 2AA
        assert_eq!(turret_art(25).map(|t| (t.base, t.top)), Some(("sentry01", "tur_02")));
        assert_eq!(turret_art(26).map(|t| (t.base, t.top)), Some(("sentry02", "2AA")));
        assert_eq!(turret_art(29).map(|t| (t.base, t.top, t.top_dy)), Some(("sentry03", "tur_05", -31)));
        assert!(turret_art(100).is_none());
        assert_eq!(turret_top_pos(25, (2, 3)), (80, 112));
        assert_eq!(turret_top_pos(29, (2, 3)), (80, 144));
    }

    #[test]
    fn the_advancer_hovers_over_the_body() {
        assert_eq!(advancer_offset(100), (2, 1)); // HQ: top row open, body from row 1
        assert_eq!(advancer_offset(101), (1, 1));
        assert_eq!(advancer_offset(28), (0, 1));
        assert_eq!(advancer_offset(108), (2, 1));
        assert_eq!(advancer_offset(25), (0, 0));
    }

    #[test]
    fn turrets_need_a_nearby_own_building() {
        let own = |x: i32, y: i32| (5..8).contains(&x) && (5..8).contains(&y);
        assert!(can_place(25, (9, 6), |_, _| true, own));
        assert!(!can_place(25, (12, 6), |_, _| true, own));
        assert!(!can_place(28, (9, 6), |x, y| (x, y) != (9, 7), own));
    }
}
