//! Game messages ("speech events"), decoded from `State of War.exe` (`0x402ec0`, jump table `0x4034ac`).
//!
//! `0x402ec0(id, x, y, player)` shows a message line and plays a speech file for the local player. The event id (1..=24) selects
//! the `langXX.txt` string; the speech wav is the one with the matching English name in `Data/Sounds/Speech`. Event 6 ("New
//! airstrike ready") has no wav of its own: the per-force "ready" events 18-22 are used for the air bases.

/// `(event id, lang string index, speech file)`; the file is `None` for events without speech.
pub const MESSAGES: [(u8, usize, Option<&str>); 24] = [
    (1, 121, Some("06-building-captured.wav")),
    (2, 122, Some("04-turret-destroyed.wav")),
    (3, 123, Some("06-building-lost.wav")),
    (4, 124, Some("06-building-upgraded.wav")),
    (5, 125, Some("03-no-space-for-units.wav")),
    (6, 126, None),
    (7, 127, Some("04-new-turret-available.wav")),
    (8, 144, Some("03-new-techlevel-available.wav")),
    (9, 128, Some("03-advancer-destroyed.wav")),
    (10, 129, Some("03-advancer-disengaged.wav")),
    (11, 130, Some("02-5000-credits-arrived.wav")),
    (12, 131, Some("02-5000-research-arrived.wav")),
    (13, 132, Some("01-renforcement.wav")),
    (14, 133, Some("07-20minutes-to-reinforce.wav")),
    (15, 134, Some("07-10minutes-to-reinforce.wav")),
    (16, 135, Some("07-05minutes-to-reinforce.wav")),
    (17, 136, Some("07-02minutes-to-reinforce.wav")),
    (18, 137, Some("05-bomber-ready.wav")),
    (19, 138, Some("05-carryall-ready.wav")),
    (20, 139, Some("05-asteroid-ready.wav")),
    (21, 140, Some("05-fighter-ready.wav")),
    (22, 141, Some("05-tripler-ready.wav")),
    (23, 142, Some("04-turret-repaired.wav")),
    (24, 143, Some("06-building-repaired.wav")),
];

pub const BUILDING_CAPTURED: u8 = 1;
pub const TURRET_DESTROYED: u8 = 2;
pub const BUILDING_LOST: u8 = 3;
pub const BUILDING_UPGRADED: u8 = 4;
pub const NO_SPACE: u8 = 5;
pub const NEW_TURRET: u8 = 7;
pub const NEW_TECH_LEVEL: u8 = 8;
pub const ADVANCER_DESTROYED: u8 = 9;
pub const ADVANCER_DISENGAGED: u8 = 10;
pub const CREDITS_ARRIVED: u8 = 11;
pub const REINFORCEMENTS: u8 = 13;
pub const TURRET_REPAIRED: u8 = 23;
pub const BUILDING_REPAIRED: u8 = 24;

/// The "<force> ready" event for an air force code 30..=34 (bomber 18, carryall 19, asteroid 20, fighter 21, tripler 22).
pub fn force_ready(code: i32) -> Option<u8> {
    Some(match code {
        30 => 18,
        31 => 19,
        34 => 20,
        32 => 21,
        33 => 22,
        _ => return None,
    })
}

/// The exe numbers strings from 1 (`strings.rs`): subtract 1 for `LangFile::get`.
pub fn lang_index(id: u8) -> Option<usize> {
    MESSAGES.iter().find(|m| m.0 == id).map(|m| m.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_strings() {
        assert_eq!(lang_index(1), Some(121));
        assert_eq!(lang_index(8), Some(144));
        assert_eq!(force_ready(32), Some(21));
        assert_eq!(lang_index(25), None);
    }
}
