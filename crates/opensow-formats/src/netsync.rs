//! State-sync records of the multiplayer protocol (docs/network.md "State sync"): the compact bit records `0x40a2d0`
//! (building), `0x4128f0` (bullet), `0x4688d0` (air squad) and `0x46c5c0` (unit) write through `0x4528f0(value, n)` and the readers
//! `0x40a160`, `0x412620`, `0x468790`, `0x46c300` take back. Fields are raw words as the exe holds them; `n` bits least
//! significant first, higher bits of a value are dropped. `scripts/verify_net.py` runs the exe's writers on random objects and
//! compares them with these.

use crate::net::{BitWriter, Bits};
use crate::Result;

/// A building (`0x40a2d0`): kind `[+0x94]` 7 bits, `[+0x90]` 32 bits, cell x `[+4]` and y `[+8]` 8 bits, owner `[+0x8c]` 2 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingSync {
    /// `[+0x94]`, 7 bits: the building's kind.
    pub kind: u32,
    /// `[+0x90]`, 32 bits, kept raw.
    pub word90: u32,
    /// `[+4]`, `[+8]`, 8 bits each: the building's cell.
    pub cell: (u32, u32),
    /// `[+0x8c]`, 2 bits: the owner.
    pub owner: u32,
}

impl BuildingSync {
    /// Encodes the record as the exe's writer does.
    pub fn write(&self) -> BitWriter {
        let mut w = BitWriter::default();
        w.lsb(self.kind, 7);
        w.lsb(self.word90, 32);
        w.lsb(self.cell.0, 8);
        w.lsb(self.cell.1, 8);
        w.lsb(self.owner, 2);
        w
    }

    /// Decodes one record from the bit stream.
    pub fn parse(r: &mut Bits) -> Result<BuildingSync> {
        Ok(BuildingSync { kind: r.lsb(7)?, word90: r.lsb(32)?, cell: (r.lsb(8)?, r.lsb(8)?), owner: r.lsb(2)? })
    }
}

/// What follows a bullet's fixed fields, by type (`0x4129a7`: byte table `0x412a48` indexed by `type - 1` into the jump table
/// `0x412a30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletExtra {
    /// Nothing follows (every type not listed below).
    None,
    /// `[+0x2688]`, 32 bits: the target handle (types 1, 10, 19, 20, 21, 31).
    Handle(u32),
    /// `[+0x26ac]`, 5 bits: the start delay (type 2).
    Delay(u32),
    /// `[+0x2664]`, 8 bits: the speed (types 13, 14).
    Speed(u32),
    /// `[+0x2680]`, 7 bits: the variant (types 15, 16).
    Variant(u32),
    /// `[+0x268c]`, 2 bits: the second variant (type 18).
    Variant2(u32),
}

/// Which extra a bullet type carries: 0 handle, 1 delay, 2 speed, 3 variant, 4 variant 2, 5 none (the exe's byte table).
pub fn bullet_extra_index(ty: u32) -> u32 {
    const TABLE: [u8; 31] = [0, 1, 5, 5, 5, 5, 5, 5, 5, 0, 5, 5, 2, 2, 3, 3, 5, 4, 0, 0, 0, 5, 5, 5, 5, 5, 5, 5, 5, 5, 0];
    if (1..=31).contains(&ty) {
        TABLE[ty as usize - 1] as u32
    } else {
        5
    }
}

/// A bullet (`0x4128f0`, once per bullet: the guard `[+0x26bc] > 0` makes a second call return false).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletSync {
    /// `[+0x2658]`, 6 bits.
    pub ty: u32,
    /// `[+0x266c]`, 2 bits.
    pub owner: u32,
    /// `[+0x269c]`, `[+0x26a0]`, `[+0x26a4]`, `[+0x26a8]`, 12 bits each: the constructor's start and end coordinates.
    pub points: [u32; 4],
    /// `[+0x2698]`, 16 bits.
    pub word2698: u32,
    /// `[+0x2660]`, 16 bits: the strength.
    pub strength: u32,
    /// `[+0x26b0]`, 32 bits.
    pub word26b0: u32,
    /// `[+0x26b4]`, 6 bits.
    pub word26b4: u32,
    /// The type-dependent field after the fixed ones.
    pub extra: BulletExtra,
    /// Types 19 to 21 only (`[+0x26c0] != 0`): one more bit.
    pub flag: Option<bool>,
}

impl BulletSync {
    /// Encodes the record as the exe's writer does.
    pub fn write(&self) -> BitWriter {
        let mut w = BitWriter::default();
        w.lsb(self.ty, 6);
        w.lsb(self.owner, 2);
        for p in self.points {
            w.lsb(p, 12);
        }
        w.lsb(self.word2698, 16);
        w.lsb(self.strength, 16);
        w.lsb(self.word26b0, 32);
        w.lsb(self.word26b4, 6);
        match self.extra {
            BulletExtra::None => {}
            BulletExtra::Handle(v) => w.lsb(v, 32),
            BulletExtra::Delay(v) => w.lsb(v, 5),
            BulletExtra::Speed(v) => w.lsb(v, 8),
            BulletExtra::Variant(v) => w.lsb(v, 7),
            BulletExtra::Variant2(v) => w.lsb(v, 2),
        }
        if let Some(f) = self.flag {
            w.bit(f);
        }
        w
    }

    /// Decodes one record from the bit stream.
    pub fn parse(r: &mut Bits) -> Result<BulletSync> {
        let ty = r.lsb(6)?;
        let owner = r.lsb(2)?;
        let points = [r.lsb(12)?, r.lsb(12)?, r.lsb(12)?, r.lsb(12)?];
        let (word2698, strength, word26b0, word26b4) = (r.lsb(16)?, r.lsb(16)?, r.lsb(32)?, r.lsb(6)?);
        let extra = match bullet_extra_index(ty) {
            0 => BulletExtra::Handle(r.lsb(32)?),
            1 => BulletExtra::Delay(r.lsb(5)?),
            2 => BulletExtra::Speed(r.lsb(8)?),
            3 => BulletExtra::Variant(r.lsb(7)?),
            4 => BulletExtra::Variant2(r.lsb(2)?),
            _ => BulletExtra::None,
        };
        let flag = if (19..=21).contains(&ty) { Some(r.bit()?) } else { None };
        Ok(BulletSync { ty, owner, points, word2698, strength, word26b0, word26b4, extra, flag })
    }
}

/// An air squad / aircraft (`0x4688d0`): code `[+0x28]` 8 bits, `[+0x1c]`, `[+0xc]`, `[+0x10]`, `[+0x98]`, `[+0x9c]` 16 bits each,
/// `[+0x20]` 2 bits, `[+0xa0]` 32 bits, `[+0xa4]` 6 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirSync {
    /// `[+0x28]`, 8 bits: the air force code.
    pub code: u32,
    /// `[+0x1c]`, 16 bits: the aircraft's handle.
    pub handle: u32,
    /// `[+0xc]`, `[+0x10]`, 16 bits each: the target point.
    pub target: (u32, u32),
    /// `[+0x98]`, 16 bits, kept raw.
    pub word98: u32,
    /// `[+0x9c]`, 16 bits, kept raw.
    pub word9c: u32,
    /// `[+0x20]`, 2 bits, kept raw.
    pub word20: u32,
    /// `[+0xa0]`, 32 bits, kept raw.
    pub worda0: u32,
    /// `[+0xa4]`, 6 bits, kept raw.
    pub worda4: u32,
}

impl AirSync {
    /// Encodes the record as the exe's writer does.
    pub fn write(&self) -> BitWriter {
        let mut w = BitWriter::default();
        w.lsb(self.code, 8);
        w.lsb(self.handle, 16);
        w.lsb(self.target.0, 16);
        w.lsb(self.target.1, 16);
        w.lsb(self.word98, 16);
        w.lsb(self.word9c, 16);
        w.lsb(self.word20, 2);
        w.lsb(self.worda0, 32);
        w.lsb(self.worda4, 6);
        w
    }

    /// Decodes one record from the bit stream.
    pub fn parse(r: &mut Bits) -> Result<AirSync> {
        Ok(AirSync {
            code: r.lsb(8)?,
            handle: r.lsb(16)?,
            target: (r.lsb(16)?, r.lsb(16)?),
            word98: r.lsb(16)?,
            word9c: r.lsb(16)?,
            word20: r.lsb(2)?,
            worda0: r.lsb(32)?,
            worda4: r.lsb(6)?,
        })
    }
}

/// The chassis part of a unit record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChassisSync {
    /// Four 2-bit relations (0 equal, 1 less, 2 greater) of the chassis cell words `[+0x64] / [+0x6c]`, `[+0x68] / [+0x70]`,
    /// `[+0x6c] / [+0x74]`, `[+0x70] / [+0x78]`: the direction of the next steps of the route.
    pub steps: [u32; 4],
    /// `[+0x5c] >> 5`, `[+0x60] >> 5` (7 bits each): the goal cell.
    pub goal: (u32, u32),
    /// `[+0x168]` 32 bits and `[+0x16c]` 6 bits (the order words, docs/walking.md).
    pub words: (u32, u32),
    /// `[+0xe0] == 2` (the Advancer's beam approach).
    pub approaching: bool,
}

/// A unit (`0x46c5c0`): kind `[+0x40]` 16 bits, handle `[+0x3c]` 32, player `[+0x44]` 2, then the cell (the chassis' `[+0x64]`,
/// `[+0x68]`, or the position `>> 5` without a chassis) and `[+0xc]`, `[+0x10]` 7 bits each, the chassis part, the top part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSync {
    /// `[+0x40]`, 16 bits: the unit kind.
    pub kind: u32,
    /// `[+0x3c]`, 32 bits: the unit's handle.
    pub handle: u32,
    /// `[+0x44]`, 2 bits: the player.
    pub player: u32,
    /// The unit's cell, 7 bits each (from the chassis, or the position `>> 5` without one).
    pub cell: (u32, u32),
    /// `[+0xc]`, `[+0x10]`, 7 bits each.
    pub home: (u32, u32),
    /// `None` = no chassis: a single 0 bit. `Some`: a 1 bit, the fields, then the `approaching` bit.
    pub chassis: Option<ChassisSync>,
    /// The top: presence bit, `[top+0x70]` (the facing) 5 bits, and for the turret tops 109 to 113 `[unit+0x68]` as 32 bits.
    pub top: Option<(u32, Option<u32>)>,
}

/// The unit kinds whose record carries `[unit+0x68]` (the turret tops).
pub fn is_turret_top(kind: u32) -> bool {
    (0x6d..=0x71).contains(&kind)
}

impl UnitSync {
    /// Encodes the record as the exe's writer does.
    pub fn write(&self) -> BitWriter {
        let mut w = BitWriter::default();
        w.lsb(self.kind, 16);
        w.lsb(self.handle, 32);
        w.lsb(self.player, 2);
        w.lsb(self.cell.0, 7);
        w.lsb(self.cell.1, 7);
        w.lsb(self.home.0, 7);
        w.lsb(self.home.1, 7);
        match &self.chassis {
            Some(c) => {
                w.lsb(1, 1);
                for s in c.steps {
                    w.lsb(s, 2);
                }
                w.lsb(c.goal.0, 7);
                w.lsb(c.goal.1, 7);
                w.lsb(c.words.0, 32);
                w.lsb(c.words.1, 6);
                w.bit(c.approaching);
            }
            None => w.bit(false),
        }
        match self.top {
            Some((facing, word)) => {
                w.lsb(1, 1);
                w.lsb(facing, 5);
                if is_turret_top(self.kind) {
                    w.lsb(word.unwrap_or(0), 32);
                }
            }
            None => w.lsb(0, 1),
        }
        w
    }

    /// Decodes one record from the bit stream.
    pub fn parse(r: &mut Bits) -> Result<UnitSync> {
        let (kind, handle, player) = (r.lsb(16)?, r.lsb(32)?, r.lsb(2)?);
        let cell = (r.lsb(7)?, r.lsb(7)?);
        let home = (r.lsb(7)?, r.lsb(7)?);
        let chassis = if r.bit()? {
            let steps = [r.lsb(2)?, r.lsb(2)?, r.lsb(2)?, r.lsb(2)?];
            let goal = (r.lsb(7)?, r.lsb(7)?);
            let words = (r.lsb(32)?, r.lsb(6)?);
            Some(ChassisSync { steps, goal, words, approaching: r.bit()? })
        } else {
            None
        };
        let top = if r.bit()? {
            let facing = r.lsb(5)?;
            Some((facing, if is_turret_top(kind) { Some(r.lsb(32)?) } else { None }))
        } else {
            None
        };
        Ok(UnitSync { kind, handle, player, cell, home, chassis, top })
    }
}

/// The ten lockstep command queues a client sends to the host every frame (`0x4296a0`, branch `[0x575720] > 0 && [0x575730] != 0`,
/// docs/network.md "Frame messages"). Each queue is a run of `1` + fields items closed by a `0` bit; a frame with every queue empty
/// sends nothing. Handles are the 32-bit unit handles of docs/projectiles.md; cells are 7 bits (pixels are divided by 32,
/// truncating towards zero, for the release point).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commands {
    /// PlaceSentry: x 7, y 7, turret tier 3, word 12 bits.
    pub place_sentry: Vec<(u32, u32, u32, u32)>,
    /// ReleasePoint: x 7, y 7 (pixel / 32), word 32.
    pub release_point: Vec<(u32, u32, u32)>,
    /// UnitGoto: target cell x 7, y 7, then the 9-bit count and that many 32-bit unit handles.
    pub unit_goto: Vec<(u32, u32, Vec<u32>)>,
    /// UnitAttack: target handle 32, 9-bit count, handles.
    pub unit_attack: Vec<(u32, Vec<u32>)>,
    /// UnitScatter: 9-bit count, handles.
    pub unit_scatter: Vec<Vec<u32>>,
    /// HDestroy, HRepair, HUpgrade (the Advancer's building orders): Advancer handle 32, building handle 32.
    pub destroy: Vec<(u32, u32)>,
    /// HRepair: Advancer handle 32, building handle 32.
    pub repair: Vec<(u32, u32)>,
    /// HUpgrade: Advancer handle 32, building handle 32.
    pub upgrade: Vec<(u32, u32)>,
    /// UfoStrike: x 14, y 14, force code 8 bits.
    pub ufo_strike: Vec<(u32, u32, u32)>,
    /// TechLevelUp: building handle 32, slot 3.
    pub tech_up: Vec<(u32, u32)>,
}

impl Commands {
    /// Whether every queue is empty (such a frame sends nothing).
    pub fn is_empty(&self) -> bool {
        *self == Commands::default()
    }

    /// The payload bits (after the header, [`crate::net::PAYLOAD_BIT`]).
    pub fn write_payload(&self, w: &mut BitWriter) {
        for &(x, y, t, id) in &self.place_sentry {
            w.lsb(1, 1);
            w.lsb(x, 7);
            w.lsb(y, 7);
            w.lsb(t, 3);
            w.lsb(id, 12);
        }
        w.lsb(0, 1);
        for &(x, y, v) in &self.release_point {
            w.lsb(1, 1);
            w.lsb(x, 7);
            w.lsb(y, 7);
            w.lsb(v, 32);
        }
        w.lsb(0, 1);
        for (x, y, hs) in &self.unit_goto {
            w.lsb(1, 1);
            w.lsb(*x, 7);
            w.lsb(*y, 7);
            w.lsb(hs.len() as u32, 9);
            for &h in hs {
                w.lsb(h, 32);
            }
        }
        w.lsb(0, 1);
        for (t, hs) in &self.unit_attack {
            w.lsb(1, 1);
            w.lsb(*t, 32);
            w.lsb(hs.len() as u32, 9);
            for &h in hs {
                w.lsb(h, 32);
            }
        }
        w.lsb(0, 1);
        for hs in &self.unit_scatter {
            w.lsb(1, 1);
            w.lsb(hs.len() as u32, 9);
            for &h in hs {
                w.lsb(h, 32);
            }
        }
        w.lsb(0, 1);
        for q in [&self.destroy, &self.repair, &self.upgrade] {
            for &(a, b) in q {
                w.lsb(1, 1);
                w.lsb(a, 32);
                w.lsb(b, 32);
            }
            w.lsb(0, 1);
        }
        for &(x, y, c) in &self.ufo_strike {
            w.lsb(1, 1);
            w.lsb(x, 14);
            w.lsb(y, 14);
            w.lsb(c, 8);
        }
        w.lsb(0, 1);
        for &(h, slot) in &self.tech_up {
            w.lsb(1, 1);
            w.lsb(h, 32);
            w.lsb(slot, 3);
        }
        w.lsb(0, 1);
    }

    /// Decodes the ten queues from the bit stream, in the order of the fields above.
    pub fn parse_payload(r: &mut Bits) -> Result<Commands> {
        let mut c = Commands::default();
        let handles = |r: &mut Bits| -> Result<Vec<u32>> {
            let n = r.lsb(9)?;
            (0..n).map(|_| r.lsb(32)).collect()
        };
        while r.bit()? {
            c.place_sentry.push((r.lsb(7)?, r.lsb(7)?, r.lsb(3)?, r.lsb(12)?));
        }
        while r.bit()? {
            c.release_point.push((r.lsb(7)?, r.lsb(7)?, r.lsb(32)?));
        }
        while r.bit()? {
            let (x, y) = (r.lsb(7)?, r.lsb(7)?);
            c.unit_goto.push((x, y, handles(r)?));
        }
        while r.bit()? {
            let t = r.lsb(32)?;
            c.unit_attack.push((t, handles(r)?));
        }
        while r.bit()? {
            c.unit_scatter.push(handles(r)?);
        }
        for q in [&mut c.destroy, &mut c.repair, &mut c.upgrade] {
            while r.bit()? {
                q.push((r.lsb(32)?, r.lsb(32)?));
            }
        }
        while r.bit()? {
            c.ufo_strike.push((r.lsb(14)?, r.lsb(14)?, r.lsb(8)?));
        }
        while r.bit()? {
            c.tech_up.push((r.lsb(32)?, r.lsb(3)?));
        }
        Ok(c)
    }

    /// A whole frame message from `player`: the header (type bit 0 set by `0x450de0`, selector 7 = player) and the payload.
    pub fn message(&self, player: u32) -> BitWriter {
        let mut h = crate::net::Header { fields: [1, 0, 0, 0, 0, 0, 0, player] };
        h.fields[0] = 1;
        let mut w = h.write();
        self.write_payload(&mut w);
        w
    }
}

/// The host's per-frame state message (`0x4296a0`, branch player 0 with remote players): `0x450b70(clock a, clock b)` puts the game
/// clock `[0x70ef94]` into the 18-bit header field 1 and `[0x70ef98]` into the 6-bit field 2, `0x4527c0(7, player)`, then every
/// living building (`0x40a2d0`, count in field 4, 9 bits), every unit with `[+0x2c] == 0` (`0x46c5c0`, field 3, 12 bits), the
/// aircraft (`0x4688d0`, field 6, 6 bits) and the bullets not yet sent except types 22, 32 and 33 (`0x4128f0`, field 5, 9 bits).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The game clock: `[0x70ef94]` (18 bits) and `[0x70ef98]` (6 bits).
    pub clock: (u32, u32),
    /// The player the message is about.
    pub player: u32,
    /// Every living building.
    pub buildings: Vec<BuildingSync>,
    /// Every unit with `[+0x2c] == 0`.
    pub units: Vec<UnitSync>,
    /// The aircraft.
    pub aircraft: Vec<AirSync>,
    /// The bullets not sent before (types 22, 32 and 33 are never sent).
    pub bullets: Vec<BulletSync>,
}

impl Snapshot {
    /// Encodes the whole message: header, then the four record lists.
    pub fn write(&self) -> BitWriter {
        let h = crate::net::Header {
            fields: [
                0,
                self.clock.0,
                self.clock.1,
                self.units.len() as u32,
                self.buildings.len() as u32,
                self.bullets.len() as u32,
                self.aircraft.len() as u32,
                self.player,
            ],
        };
        let mut w = h.write();
        for b in &self.buildings {
            let r = b.write();
            for i in 0..r.len {
                w.bit(r.data[i >> 3] >> (i & 7) & 1 == 1);
            }
        }
        macro_rules! append {
            ($v:expr) => {
                for x in $v {
                    let r = x.write();
                    for i in 0..r.len {
                        w.bit(r.data[i >> 3] >> (i & 7) & 1 == 1);
                    }
                }
            };
        }
        append!(&self.units);
        append!(&self.aircraft);
        append!(&self.bullets);
        w
    }

    /// Decodes a message of `len` bytes.
    pub fn parse(data: &[u8], len: usize) -> Result<Snapshot> {
        let h = crate::net::Header::parse(data, len)?;
        let mut r = Bits::new(data, len);
        r.pos = crate::net::PAYLOAD_BIT as usize;
        let mut s = Snapshot { clock: (h.fields[1], h.fields[2]), player: h.fields[7], ..Default::default() };
        for _ in 0..h.fields[4] {
            s.buildings.push(BuildingSync::parse(&mut r)?);
        }
        for _ in 0..h.fields[3] {
            s.units.push(UnitSync::parse(&mut r)?);
        }
        for _ in 0..h.fields[6] {
            s.aircraft.push(AirSync::parse(&mut r)?);
        }
        for _ in 0..h.fields[5] {
            s.bullets.push(BulletSync::parse(&mut r)?);
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round<T: PartialEq + std::fmt::Debug>(w: BitWriter, parse: impl Fn(&mut Bits) -> Result<T>, want: T) {
        let mut r = Bits::new(&w.data, w.len);
        assert_eq!(parse(&mut r).unwrap(), want);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn building_and_air_round_trip() {
        let b = BuildingSync { kind: 102, word90: 0xdead_beef, cell: (43, 55), owner: 1 };
        assert_eq!(b.write().len, 57);
        round(b.write(), BuildingSync::parse, b);
        let a = AirSync { code: 31, handle: 0x1234, target: (500, 600), word98: 7, word9c: 9, word20: 2, worda0: 0xffff_0000, worda4: 33 };
        assert_eq!(a.write().len, 8 + 16 * 5 + 2 + 32 + 6);
        round(a.write(), AirSync::parse, a);
    }

    #[test]
    fn bullets_carry_the_extra_their_type_selects() {
        let base = |ty, extra, flag| BulletSync { ty, owner: 1, points: [1, 2, 3, 4], word2698: 5, strength: 6, word26b0: 7, word26b4: 8, extra, flag };
        let fixed = 6 + 2 + 48 + 16 + 16 + 32 + 6;
        for (ty, extra, flag, bits) in [
            (1, BulletExtra::Handle(0xabcd), None, 32),
            (2, BulletExtra::Delay(5), None, 5),
            (6, BulletExtra::None, None, 0),
            (13, BulletExtra::Speed(99), None, 8),
            (16, BulletExtra::Variant(14), None, 7),
            (18, BulletExtra::Variant2(1), None, 2),
            (20, BulletExtra::Handle(1), Some(true), 33),
            (31, BulletExtra::Handle(2), None, 32),
        ] {
            let b = base(ty, extra, flag);
            assert_eq!(b.write().len, fixed + bits, "type {ty}");
            round(b.write(), BulletSync::parse, b);
        }
    }

    #[test]
    fn units_round_trip_with_and_without_parts() {
        let chassis = ChassisSync { steps: [0, 1, 2, 1], goal: (12, 99), words: (0x1234_5678, 33), approaching: true };
        for (kind, top) in [(7u32, Some((9, None))), (109, Some((31, Some(0xcafe)))), (46, None)] {
            let u = UnitSync { kind, handle: 0x0012_0345, player: 1, cell: (5, 6), home: (7, 8), chassis: Some(chassis), top };
            round(u.write(), UnitSync::parse, u);
        }
        let bare = UnitSync { kind: 40, handle: 1, player: 0, cell: (1, 2), home: (3, 4), chassis: None, top: None };
        assert_eq!(bare.write().len, 16 + 32 + 2 + 28 + 1 + 1);
        round(bare.write(), UnitSync::parse, bare);
    }

    #[test]
    fn command_queues_round_trip() {
        let c = Commands {
            place_sentry: vec![(5, 6, 3, 100)],
            release_point: vec![(1, 2, 0xdead)],
            unit_goto: vec![(10, 11, vec![1, 2, 3]), (0, 0, vec![])],
            unit_attack: vec![(0x77, vec![9])],
            unit_scatter: vec![vec![4, 5]],
            destroy: vec![(1, 2)],
            repair: vec![],
            upgrade: vec![(3, 4), (5, 6)],
            ufo_strike: vec![(9000, 100, 33)],
            tech_up: vec![(0x1234, 4)],
        };
        let w = c.message(1);
        let mut r = Bits::new(&w.data, w.len);
        r.pos = crate::net::PAYLOAD_BIT as usize;
        assert_eq!(Commands::parse_payload(&mut r).unwrap(), c);
        assert_eq!(r.remaining(), 0);
        assert!(Commands::default().is_empty() && !c.is_empty());
        // an empty frame is ten terminator bits
        let mut e = BitWriter::default();
        Commands::default().write_payload(&mut e);
        assert_eq!(e.len, 10);
    }

    #[test]
    fn snapshots_keep_counts_in_the_header() {
        let s = Snapshot {
            clock: (123_456, 17),
            player: 0,
            buildings: vec![BuildingSync { kind: 100, word90: 5, cell: (1, 2), owner: 0 }; 3],
            units: vec![UnitSync { kind: 7, handle: 9, player: 1, cell: (1, 1), home: (2, 2), chassis: None, top: None }],
            aircraft: vec![AirSync { code: 30, handle: 1, target: (2, 3), word98: 0, word9c: 0, word20: 0, worda0: 0, worda4: 0 }],
            bullets: vec![
                BulletSync {
                    ty: 6,
                    owner: 0,
                    points: [1, 2, 3, 4],
                    word2698: 0,
                    strength: 20,
                    word26b0: 0,
                    word26b4: 0,
                    extra: BulletExtra::None,
                    flag: None
                };
                2
            ],
        };
        let w = s.write();
        assert_eq!(Snapshot::parse(&w.data, w.len).unwrap(), s);
        let h = crate::net::Header::parse(&w.data, w.len).unwrap();
        assert_eq!((h.fields[3], h.fields[4], h.fields[5], h.fields[6]), (1, 3, 2, 1));
    }
}
