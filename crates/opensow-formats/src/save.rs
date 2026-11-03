//! Savegames (`savegame\savegame_N.data` / `.info`), decoded from `State of War.exe` (save `0x428bb0`, state serializer
//! `0x42a400`, info `0x42be20`, compressor `0x439240`, decompressor `0x439860`). Layout notes: docs/savegame.md.
//!
//! * `.info` is a CArchive stream: `u16 2, u16 0x1e5`, `u32` save time, four MFC strings (name, time, date, weekday),
//!   `u32 [0x517aec]` and a 160x120 RGB565 thumbnail (38400 bytes).
//! * `.data` is the game state (a CArchive stream) compressed with the exe's `CHuffcomp`: `u32` length, `u16` root, the
//!   tree nodes from the root down to node 0 (`u8` symbol, `u16` left, `u16` right; both `0xffff` = leaf), then the codes
//!   as `u32` words, most significant bit first (0 = left, 1 = right).

use crate::{Error, Reader, Result};

/// Header words of the `.info` file.
pub const INFO_VERSION: (u16, u16) = (2, 0x1e5);
/// Header words of the state stream (`0x42a400`).
pub const STATE_VERSION: (u16, u16) = (1, 0x5a10);
pub const THUMB_W: usize = 160;
pub const THUMB_H: usize = 120;

/// Decompresses a `.data` file.
pub fn huff_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    let len = r.u32("length")? as usize;
    let root = r.u16("root")? as usize;
    if len > 1 << 28 {
        return Err(Error::Invalid(format!("savegame length {len}")));
    }
    let mut nodes = vec![(0u8, 0xffffu16, 0xffffu16); root + 1];
    for i in (0..=root).rev() {
        nodes[i] = (r.u8("symbol")?, r.u16("left")?, r.u16("right")?);
    }
    let leaf = |n: usize| nodes[n].1 == 0xffff && nodes[n].2 == 0xffff;
    if leaf(root) {
        return Err(Error::Invalid("savegame tree has a single node".into()));
    }
    let mut out = Vec::with_capacity(len);
    let mut at = root;
    while out.len() < len {
        let word = r.u32("code")?;
        for bit in (0..32).rev() {
            let next = if word >> bit & 1 == 1 { nodes[at].2 } else { nodes[at].1 } as usize;
            if next > root {
                return Err(Error::Invalid(format!("savegame node {next} out of range")));
            }
            if leaf(next) {
                out.push(nodes[next].0);
                at = root;
                if out.len() == len {
                    break;
                }
            } else {
                at = next;
            }
        }
    }
    Ok(out)
}

/// Compresses like the exe does (a plain Huffman tree; tie-breaking differs, so the bytes are not identical to the
/// exe's output but decode the same). Used by the tests.
pub fn huff_encode(data: &[u8]) -> Vec<u8> {
    let mut freq = [0u32; 256];
    for &b in data {
        freq[b as usize] += 1;
    }
    // nodes in creation order: leaves first, then merges; the root is the last one
    let mut nodes: Vec<(u8, u16, u16)> = Vec::new();
    let mut live: Vec<(u32, usize)> = Vec::new();
    for (sym, &f) in freq.iter().enumerate() {
        if f > 0 {
            live.push((f, nodes.len()));
            nodes.push((sym as u8, 0xffff, 0xffff));
        }
    }
    if nodes.len() == 1 {
        // the decoder needs at least one inner node
        nodes.push((0, 0xffff, 0xffff));
        live.push((0, 1));
    }
    while live.len() > 1 {
        live.sort_by(|a, b| b.cmp(a));
        let (fa, a) = live.pop().unwrap();
        let (fb, b) = live.pop().unwrap();
        live.push((fa + fb, nodes.len()));
        nodes.push((0, a as u16, b as u16));
    }
    let root = nodes.len() - 1;
    let mut codes: Vec<Vec<bool>> = vec![Vec::new(); nodes.len()];
    let mut stack = vec![(root, Vec::new())];
    while let Some((n, code)) = stack.pop() {
        let (_, l, r) = nodes[n];
        if l == 0xffff && r == 0xffff {
            codes[n] = code;
        } else {
            let mut cl = code.clone();
            cl.push(false);
            let mut cr = code;
            cr.push(true);
            stack.push((l as usize, cl));
            stack.push((r as usize, cr));
        }
    }
    let mut leaf_of = [0usize; 256];
    for (i, n) in nodes.iter().enumerate() {
        if n.1 == 0xffff && n.2 == 0xffff && freq[n.0 as usize] > 0 {
            leaf_of[n.0 as usize] = i;
        }
    }
    let mut out = Vec::new();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend((root as u16).to_le_bytes());
    for i in (0..=root).rev() {
        out.push(nodes[i].0);
        out.extend(nodes[i].1.to_le_bytes());
        out.extend(nodes[i].2.to_le_bytes());
    }
    let mut word = 0u32;
    let mut used = 0;
    for &b in data {
        for &bit in &codes[leaf_of[b as usize]] {
            word = word << 1 | bit as u32;
            used += 1;
            if used == 32 {
                out.extend(word.to_le_bytes());
                word = 0;
                used = 0;
            }
        }
    }
    if used > 0 {
        out.extend((word << (32 - used)).to_le_bytes());
    }
    out
}

use crate::saveobj::{mfc_string, Class, Object};

/// The `.info` file: what the load menu lists.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveInfo {
    /// `[this+0x94]`, the time stamp taken when the save was made.
    pub stamp: u32,
    /// `[this+0x84]`: the savegame's title.
    pub name: String,
    /// `strftime("%X")` (`[this+0x88]`).
    pub time: String,
    /// `strftime("%b %d, %Y")` (`[this+0x8c]`).
    pub date: String,
    /// `strftime("%a %H:%M")` (`[this+0x90]`).
    pub weekday: String,
    /// `[0x517aec]`.
    pub extra: u32,
    /// 160x120 RGB565 screenshot, row-major.
    pub thumbnail: Vec<u16>,
}

impl SaveInfo {
    pub fn parse(data: &[u8]) -> Result<SaveInfo> {
        let mut r = Reader::new(data);
        let head = (r.u16("version")?, r.u16("magic")?);
        if head != INFO_VERSION {
            return Err(Error::Invalid(format!("savegame info header {head:?}")));
        }
        let stamp = r.u32("stamp")?;
        let name = mfc_string(&mut r, "name")?;
        let time = mfc_string(&mut r, "time")?;
        let date = mfc_string(&mut r, "date")?;
        let weekday = mfc_string(&mut r, "weekday")?;
        let extra = r.u32("extra")?;
        let mut thumbnail = Vec::with_capacity(THUMB_W * THUMB_H);
        for _ in 0..THUMB_W * THUMB_H {
            thumbnail.push(r.u16("thumbnail")?);
        }
        Ok(SaveInfo { stamp, name, time, date, weekday, extra, thumbnail })
    }
}

/// The scalar prefix of the state stream, up to the per-player blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateHead {
    /// `[0x49a33c]`: the level, reloaded through `0x4267d0` before the rest is applied.
    pub level: u32,
    /// `[0x71c0f4]`: index into the lockstep random table (docs/rng, `rng.rs`).
    pub rng_index: u32,
    pub v_5baafc: u32,
    /// `[0x49a339] == 1`.
    pub flag_49a339: bool,
    pub v_5baad8: u32,
    pub v_5baadc: u32,
    pub v_5baac4: u32,
    pub v_5baac8: u32,
    /// `[0x5baa99] == 1`.
    pub flag_5baa99: bool,
    pub v_89cf50: u32,
    /// Bytes consumed (the three per-player blocks start here).
    pub len: usize,
}

impl StateHead {
    pub fn parse(data: &[u8]) -> Result<StateHead> {
        let mut r = Reader::new(data);
        let head = (r.u16("version")?, r.u16("magic")?);
        if head != STATE_VERSION {
            return Err(Error::Invalid(format!("savegame state header {head:?}")));
        }
        let level = r.u32("level")?;
        let rng_index = r.u32("rng")?;
        let v_5baafc = r.u32("5baafc")?;
        let flag_49a339 = r.u8("49a339")? != 0;
        let v_5baad8 = r.u32("5baad8")?;
        let v_5baadc = r.u32("5baadc")?;
        let v_5baac4 = r.u32("5baac4")?;
        let v_5baac8 = r.u32("5baac8")?;
        let flag_5baa99 = r.u8("5baa99")? != 0;
        let v_89cf50 = r.u32("89cf50")?;
        Ok(StateHead {
            level,
            rng_index,
            v_5baafc,
            flag_49a339,
            v_5baad8,
            v_5baadc,
            v_5baac4,
            v_5baac8,
            flag_5baa99,
            v_89cf50,
            len: r.pos(),
        })
    }
}

/// Per-player scalar block that follows the head (`0x42a660..0x42a813`, three players): three `u32`, then arrays of
/// 1000, 300, 500 and 30 bytes.
pub const PLAYER_BLOCK: [usize; 4] = [1000, 300, 500, 30];

/// Section tags the exe writes (as MFC strings) after each group of objects and checks on load.
pub const TAGS: [&str; 5] = ["P2v3", "aS2n", "Ua63", "B17u", "Checksum"];

/// The map object (`0x459960`, global `0x6bf980`): the level path, the 128-wide cell layers and a list of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapState {
    /// `0x71cec8`, the level path as the loader got it.
    pub level: String,
    /// `[+0x20]`, `[+0x2c]`.
    pub v20: u32,
    pub v2c: u32,
    /// `[+0x40]` columns (inner loop) and `[+0x44]` rows (outer loop); cell `(i, j)` is stored at index `j * 128 + i`.
    pub width: u32,
    pub height: u32,
    pub cells: Vec<MapCell>,
    /// Three per-player records: `(flag +4+k, [+8+4k], [+0x14+4k], [+0x54+4k], [+0x48+4k], [+0x60+4k], [+0x6c+4k])`.
    pub players: Vec<(bool, [u32; 6])>,
    pub items: Vec<Object>,
}

/// One map cell, in the order the exe writes it: `u16 [+0xcc]`, `u32 [+0x170cc]`, `u32 [+0x180cc]`,
/// `u8 [+0x280cc]`, `u8 [+0x2c0cc]`, `u8 [+0x300cc]`, `u32 [+0x340cc]` (indices `j * 128 + i`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapCell {
    pub tile: u16,
    pub a: u32,
    pub b: u32,
    pub c: [u8; 3],
    pub d: u32,
}

impl MapState {
    pub fn parse(r: &mut Reader) -> Result<MapState> {
        let level = mfc_string(r, "level path")?;
        let v20 = r.u32("+0x20")?;
        let v2c = r.u32("+0x2c")?;
        let width = r.u32("width")?;
        let height = r.u32("height")?;
        if width > 128 || height > 128 {
            return Err(Error::Invalid(format!("map size {width}x{height}")));
        }
        let mut cells = Vec::with_capacity((width * height) as usize);
        for _ in 0..width * height {
            cells.push(MapCell {
                tile: r.u16("tile")?,
                a: r.u32("cell a")?,
                b: r.u32("cell b")?,
                c: [r.u8("cell c0")?, r.u8("cell c1")?, r.u8("cell c2")?],
                d: r.u32("cell d")?,
            });
        }
        if r.u8("marker")? != 1 || r.u32("player count")? != 3 {
            return Err(Error::Invalid("map block marker".into()));
        }
        let mut players = Vec::new();
        for _ in 0..3 {
            let flag = r.u8("player flag")? != 0;
            let mut v = [0u32; 6];
            for x in &mut v {
                *x = r.u32("player word")?;
            }
            players.push((flag, v));
        }
        let n = r.u32("map item count")?;
        if n > 1 << 20 {
            return Err(Error::Invalid(format!("map item count {n}")));
        }
        let mut items = Vec::new();
        for _ in 0..n {
            items.push(Object::parse(r, Class::MapItem)?);
        }
        Ok(MapState { level, v20, v2c, width, height, cells, players, items })
    }
}

/// Slots per player for units, buildings, bullets and squads.
pub const SLOTS: [usize; 4] = [1000, 300, 500, 30];

/// A whole state stream, walked section by section (`docs/savegame.md`).
#[derive(Debug, Clone)]
pub struct State {
    pub head: StateHead,
    /// Per player: three words and the byte arrays of [`PLAYER_BLOCK`].
    pub player_words: Vec<[u32; 3]>,
    pub player_arrays: Vec<[Vec<u8>; 4]>,
    /// Three records of two counted `u32` lists.
    pub lists: Vec<[Vec<u32>; 2]>,
    pub map: MapState,
    pub players: Vec<Object>,
    pub stocks: Vec<Object>,
    pub params: Vec<Object>,
    /// The 313 filler bytes of `0x456ac0` (random bytes around a checksum of the object).
    pub signature: Vec<u8>,
    pub speech: Vec<Object>,
    /// 128x128 decal flags, index `x * 128 + y`.
    pub decal_flags: Vec<u8>,
    /// Slot groups per player: units, buildings, bullets, squads, each `(count word, present objects with slot index)`.
    pub units: Vec<(u32, Vec<(usize, Object)>)>,
    pub buildings: Vec<(u32, Vec<(usize, Object)>)>,
    pub bullets: Vec<(u32, Vec<(usize, Object)>)>,
    pub squads: Vec<(u32, Vec<(usize, Object)>)>,
    pub decals: Vec<Object>,
    pub clouds: Vec<Object>,
}

/// Length of the `0x456ac0` block: 79 + 20 random bytes, 1 balancing byte, 50 x 2 and 113.
pub const SIGNATURE_LEN: usize = 79 + 20 + 1 + 100 + 113;

fn tag(r: &mut Reader, want: &str) -> Result<()> {
    let got = mfc_string(r, "tag")?;
    if got != want {
        return Err(Error::Invalid(format!("savegame tag {got:?}, expected {want:?}")));
    }
    Ok(())
}

fn slots(r: &mut Reader, n: usize, class: Class) -> Result<(u32, Vec<(usize, Object)>)> {
    let count = r.u32("slot count")?;
    let mut v = Vec::new();
    for i in 0..n {
        if r.u8("slot flag")? == 1 {
            v.push((i, Object::parse(r, class)?));
        }
    }
    Ok((count, v))
}

impl State {
    /// `speech_objects` is `[0x575720] + 1` (a run-time global that is not saved; 1 for a single-player game).
    pub fn parse(data: &[u8], speech_objects: usize) -> Result<State> {
        let head = StateHead::parse(data)?;
        let mut r = Reader::at(data, head.len);
        let mut player_words = Vec::new();
        let mut player_arrays = Vec::new();
        for _ in 0..3 {
            player_words.push([r.u32("word")?, r.u32("word")?, r.u32("word")?]);
            player_arrays.push([
                r.bytes(PLAYER_BLOCK[0], "array")?.to_vec(),
                r.bytes(PLAYER_BLOCK[1], "array")?.to_vec(),
                r.bytes(PLAYER_BLOCK[2], "array")?.to_vec(),
                r.bytes(PLAYER_BLOCK[3], "array")?.to_vec(),
            ]);
        }
        let mut lists = Vec::new();
        for _ in 0..3 {
            let mut pair = [Vec::new(), Vec::new()];
            for l in &mut pair {
                let n = r.u32("list length")?;
                if n as usize > r.remaining() / 4 {
                    return Err(Error::Invalid(format!("list length {n}")));
                }
                for _ in 0..n {
                    l.push(r.u32("list item")?);
                }
            }
            lists.push(pair);
        }
        let map = MapState::parse(&mut r)?;
        let mut players = Vec::new();
        for _ in 0..3 {
            players.push(Object::parse(&mut r, Class::PlayerState)?);
        }
        let mut stocks = Vec::new();
        for _ in 0..3 {
            stocks.push(Object::parse(&mut r, Class::Stock)?);
        }
        let mut params = Vec::new();
        for _ in 0..3 {
            params.push(Object::parse(&mut r, Class::Params)?);
        }
        let signature = r.bytes(SIGNATURE_LEN, "signature")?.to_vec();
        tag(&mut r, TAGS[0])?;
        let mut speech = Vec::new();
        for _ in 0..speech_objects {
            speech.push(Object::parse(&mut r, Class::Speech)?);
        }
        tag(&mut r, TAGS[1])?;
        let decal_flags = r.bytes(128 * 128, "decal flags")?.to_vec();
        let mut units = Vec::new();
        for _ in 0..3 {
            units.push(slots(&mut r, SLOTS[0], Class::Unit)?);
        }
        tag(&mut r, TAGS[2])?;
        let mut buildings = Vec::new();
        for _ in 0..3 {
            buildings.push(slots(&mut r, SLOTS[1], Class::Building)?);
        }
        tag(&mut r, TAGS[3])?;
        let mut bullets = Vec::new();
        for _ in 0..3 {
            bullets.push(slots(&mut r, SLOTS[2], Class::Bullet)?);
        }
        let mut squads = Vec::new();
        for _ in 0..3 {
            squads.push(slots(&mut r, SLOTS[3], Class::Aircraft)?);
        }
        let mut decals = Vec::new();
        let n = r.u32("decal count")?;
        for _ in 0..n {
            decals.push(Object::parse(&mut r, Class::Decal)?);
        }
        let mut clouds = Vec::new();
        let n = r.u32("cloud count")?;
        for _ in 0..n {
            clouds.push(Object::parse(&mut r, Class::Cloud)?);
        }
        tag(&mut r, TAGS[4])?;
        if r.remaining() != 0 {
            return Err(Error::Invalid(format!("{} bytes after the checksum tag", r.remaining())));
        }
        Ok(State {
            head,
            player_words,
            player_arrays,
            lists,
            map,
            players,
            stocks,
            params,
            signature,
            speech,
            decal_flags,
            units,
            buildings,
            bullets,
            squads,
            decals,
            clouds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huffman_round_trip() {
        let mut data = Vec::new();
        for i in 0..5000u32 {
            data.push((i * i % 7) as u8 * 3);
            if i % 5 == 0 {
                data.push(0xAA);
            }
        }
        let packed = huff_encode(&data);
        assert!(packed.len() < data.len());
        assert_eq!(huff_decode(&packed).unwrap(), data);
        let tiny = [1u8, 2, 1, 1, 2];
        assert_eq!(huff_decode(&huff_encode(&tiny)).unwrap(), tiny);
        assert_eq!(huff_decode(&huff_encode(&[7u8; 40])).unwrap(), vec![7u8; 40]);
        assert!(huff_decode(&packed[..packed.len() - 8]).is_err());
    }

    #[test]
    fn decodes_a_hand_built_stream() {
        // tree: node 2 = root (left node 1, right leaf 0 'A'); node 1 = (leaf 'B' = node ... )
        // nodes: 0 = leaf 'A', 1 = leaf 'B', 2 = (0, 1)  -> 'A' = 0, 'B' = 1
        let mut f = Vec::new();
        f.extend(5u32.to_le_bytes());
        f.extend(2u16.to_le_bytes());
        for (s, l, r) in [(0u8, 0u16, 1u16), (b'B', 0xffff, 0xffff), (b'A', 0xffff, 0xffff)] {
            f.push(s);
            f.extend(l.to_le_bytes());
            f.extend(r.to_le_bytes());
        }
        // ABBAB -> 0 1 1 0 1 then padding, MSB first
        f.extend(0b0110_1000_0000_0000_0000_0000_0000_0000u32.to_le_bytes());
        assert_eq!(huff_decode(&f).unwrap(), b"ABBAB");
    }

    fn put_string(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        out.extend(s.as_bytes());
    }

    #[test]
    fn parses_info() {
        let mut f = Vec::new();
        f.extend(2u16.to_le_bytes());
        f.extend(0x1e5u16.to_le_bytes());
        f.extend(1234u32.to_le_bytes());
        for s in ["Mission 3", "12:00:01", "Nov 02, 2026", "Mon 12:00"] {
            put_string(&mut f, s);
        }
        f.extend(9u32.to_le_bytes());
        for i in 0..THUMB_W * THUMB_H {
            f.extend((i as u16).to_le_bytes());
        }
        let info = SaveInfo::parse(&f).unwrap();
        assert_eq!(info.name, "Mission 3");
        assert_eq!(info.date, "Nov 02, 2026");
        assert_eq!(info.thumbnail.len(), 19200);
        assert_eq!(info.thumbnail[300], 300);
        assert!(SaveInfo::parse(&f[..100]).is_err());
        f[0] = 3;
        assert!(SaveInfo::parse(&f).is_err());
    }

    #[test]
    fn parses_state_head() {
        let mut f = Vec::new();
        f.extend(1u16.to_le_bytes());
        f.extend(0x5a10u16.to_le_bytes());
        f.extend(7u32.to_le_bytes()); // level
        f.extend(99u32.to_le_bytes()); // rng
        f.extend(1u32.to_le_bytes());
        f.push(1);
        for v in 2u32..6 {
            f.extend(v.to_le_bytes());
        }
        f.push(0);
        f.extend(8u32.to_le_bytes());
        let h = StateHead::parse(&f).unwrap();
        assert_eq!((h.level, h.rng_index, h.flag_49a339, h.flag_5baa99, h.v_89cf50), (7, 99, true, false, 8));
        assert_eq!(h.len, f.len());
    }

    fn put_tag(out: &mut Vec<u8>, t: &str) {
        put_string(out, t);
    }

    fn header(out: &mut Vec<u8>, c: Class) {
        out.extend(c.header().0.to_le_bytes());
        out.extend(c.header().1.to_le_bytes());
    }

    #[test]
    fn walks_a_synthetic_state() {
        let mut f = Vec::new();
        f.extend(1u16.to_le_bytes());
        f.extend(0x5a10u16.to_le_bytes());
        // level, rng, 5baafc, flag, four words, flag, 89cf50
        f.extend(vec![0u8; 4 + 4 + 4 + 1 + 16 + 1 + 4]);
        for _ in 0..3 {
            f.extend(vec![0u8; 12 + 1000 + 300 + 500 + 30]);
        }
        for _ in 0..3 {
            f.extend(2u32.to_le_bytes());
            f.extend([1, 0, 0, 0, 2, 0, 0, 0]);
            f.extend(0u32.to_le_bytes());
        }
        // map: 2x2 cells, one item of each kind
        put_string(&mut f, "data\\levels\\m01");
        f.extend(vec![0u8; 8]);
        f.extend(2u32.to_le_bytes());
        f.extend(2u32.to_le_bytes());
        f.extend(vec![0u8; 4 * (2 + 4 + 4 + 3 + 4)]);
        f.push(1);
        f.extend(3u32.to_le_bytes());
        f.extend(vec![0u8; 3 * (1 + 24)]);
        f.extend(2u32.to_le_bytes());
        header(&mut f, Class::MapItem);
        f.extend(1u32.to_le_bytes());
        f.extend(vec![0u8; 12]);
        header(&mut f, Class::MapItem);
        f.extend(0u32.to_le_bytes());
        f.extend(vec![0u8; 20 + 2]);
        for _ in 0..3 {
            header(&mut f, Class::PlayerState);
            f.extend(vec![0u8; 13 * 4]);
        }
        for _ in 0..3 {
            header(&mut f, Class::Stock);
            f.extend(vec![0u8; 4 * 4 + 18 * 4]);
            for i in 0..10u32 {
                f.extend((i % 2).to_le_bytes());
                if i % 2 == 1 {
                    f.extend(7u32.to_le_bytes());
                }
            }
        }
        for _ in 0..3 {
            header(&mut f, Class::Params);
            f.extend(vec![0u8; 31 * 4]);
        }
        f.extend(vec![9u8; SIGNATURE_LEN]);
        put_tag(&mut f, "P2v3");
        header(&mut f, Class::Speech);
        f.extend(vec![0u8; 8]);
        f.extend(1u32.to_le_bytes());
        header(&mut f, Class::SpeechItem);
        put_string(&mut f, "01-game-saved");
        f.extend(vec![0u8; 16]);
        put_tag(&mut f, "aS2n");
        f.extend(vec![0u8; 128 * 128]);
        for (n, extra) in [(1000, "Ua63"), (300, "B17u")] {
            for _ in 0..3 {
                f.extend(0u32.to_le_bytes());
                f.extend(vec![0u8; n]);
            }
            put_tag(&mut f, extra);
        }
        for n in [500, 30] {
            for _ in 0..3 {
                f.extend(0u32.to_le_bytes());
                f.extend(vec![0u8; n]);
            }
        }
        f.extend(1u32.to_le_bytes());
        header(&mut f, Class::Decal);
        f.extend(vec![0u8; 16]);
        f.extend(0u32.to_le_bytes());
        put_tag(&mut f, "Checksum");
        let st = State::parse(&f, 1).unwrap();
        assert_eq!(st.map.cells.len(), 4);
        assert_eq!(st.map.items.len(), 2);
        assert_eq!(st.lists[1][0], vec![1, 2]);
        assert_eq!(st.stocks[0].get(2 << 16), Some(7));
        assert_eq!(st.speech[0].orders[0].strings, vec!["01-game-saved".to_string()]);
        assert_eq!(st.decals.len(), 1);
        assert!(State::parse(&f[..f.len() - 1], 1).is_err());
        assert!(State::parse(&f, 2).is_err());
    }
}
