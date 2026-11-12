//! Names for the kind numbers of the game's objects and units.
//!
//! The level files, the savegames and the exe all identify a building, turret or unit by a plain number
//! (docs/levels.md). Code that tests for one particular kind should use these names instead of the number.
//! Only kinds that the code singles out are named here; the ground units (1..=15, three tiers of five weapon
//! classes, see [`crate::units`]) and the walkers have range constants; single ones of them are still plain numbers.

use std::ops::RangeInclusive;

/// Unit kind of the Advancer, the flying builder that captures, repairs and upgrades buildings.
/// (Two per two-player map; the exe's loader skips it when a per-player flag is set and its constructor gives it 800 HP.)
pub const ADVANCER: i32 = 40;
/// Unit kind of the Rogon pod: the bare `chN` chassis that carries the bomb in missions 3 and 8 and is the
/// convoy unit of escort maps.
pub const POD: i32 = 46;

/// Unit kinds of the ground units: five weapon classes (anti-air, artillery, cannon, flamer, special) in three tiers,
/// `kind = class * 3 + tier` with class 0..5 and tier 1..=3 (see [`crate::units`]).
pub const GROUND_UNITS: RangeInclusive<i32> = 1..=15;
/// Unit kinds of the five walker robots.
pub const WALKERS: RangeInclusive<i32> = 41..=45;
/// Unit kinds that shoot at aircraft: the three anti-air ground units and the anti-air walker.
pub const ANTI_AIR_UNITS: [i32; 4] = [1, 2, 3, 42];

/// Object kinds of the five turrets, `tur_01`..`tur_05`.
pub const TURRETS: RangeInclusive<i32> = 25..=29;
/// Object kind of turret 2, the anti-air turret.
pub const ANTI_AIR_TURRET: i32 = 26;
/// Unit kinds of the turrets' weapon tops: a turret's gun is a unit of kind `84 + turret kind` that the game creates with it.
pub const TURRET_TOPS: RangeInclusive<i32> = 109..=113;
/// Unit kind of the anti-air turret's weapon top.
pub const ANTI_AIR_TURRET_TOP: i32 = 110;

/// Object kind of the headquarters.
pub const HEADQUARTERS: i32 = 100;
/// Object kind of the light factory.
pub const LIGHT_FACTORY: i32 = 101;
/// Object kind of the medium factory.
pub const MEDIUM_FACTORY: i32 = 102;
/// Object kind of the heavy factory.
pub const HEAVY_FACTORY: i32 = 103;
/// Object kind of the air base.
pub const AIR_BASE: i32 = 104;
/// Object kind of the gold mine.
pub const GOLD_MINE: i32 = 105;
/// Object kind of the research lab.
pub const RESEARCH_LAB: i32 = 106;
/// Object kind of the windtrap (`Wind` in the exe's name table; it speeds up production).
pub const WINDTRAP: i32 = 107;
/// Object kind of the walker factory.
pub const WALKER_FACTORY: i32 = 108;
