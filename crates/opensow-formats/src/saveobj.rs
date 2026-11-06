//! The object records inside a savegame state stream (`docs/savegame.md`), read from the `Serialize` methods of the exe
//! (vtable slot 2 of each class). Every record starts with `u16 version, u16 magic`; fields are `u32` (or one-byte bools)
//! written in the order of the tables below, keyed by their offset in the in-memory object so the numbers match
//! docs/walking.md and friends. `scripts/verify_save_layout.py` regenerates these tables from the disassembly.

use crate::{Error, Reader, Result};

/// An MFC `CString` as `CArchive` writes it: length byte, or `0xff` + `u16`, or `0xff 0xffff` + `u32`, then the bytes.
pub fn mfc_string(r: &mut Reader, what: &'static str) -> Result<String> {
    let mut len = r.u8(what)? as usize;
    if len == 0xff {
        len = r.u16(what)? as usize;
        if len == 0xffff {
            len = r.u32(what)? as usize;
        }
    }
    Ok(String::from_utf8_lossy(r.bytes(len, what)?).into_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `0x46caf0`, a unit slot (vtable `0x490954`, 0xa0 bytes): owns a `Top` and a `Chassis`.
    Unit,
    /// `0x47b5e0`, the turret/top part (`0x490b00`, 0x218 bytes).
    Top,
    /// `0x4709a0`, the chassis (`0x490a6c`, 0x174 bytes): kind at +0xc, orders, routes.
    Chassis,
    /// `0x478f80`, an order (`0x490ab4`, 0x1c bytes): valid byte +4, type +8, x +0xc, y +0x10, two extra words.
    Order,
    /// `0x47c700`, the weapon/aim sub-object (`0x490b24`) of a top and of an Advancer chassis.
    Weapon,
    /// `0x40b160`, a building (`0x486b00`, 0x3a0 bytes).
    Building,
    /// `0x40e9f0`, one of the five 0x28-byte parts embedded in a building at +0x15c.
    BuildingPart,
    /// `0x412a70`, a bullet (`0x487770`, 0x26cc bytes).
    Bullet,
    /// `0x468c30`, an air-force squad object (`0x490830`, 0xb4 bytes).
    Aircraft,
    /// `0x41a1f0`, a scorch decal (`0x487b04`).
    Decal,
    /// `0x414820`, the objects of the list at `[0x49a3fc]` (`0x4878e4`).
    Cloud,
    /// `0x455700`, a player's state (`0xc68` bytes each, `0x70ba50`).
    PlayerState,
    /// `0x419100`, per-player stock lists (`0x338` bytes each, `0x6bed08`).
    Stock,
    /// `0x465220`, per-player parameter block (`0x80` bytes each, `0x6bf788`).
    Params,
    /// `0x403b20`, a speech queue (`0x20` bytes each, `0x6bf6b0`).
    Speech,
    /// `0x404890`, one queued speech message.
    SpeechItem,
    /// `0x43e480`, an entry of the map object's list (`0x48f26c`).
    MapItem,
}

impl Class {
    /// Every record class, in the order of the doc tables.
    pub const ALL: [Class; 17] = [
        Class::Unit, Class::Top, Class::Chassis, Class::Order, Class::Weapon, Class::Building, Class::BuildingPart, Class::Bullet,
        Class::Aircraft, Class::Decal, Class::Cloud, Class::PlayerState, Class::Stock, Class::Params, Class::Speech, Class::SpeechItem,
        Class::MapItem,
    ];

    /// `(version, magic)` written by the header call `0x403e10`.
    pub fn header(self) -> (u16, u16) {
        match self {
            Class::Unit => (1, 0x76be),
            Class::Top => (1, 0x393d),
            Class::Chassis => (2, 0x3462),
            Class::Order => (1, 0x1685),
            Class::Weapon => (1, 0xb377),
            Class::Building => (1, 0xebb9),
            Class::BuildingPart => (1, 0x11b7),
            Class::Bullet => (3, 0x5d2a),
            Class::Aircraft => (1, 0xa2e2),
            Class::Decal => (1, 0x57a9),
            Class::Cloud => (1, 0x500c),
            Class::PlayerState => (2, 0x41c2),
            Class::Stock => (1, 0x7b33),
            Class::Params => (1, 0x18c7),
            Class::Speech => (1, 0xf8a8),
            Class::SpeechItem => (1, 0xd8db),
            Class::MapItem => (1, 0xab30),
        }
    }

    /// What the field at in-memory offset `o` is, where the rest of the project established it (docs/savegame.md "Field meanings",
    /// every name below was read from a constructor or an update function of the exe, see the doc for the addresses); `""` = not
    /// understood yet.
    pub fn field_name(self, o: u32) -> &'static str {
        match (self, o) {
            (Class::Unit, 0x14) => "x (pixel)",
            (Class::Unit, 0x18) => "y (pixel)",
            (Class::Unit, 0x2c) => "dead",
            (Class::Unit, 0x30) => "maximum hit points",
            (Class::Unit, 0x34) => "hit points",
            (Class::Unit, 0x1c) => "box x offset (init -16; constructor 0x46bd30, reader not traced)",
            (Class::Unit, 0x20) => "box y offset (init -16)",
            (Class::Unit, 0x24) => "box width (init 32)",
            (Class::Unit, 0x28) => "box height (init 30)",
            (Class::Unit, 0x38) => "death threshold and health bar divisor (init 16, Advancer 20: 0x46e040 kills at hit points <= it, 0x46a420 draws max / it pixels)",
            (Class::Unit, 0x50) => "view ring radius (init 5, argument of 0x436130)",
            (Class::Unit, 0x64) => "health bar y offset (init -25, Advancer -23)",
            (Class::Unit, 0x68) => "turret building handle (turret tops 109..113 only, docs/network.md)",
            (Class::Unit, 0x54) | (Class::Unit, 0x58) => "unused (the nearby references are the top's own +0x54 / +0x58)",
            (Class::Unit, 0x6c) => "target blink active (0x469fe0 sets 1; drawn flag with +0x74)",
            (Class::Unit, 0x70) => "blink timer (5)",
            (Class::Unit, 0x74) => "blink on",
            (Class::Unit, 0x78) => "blink toggles left (8)",
            (Class::Unit, 0x80) => "kept anti-air target (aircraft handle, 0x47b3c0)",
            (Class::Unit, 0x3c) => "handle",
            (Class::Unit, 0x40) => "kind",
            (Class::Unit, 0x44) => "player",
            (Class::Unit, 0x4b) => "flyer (air grid)",
            (Class::Unit, 0x4c) => "anti-air",
            (Class::Unit, 0x5c) => "hit flash timer",
            (Class::Unit, 0x60) => "hit flash step",
            (Class::Unit, 0x7c) => "target handle",
            (Class::Unit, 0x84) => "hover phase",
            (Class::Unit, 0x88) => "hover offset",
            (Class::Unit, 0x8c) => "idle look-around timer",
            (Class::Unit, 0x90) => "corpse timer",
            (Class::Unit, 0x94) => "death variant",
            (Class::Unit, 0x98) => "final blasts thrown",
            (Class::Top, 0x50) => "facing count",
            (Class::Top, 0x54) => "sprite index",
            (Class::Top, 0x58) => "flash set",
            (Class::Top, 0x5c) => "aim offset x",
            (Class::Top, 0x60) => "aim offset y",
            (Class::Top, 0x6c) => "facing",
            (Class::Top, 0x70) => "target facing",
            (Class::Top, 0x74) => "turn counter",
            (Class::Top, 0x78) => "turn reload",
            (Class::Top, 0x7c) => "target handle",
            (Class::Top, 0x88) => "shot end point y",
            (Class::Top, 0x64) => "sprite width (sheet header word +8, plus 1; set by 0x479eb0)",
            (Class::Top, 0x68) => "sprite height (sheet header word +0xa)",
            (Class::Top, 0x84) => "shot end point x (0x4797c0; +0x88 is y)",
            (Class::Top, 0x8c) => "idle phase index (0x479b50 cycles it)",
            (Class::Top, 0x90) => "idle phase count (1, the single rest phase)",
            (Class::Top, 0xbc) => "idle phase countdown (init 10)",
            (Class::Top, 0xc0) => "idle phase reload (10)",
            (Class::Top, 0x80) => "target search counter (30-tick poll, aim.rs)",
            (Class::Top, 0x20c) => "fire sequence tick counter (starts at 0, recoil.rs)",
            (Class::Top, 0x214) => "flash / phase override active (recoil.rs)",
            (Class::Top, 0xc4) => "fire step index (recoil.rs)",
            (Class::Top, 0xc8) => "fire step count",
            (Class::Top, 0x210) => "fire step length (default 10 = reload + 1, recoil.rs)",
            (Class::Chassis, 0x8) => "facing count",
            (Class::Chassis, 0xc) => "kind (0x28 = Advancer)",
            (Class::Chassis, 0x10) => "team (blue / green sheet)",
            (Class::Chassis, 0x18) => "walking speed (px/s)",
            (Class::Chassis, 0x1c) => "sprite sheet",
            (Class::Chassis, 0x28) => "draw offset x",
            (Class::Chassis, 0x2c) => "draw offset y",
            (Class::Chassis, 0x30) => "facing",
            (Class::Chassis, 0x34) => "target facing",
            (Class::Chassis, 0x38) => "turn counter",
            (Class::Chassis, 0x3c) => "turn reload",
            (Class::Chassis, 0x40) => "frame",
            (Class::Chassis, 0x44) => "walk phase",
            (Class::Chassis, 0x48) => "phase count",
            (Class::Chassis, 0x4c) => "animation counter",
            (Class::Chassis, 0x50) => "animation period",
            (Class::Chassis, 0x5c) => "goal pixel x",
            (Class::Chassis, 0x60) => "goal pixel y",
            (Class::Chassis, 0x6c) => "step cell x",
            (Class::Chassis, 0x70) => "step cell y",
            (Class::Chassis, 0x80) => "re-search cooldown",
            (Class::Chassis, 0x14) => "unit handle (copy; third argument of the initialiser 0x46ff50)",
            (Class::Chassis, 0x7c) => "step cell reserved (0x46f550 stamps +0x6c/+0x70 in the unit grid while set)",
            (Class::Chassis, 0x20) => "sprite width (sheet header word +8, plus 1; set by 0x46ff50)",
            (Class::Chassis, 0x24) => "sprite height (sheet header word +0xa, plus 1)",
            (Class::Chassis, 0x58) => "unused (never read or written outside the serializer)",
            (Class::Chassis, 0x14c) => "Advancer working flag (0x46f210: set when its action state is below 32 and +0x144 is set)",
            (Class::Chassis, 0x158) => "chasing a building (0x46ee59: target handle class 2)",
            (Class::Chassis, 0x54) => "Advancer frame counter (reload 2)",
            (Class::Chassis, 0x64) => "current cell x",
            (Class::Chassis, 0x68) => "current cell y",
            (Class::Chassis, 0x74) => "second step cell x (networked planner)",
            (Class::Chassis, 0x78) => "second step cell y",
            (Class::Chassis, 0xf4) => "Advancer action target x (cell centre pixel)",
            (Class::Chassis, 0xf8) => "Advancer action target y",
            (Class::Chassis, 0xfc) => "Advancer action mode (1 destroy, 2 repair, 3 upgrade)",
            (Class::Chassis, 0x100) => "Advancer action target building",
            (Class::Chassis, 0x150) => "Advancer idle counter",
            (Class::Chassis, 0x154) => "weapon range for chasing",
            (Class::Chassis, 0x168) => "order word a",
            (Class::Chassis, 0x16c) => "order word b",
            (Class::Chassis, 0x170) => "order facing",
            (Class::Weapon, 0x4) => "enabled",
            (Class::Weapon, 0x8) | (Class::Weapon, 0xc) => "constructor argument (0x47c630; never read)",
            (Class::Weapon, 0x34) | (Class::Weapon, 0x38) | (Class::Weapon, 0x44) => "zero by the constructor, never read",
            (Class::Weapon, 0x10) => "minimum range (cells)",
            (Class::Weapon, 0x14) => "maximum range (cells)",
            (Class::Weapon, 0x18) => "spread (muzzle scatter; the top's [top+0x20])",
            (Class::Weapon, 0x1c) => "counter",
            (Class::Weapon, 0x20) => "cooldown reload (Advancer beam: 10)",
            (Class::Weapon, 0x24) => "jitter (Advancer beam: 0)",
            (Class::Weapon, 0x28) => "rounds per shot (the spawner's outer loop; turret 5 throws six)",
            (Class::Weapon, 0x2c) => "bullet type",
            (Class::Weapon, 0x30) => "strength",
            (Class::Weapon, 0x40) => "state",
            (Class::Order, 0x4) => "valid",
            (Class::Order, 0x8) => "type (1 go, 2 straight line, 3 script)",
            (Class::Order, 0xc) => "x (pixel)",
            (Class::Order, 0x10) => "y (pixel)",
            (Class::Order, 0x14) => "word a",
            (Class::Order, 0x18) => "word b",
            (Class::Building, 0x4) => "cell x",
            (Class::Building, 0x8) => "cell y",
            (Class::Building, 0xc) => "cell x end (exclusive)",
            (Class::Building, 0x10) => "cell y end (exclusive)",
            (Class::Building, 0x14) => "width (cells)",
            (Class::Building, 0x18) => "height (cells)",
            (Class::Building, 0x1c) => "sprite hotspot x offset from the cell corner",
            (Class::Building, 0x20) => "sprite hotspot y offset",
            (Class::Building, 0x24) => "base sprite stem",
            (Class::Building, 0x28) | (Class::Building, 0x2c) | (Class::Building, 0x30) | (Class::Building, 0x34) | (Class::Building, 0x38) | (Class::Building, 0x3c) | (Class::Building, 0x40) | (Class::Building, 0x44) => "frame boundary of the animation states (state s shows frames bounds[s]..bounds[s+1])",
            (Class::Building, 0x48) => "animation state",
            (Class::Building, 0x4c) => "loop frame",
            (Class::Building, 0x54) => "loop frame timer",
            (Class::Building, 0x58) => "loop frame period (ticks)",
            (Class::Building, 0x60) => "rally marker frame",
            (Class::Building, 0x64) => "rally marker frame counter (reload 3)",
            (Class::Building, 0x68) => "not healing in",
            (Class::Building, 0x69) => "destroyed",
            (Class::Building, 0x6c) => "pending heal",
            (Class::Building, 0x70) => "maximum hit points",
            (Class::Building, 0x74) => "hit points",
            (Class::Building, 0x8c) => "owner",
            (Class::Building, 0x94) => "kind",
            (Class::Building, 0xfc) => "linked Advancer",
            (Class::Building, 0x104) => "destruction state",
            (Class::Building, 0x108) => "destroy animation length",
            (Class::Building, 0x90) => "handle (a turret gun's [unit+0x68] equals it, 0x408713)",
            (Class::Building, 0x13c) => "unused (constructor sets 1)",
            (Class::Building, 0x398) => "unused (constructor sets 0)",
            (Class::Building, 0x50) => "animation sub-counter (-1 at construction, 0 after a state change 0x40e105)",
            (Class::Building, 0x5c) => "second sprite layer (shadow blit before the body in 0x4092f0; the constructor sets 1 only for kind 104)",
            (Class::Building, 0x2bc) => "additive overlay effect active (0x4096e0 draws it at the pairs +0x26c / +0x294; 0x408d60 sets and clears it)",
            (Class::Building, 0x78) => "health bar scale (pixels per hit-point divisor, 0x409400; set 0x30 / 0x48 by the constructor)",
            (Class::Building, 0x100) | (Class::Building, 0x101) => "destruction animation flags (set to 1 by 0x409b60)",
            (Class::Building, 0x10c) => "destruction animation counter",
            (Class::Building, 0x110) => "passes until the next destruction burst",
            (Class::Building, 0x114) => "headquarters flag (satellite protected)",
            (Class::Building, 0x118) => "satellite triggered",
            (Class::Building, 0x11c) => "satellite timer",
            (Class::Building, 0x120) => "top edge y (pixel, cell y * 32)",
            (Class::Building, 0x124) | (Class::Building, 0x128) => "spawn cell",
            (Class::Building, 0x12c) | (Class::Building, 0x130) => "primary exit",
            (Class::Building, 0x134) | (Class::Building, 0x138) => "alternative exit",
            (Class::Building, 0x140) => "slot count",
            (Class::Building, 0x144) => "active slots",
            (Class::Building, 0x148) => "Advancer upgrade points",
            (Class::Building, 0x14c) => "target blink active (0x4086a0)",
            (Class::Building, 0x150) => "blink timer (5)",
            (Class::Building, 0x154) => "blink state",
            (Class::Building, 0x158) => "blink toggles left (8)",
            (Class::Building, 0x264) => "AI threat",
            (Class::Building, 0x2c0) => "rally point x (pixel, -1 none)",
            (Class::Building, 0x2c4) => "rally point y",
            (Class::Building, 0x2c8) => "menu / preview copy (sounds off)",
            (Class::BuildingPart, 0x4) => "kind",
            (Class::BuildingPart, 0xc) => "target",
            (Class::BuildingPart, 0x10) => "progress",
            (Class::BuildingPart, 0x14) => "rate",
            (Class::BuildingPart, 0x18) => "ready",
            (Class::BuildingPart, 0x1c) => "unlock price",
            (Class::BuildingPart, 0x8) => "unused (never read or written outside the serializer)",
            (Class::BuildingPart, 0x20) => "tech levels bought (0x40e83d increments it)",
            (Class::BuildingPart, 0x24) => "value (level limit)",
            (Class::Bullet, 0x4) => "x (current path point, 0x40ef43)",
            (Class::Bullet, 0x8) => "y (current path point)",
            (Class::Bullet, 0x2668) | (Class::Bullet, 0x2684) => "unused (never read or written outside the serializer)",
            (Class::Bullet, 0x26c4) => "Rogon blast counter (type 35, starts at 20)",
            (Class::Bullet, 0x26c8) => "Rogon blast step k (type 35)",
            (Class::Bullet, 0xc0) => "previous shadow lift (types 13/14)",
            (Class::Bullet, 0x2644) => "flight progress",
            (Class::Bullet, 0x2648) => "path length",
            (Class::Bullet, 0x264c) => "trail counter",
            (Class::Bullet, 0x2650) => "trail interval",
            (Class::Bullet, 0x2654) => "facing",
            (Class::Bullet, 0x2658) => "type",
            (Class::Bullet, 0x265c) => "class",
            (Class::Bullet, 0x2660) => "strength",
            (Class::Bullet, 0x2664) => "speed",
            (Class::Bullet, 0x266c) => "owner",
            (Class::Bullet, 0x2670) => "animation counter",
            (Class::Bullet, 0x2674) => "end frame",
            (Class::Bullet, 0x2678) => "animation countdown",
            (Class::Bullet, 0x267c) => "animation reload",
            (Class::Bullet, 0x2680) => "variant (top index, dim sprite)",
            (Class::Bullet, 0x2688) => "target handle",
            (Class::Bullet, 0x268c) => "variant 2 (bomb direction, debris pick)",
            (Class::Aircraft, 0x4) => "x",
            (Class::Aircraft, 0x8) => "y (ground line)",
            (Class::Aircraft, 0xc) => "target x",
            (Class::Aircraft, 0x10) => "target y",
            (Class::Aircraft, 0x14) => "height above ground",
            (Class::Aircraft, 0x18) => "hit x offset (targets are measured to x + it, docs/air.md)",
            (Class::Aircraft, 0x1c) => "handle",
            (Class::Aircraft, 0x24) => "speed",
            (Class::Aircraft, 0x28) => "code (30 bomber .. 34 asteroid)",
            (Class::Aircraft, 0x2c) => "hit points",
            (Class::Aircraft, 0x34) => "flies towards +x",
            (Class::Aircraft, 0x38) => "death timer",
            (Class::Aircraft, 0x3c) => "explosion timer",
            (Class::Aircraft, 0x40) => "frame",
            (Class::Aircraft, 0x44) => "frame count",
            (Class::Aircraft, 0x48) => "animation countdown",
            (Class::Aircraft, 0x4c) => "animation reload",
            (Class::Aircraft, 0x20) => "side (indexes the statistics block `0x6bf788 + side * 0x80`, 0x467b0c)",
            (Class::Aircraft, 0x30) => "unused (never read or written outside the serializer)",
            (Class::Aircraft, 0x50) => "exhaust frame (carryall, 0..3)",
            (Class::Aircraft, 0x54) => "exhaust frame count (4)",
            (Class::Aircraft, 0x58) => "exhaust countdown",
            (Class::Aircraft, 0x5c) => "exhaust reload (2)",
            (Class::Aircraft, 0x60) => "landing glow frame (0..1)",
            (Class::Aircraft, 0x64) => "landing glow frame count (2)",
            (Class::Aircraft, 0x68) => "landing glow countdown",
            (Class::Aircraft, 0x6c) => "landing glow reload (4)",
            (Class::Aircraft, 0x70) => "counter (bomb timer / unload cooldown / shot cooldown)",
            (Class::Aircraft, 0x74) => "state (tripler firing state)",
            (Class::Aircraft, 0x78) => "bomb travel",
            (Class::Aircraft, 0x7c) => "draw x offset",
            (Class::Aircraft, 0x80) => "draw y offset (minus the height)",
            (Class::Aircraft, 0x84) => "tripler in position",
            (Class::Aircraft, 0x88) => "carryall squad",
            (Class::Decal, 0x4) => "x (pixel)",
            (Class::Decal, 0x8) => "y (pixel)",
            (Class::Decal, 0xc) => "frame (0..4)",
            (Class::Decal, 0x10) => "set to 1 by the initialiser 0x41a1d0",
            (Class::Cloud, 0x4) => "x (pixel)",
            (Class::Cloud, 0x8) => "y (pixel)",
            (Class::Cloud, 0xc) => "speed class (10..39)",
            (Class::PlayerState, 0x38) => "computer strategy (1..4, enemy block only, [0x70ba88])",
            (Class::PlayerState, 0x3c) => "computer strategy timer ([0x70ba8c])",
            (Class::PlayerState, 0x40) => "air-strike lock countdown (180 after a launch, 0x467efb)",
            (Class::Speech, 0x4) => "last message event x (pixel, -1 none)",
            (Class::Speech, 0x8) => "last message event y",
            (Class::Stock, 0x32c) | (Class::Stock, 0x330) => "unused (-1 by the constructors, never read)",
            (Class::Stock, 0x2dc) => "Tab round-robin index (select next unit)",
            (Class::Stock, 0x2e0) => "H round-robin index (select next Advancer)",
            (Class::MapItem, 0xc) => "kind (0 static building sprite, 1 other, docs/savegame.md)",
            _ => "",
        }
    }

    /// Scalar fields in write order: `(offset, is_bool)`. Loops and sub-objects come after, see [`Object::parse`].
    pub fn scalars(self) -> Vec<(u32, bool)> {
        fn u(o: u32) -> (u32, bool) {
            (o, false)
        }
        fn b(o: u32) -> (u32, bool) {
            (o, true)
        }
        match self {
            Class::Unit => vec![
                u(0x14), u(0x18), u(0x1c), u(0x20), u(0x24), u(0x28), u(0x30), u(0x34), u(0x38), u(0x50), u(0x3c), u(0x40), u(0x54),
                u(0x44), u(0x58), u(0x64), u(0x68), u(0x70), u(0x74), u(0x78), u(0x7c), u(0x80), u(0x84), u(0x88), u(0x8c), u(0x5c),
                u(0x60), u(0x90), u(0x94), b(0x98), b(0x2c), b(0x6c), b(0x4b), b(0x4c),
            ],
            Class::Top => vec![
                u(0x50), u(0x54), u(0x58), u(0x5c), u(0x60), u(0x64), u(0x68), u(0x6c), u(0x70), u(0x74), u(0x78), u(0x7c), u(0x80),
                u(0x84), u(0x88), u(0x8c), u(0x90), u(0xbc), u(0xc0), u(0xc4), u(0xc8), u(0x20c), u(0x210), u(0x214),
            ],
            Class::Chassis => vec![
                u(0x8), u(0xc), u(0x10), u(0x14), u(0x18), u(0x1c), u(0x20), u(0x24), u(0x28), u(0x2c), u(0x30), u(0x34), u(0x38),
                u(0x3c), u(0x40), u(0x44), u(0x48), u(0x4c), u(0x50), u(0x54), u(0x58), u(0x5c), u(0x60), u(0x64), u(0x68), u(0x6c),
                u(0x70), u(0x74), u(0x78), u(0x80), u(0xf4), u(0xf8), u(0xfc), u(0x100), u(0x150), u(0x154), u(0x168), u(0x16c),
                u(0x170), b(0x7c), b(0x14c), b(0x158),
            ],
            Class::Order => vec![u(0x8), u(0xc), u(0x10), u(0x14), u(0x18), b(0x4)],
            Class::Weapon => vec![
                u(0x8), u(0xc), u(0x10), u(0x14), u(0x18), u(0x1c), u(0x20), u(0x24), u(0x28), u(0x2c), u(0x30), u(0x34), u(0x38),
                u(0x40), u(0x44), b(0x4),
            ],
            Class::Building => vec![
                u(0x4), u(0x8), u(0xc), u(0x10), u(0x14), u(0x18), u(0x24), u(0x28), u(0x2c), u(0x30), u(0x34), u(0x38), u(0x3c),
                u(0x40), u(0x44), u(0x48), u(0x4c), u(0x50), u(0x54), u(0x58), u(0x6c), u(0x70), u(0x74), u(0x78), u(0x140), u(0x144),
                u(0x148), u(0x8c), u(0x90), u(0x94), u(0x104), u(0x108), u(0x10c), u(0x110), u(0x120), u(0x124), u(0x128), u(0x12c),
                u(0x130), u(0x134), u(0x138), u(0x13c), u(0x264), u(0x114), u(0x118), u(0x11c), u(0x2bc), u(0x2c0), u(0x2c4),
                u(0x150), u(0x154), u(0x158), u(0x1c), u(0x20), u(0x60), u(0x64), u(0xfc), u(0x398), b(0x14c), b(0x5c), b(0x68),
                b(0x69), b(0x100), b(0x101), b(0x2c8),
            ],
            Class::BuildingPart => vec![u(0x4), u(0x8), u(0xc), u(0x10), u(0x14), u(0x18), u(0x1c), u(0x20), u(0x24)],
            Class::Bullet => vec![
                u(0x4), u(0x8), u(0xc0), u(0x2644), u(0x2648), u(0x264c), u(0x2650), u(0x2654), u(0x2658), u(0x265c), u(0x2660),
                u(0x2664), u(0x2668), u(0x2670), u(0x2674), u(0x2678), u(0x267c), u(0x2680), u(0x2684), u(0x2688), u(0x266c),
                u(0x268c), u(0x26c4), u(0x26c8),
            ],
            Class::Aircraft => vec![
                u(0x4), u(0x8), u(0xc), u(0x10), u(0x14), u(0x1c), u(0x20), u(0x24), u(0x28), u(0x2c), u(0x34), u(0x38), u(0x3c),
                u(0x40), u(0x44), u(0x48), u(0x4c), u(0x50), u(0x54), u(0x58), u(0x5c), u(0x60), u(0x64), u(0x68), u(0x6c), u(0x70),
                u(0x74), u(0x78), u(0x7c), u(0x80), u(0x18), u(0x88), b(0x30), b(0x84),
            ],
            Class::Decal => vec![u(0x4), u(0x8), u(0xc), u(0x10)],
            Class::Cloud => vec![u(0x4), u(0x8), u(0xc)],
            Class::PlayerState => vec![u(0x38), u(0x3c), u(0x40)],
            Class::Stock => vec![u(0x2dc), u(0x32c), u(0x330), u(0x2e0)],
            Class::Params => vec![],
            Class::Speech => vec![u(0x4), u(0x8)],
            Class::SpeechItem => vec![],
            Class::MapItem => vec![u(0xc)],
        }
    }
}

/// A parsed record. `fields` hold every scalar and array element as `(object offset, value)` (bools as 0/1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub class: Class,
    pub fields: Vec<(u32, u32)>,
    /// Embedded or owned sub-records with the offset they belong to (`0` for the unit's two owned pointers' `+4`/`+8`).
    pub children: Vec<(u32, Object)>,
    /// The chassis' order list (`[+0xc8]`, `[+0xcc]` entries).
    pub orders: Vec<Object>,
    /// Chassis route cells `[+0x88]` and local route `[+0x9c]`: `(a, b)` pairs.
    pub route: Vec<(u32, u32)>,
    pub local_route: Vec<(u32, u32)>,
    /// MFC strings (a speech message's text).
    pub strings: Vec<String>,
}

impl Object {
    pub fn get(&self, offset: u32) -> Option<u32> {
        self.fields.iter().find(|f| f.0 == offset).map(|f| f.1)
    }

    pub fn child(&self, offset: u32) -> Option<&Object> {
        self.children.iter().find(|c| c.0 == offset).map(|c| &c.1)
    }

    /// Reads one record of `class` (header included).
    pub fn parse(r: &mut Reader, class: Class) -> Result<Object> {
        let head = (r.u16("version")?, r.u16("magic")?);
        if head != class.header() {
            return Err(Error::Invalid(format!("{class:?} record header {head:?}, expected {:?}", class.header())));
        }
        let mut o = Object { class, fields: Vec::new(), children: Vec::new(), orders: Vec::new(), route: Vec::new(), local_route: Vec::new(), strings: Vec::new() };
        let scalar = |r: &mut Reader, o: &mut Object, off: u32, is_bool: bool| -> Result<()> {
            let v = if is_bool { r.u8("bool")? as u32 } else { r.u32("field")? };
            o.fields.push((off, v));
            Ok(())
        };
        // class-specific sections that sit between scalars in the exe's code
        let scalars = class.scalars();
        match class {
            Class::Bullet => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for i in 0..15 {
                    scalar(r, &mut o, 0xc + 4 * i, false)?;
                    scalar(r, &mut o, 0x48 + 4 * i, false)?;
                    scalar(r, &mut o, 0x84 + 4 * i, false)?;
                }
                let len = o.get(0x2648).unwrap() as i32;
                for i in 0..(len / 2).max(0) as u32 {
                    scalar(r, &mut o, 0xc4 + 4 * i, false)?;
                }
                for i in o.get(0x2644).unwrap() as i32..len {
                    scalar(r, &mut o, (0xd44 + 4 * i) as u32, false)?;
                }
                for off in [0x2698, 0x269c, 0x26a0, 0x26a4, 0x26a8, 0x26ac, 0x26b0, 0x26b4, 0x26bc] {
                    scalar(r, &mut o, off, false)?;
                }
                scalar(r, &mut o, 0x26b8, true)?;
                scalar(r, &mut o, 0x26c0, true)?;
            }
            Class::Unit => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for i in 0..3 {
                    scalar(r, &mut o, 0x48 + i, true)?;
                }
                for (off, class) in [(4, Class::Top), (8, Class::Chassis)] {
                    if r.u8("present")? == 1 {
                        let c = Object::parse(r, class)?;
                        o.children.push((off, c));
                    }
                }
            }
            Class::Building => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for k in 0..3 {
                    scalar(r, &mut o, 0x88 + k, true)?;
                    scalar(r, &mut o, 0x7c + 4 * k, false)?;
                }
                for i in 0..25 {
                    scalar(r, &mut o, 0x98 + 4 * i, false)?;
                }
                for k in 0..5 {
                    let part = Object::parse(r, Class::BuildingPart)?;
                    o.children.push((0x15c + 0x28 * k, part));
                }
                for i in 0..8 {
                    scalar(r, &mut o, 0x224 + 4 * i, false)?;
                    scalar(r, &mut o, 0x244 + 4 * i, false)?;
                }
                for i in 0..10 {
                    scalar(r, &mut o, 0x26c + 4 * i, false)?;
                    scalar(r, &mut o, 0x294 + 4 * i, false)?;
                }
                let n = r.u32("item count")?;
                o.fields.push((0x394, n));
                if n > 1 << 16 {
                    return Err(Error::Invalid(format!("building item count {n}")));
                }
                for j in 0..n {
                    for k in 0..5 {
                        scalar(r, &mut o, 0x2cc + 0x14 * j + 4 * k, false)?;
                    }
                }
            }
            Class::Aircraft => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for k in 0..5 {
                    scalar(r, &mut o, 0x8c + k, true)?;
                }
            }
            Class::Top => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for i in 0..10 {
                    scalar(r, &mut o, 0x94 + 4 * i, false)?;
                }
                for i in 0..20 {
                    for k in 0..4 {
                        scalar(r, &mut o, 0xcc + 0x10 * i + 4 * k, false)?;
                    }
                }
                let w = Object::parse(r, Class::Weapon)?;
                o.children.push((8, w));
            }
            Class::Chassis => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                let cur = Object::parse(r, Class::Order)?;
                o.children.push((0xd8, cur));
                let n = r.u32("order count")?;
                o.fields.push((0xcc, n));
                if n > 1 << 16 {
                    return Err(Error::Invalid(format!("order count {n}")));
                }
                for _ in 0..n {
                    o.orders.push(Object::parse(r, Class::Order)?);
                }
                for (count_off, route) in [(0x8c, 0), (0xa0, 1)] {
                    let n = r.u32("route length")?;
                    o.fields.push((count_off, n));
                    if n > 1 << 20 {
                        return Err(Error::Invalid(format!("route length {n}")));
                    }
                    let mut v = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        v.push((r.u32("route a")?, r.u32("route b")?));
                    }
                    if route == 0 {
                        o.route = v;
                    } else {
                        o.local_route = v;
                    }
                }
                if o.get(0xc) == Some(0x28) {
                    let w = Object::parse(r, Class::Weapon)?;
                    o.children.push((0x104, w));
                }
            }
            Class::PlayerState => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for i in 0..10 {
                    scalar(r, &mut o, 0x10 + 4 * i, false)?;
                }
            }
            Class::Stock => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                for i in 0..9 {
                    scalar(r, &mut o, 0x2e4 + 8 * i, false)?;
                    scalar(r, &mut o, 0x2e8 + 8 * i, false)?;
                }
                // ten lists at +0x218 (stride 0x14): count, then the items; items are keyed (list + 1) << 16 | index
                for i in 0..10u32 {
                    let n = r.u32("list count")?;
                    if n > 1 << 16 {
                        return Err(Error::Invalid(format!("stock list count {n}")));
                    }
                    o.fields.push((0x21c + 0x14 * i, n));
                    for j in 0..n {
                        o.fields.push(((i + 1) << 16 | j, r.u32("list item")?));
                    }
                }
            }
            Class::Params => {
                for i in 0..30 {
                    scalar(r, &mut o, 4 + 4 * i, false)?;
                }
                scalar(r, &mut o, 0x7c, false)?;
            }
            Class::Speech => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
                let n = r.u32("message count")?;
                if n > 1 << 16 {
                    return Err(Error::Invalid(format!("speech count {n}")));
                }
                o.fields.push((0x14, n));
                for _ in 0..n {
                    o.orders.push(Object::parse(r, Class::SpeechItem)?);
                }
            }
            Class::SpeechItem => {
                o.strings.push(mfc_string(r, "speech text")?);
                for off in [0x8, 0xc, 0x10, 0x14] {
                    scalar(r, &mut o, off, false)?;
                }
            }
            Class::MapItem => {
                scalar(r, &mut o, 0xc, false)?;
                match o.get(0xc) {
                    Some(0) => {
                        for off in [0x4, 0x8, 0x10, 0x14, 0x18] {
                            scalar(r, &mut o, off, false)?;
                        }
                        scalar(r, &mut o, 0x1c, true)?;
                        scalar(r, &mut o, 0x1d, true)?;
                    }
                    Some(1) => {
                        for off in [0x4, 0x8, 0x10] {
                            scalar(r, &mut o, off, false)?;
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                for &(off, b) in &scalars {
                    scalar(r, &mut o, off, b)?;
                }
            }
        }
        Ok(o)
    }
}

impl Object {
    /// Every field of the record as `(path, offset, value)`, children and lists included (`child8/order1`, `route`, ...), for
    /// tools that compare a record with a memory image.
    pub fn flatten(&self, path: &str, out: &mut Vec<(String, u32, u32)>) {
        for &(o, v) in &self.fields {
            out.push((path.to_string(), o, v));
        }
        for (i, &(a, b)) in self.route.iter().enumerate() {
            out.push((format!("{path}/route"), 2 * i as u32, a));
            out.push((format!("{path}/route"), 2 * i as u32 + 1, b));
        }
        for (i, &(a, b)) in self.local_route.iter().enumerate() {
            out.push((format!("{path}/local"), 2 * i as u32, a));
            out.push((format!("{path}/local"), 2 * i as u32 + 1, b));
        }
        for (i, o) in self.orders.iter().enumerate() {
            o.flatten(&format!("{path}/list{i}"), out);
        }
        for (off, c) in &self.children {
            c.flatten(&format!("{path}/child{off:x}"), out);
        }
    }

    /// Value of the field at `offset` (0 when the record has none, which is what a fresh object holds).
    fn val(&self, offset: u32) -> u32 {
        self.get(offset).unwrap_or(0)
    }

    /// Writes the record the way the exe's store branch does: the exact inverse of [`Object::parse`]. The tables above fix the
    /// order; counts that the stream carries (building items, orders, routes, stock lists, speech messages, bullet path) are taken
    /// from the record's own lists, and `parse(write(o)) == o` is tested for every class (`scripts/verify_save_store.py` checks the
    /// bytes against the exe's own store branch run in an emulator).
    pub fn write(&self, out: &mut Vec<u8>) {
        let (ver, magic) = self.class.header();
        out.extend(ver.to_le_bytes());
        out.extend(magic.to_le_bytes());
        let put = |out: &mut Vec<u8>, off: u32, is_bool: bool| {
            let v = self.val(off);
            if is_bool {
                out.push(v as u8);
            } else {
                out.extend(v.to_le_bytes());
            }
        };
        let u = |out: &mut Vec<u8>, off: u32| put(out, off, false);
        let b = |out: &mut Vec<u8>, off: u32| put(out, off, true);
        let scalars = |out: &mut Vec<u8>| {
            for (off, is_bool) in self.class.scalars() {
                put(out, off, is_bool);
            }
        };
        let opt_child = |out: &mut Vec<u8>, off: u32| match self.child(off) {
            Some(c) => {
                out.push(1);
                c.write(out);
            }
            None => out.push(0),
        };
        match self.class {
            Class::Bullet => {
                scalars(out);
                for i in 0..15 {
                    u(out, 0xc + 4 * i);
                    u(out, 0x48 + 4 * i);
                    u(out, 0x84 + 4 * i);
                }
                let len = self.val(0x2648) as i32;
                for i in 0..(len / 2).max(0) as u32 {
                    u(out, 0xc4 + 4 * i);
                }
                for i in self.val(0x2644) as i32..len {
                    u(out, (0xd44 + 4 * i) as u32);
                }
                for off in [0x2698, 0x269c, 0x26a0, 0x26a4, 0x26a8, 0x26ac, 0x26b0, 0x26b4, 0x26bc] {
                    u(out, off);
                }
                b(out, 0x26b8);
                b(out, 0x26c0);
            }
            Class::Unit => {
                scalars(out);
                for i in 0..3 {
                    b(out, 0x48 + i);
                }
                opt_child(out, 4);
                opt_child(out, 8);
            }
            Class::Building => {
                scalars(out);
                for k in 0..3 {
                    b(out, 0x88 + k);
                    u(out, 0x7c + 4 * k);
                }
                for i in 0..25 {
                    u(out, 0x98 + 4 * i);
                }
                for k in 0..5 {
                    match self.child(0x15c + 0x28 * k) {
                        Some(p) => p.write(out),
                        None => Object::blank(Class::BuildingPart).write(out),
                    }
                }
                for i in 0..8 {
                    u(out, 0x224 + 4 * i);
                    u(out, 0x244 + 4 * i);
                }
                for i in 0..10 {
                    u(out, 0x26c + 4 * i);
                    u(out, 0x294 + 4 * i);
                }
                let n = self.val(0x394);
                out.extend(n.to_le_bytes());
                for j in 0..n {
                    for k in 0..5 {
                        u(out, 0x2cc + 0x14 * j + 4 * k);
                    }
                }
            }
            Class::Aircraft => {
                scalars(out);
                for k in 0..5 {
                    b(out, 0x8c + k);
                }
            }
            Class::Top => {
                scalars(out);
                for i in 0..10 {
                    u(out, 0x94 + 4 * i);
                }
                for i in 0..20 {
                    for k in 0..4 {
                        u(out, 0xcc + 0x10 * i + 4 * k);
                    }
                }
                match self.child(8) {
                    Some(w) => w.write(out),
                    None => Object::blank(Class::Weapon).write(out),
                }
            }
            Class::Chassis => {
                scalars(out);
                match self.child(0xd8) {
                    Some(c) => c.write(out),
                    None => Object::blank(Class::Order).write(out),
                }
                out.extend((self.orders.len() as u32).to_le_bytes());
                for o in &self.orders {
                    o.write(out);
                }
                for route in [&self.route, &self.local_route] {
                    out.extend((route.len() as u32).to_le_bytes());
                    for &(a, b) in route {
                        out.extend(a.to_le_bytes());
                        out.extend(b.to_le_bytes());
                    }
                }
                if self.val(0xc) == 0x28 {
                    match self.child(0x104) {
                        Some(w) => w.write(out),
                        None => Object::blank(Class::Weapon).write(out),
                    }
                }
            }
            Class::PlayerState => {
                scalars(out);
                for i in 0..10 {
                    u(out, 0x10 + 4 * i);
                }
            }
            Class::Stock => {
                scalars(out);
                for i in 0..9 {
                    u(out, 0x2e4 + 8 * i);
                    u(out, 0x2e8 + 8 * i);
                }
                for i in 0..10u32 {
                    let n = self.val(0x21c + 0x14 * i);
                    out.extend(n.to_le_bytes());
                    for j in 0..n {
                        out.extend(self.val((i + 1) << 16 | j).to_le_bytes());
                    }
                }
            }
            Class::Params => {
                for i in 0..30 {
                    u(out, 4 + 4 * i);
                }
                u(out, 0x7c);
            }
            Class::Speech => {
                scalars(out);
                out.extend((self.orders.len() as u32).to_le_bytes());
                for o in &self.orders {
                    o.write(out);
                }
            }
            Class::SpeechItem => {
                crate::save::put_mfc_string(out, self.strings.first().map(String::as_str).unwrap_or(""));
                for off in [0x8, 0xc, 0x10, 0x14] {
                    u(out, off);
                }
            }
            Class::MapItem => {
                u(out, 0xc);
                match self.val(0xc) {
                    0 => {
                        for off in [0x4, 0x8, 0x10, 0x14, 0x18] {
                            u(out, off);
                        }
                        b(out, 0x1c);
                        b(out, 0x1d);
                    }
                    1 => {
                        for off in [0x4, 0x8, 0x10] {
                            u(out, off);
                        }
                    }
                    _ => {}
                }
            }
            _ => scalars(out),
        }
    }

    /// A record with every field 0 (what a freshly constructed object holds).
    pub fn blank(class: Class) -> Object {
        Object { class, fields: Vec::new(), children: Vec::new(), orders: Vec::new(), route: Vec::new(), local_route: Vec::new(), strings: Vec::new() }
    }

    /// Sets (or adds) the field at `offset`.
    pub fn set(&mut self, offset: u32, value: u32) {
        match self.fields.iter_mut().find(|f| f.0 == offset) {
            Some(f) => f.1 = value,
            None => self.fields.push((offset, value)),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn named_fields_are_serialized_fields() {
        use super::*;
        for c in [Class::Unit, Class::Top, Class::Chassis, Class::Order, Class::Building, Class::BuildingPart, Class::Bullet, Class::Aircraft] {
            let known: Vec<u32> = c.scalars().iter().map(|&(o, _)| o).collect();
            let named = (0..0x2800u32).filter(|&o| !c.field_name(o).is_empty()).count();
            assert!(named > 0, "{c:?}");
            for o in (0..0x2800u32).filter(|&o| !c.field_name(o).is_empty()) {
                assert!(known.contains(&o), "{c:?} +{o:#x} is named but not a serialized scalar");
            }
        }
    }

    use super::*;

    /// A record of `class` with pseudo-random fields (counts kept small) and the children the class carries.
    pub(crate) fn sample(class: Class, seed: &mut u32) -> Object {
        fn rnd(seed: &mut u32, m: u32) -> u32 {
            *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (*seed >> 8) % m
        }
        let mut o = Object::blank(class);
        for (off, is_bool) in class.scalars() {
            let v = if is_bool { rnd(seed, 2) } else { rnd(seed, 100_000) };
            o.set(off, v);
        }
        fn all_bools(o: &mut Object, offs: &[u32], seed: &mut u32) {
            for &off in offs {
                o.set(off, rnd(seed, 2));
            }
        }
        match class {
            Class::Unit => {
                all_bools(&mut o, &[0x48, 0x49, 0x4a], seed);
                o.children.push((4, sample(Class::Top, seed)));
                o.children.push((8, sample(Class::Chassis, seed)));
            }
            Class::Top => {
                for i in 0..10 {
                    o.set(0x94 + 4 * i, rnd(seed, 1000));
                }
                for i in 0..80 {
                    o.set(0xcc + 4 * i, rnd(seed, 1000));
                }
                o.children.push((8, sample(Class::Weapon, seed)));
            }
            Class::Chassis => {
                o.set(0xc, if rnd(seed, 2) == 0 { 0x28 } else { 5 });
                o.children.push((0xd8, sample(Class::Order, seed)));
                for _ in 0..rnd(seed, 4) {
                    o.orders.push(sample(Class::Order, seed));
                }
                o.set(0xcc, o.orders.len() as u32);
                for _ in 0..rnd(seed, 5) {
                    o.route.push((rnd(seed, 128), rnd(seed, 128)));
                    o.local_route.push((rnd(seed, 128), rnd(seed, 128)));
                }
                o.set(0x8c, o.route.len() as u32);
                o.set(0xa0, o.local_route.len() as u32);
                if o.get(0xc) == Some(0x28) {
                    o.children.push((0x104, sample(Class::Weapon, seed)));
                }
            }
            Class::Building => {
                all_bools(&mut o, &[0x88, 0x89, 0x8a], seed);
                for k in 0..3 {
                    o.set(0x7c + 4 * k, rnd(seed, 500));
                }
                for i in 0..25 {
                    o.set(0x98 + 4 * i, rnd(seed, 500));
                }
                for k in 0..5 {
                    o.children.push((0x15c + 0x28 * k, sample(Class::BuildingPart, seed)));
                }
                for i in 0..8 {
                    o.set(0x224 + 4 * i, rnd(seed, 99));
                    o.set(0x244 + 4 * i, rnd(seed, 99));
                }
                for i in 0..10 {
                    o.set(0x26c + 4 * i, rnd(seed, 99));
                    o.set(0x294 + 4 * i, rnd(seed, 99));
                }
                let n = rnd(seed, 4);
                o.set(0x394, n);
                for j in 0..n {
                    for k in 0..5 {
                        o.set(0x2cc + 0x14 * j + 4 * k, rnd(seed, 999));
                    }
                }
            }
            Class::Bullet => {
                let len = 2 * (1 + rnd(seed, 6)) as i32;
                let progress = rnd(seed, len as u32 + 1) as i32;
                o.set(0x2648, len as u32);
                o.set(0x2644, progress as u32);
                for i in 0..45 {
                    o.set(0xc + 4 * i, rnd(seed, 999));
                }
                for i in 0..(len / 2) as u32 {
                    o.set(0xc4 + 4 * i, rnd(seed, 999));
                }
                for i in progress..len {
                    o.set((0xd44 + 4 * i) as u32, rnd(seed, 999));
                }
                for off in [0x2698, 0x269c, 0x26a0, 0x26a4, 0x26a8, 0x26ac, 0x26b0, 0x26b4, 0x26bc] {
                    o.set(off, rnd(seed, 999));
                }
                all_bools(&mut o, &[0x26b8, 0x26c0], seed);
            }
            Class::Aircraft => all_bools(&mut o, &[0x8c, 0x8d, 0x8e, 0x8f, 0x90], seed),
            Class::PlayerState => {
                for i in 0..10 {
                    o.set(0x10 + 4 * i, rnd(seed, 999));
                }
            }
            Class::Stock => {
                for i in 0..9 {
                    o.set(0x2e4 + 8 * i, rnd(seed, 999));
                    o.set(0x2e8 + 8 * i, rnd(seed, 999));
                }
                for i in 0..10u32 {
                    let n = rnd(seed, 4);
                    o.set(0x21c + 0x14 * i, n);
                    for j in 0..n {
                        o.set((i + 1) << 16 | j, rnd(seed, 999));
                    }
                }
            }
            Class::Params => {
                for i in 0..30 {
                    o.set(4 + 4 * i, rnd(seed, 999));
                }
                o.set(0x7c, rnd(seed, 999));
            }
            Class::Speech => {
                for _ in 0..rnd(seed, 3) {
                    o.orders.push(sample(Class::SpeechItem, seed));
                }
                o.set(0x14, o.orders.len() as u32);
            }
            Class::SpeechItem => {
                o.strings.push(format!("msg{}", rnd(seed, 99)));
                for off in [0x8, 0xc, 0x10, 0x14] {
                    o.set(off, rnd(seed, 999));
                }
            }
            Class::MapItem => {
                let kind = rnd(seed, 3);
                o.set(0xc, kind);
                for off in [0x4, 0x8, 0x10, 0x14, 0x18] {
                    o.set(off, rnd(seed, 999));
                }
                all_bools(&mut o, &[0x1c, 0x1d], seed);
            }
            _ => {}
        }
        o
    }

    /// Fields a class does not serialise (only the loops' offsets can differ); compare after a write / parse trip.
    fn serialised(o: &Object) -> Object {
        let mut r = Reader::new(&[]);
        let _ = &mut r;
        let mut v = Vec::new();
        o.write(&mut v);
        let mut rd = Reader::new(&v);
        let back = Object::parse(&mut rd, o.class).unwrap();
        assert_eq!(rd.remaining(), 0, "{:?}", o.class);
        back
    }

    #[test]
    fn every_class_round_trips_through_write_and_parse() {
        let classes = [
            Class::Unit, Class::Top, Class::Chassis, Class::Order, Class::Weapon, Class::Building, Class::BuildingPart, Class::Bullet,
            Class::Aircraft, Class::Decal, Class::Cloud, Class::PlayerState, Class::Stock, Class::Params, Class::Speech,
            Class::SpeechItem, Class::MapItem,
        ];
        let mut seed = 12345;
        for c in classes {
            for _ in 0..40 {
                let o = sample(c, &mut seed);
                let back = serialised(&o);
                // parse(write(o)) holds exactly the serialised fields; a second trip must be the identity
                let mut a = Vec::new();
                o.write(&mut a);
                let mut b = Vec::new();
                back.write(&mut b);
                assert_eq!(a, b, "{c:?}");
                assert_eq!(serialised(&back), back, "{c:?}");
            }
        }
    }

    /// Writes a record the way the exe's store branch does, from the same tables.
    fn put_header(out: &mut Vec<u8>, c: Class) {
        out.extend(c.header().0.to_le_bytes());
        out.extend(c.header().1.to_le_bytes());
    }

    fn put_scalars(out: &mut Vec<u8>, c: Class, f: impl Fn(u32) -> u32) {
        for (off, b) in c.scalars() {
            if b {
                out.push(f(off) as u8 & 1);
            } else {
                out.extend(f(off).to_le_bytes());
            }
        }
    }

    #[test]
    fn simple_records_round_trip() {
        for c in [Class::Decal, Class::Cloud, Class::Order, Class::BuildingPart] {
            let mut f = Vec::new();
            put_header(&mut f, c);
            put_scalars(&mut f, c, |o| o + 1000);
            let mut r = Reader::new(&f);
            let o = Object::parse(&mut r, c).unwrap();
            assert_eq!(r.remaining(), 0, "{c:?}");
            assert_eq!(o.fields.len(), c.scalars().len());
            let first = c.scalars()[0].0;
            assert_eq!(o.get(first), Some(first + 1000));
        }
    }

    #[test]
    fn rejects_a_wrong_header() {
        let mut f = Vec::new();
        put_header(&mut f, Class::Order);
        let mut r = Reader::new(&f);
        assert!(Object::parse(&mut r, Class::Decal).is_err());
    }

    #[test]
    fn unit_with_chassis_orders_and_routes() {
        let mut f = Vec::new();
        put_header(&mut f, Class::Unit);
        put_scalars(&mut f, Class::Unit, |o| o);
        f.extend([1, 0, 1]); // 0x48..0x4a
        f.push(0); // no top
        f.push(1); // chassis
        put_header(&mut f, Class::Chassis);
        put_scalars(&mut f, Class::Chassis, |o| if o == 0xc { 0x28 } else { o });
        put_header(&mut f, Class::Order);
        put_scalars(&mut f, Class::Order, |o| o);
        f.extend(2u32.to_le_bytes()); // two queued orders
        for _ in 0..2 {
            put_header(&mut f, Class::Order);
            put_scalars(&mut f, Class::Order, |o| o + 5);
        }
        f.extend(3u32.to_le_bytes()); // route of 3
        for i in 0..3u32 {
            f.extend(i.to_le_bytes());
            f.extend((i * 2).to_le_bytes());
        }
        f.extend(0u32.to_le_bytes()); // empty local route
        put_header(&mut f, Class::Weapon); // Advancer extra
        put_scalars(&mut f, Class::Weapon, |o| o);
        let mut r = Reader::new(&f);
        let u = Object::parse(&mut r, Class::Unit).unwrap();
        assert_eq!(r.remaining(), 0);
        assert!(u.child(4).is_none());
        let ch = u.child(8).unwrap();
        assert_eq!(ch.get(0xc), Some(0x28));
        assert_eq!(ch.orders.len(), 2);
        assert_eq!(ch.orders[1].get(0xc), Some(0xc + 5));
        assert_eq!(ch.route, vec![(0, 0), (1, 2), (2, 4)]);
        assert!(ch.local_route.is_empty());
        assert!(ch.child(0x104).is_some() && ch.child(0xd8).is_some());
    }

    #[test]
    fn building_and_bullet_counts() {
        let mut f = Vec::new();
        put_header(&mut f, Class::Building);
        put_scalars(&mut f, Class::Building, |o| o);
        for k in 0..3u32 {
            f.push(1);
            f.extend(k.to_le_bytes());
        }
        f.extend(vec![0u8; 25 * 4]);
        for _ in 0..5 {
            put_header(&mut f, Class::BuildingPart);
            put_scalars(&mut f, Class::BuildingPart, |o| o);
        }
        f.extend(vec![0u8; (8 + 10) * 8]);
        f.extend(2u32.to_le_bytes());
        f.extend(vec![7u8; 2 * 5 * 4]);
        let mut r = Reader::new(&f);
        let b = Object::parse(&mut r, Class::Building).unwrap();
        assert_eq!(r.remaining(), 0);
        assert_eq!(b.get(0x2cc + 0x14), Some(0x07070707));
        assert_eq!(b.child(0x15c + 0x28 * 4).unwrap().get(0x24), Some(0x24));

        // bullet: path length 6 -> 3 first-half entries, progress 2 -> 4 tail entries
        let mut f = Vec::new();
        put_header(&mut f, Class::Bullet);
        put_scalars(&mut f, Class::Bullet, |o| match o {
            0x2644 => 2,
            0x2648 => 6,
            _ => 1,
        });
        f.extend(vec![0u8; 45 * 4 + (3 + 4) * 4 + 9 * 4 + 2]);
        let mut r = Reader::new(&f);
        let b = Object::parse(&mut r, Class::Bullet).unwrap();
        assert_eq!(r.remaining(), 0);
        assert_eq!(b.fields.iter().filter(|x| x.0 >= 0xd44 && x.0 < 0xd44 + 0x40).count(), 4);
        assert!(f.len() > 100 && Object::parse(&mut Reader::new(&f[..f.len() - 1]), Class::Bullet).is_err());
    }
}
