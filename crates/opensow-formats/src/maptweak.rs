//! Skirmish map tweaks (not in the original; docs/map-editor.md): small edits of a `.net` script kept as text, so the port can
//! rebalance the shipped multiplayer maps without storing or shipping any game data. The file `data/skirmish-maps.txt` holds
//! them (the game compiles it in, `opensow-viewer --edit MAP` writes it):
//!
//! ```text
//! # comment
//! [06-Meteor Swarm]
//! move mine 27,62 -> 75,47
//! owner lab 47,3 blue
//! remove wind 2,29
//! add heavy 40,30 neutral
//! move unit advancer 18,4 -> 20,6
//! add unit cannon1 20,20 green
//! slots medium 36,72 active 2 cannon1+1 special1+1 artillery1
//! add air 50,40 neutral active 1 fighter bomber
//! stock fighter 0
//! ```
//!
//! `stock <force> N` sets the air force both sides start with (header words `5+3g` and `6+3g`; `bomber carryall fighter tripler
//! asteroid`).
//!
//! `slots` sets a building's production lines (docs/production.md: object word 2 = active lines, words 3-7 the lines' kinds,
//! words 8-12 how many tech-levels each may still go up): `cannon1+1` = a cannon line at tier 1 that research can raise once.
//!
//! An edit names its object by kind and the cell it has **in the shipped file** (`#n` picks the n-th of several, 1-based), so
//! every edit of a map is resolved against the original script first and then applied together: moves never chain. Added
//! buildings copy the words of the first building of the same kind in the map (factory slots, levels), else [`template`].

use crate::level::{Level, Marker, Object};
use crate::{Error, Result};

/// Owner of an object: neutral (2), blue = player 0, green = player 1 (object word 14, marker player).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Neutral,
    Blue,
    Green,
}

impl Side {
    pub fn code(self) -> i32 {
        match self {
            Side::Blue => 0,
            Side::Green => 1,
            Side::Neutral => 2,
        }
    }

    pub fn from_code(c: i32) -> Option<Side> {
        match c {
            0 => Some(Side::Blue),
            1 => Some(Side::Green),
            2 => Some(Side::Neutral),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Side::Neutral => "neutral",
            Side::Blue => "blue",
            Side::Green => "green",
        }
    }

    fn parse(s: &str) -> Option<Side> {
        [Side::Neutral, Side::Blue, Side::Green].into_iter().find(|v| v.name().eq_ignore_ascii_case(s))
    }
}

/// What an edit acts on: a building or turret (object kind 25-29, 100-108) or a unit marker (kind 1-15, 40-46).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thing {
    Object(i32),
    Unit(i32),
}

/// Names of the object kinds in the file (`hq`, `mine`, `turret1`..).
pub const OBJECT_NAMES: [(i32, &str); 14] = [
    (100, "hq"),
    (101, "light"),
    (102, "medium"),
    (103, "heavy"),
    (104, "air"),
    (105, "mine"),
    (106, "lab"),
    (107, "wind"),
    (108, "walker"),
    (25, "turret1"),
    (26, "turret2"),
    (27, "turret3"),
    (28, "turret4"),
    (29, "turret5"),
];

/// Names of the unit kinds: the five classes (`docs/levels.md`: anti-air, artillery, cannon, flamer, special) with their tier,
/// the Advancer, the five walkers and the pod.
pub fn unit_name(kind: i32) -> String {
    const CLASSES: [&str; 5] = ["antiair", "artillery", "cannon", "flamer", "special"];
    match kind {
        1..=15 => format!("{}{}", CLASSES[((kind - 1) / 3) as usize], (kind - 1) % 3 + 1),
        40 => "advancer".into(),
        41..=45 => format!("walker{}", kind - 40),
        46 => "pod".into(),
        k => k.to_string(),
    }
}

pub fn object_name(kind: i32) -> String {
    OBJECT_NAMES.iter().find(|(k, _)| *k == kind).map_or(kind.to_string(), |(_, n)| (*n).to_string())
}

fn parse_unit(s: &str) -> Option<i32> {
    s.parse().ok().or_else(|| [(1..=15).collect::<Vec<i32>>(), vec![40, 41, 42, 43, 44, 45, 46]].concat().into_iter().find(|&k| unit_name(k).eq_ignore_ascii_case(s)))
}

fn parse_object(s: &str) -> Option<i32> {
    s.parse().ok().or_else(|| OBJECT_NAMES.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map(|(k, _)| *k))
}

/// One object of the shipped script: its kind, its cell there and which of several equal ones (1-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub thing: Thing,
    pub cell: (i32, i32),
    pub nth: u32,
}

/// A building's production lines: how many run from the start (`active`, word 2) and each line's kind with the number of
/// tech-levels it may rise (words 3-7 and 8-12; at most five lines).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Production {
    pub active: i32,
    pub lines: Vec<(i32, i32)>,
}

pub const MAX_LINES: usize = 5;

impl Production {
    pub fn of(o: &Object) -> Production {
        let lines = (0..MAX_LINES).filter(|&i| o.raw[3 + i] != 0).map(|i| (o.raw[3 + i], o.raw[8 + i])).collect();
        Production { active: o.raw[2], lines }
    }

    pub fn write(&self, o: &mut Object) {
        o.raw[2] = self.active;
        for i in 0..MAX_LINES {
            let (k, v) = self.lines.get(i).copied().unwrap_or((0, 0));
            o.raw[3 + i] = k;
            o.raw[8 + i] = v;
        }
    }
}

/// Name of a production line's kind: a unit (`cannon1`), an upgrade level (`gold1`..`energy3`) or an air force (`fighter`).
pub fn line_name(kind: i32) -> String {
    match kind {
        16..=24 => format!("{}{}", ["gold", "research", "energy"][((kind - 16) / 3) as usize], (kind - 16) % 3 + 1),
        30..=34 => ["bomber", "carryall", "fighter", "tripler", "asteroid"][(kind - 30) as usize].to_string(),
        k => unit_name(k),
    }
}

fn parse_line_kind(s: &str) -> Option<i32> {
    s.parse().ok().or_else(|| (1..=46).find(|&k| line_name(k).eq_ignore_ascii_case(s)))
}

fn production_text(p: &Production) -> String {
    let mut s = format!("active {}", p.active);
    for &(k, v) in &p.lines {
        s += &format!(" {}", line_name(k));
        if v != 0 {
            s += &format!("+{v}");
        }
    }
    s
}

fn parse_production(words: &[&str]) -> Option<Production> {
    let (first, rest) = words.split_first()?;
    if !first.eq_ignore_ascii_case("active") {
        return None;
    }
    let (n, rest) = rest.split_first()?;
    let lines: Option<Vec<(i32, i32)>> = rest
        .iter()
        .map(|w| {
            let (k, v) = w.split_once('+').unwrap_or((w, "0"));
            Some((parse_line_kind(k)?, v.parse().ok()?))
        })
        .collect();
    let p = Production { active: n.parse().ok()?, lines: lines? };
    (p.lines.len() <= MAX_LINES && (0..=MAX_LINES as i32).contains(&p.active)).then_some(p)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Move(Key, (i32, i32)),
    Owner(Key, Side),
    Remove(Key),
    /// A new object; buildings may come with their own production lines (else those of one of its kind in the map).
    Add(Thing, (i32, i32), Side, Option<Production>),
    /// A building's production lines.
    Slots(Key, Production),
    /// The start stock of an air force (code 30..34) for both sides.
    Stock(i32, i32),
}

/// The tweaks of every map, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tweaks {
    pub maps: Vec<(String, Vec<Edit>)>,
}

fn cell(s: &str) -> Option<(i32, i32)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

impl Tweaks {
    pub fn parse(text: &str) -> Result<Tweaks> {
        let mut t = Tweaks::default();
        for (n, raw) in text.lines().enumerate() {
            // a comment is a line starting with `#` or the rest after ` #` (the `#` of `X,Y#2` follows the cell directly)
            let line = if raw.trim_start().starts_with('#') { "" } else { raw.split(" #").next().unwrap_or("").trim() };
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| Error::Invalid(format!("skirmish tweaks line {}: {why}: {raw}", n + 1));
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                t.maps.push((name.trim().to_string(), Vec::new()));
                continue;
            }
            let Some((_, edits)) = t.maps.last_mut() else { return Err(bad("edit before any [map]")) };
            let mut words: Vec<&str> = line.split_whitespace().collect();
            let op = words.remove(0).to_ascii_lowercase();
            if op == "stock" {
                let force = words.first().and_then(|w| (30..=34).find(|&k| line_name(k).eq_ignore_ascii_case(w)));
                let n = words.get(1).and_then(|n| n.parse::<i32>().ok()).filter(|&n| n >= 0);
                match (force, n, words.len()) {
                    (Some(f), Some(n), 2) => edits.push(Edit::Stock(f, n)),
                    _ => return Err(bad("expected `stock <bomber|carryall|fighter|tripler|asteroid> N`")),
                }
                continue;
            }
            let thing = match words.first().map(|w| w.to_ascii_lowercase()) {
                Some(w) if w == "unit" => {
                    words.remove(0);
                    Thing::Unit(words.first().and_then(|w| parse_unit(w)).ok_or_else(|| bad("unknown unit kind"))?)
                }
                Some(_) => Thing::Object(words.first().and_then(|w| parse_object(w)).ok_or_else(|| bad("unknown building kind"))?),
                None => return Err(bad("missing kind")),
            };
            words.remove(0);
            let key = |w: &str| -> Option<Key> {
                let (c, nth) = match w.split_once('#') {
                    Some((c, k)) => (c, k.parse().ok().filter(|&k: &u32| k >= 1)?),
                    None => (w, 1),
                };
                Some(Key { thing, cell: cell(c)?, nth })
            };
            let edit = match (op.as_str(), words.as_slice()) {
                ("move", [from, "->", to]) => Edit::Move(key(from).ok_or_else(|| bad("bad cell"))?, cell(to).ok_or_else(|| bad("bad cell"))?),
                ("owner", [at, side]) => Edit::Owner(key(at).ok_or_else(|| bad("bad cell"))?, Side::parse(side).ok_or_else(|| bad("owner is neutral, blue or green"))?),
                ("remove", [at]) => Edit::Remove(key(at).ok_or_else(|| bad("bad cell"))?),
                ("add", [at, side, prod @ ..]) => {
                    let p = if prod.is_empty() { None } else { Some(parse_production(prod).ok_or_else(|| bad("expected `active N` and the lines"))?) };
                    if p.is_some() && matches!(thing, Thing::Unit(_)) {
                        return Err(bad("units have no production lines"));
                    }
                    Edit::Add(thing, cell(at).ok_or_else(|| bad("bad cell"))?, Side::parse(side).ok_or_else(|| bad("owner is neutral, blue or green"))?, p)
                }
                ("slots", [at, prod @ ..]) if matches!(thing, Thing::Object(_)) => Edit::Slots(key(at).ok_or_else(|| bad("bad cell"))?, parse_production(prod).ok_or_else(|| bad("expected `active N` and up to five lines"))?),
                _ => return Err(bad("expected move/owner/remove/add/slots")),
            };
            if matches!(edit, Edit::Owner(Key { thing: Thing::Unit(_), .. }, Side::Neutral) | Edit::Add(Thing::Unit(_), _, Side::Neutral, _)) {
                return Err(bad("a unit is blue or green"));
            }
            edits.push(edit);
        }
        Ok(t)
    }

    /// The edits of `map` (file name stem, any case); none when the map has no section.
    pub fn edits(&self, map: &str) -> &[Edit] {
        self.maps.iter().find(|(m, _)| m.eq_ignore_ascii_case(map)).map_or(&[], |(_, e)| e.as_slice())
    }

    /// Replaces (or adds, or with no edits drops) the section of `map`.
    pub fn set(&mut self, map: &str, edits: Vec<Edit>) {
        match self.maps.iter().position(|(m, _)| m.eq_ignore_ascii_case(map)) {
            Some(i) if edits.is_empty() => {
                self.maps.remove(i);
            }
            Some(i) => self.maps[i].1 = edits,
            None if edits.is_empty() => {}
            None => self.maps.push((map.to_string(), edits)),
        }
    }

    /// The file text: `header` (comment lines), then one section per map.
    pub fn to_text(&self, header: &str) -> String {
        let mut s = header.to_string();
        for (map, edits) in &self.maps {
            s += &format!("\n[{map}]\n");
            for e in edits {
                s += &edit_line(e);
                s.push('\n');
            }
        }
        s
    }
}

fn thing_text(t: Thing) -> String {
    match t {
        Thing::Object(k) => object_name(k),
        Thing::Unit(k) => format!("unit {}", unit_name(k)),
    }
}

fn key_text(k: &Key) -> String {
    let n = if k.nth > 1 { format!("#{}", k.nth) } else { String::new() };
    format!("{} {},{}{n}", thing_text(k.thing), k.cell.0, k.cell.1)
}

pub fn edit_line(e: &Edit) -> String {
    match e {
        Edit::Move(k, to) => format!("move {} -> {},{}", key_text(k), to.0, to.1),
        Edit::Owner(k, s) => format!("owner {} {}", key_text(k), s.name()),
        Edit::Remove(k) => format!("remove {}", key_text(k)),
        Edit::Add(t, c, s, None) => format!("add {} {},{} {}", thing_text(*t), c.0, c.1, s.name()),
        Edit::Add(t, c, s, Some(p)) => format!("add {} {},{} {} {}", thing_text(*t), c.0, c.1, s.name(), production_text(p)),
        Edit::Slots(k, p) => format!("slots {} {}", key_text(k), production_text(p)),
        Edit::Stock(f, n) => format!("stock {} {n}", line_name(*f)),
    }
}

/// The header comment of `data/skirmish-maps.txt`.
pub const HEADER: &str = "# OpenSOW skirmish map tweaks (docs/map-editor.md): edits of the shipped .net maps, applied when a skirmish loads one.
# Edit with `cargo run --release -p opensow-viewer -- --edit \"<map>\"` or by hand. Cells are the ones in the shipped file.
#   move <kind> X,Y -> X,Y | owner <kind> X,Y neutral|blue|green | remove <kind> X,Y | add <kind> X,Y neutral|blue|green [lines]
#   stock <bomber|carryall|fighter|tripler|asteroid> N (the air force both sides start with)
#   slots <kind> X,Y active N <line>... (lines: cannon1+1 = cannon line at tier 1, may rise 1 tier; gold1..3 research1..3
#   energy1..3; bomber carryall fighter tripler asteroid; walker1..5 pod)
#   kinds: hq light medium heavy air mine lab wind walker turret1..5; units: unit antiair1..3 artillery1..3 cannon1..3
#   flamer1..3 special1..3 advancer walker1..5 pod (units are blue or green); X,Y#2 = the second of that kind on that cell
";

/// Words of a new object of `kind` when the map has none to copy: one active slot and the slots the shipped maps give that
/// kind (docs/production.md), turrets none; full hit points.
pub fn template(kind: i32, cell: (i32, i32), side: Side) -> Object {
    let mut raw = [0i32; 19];
    raw[0] = 123;
    raw[1] = kind;
    let words: &[i32] = match kind {
        101 => &[1, 7, 13],
        102 => &[1, 7, 13, 4, 0, 0, 1, 1, 1],
        103 => &[1, 7, 10, 13, 0, 0, 2, 2, 2],
        104 => &[1, 32, 30],
        105 => &[1, 16, 17, 18],
        106 => &[1, 19],
        107 => &[1, 22, 23],
        108 => &[1, 43],
        100 => &[1],
        _ => &[],
    };
    raw[2..2 + words.len()].copy_from_slice(words);
    raw[13] = if (25..=29).contains(&kind) { [900, 1250, 1540, 2100, 2600][(kind - 25) as usize] } else { 13536 };
    raw[15] = i32::from(kind == 100);
    raw[18] = 100;
    let mut o = Object { kind, cell_x: cell.0, cell_y: cell.1, raw };
    set_object(&mut o, cell, side);
    o
}

fn set_object(o: &mut Object, cell: (i32, i32), side: Side) {
    (o.cell_x, o.cell_y) = cell;
    (o.raw[16], o.raw[17]) = cell;
    set_owner(o, side);
}

/// Gives `o` to `side`. Word 18 is the start hit-point percentage, which the exe applies to owned buildings only (neutral ones
/// always start at half, `Object::start_hp`, `0x43a003`); the shipped files store 50 on neutral ones, so a building that turns
/// blue or green from neutral (or copies a neutral one's words) would start half destroyed: it gets 100 then, and 50 when it
/// turns neutral. An owned building that changes sides keeps its percentage.
pub fn set_owner(o: &mut Object, side: Side) {
    let was_neutral = o.raw[14] == Side::Neutral.code();
    o.raw[14] = side.code();
    if side == Side::Neutral {
        o.raw[18] = 50;
    } else if was_neutral || o.raw[18] <= 0 {
        o.raw[18] = 100;
    }
}

/// A marker moved to `cell`, keeping its offset inside the cell.
fn move_marker(m: &mut Marker, cell: (i32, i32)) {
    m.x = cell.0 * 32 + (m.x & 31);
    m.y = cell.1 * 32 + (m.y & 31);
}

/// The index in `original` that `key` names.
pub fn resolve(original: &Level, key: &Key) -> Option<usize> {
    let n = key.nth as usize;
    match key.thing {
        Thing::Object(k) => original.objects.iter().enumerate().filter(|(_, o)| o.kind == k && (o.cell_x, o.cell_y) == key.cell).nth(n - 1).map(|(i, _)| i),
        Thing::Unit(k) => original.markers.iter().enumerate().filter(|(_, m)| m.kind == k && m.cell() == key.cell).nth(n - 1).map(|(i, _)| i),
    }
}

/// The key that names `original`'s object (or, with `unit`, marker) `i`.
pub fn key_of(original: &Level, unit: bool, i: usize) -> Key {
    if unit {
        let m = &original.markers[i];
        let nth = original.markers[..i].iter().filter(|o| o.kind == m.kind && o.cell() == m.cell()).count() as u32 + 1;
        Key { thing: Thing::Unit(m.kind), cell: m.cell(), nth }
    } else {
        let o = &original.objects[i];
        let nth = original.objects[..i].iter().filter(|p| p.kind == o.kind && (p.cell_x, p.cell_y) == (o.cell_x, o.cell_y)).count() as u32 + 1;
        Key { thing: Thing::Object(o.kind), cell: (o.cell_x, o.cell_y), nth }
    }
}

/// A script with its edits and where every object and marker came from (`Some(i)` = `original`'s object/marker `i`, `None` = added).
#[derive(Debug, Clone)]
pub struct Tracked {
    pub level: Level,
    pub object_origin: Vec<Option<usize>>,
    pub marker_origin: Vec<Option<usize>>,
}

/// Applies `edits` to `original`. Edits naming nothing in `original` are skipped and returned (their line text).
pub fn apply(original: &Level, edits: &[Edit]) -> (Tracked, Vec<String>) {
    let mut level = original.clone();
    let mut gone_o = vec![false; level.objects.len()];
    let mut gone_m = vec![false; level.markers.len()];
    let mut skipped = Vec::new();
    for e in edits {
        let key = match e {
            Edit::Move(k, _) | Edit::Owner(k, _) | Edit::Remove(k) | Edit::Slots(k, _) => *k,
            Edit::Add(..) | Edit::Stock(..) => continue,
        };
        let Some(i) = resolve(original, &key) else {
            skipped.push(edit_line(e));
            continue;
        };
        match (e, key.thing) {
            (Edit::Move(_, to), Thing::Object(_)) => {
                let side = Side::from_code(level.objects[i].raw[14]).unwrap_or(Side::Neutral);
                set_object(&mut level.objects[i], *to, side);
            }
            (Edit::Move(_, to), Thing::Unit(_)) => move_marker(&mut level.markers[i], *to),
            (Edit::Owner(_, s), Thing::Object(_)) => set_owner(&mut level.objects[i], *s),
            (Edit::Owner(_, s), Thing::Unit(_)) => level.markers[i].player = s.code(),
            (Edit::Remove(_), Thing::Object(_)) => gone_o[i] = true,
            (Edit::Remove(_), Thing::Unit(_)) => gone_m[i] = true,
            (Edit::Slots(_, p), Thing::Object(_)) => p.write(&mut level.objects[i]),
            _ => {}
        }
    }
    let mut object_origin: Vec<Option<usize>> = (0..level.objects.len()).map(Some).collect();
    let mut marker_origin: Vec<Option<usize>> = (0..level.markers.len()).map(Some).collect();
    let mut i = 0;
    level.objects.retain(|_| {
        i += 1;
        !gone_o[i - 1]
    });
    object_origin.retain(|o| !gone_o[o.unwrap()]);
    let mut i = 0;
    level.markers.retain(|_| {
        i += 1;
        !gone_m[i - 1]
    });
    marker_origin.retain(|o| !gone_m[o.unwrap()]);
    for e in edits {
        match e {
            Edit::Add(Thing::Object(k), c, s, p) => {
                let mut o = added(original, *k, *c, *s);
                if let Some(p) = p {
                    p.write(&mut o);
                }
                level.objects.push(o);
                object_origin.push(None);
            }
            Edit::Add(Thing::Unit(k), c, s, _) => {
                level.markers.push(Marker { kind: *k, player: s.code(), x: c.0 * 32 + 16, y: c.1 * 32 + 16 });
                marker_origin.push(None);
            }
            _ => {}
        }
    }
    for e in edits {
        if let Edit::Stock(force, n) = e {
            if let Some(g) = crate::air::AirForce::from_code(*force).and_then(|f| crate::air::HEADER_ORDER.iter().position(|&h| h == f)) {
                level.header[5 + 3 * g] = *n;
                level.header[6 + 3 * g] = *n;
            }
        }
    }
    (Tracked { level, object_origin, marker_origin }, skipped)
}

/// A new building of `kind` as [`apply`] makes it without production lines of its own: the words of the first of its kind in
/// `original`, else [`template`].
pub fn added(original: &Level, kind: i32, cell: (i32, i32), side: Side) -> Object {
    let mut o = original.objects.iter().find(|o| o.kind == kind).cloned().unwrap_or_else(|| template(kind, cell, side));
    // the copy starts as neutral so `set_owner` gives an owned one its full hit points whatever it was copied from
    o.raw[14] = Side::Neutral.code();
    set_object(&mut o, cell, side);
    o.raw[13] = template(kind, cell, side).raw[13];
    o
}

/// The edits that turn `original` into `t.level` (the inverse of [`apply`]): removals, then moves and owner changes, then additions.
pub fn diff(original: &Level, t: &Tracked) -> Vec<Edit> {
    let mut out = Vec::new();
    for (i, _) in original.objects.iter().enumerate().filter(|(i, _)| !t.object_origin.contains(&Some(*i))) {
        out.push(Edit::Remove(key_of(original, false, i)));
    }
    for (i, _) in original.markers.iter().enumerate().filter(|(i, _)| !t.marker_origin.contains(&Some(*i))) {
        out.push(Edit::Remove(key_of(original, true, i)));
    }
    for (o, origin) in t.level.objects.iter().zip(&t.object_origin) {
        let Some(i) = *origin else { continue };
        let was = &original.objects[i];
        if (o.cell_x, o.cell_y) != (was.cell_x, was.cell_y) {
            out.push(Edit::Move(key_of(original, false, i), (o.cell_x, o.cell_y)));
        }
        if o.raw[14] != was.raw[14] {
            out.push(Edit::Owner(key_of(original, false, i), Side::from_code(o.raw[14]).unwrap_or(Side::Neutral)));
        }
        if Production::of(o) != Production::of(was) {
            out.push(Edit::Slots(key_of(original, false, i), Production::of(o)));
        }
    }
    for (m, origin) in t.level.markers.iter().zip(&t.marker_origin) {
        let Some(i) = *origin else { continue };
        let was = &original.markers[i];
        if m.cell() != was.cell() {
            out.push(Edit::Move(key_of(original, true, i), m.cell()));
        }
        if m.player != was.player {
            out.push(Edit::Owner(key_of(original, true, i), Side::from_code(m.player).unwrap_or(Side::Blue)));
        }
    }
    for (o, origin) in t.level.objects.iter().zip(&t.object_origin) {
        if origin.is_none() {
            let side = Side::from_code(o.raw[14]).unwrap_or(Side::Neutral);
            let default = Production::of(&added(original, o.kind, (o.cell_x, o.cell_y), side));
            let p = Production::of(o);
            out.push(Edit::Add(Thing::Object(o.kind), (o.cell_x, o.cell_y), side, (p != default).then_some(p)));
        }
    }
    for (m, origin) in t.level.markers.iter().zip(&t.marker_origin) {
        if origin.is_none() {
            out.push(Edit::Add(Thing::Unit(m.kind), m.cell(), Side::from_code(m.player).unwrap_or(Side::Blue), None));
        }
    }
    // an air force whose start stock changed for both sides alike
    for (g, force) in crate::air::HEADER_ORDER.iter().enumerate() {
        let (a, b) = (t.level.header[5 + 3 * g], t.level.header[6 + 3 * g]);
        if a == b && (a != original.header[5 + 3 * g] || b != original.header[6 + 3 * g]) {
            out.push(Edit::Stock(force.code(), a));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::HEADER_WORDS;

    fn obj(kind: i32, x: i32, y: i32, owner: i32, slot: i32) -> Object {
        let mut raw = [0i32; 19];
        (raw[0], raw[1], raw[2], raw[3], raw[14], raw[16], raw[17], raw[18]) = (123, kind, 1, slot, owner, x, y, 50);
        Object { kind, cell_x: x, cell_y: y, raw }
    }

    fn level() -> Level {
        Level {
            version: 4,
            header: [0; HEADER_WORDS],
            objects: vec![obj(100, 1, 1, 0, 0), obj(100, 50, 50, 1, 0), obj(105, 20, 20, 2, 16), obj(105, 30, 30, 2, 16), obj(107, 40, 10, 2, 22)],
            markers: vec![Marker { kind: 40, player: 0, x: 3 * 32 + 5, y: 4 * 32 + 9 }, Marker { kind: 7, player: 1, x: 10 * 32, y: 10 * 32 }, Marker { kind: 7, player: 1, x: 10 * 32 + 8, y: 10 * 32 }],
        }
    }

    const TEXT: &str = "# comment\n\n[06-Meteor Swarm]\nmove mine 20,20 -> 25,26\nowner wind 40,10 blue\nremove mine 30,30\nadd heavy 12,13 neutral\nmove unit advancer 3,4 -> 6,7\nremove unit cannon1 10,10#2\nadd unit flamer2 9,9 green\nslots mine 20,20 active 2 gold1 gold2 gold3\nadd medium 5,5 blue active 1 cannon1+2 special1+1\n";

    #[test]
    fn stock_edits_set_both_sides_and_come_back_from_the_diff() {
        let mut l = level();
        l.header[5 + 3 * 4] = 12; // fighters (group 4) of both sides
        l.header[6 + 3 * 4] = 12;
        let t = Tweaks::parse("[m]\nstock fighter 0\nstock bomber 2\n").unwrap();
        assert_eq!(t.edits("m"), [Edit::Stock(32, 0), Edit::Stock(30, 2)]);
        assert!(Tweaks::parse("[m]\nstock fighter\n").is_err() && Tweaks::parse("[m]\nstock gunship 1\n").is_err());
        let (tracked, skipped) = apply(&l, t.edits("m"));
        assert!(skipped.is_empty());
        let air = tracked.level.air_stock();
        assert_eq!((air[4].counts[..2].to_vec(), air[0].counts[..2].to_vec()), (vec![0, 0], vec![2, 2]));
        let back = diff(&l, &tracked);
        assert_eq!(back, [Edit::Stock(30, 2), Edit::Stock(32, 0)]);
        assert!(Tweaks::parse(&format!("[m]\n{}\n", back.iter().map(edit_line).collect::<Vec<_>>().join("\n"))).unwrap().edits("m") == back.as_slice());
    }

    #[test]
    fn the_file_parses_and_writes_back_the_same() {
        let t = Tweaks::parse(TEXT).unwrap();
        let e = t.edits("06-meteor swarm");
        assert_eq!(e.len(), 9);
        assert_eq!(e[7], Edit::Slots(Key { thing: Thing::Object(105), cell: (20, 20), nth: 1 }, Production { active: 2, lines: vec![(16, 0), (17, 0), (18, 0)] }));
        assert_eq!(e[8], Edit::Add(Thing::Object(102), (5, 5), Side::Blue, Some(Production { active: 1, lines: vec![(7, 2), (13, 1)] })));
        assert_eq!(e[0], Edit::Move(Key { thing: Thing::Object(105), cell: (20, 20), nth: 1 }, (25, 26)));
        assert_eq!(e[5], Edit::Remove(Key { thing: Thing::Unit(7), cell: (10, 10), nth: 2 }));
        assert_eq!(e[6], Edit::Add(Thing::Unit(11), (9, 9), Side::Green, None));
        assert!(t.edits("02-Ice Blizzard").is_empty());
        let again = Tweaks::parse(&t.to_text(HEADER)).unwrap();
        assert_eq!(again, t);
        assert!(t.to_text("").contains("remove unit cannon1 10,10#2\n"));
    }

    #[test]
    fn bad_lines_are_refused_with_their_number() {
        for bad in ["move mine 1,1 -> 2,2\n", "[m]\nmove mine 1,1 2,2\n", "[m]\nadd castle 1,1 blue\n", "[m]\nadd unit cannon1 1,1 neutral\n", "[m]\nowner mine 1,1 red\n", "[m]\nslots mine 1,1 gold1\n", "[m]\nslots medium 1,1 active 1 a b c d e f\n", "[m]\nslots medium 1,1 active 1 castle1\n", "[m]\nslots unit cannon1 1,1 active 0\n"] {
            assert!(Tweaks::parse(bad).is_err(), "{bad}");
        }
        assert!(format!("{:?}", Tweaks::parse("[m]\n\nmove x").unwrap_err()).contains("line 3"));
    }

    #[test]
    fn edits_resolve_against_the_shipped_cells_and_apply_together() {
        let t = Tweaks::parse(TEXT).unwrap();
        let (r, skipped) = apply(&level(), t.edits("06-Meteor Swarm"));
        assert!(skipped.is_empty());
        let l = &r.level;
        assert_eq!(l.objects.len(), 6, "one removed, two added");
        let mine = l.objects.iter().find(|o| o.kind == 105).unwrap();
        assert_eq!((mine.cell_x, mine.cell_y, mine.raw[16], mine.raw[17], mine.raw[14]), (25, 26, 25, 26, 2));
        assert_eq!(l.objects.iter().find(|o| o.kind == 107).unwrap().raw[14], 0, "the windtrap is blue");
        let heavy = &l.objects[4];
        assert_eq!((heavy.kind, heavy.cell_x, heavy.raw[14], &heavy.raw[2..11]), (103, 12, 2, &[1, 7, 10, 13, 0, 0, 2, 2, 2][..]), "no heavy factory to copy: the template");
        assert_eq!(l.markers[0].cell(), (6, 7));
        assert_eq!((l.markers[0].x & 31, l.markers[0].y & 31), (5, 9), "the offset inside the cell stays");
        assert_eq!(l.markers.len(), 3);
        assert_eq!(l.markers[1].x, 10 * 32, "the first cannon stays, the second went");
        assert_eq!((l.markers[2].kind, l.markers[2].player, l.markers[2].cell()), (11, 1, (9, 9)));
        assert_eq!(r.object_origin, vec![Some(0), Some(1), Some(2), Some(4), None, None]);
        assert_eq!(Production::of(mine), Production { active: 2, lines: vec![(16, 0), (17, 0), (18, 0)] });
        let medium = &l.objects[5];
        assert_eq!((medium.raw[2], &medium.raw[3..13]), (1, &[7, 13, 0, 0, 0, 2, 1, 0, 0, 0][..]), "its own lines, the rest cleared");
    }

    #[test]
    fn an_added_building_copies_the_words_of_one_in_the_map() {
        let (r, _) = apply(&level(), &[Edit::Add(Thing::Object(105), (5, 6), Side::Blue, None)]);
        let o = r.level.objects.last().unwrap();
        assert_eq!((o.raw[3], o.raw[14], o.cell_x, o.raw[17]), (16, 0, 5, 6));
    }

    #[test]
    fn diff_is_the_inverse_of_apply() {
        let t = Tweaks::parse(TEXT).unwrap();
        let (r, _) = apply(&level(), t.edits("06-Meteor Swarm"));
        let d = diff(&level(), &r);
        let (again, _) = apply(&level(), &d);
        assert_eq!(again.level.objects, r.level.objects);
        assert_eq!(again.level.markers.iter().map(|m| (m.kind, m.player, m.cell())).collect::<Vec<_>>(), r.level.markers.iter().map(|m| (m.kind, m.player, m.cell())).collect::<Vec<_>>());
        assert!(diff(&level(), &apply(&level(), &[]).0).is_empty(), "no change, no edits");
        assert!(d.iter().any(|e| matches!(e, Edit::Slots(..))) && d.iter().any(|e| matches!(e, Edit::Add(_, _, _, Some(_)))), "{d:?}");
        let plain = apply(&level(), &[Edit::Add(Thing::Object(105), (5, 6), Side::Blue, None)]).0;
        assert_eq!(diff(&level(), &plain), vec![Edit::Add(Thing::Object(105), (5, 6), Side::Blue, None)], "lines like the copied ones are not written");
    }

    #[test]
    fn a_missing_object_is_skipped_and_reported() {
        let (r, skipped) = apply(&level(), &[Edit::Remove(Key { thing: Thing::Object(106), cell: (1, 1), nth: 1 })]);
        assert_eq!(r.level.objects.len(), 5);
        assert_eq!(skipped, vec!["remove lab 1,1".to_string()]);
    }

    #[test]
    fn a_building_that_becomes_owned_starts_with_full_hit_points() {
        // the mines of the test map are neutral with word 18 = 50 (half), like the shipped ones
        let (r, _) = apply(&level(), &[
            Edit::Add(Thing::Object(105), (5, 6), Side::Blue, None),
            Edit::Owner(Key { thing: Thing::Object(105), cell: (20, 20), nth: 1 }, Side::Green),
            Edit::Owner(Key { thing: Thing::Object(100), cell: (50, 50), nth: 1 }, Side::Blue),
        ]);
        let added = r.level.objects.last().unwrap();
        assert_eq!((added.raw[14], added.raw[18]), (0, 100), "added as blue from a neutral mine's words");
        let mine = r.level.objects.iter().find(|o| (o.cell_x, o.cell_y) == (20, 20)).unwrap();
        assert_eq!((mine.raw[14], mine.raw[18]), (1, 100), "neutral turned green");
        let hq = r.level.objects.iter().find(|o| (o.cell_x, o.cell_y) == (50, 50)).unwrap();
        assert_eq!((hq.raw[14], hq.raw[18]), (0, 50), "an owned building changing sides keeps its percentage");
        let (n, _) = apply(&level(), &[Edit::Add(Thing::Object(105), (5, 6), Side::Neutral, None)]);
        assert_eq!(n.level.objects.last().unwrap().raw[18], 50, "a neutral one keeps the neutral half");
    }

    #[test]
    fn production_lines_round_trip_through_the_words() {
        let mut o = obj(102, 1, 1, 2, 0);
        let p = Production { active: 2, lines: vec![(7, 1), (13, 0), (4, 2)] };
        p.write(&mut o);
        assert_eq!(&o.raw[2..13], &[2, 7, 13, 4, 0, 0, 1, 0, 2, 0, 0]);
        assert_eq!(Production::of(&o), p);
        assert_eq!(line_name(17), "gold2");
        assert_eq!(line_name(23), "energy2");
        assert_eq!(line_name(32), "fighter");
        assert_eq!(line_name(8), "cannon2");
        assert_eq!(production_text(&p), "active 2 cannon1+1 special1 artillery1+2");
    }

    #[test]
    fn set_replaces_and_drops_sections() {
        let mut t = Tweaks::parse(TEXT).unwrap();
        t.set("06-METEOR SWARM", vec![Edit::Remove(Key { thing: Thing::Object(105), cell: (1, 2), nth: 1 })]);
        assert_eq!(t.maps.len(), 1);
        assert_eq!(t.edits("06-Meteor Swarm").len(), 1);
        t.set("02-Ice Blizzard", Vec::new());
        assert_eq!(t.maps.len(), 1);
        t.set("06-Meteor Swarm", Vec::new());
        assert!(t.maps.is_empty());
    }
}
