//! The mouse pointer, decoded from `State of War.exe` (docs/cursor.md).
//!
//! The game draws its own pointer from `cursor.ps6` (inside `sprites.data`, loaded to the array at `0x5de120` by
//! `0x425ac0`): 19 *types* of 8 animation frames, `frame = 8 * type + phase`. The pointer object keeps the type in
//! `[+0x50]`, the draw offset in `[+0x54]/[+0x58]` and the phase in `[+0x60]`; the update `0x416940..0x417e84` chooses
//! type and offset every frame, `0x4152d0` draws frame at `position - offset - hotspot`, `0x4158a0` steps the phase
//! (one frame per [`ANIM_TICKS`] logic ticks, wrapping at 8).

/// Frames of one pointer type.
pub const FRAMES_PER_TYPE: usize = 8;
/// Logic ticks one animation frame stays (`0x4158a0`: `[+0x5c]` is reloaded with 4).
pub const ANIM_TICKS: u64 = 4;
/// Name of the sprite file in the archive.
pub const SPRITE: &str = "cursor.ps6";

/// Pointer types (`[+0x50]`). The names for 2..=8 follow where the exe chooses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// Default arrow.
    Arrow = 0,
    /// Arrow over blocked ground (`0x417879`: the cell is blocked for the selection's movement layer).
    Blocked = 1,
    /// Over an own unit or building (select).
    Own = 2,
    /// Over ground / own HQ with a selection: nothing special happens but a go-to (small purple arrows).
    Move = 3,
    /// Red bracket: attack (enemy unit or building with a selection; type 5 is the variant with the modifier key).
    Attack = 4,
    /// The Advancer repairs this building (`0x417c4a`).
    Repair = 6,
    /// The Advancer attacks this building to capture it (`0x417cdd`).
    Destroy = 7,
    /// The Advancer upgrades this building (`0x417c8d`).
    Upgrade = 8,
    /// Rally point placement (`0x416fc6`, `0x417678`): shown over free ground while the mode is on.
    Rally = 9,
    /// Air-strike mode (`0x416f80`, `0x41727b`): shown over the map while a strike is armed and the air layer lets the target cell be hit.
    AirStrike = 10,
    /// Scroll arrows, `0x0b..=0x12`.
    ScrollUpLeft = 0x0b,
    ScrollUpRight = 0x0c,
    ScrollDownRight = 0x0d,
    ScrollDownLeft = 0x0e,
    ScrollDown = 0x0f,
    ScrollLeft = 0x10,
    ScrollRight = 0x11,
    ScrollUp = 0x12,
}

impl Pointer {
    /// Frame index in `cursor.ps6` at animation phase `phase` (0..8).
    pub fn frame(self, phase: usize) -> usize {
        self as usize * FRAMES_PER_TYPE + phase % FRAMES_PER_TYPE
    }

    /// The draw offset `([+0x54], [+0x58])`: the frame's hotspot is placed at `position - offset`.
    pub fn offset(self) -> (i32, i32) {
        match self {
            Pointer::Arrow | Pointer::Blocked => (-12, -8),
            Pointer::Rally => (-1, 0),
            Pointer::ScrollUpLeft => (-8, -6),
            Pointer::ScrollUpRight => (8, -6),
            Pointer::ScrollDownRight => (8, 7),
            Pointer::ScrollDownLeft => (-10, 7),
            Pointer::ScrollDown => (0, 8),
            Pointer::ScrollLeft => (-11, 0),
            Pointer::ScrollRight => (9, 0),
            Pointer::ScrollUp => (1, -8),
            _ => (0, 0),
        }
    }
}

/// Animation phase after `tick` logic ticks.
pub fn phase_at(tick: u64) -> usize {
    (tick / ANIM_TICKS % FRAMES_PER_TYPE as u64) as usize
}

/// Scroll arrow for a pointer on a screen edge (`0x416a50..0x416b79`): `dx`/`dy` are -1 (left/top edge), 0 or 1.
pub fn scroll_pointer(dx: i32, dy: i32) -> Option<Pointer> {
    Some(match (dx, dy) {
        (-1, -1) => Pointer::ScrollUpLeft,
        (-1, 1) => Pointer::ScrollDownLeft,
        (-1, 0) => Pointer::ScrollLeft,
        (1, -1) => Pointer::ScrollUpRight,
        (1, 1) => Pointer::ScrollDownRight,
        (1, 0) => Pointer::ScrollRight,
        (0, -1) => Pointer::ScrollUp,
        (0, 1) => Pointer::ScrollDown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_eight_per_type() {
        assert_eq!(Pointer::Arrow.frame(0), 0);
        assert_eq!(Pointer::Repair.frame(3), 51);
        assert_eq!(Pointer::Upgrade.frame(7), 71);
        assert_eq!(Pointer::Rally.frame(2), 74);
        assert_eq!(Pointer::AirStrike.frame(3), 83);
        assert_eq!(Pointer::AirStrike.offset(), (0, 0));
        assert_eq!(Pointer::ScrollUp.frame(8), 18 * 8);
    }

    #[test]
    fn the_phase_advances_every_four_ticks() {
        assert_eq!((0..9).map(|t| phase_at(t * 4)).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6, 7, 0]);
        assert_eq!(phase_at(3), 0);
    }

    #[test]
    fn edge_arrows() {
        assert_eq!(scroll_pointer(0, 0), None);
        assert_eq!(scroll_pointer(-1, 0), Some(Pointer::ScrollLeft));
        assert_eq!(scroll_pointer(1, 1), Some(Pointer::ScrollDownRight));
    }
}
