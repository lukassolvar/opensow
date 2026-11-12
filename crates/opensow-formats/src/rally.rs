//! Factory rally point (decoded from the exe, docs/rally.md).
//!
//! Every building object has a rally point `[bld+0x2c0]` / `[bld+0x2c4]` (pixels, -1 = none, set by the constructor `0x40a40d`).
//! The player's selected factory shows a "RALLY POINT" button above its top production icon; clicking it (or pressing R) makes the
//! next left click on the map set the point to the clicked cell's centre. Units leaving the factory walk on to it.

/// Buildings that can have a rally point and show the button: light, medium, heavy and walker factory (`0x455da4`, `0x416d30`).
pub const FACTORY_KINDS: [i32; 4] = [101, 102, 103, 108];

/// DirectInput scan code of the hotkey (R): the pointer's key array is `[ptr + 0x64 + scan]`, `cmp [esi+0x77], 1` at `0x416769`.
pub const HOTKEY_SCAN: u8 = 0x13;

/// HUD widget id of the button (`0x454b45`); the click case is `0x416cf9`.
pub const WIDGET_ID: u32 = 0x7b;

/// `ikonyrpoints` frames of the button: idle `[0x5e719c]` and hovered `[0x5e71a4]` (48x21, hot (24, 10)).
pub const BUTTON_FRAME: usize = 1;
/// `ikonyrpoints` frame of the button while the pointer is over it.
pub const BUTTON_HOVER_FRAME: usize = 3;

/// The button's hot spot sits at the top icon's position + this (`0x455dcf`: x + 4, y - 0x29).
pub const BUTTON_OFFSET: (i32, i32) = (4, -0x29);

/// Hit rectangle relative to the hot spot, **exclusive** on every side (`0x456490(-0x19, -0xb, 0x19, 0xc)`, test `0x456350`).
pub const BUTTON_RECT: (i32, i32, i32, i32) = (-0x19, -0xb, 0x19, 0xc);

/// The map marker (`extras\rallypoint`, array `0x5eab00`, 16 frames) is drawn with the additive blit at the point while the
/// building is selected; its frame `[bld+0x60]` advances every 3 ticks (`0x40de60`: counter `[bld+0x64]` reloaded with 3).
pub const MARKER_FRAMES: u64 = 16;
/// Ticks each frame of the map marker is shown.
pub const MARKER_PERIOD: u64 = 3;

/// The marker frame of a building at game tick `tick` (all buildings share the phase: every building's counter starts at 0 when
/// the mission starts).
pub fn marker_frame(tick: u64) -> usize {
    (tick / MARKER_PERIOD % MARKER_FRAMES) as usize
}

/// The animation played once at the point when it is set (`0x4175c4`): `extras\cross_rp` (array `0x5e6028`) frames 0..=20,
/// 3 ticks each, additive; sound 68 (`airforce-dest`), not positional.
pub const CLICK_ARRAY: u32 = 0x5e6028;
/// Last frame of the animation played when the point is set.
pub const CLICK_LAST_FRAME: u16 = 20;
/// Ticks each frame of that animation is shown.
pub const CLICK_PERIOD: u8 = 3;
/// Sound played when the point is set (`airforce-dest`).
pub const CLICK_SOUND: u8 = 68;

/// Whether a building kind has the rally feature.
pub fn has_rally(kind_id: i32) -> bool {
    FACTORY_KINDS.contains(&kind_id)
}

/// The point a click at map pixel `(x, y)` sets: the cell's centre (`0x417489`: `(v & -0x20) + 0x10`).
pub fn snap(x: i32, y: i32) -> (i32, i32) {
    ((x & -0x20) + 0x10, (y & -0x20) + 0x10)
}

/// The spawn code only gives the third order when the point is inside the map: `0 <= x <= width * 32` and `0 <= y <= height * 32`
/// (`0x40896a..0x4089a0`: `js` / `jg` against `[0x71cec0] << 5`, `[0x71cec4] << 5`; -1 means none).
pub fn usable(point: (i32, i32), cells: (i32, i32)) -> bool {
    point.0 >= 0 && point.0 <= cells.0 * 32 && point.1 >= 0 && point.1 <= cells.1 * 32
}

/// The pointer while the mode is active (`0x41763b..0x417681`): type 9 at offset (-1, 0) over a free ground cell, else the
/// blocked arrow (type 1, offset (-12, -8)); `blocked` is the walkers' grid `0x6c7a4c`.
pub const POINTER_TYPE: usize = 9;
/// Offset of the rally pointer's picture from the mouse position.
pub const POINTER_OFFSET: (i32, i32) = (-1, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_point_snaps_to_the_cell_centre() {
        assert_eq!(snap(0, 0), (16, 16));
        assert_eq!(snap(31, 32), (16, 48));
        assert_eq!(snap(2047, 1000), (2032, 1008));
    }

    #[test]
    fn only_points_inside_the_map_are_used() {
        assert!(!usable((-1, -1), (64, 64)));
        assert!(usable((16, 16), (64, 64)));
        assert!(usable((2048, 2048), (64, 64))); // `jg`: the border itself passes
        assert!(!usable((2049, 16), (64, 64)));
    }

    #[test]
    fn the_marker_runs_through_16_frames_every_3_ticks() {
        assert_eq!((0..5).map(marker_frame).collect::<Vec<_>>(), [0, 0, 0, 1, 1]);
        assert_eq!(marker_frame(47), 15);
        assert_eq!(marker_frame(48), 0);
    }

    #[test]
    fn only_factories_have_one() {
        assert!(has_rally(101) && has_rally(108));
        assert!(!has_rally(100) && !has_rally(104) && !has_rally(25));
    }
}
