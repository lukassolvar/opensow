//! Unit sprite composition, taken from the exe's per-kind constructors.
//!
//! A unit is drawn as a chassis plus a weapon top. Tables recovered from `State of War.exe`:
//! - chassis: `0x46ff50` sets `[obj+0x1c]`, the index into `[chA, chB, chC, chI1, chW, chN]`
//!   (draw code at `0x46e8f0`: sprite = `chassis[type*200 + frame]`, team at `+0x10`);
//! - top: the weapon constructor (byte table `0x47ac70`, jump table `0x47ac04`) sets `[obj+0x54]`, the index into
//!   `[1AA 1A 1C 1F 1S, 2AA .. 2S, 3AA .. 3S, 1I, tur_01 tur_02 tur_03 tur_05, 5W1 .. 5W5]`
//!   (draw code at `0x479a80`: sprite = `top[idx*150 + frame]`).

use crate::level::UnitKind;

pub const CHASSIS: [&str; 6] = ["chA", "chB", "chC", "chI1", "chW", "chN"];
pub const TOPS: [&str; 25] = [
    "1AA", "1A", "1C", "1F", "1S", "2AA", "2A", "2C", "2F", "2S", "3AA", "3A", "3C", "3F", "3S", "1I", "tur_01", "tur_02", "tur_03",
    "tur_05", "5W1", "5W2", "5W3", "5W4", "5W5",
];

/// Weapon classes of ground units, in kind order (kinds 1-3 = AA, 4-6 = A, ...).
pub const CLASSES: [&str; 5] = ["Anti-air", "Artillery", "Cannon", "Flamer", "Special"];

/// Chassis index per ground kind 1..=15 (from `0x46ff50`): tier 1 -> chA, tier 2 -> chB, tier 3 -> chC for every class.
/// (An earlier version of this table gave kind 6 the light chassis; that came from a case that was read past its end.)
const GROUND_CHASSIS: [u8; 15] = [0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitArt {
    pub chassis: &'static str,
    /// `None` for kind 46, which is a bare chassis (`chN`, a round hazard-marked pod).
    pub top: Option<&'static str>,
}

/// Sprite names (inside `units\`) for a unit kind, or `None` for kinds without unit art.
pub fn unit_art(kind: i32) -> Option<UnitArt> {
    let (chassis, top) = match kind {
        1..=15 => {
            let (class, tier) = (((kind - 1) / 3) as usize, ((kind - 1) % 3) as usize);
            (GROUND_CHASSIS[(kind - 1) as usize] as usize, Some(tier * 5 + class))
        }
        40 => (3, Some(15)),
        41..=45 => (4, Some(20 + (kind - 41) as usize)),
        46 => (5, None),
        _ => return None,
    };
    Some(UnitArt { chassis: CHASSIS[chassis], top: top.map(|t| TOPS[t]) })
}

/// The first frame's header words of every unit sprite sheet, as the exe reads them to size a new unit (`0x479eb0` for the top, `0x46ff50`
/// for the chassis): `(extra[0], extra[1])` of `units\\{name}_blue.ps6` frame 0 (`sprite::Frame::extra`, the 16-bit words at `+8` / `+0xa`
/// of the frame), `None` for a sheet that is not there. They come from the install, so they are not part of any table in the code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetSizes {
    pub chassis: [Option<(u16, u16)>; 6],
    pub tops: [Option<(u16, u16)>; 25],
}

fn sext16(v: u16) -> u32 {
    v as i16 as i32 as u32
}

impl SheetSizes {
    /// Top `+0x64` / `+0x68` (sprite width and height): the words plus 1 as a signed 16-bit value, 0 without a sheet. Verified against the
    /// exe's initialiser by `scripts/gen_unit_defaults.py` (two marker values, including the sign wrap at `0xffff`).
    pub fn top(&self, sheet: usize) -> (u32, u32) {
        match self.tops.get(sheet).copied().flatten() {
            Some((w, h)) => (sext16(w.wrapping_add(1)), sext16(h.wrapping_add(1))),
            None => (0, 0),
        }
    }

    /// Chassis `+0x20` / `+0x24`: the same plus one more, 1 without a sheet.
    pub fn chassis(&self, sheet: usize) -> (u32, u32) {
        match self.chassis.get(sheet).copied().flatten() {
            Some((w, h)) => (sext16(w.wrapping_add(1)).wrapping_add(1), sext16(h.wrapping_add(1)).wrapping_add(1)),
            None => (1, 1),
        }
    }
}

impl UnitKind {
    /// Human name using the decoded class names, e.g. "Artillery tier 2" or "Walker 3".
    pub fn describe(id: i32) -> String {
        match UnitKind::from_id(id) {
            UnitKind::Ground { class, tier } => format!("{} tier {tier}", CLASSES[class as usize]),
            UnitKind::Special(k @ 41..=45) => format!("Walker {}", k - 40),
            UnitKind::Special(46) => "Pod (chN)".into(),
            other => other.name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_sizes_follow_the_exes_arithmetic() {
        let mut s = SheetSizes::default();
        assert_eq!((s.top(3), s.chassis(2)), ((0, 0), (1, 1)), "no sheet");
        s.tops[3] = Some((0x1000, 0x1111));
        s.chassis[2] = Some((0xffff, 0x7fff));
        assert_eq!(s.top(3), (0x1001, 0x1112));
        // 0xffff + 1 wraps to 0; 0x7fff + 1 = 0x8000 is -32768 as a signed 16-bit value, plus the chassis' extra 1
        assert_eq!(s.chassis(2), (1, (-32767i32) as u32));
    }

    #[test]
    fn art_mapping() {
        assert_eq!(unit_art(1), Some(UnitArt { chassis: "chA", top: Some("1AA") }));
        assert_eq!(unit_art(2), Some(UnitArt { chassis: "chB", top: Some("2AA") }));
        assert_eq!(unit_art(5), Some(UnitArt { chassis: "chB", top: Some("2A") }));
        assert_eq!(unit_art(6), Some(UnitArt { chassis: "chC", top: Some("3A") }));
        assert_eq!(unit_art(15), Some(UnitArt { chassis: "chC", top: Some("3S") }));
        assert_eq!(unit_art(40), Some(UnitArt { chassis: "chI1", top: Some("1I") }));
        assert_eq!(unit_art(43), Some(UnitArt { chassis: "chW", top: Some("5W3") }));
        assert_eq!(unit_art(46), Some(UnitArt { chassis: "chN", top: None }));
        assert_eq!(unit_art(100), None);
        assert_eq!(UnitKind::describe(8), "Cannon tier 2");
        assert_eq!(UnitKind::describe(41), "Walker 1");
    }
}
