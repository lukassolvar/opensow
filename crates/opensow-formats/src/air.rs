//! The five "air forces": special strikes a player keeps in stock and launches from the Air base.
//!
//! Decoded from `State of War.exe`:
//! - type codes 30..=34 (`0x1e..=0x22`); the aircraft draw routine (`0x467240`) selects sprites with
//!   `code - 30`: 0 bomber, 1 carryall, 2 fighter (one frame, `0x5ea3a4`), 3 tripler (ten frames, `0x5ea328`);
//!   code 34 has no aircraft and uses the meteor sprites (`0x410xxx` cluster), so it is the asteroid strike;
//! - the level header stores the starting stock as five groups of three words in the exe's *icon order*
//!   (30, 34, 31, 33, 32): see [`HEADER_ORDER`] and [`crate::level::Level::air_stock`]; the icon tooltips are
//!   strings 200..=204 ("Bomber Attack", "Meteor Storm", "CarryAll", "Tripler Strafe", "Fighter Assault");
//! - event ids 17..=21 announce "bomber/carryall/asteroid/fighter/tripler ready" (`0x403340`);
//! - manual: the fighter "cannot be destroyed by antiaircraft means".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirForce {
    Bomber,
    Carryall,
    Tripler,
    Fighter,
    Asteroid,
}

/// Order in which the level header lists the five stocks (and the order of the in-game icons).
pub const HEADER_ORDER: [AirForce; 5] =
    [AirForce::Bomber, AirForce::Asteroid, AirForce::Carryall, AirForce::Tripler, AirForce::Fighter];

impl AirForce {
    pub fn code(self) -> i32 {
        match self {
            AirForce::Bomber => 30,
            AirForce::Carryall => 31,
            AirForce::Fighter => 32,
            AirForce::Tripler => 33,
            AirForce::Asteroid => 34,
        }
    }

    pub fn from_code(code: i32) -> Option<AirForce> {
        [AirForce::Bomber, AirForce::Carryall, AirForce::Fighter, AirForce::Tripler, AirForce::Asteroid]
            .into_iter()
            .find(|a| a.code() == code)
    }

    pub fn name(self) -> &'static str {
        match self {
            AirForce::Bomber => "Bomber",
            AirForce::Carryall => "Carryall",
            AirForce::Tripler => "Tripler",
            AirForce::Fighter => "Fighter",
            AirForce::Asteroid => "Asteroid",
        }
    }

    /// Sprite sets in `specials\` used by this force (aircraft first, then effects such as bombs, smoke, explosions).
    pub fn sprites(self) -> &'static [&'static str] {
        match self {
            AirForce::Bomber => &["bomber_l", "bomber_r", "bomba_l", "bomba_r", "bomba_ex"],
            AirForce::Carryall => &["carryAll", "carryAll_down", "carryAll_rear"],
            AirForce::Tripler => &["tripler", "tripler_ph"],
            AirForce::Fighter => &["fighter", "fighter_smoke", "fighter_bomb", "fighter_ex"],
            AirForce::Asteroid => {
                &["meteor_medium", "meteor_medium_aura", "meteor_medium_smoke", "meteor_small", "meteor_small_aura", "meteor_small_smoke"]
            }
        }
    }
}

/// Starting stock of one air force. `counts[side]`: in campaign levels side 0 is the human (always 0 in the shipped
/// levels) and side 1 the enemy; multiplayer files repeat the count in slots 1 and 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirStock {
    pub force: AirForce,
    pub counts: [i32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_roundtrip() {
        for f in HEADER_ORDER {
            assert_eq!(AirForce::from_code(f.code()), Some(f));
        }
        assert_eq!(AirForce::from_code(35), None);
        assert_eq!(HEADER_ORDER.map(|f| f.code()), [30, 34, 31, 33, 32]);
    }
}
