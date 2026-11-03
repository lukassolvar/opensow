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

#[cfg(test)]
mod tests {
    use super::*;

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
        f.extend(vec![0u8; 30 * 4 + (3 + 4) * 4 + 9 * 4 + 2]);
        let mut r = Reader::new(&f);
        let b = Object::parse(&mut r, Class::Bullet).unwrap();
        assert_eq!(r.remaining(), 0);
        assert_eq!(b.fields.iter().filter(|x| x.0 >= 0xd44 && x.0 < 0xd44 + 0x40).count(), 4);
        assert!(f.len() > 100 && Object::parse(&mut Reader::new(&f[..f.len() - 1]), Class::Bullet).is_err());
    }
}
