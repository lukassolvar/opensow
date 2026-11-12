//! Level scripts: `Mxx.edt` (campaign) and `NN-Name.net` (multiplayer) share one format.
//!
//! ```text
//! u16 version (3 = empty template M00, 4 = all shipped levels), u16 magic 9870 (0x268E)
//! header: 94 i32 words in total including the two above (376 bytes)
//! objects: 76-byte records while word 0 == 123 (19 × i32)
//! markers: 20-byte records while word 0 == 224 (5 × i32)
//! u32 footer = 7656
//! ```
//! Verified by parsing all 31 shipped files to exactly their last byte, and
//! (for campaign levels) by checking every object's cell lies inside the
//! matching `.map` and never on a blocked cell (2131 objects).
//!
//! Object words (index: meaning): 0 tag 123, 1 kind (see [`ObjectKind`]),
//! 2 flag (usually 1), 3 per-kind constant for buildings (e.g. 16 air.. unknown use),
//! 13 hit points, 14 owner (see [`Owner`]), 16 cell x, 17 cell y,
//! 18 percentage-like value near 100 (50 for half-health neutral buildings); others mostly zero.
//! Marker words: 0 tag 224, 1 unit kind (see [`UnitKind`]), 2 player index, 3 x pixels, 4 y pixels
//! (pixel = 32 per cell). The exe's loader (`0x41da00`) spawns each marker as a unit at `(x>>5, y>>5)`.

use crate::air::{AirStock, HEADER_ORDER};
use crate::kinds;
use crate::{Error, Reader, Result};

/// Magic number in the upper half of header word 0.
pub const MAGIC: u16 = 9870;
/// First word of every object record.
pub const OBJECT_TAG: i32 = 123;
/// First word of every unit marker record.
pub const MARKER_TAG: i32 = 224;
/// The word that ends the file.
pub const FOOTER: u32 = 7656;
/// Number of 32-bit words in the header.
pub const HEADER_WORDS: usize = 94;

/// What an [`Object`] is, from its kind number.
///
/// Evidence: the exe's building-name table (`_base _light _med _heavy _walker _air _mine _rlab _wind`),
/// `Data/Sprites2/InfoBox/2/{Base,Light,Med,Heavy,Walk,Air,Mine,Rlab,Wind,Tur1..5}.tsp`, the exe treating
/// kinds {101,102,103,108} as unit producers, and the manual's per-map descriptions
/// (heavy factories first appear on map 06, walker factories only on maps 04/05/07/08,
/// gold mines dominate "Gold Rush", air bases are absent from "First Spark").
/// Kind 107 (`Wind`) is assumed to be the energy source (InfoBox has energy meters); unconfirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ObjectKind {
    /// Kinds 25..=29 = `tur_01`..`tur_05` (HP 900/1250/1540/2100/2600); `1..=5`.
    Turret(u8),
    /// Kind 100.
    Headquarters,
    /// Kind 101.
    LightFactory,
    /// Kind 102.
    MediumFactory,
    /// Kind 103.
    HeavyFactory,
    /// Kind 104.
    AirBase,
    /// Kind 105.
    GoldMine,
    /// Kind 106.
    ResearchLab,
    /// Kind 107, the windtrap (`Wind` in the exe's name table).
    Wind,
    /// Kind 108.
    WalkerFactory,
    /// Any other kind number, kept as read.
    Unknown(i32),
}

impl ObjectKind {
    /// The kind an object's kind number (object word 1) stands for.
    pub fn from_id(id: i32) -> ObjectKind {
        match id {
            25..=29 => ObjectKind::Turret((id - 24) as u8),
            kinds::HEADQUARTERS => ObjectKind::Headquarters,
            kinds::LIGHT_FACTORY => ObjectKind::LightFactory,
            kinds::MEDIUM_FACTORY => ObjectKind::MediumFactory,
            kinds::HEAVY_FACTORY => ObjectKind::HeavyFactory,
            kinds::AIR_BASE => ObjectKind::AirBase,
            kinds::GOLD_MINE => ObjectKind::GoldMine,
            kinds::RESEARCH_LAB => ObjectKind::ResearchLab,
            kinds::WINDTRAP => ObjectKind::Wind,
            kinds::WALKER_FACTORY => ObjectKind::WalkerFactory,
            other => ObjectKind::Unknown(other),
        }
    }

    /// Whether this is one of the nine buildings (not a turret, not an unknown kind).
    pub fn is_building(self) -> bool {
        !matches!(self, ObjectKind::Turret(_) | ObjectKind::Unknown(_))
    }

    /// Whether the exe's factory logic applies (kinds 101, 102, 103, 108).
    pub fn produces_units(self) -> bool {
        matches!(self, ObjectKind::LightFactory | ObjectKind::MediumFactory | ObjectKind::HeavyFactory | ObjectKind::WalkerFactory)
    }

    /// English name for listings and the viewer.
    pub fn name(self) -> String {
        match self {
            ObjectKind::Turret(n) => format!("Turret {n}"),
            ObjectKind::Headquarters => "Headquarters".into(),
            ObjectKind::LightFactory => "Light factory".into(),
            ObjectKind::MediumFactory => "Medium factory".into(),
            ObjectKind::HeavyFactory => "Heavy factory".into(),
            ObjectKind::AirBase => "Air base".into(),
            ObjectKind::GoldMine => "Gold mine".into(),
            ObjectKind::ResearchLab => "Research lab".into(),
            ObjectKind::Wind => "Wind (energy?)".into(),
            ObjectKind::WalkerFactory => "Walker factory".into(),
            ObjectKind::Unknown(k) => format!("Unknown kind {k}"),
        }
    }
}

/// Probable owner from object word 14. Evidence: on M01 the player's HQ has 0,
/// the island enemy HQ has 1, and the neutral capturable research lab (half health,
/// the one the briefing says to seize) has 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Owner {
    /// Owner 0: the first player (blue).
    Player,
    /// Owner 1: the second player (green).
    Enemy,
    /// Owner 2: nobody; such a building can be captured by either side.
    Neutral,
    /// Any other owner number, kept as read.
    Unknown(i32),
}

/// A building or turret placed on the map (an object record of the level file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Kind number (object word 1), see [`ObjectKind`] and [`crate::kinds`].
    pub kind: i32,
    /// Cell column of the object (object word 16).
    pub cell_x: i32,
    /// Cell row of the object (object word 17).
    pub cell_y: i32,
    /// All 19 words of the record as stored, including the ones above.
    pub raw: [i32; 19],
}

impl Object {
    /// The object's kind as an enum.
    pub fn object_kind(&self) -> ObjectKind {
        ObjectKind::from_id(self.kind)
    }

    /// The object's owner (object word 14).
    pub fn owner(&self) -> Owner {
        match self.raw[14] {
            0 => Owner::Player,
            1 => Owner::Enemy,
            2 => Owner::Neutral,
            o => Owner::Unknown(o),
        }
    }

    /// Current hit points as stored in word 13 (the exe reads it but never uses it: see [`Object::start_hp`]).
    pub fn hp(&self) -> i32 {
        self.raw[13]
    }

    /// The hit points a building really starts with (`0x41dfc8..0x41dff3`, `0x43a003`): a neutral building (owner 2) gets half its
    /// maximum; every other owner gets `max * word18 / 100` (the percentage, 100 in files without the word), capped at `max`.
    pub fn start_hp(&self, max: i32) -> i32 {
        if self.raw[14] == 2 {
            max / 2
        } else {
            (max * self.raw[18] / 100).min(max)
        }
    }
}

/// Marker kind of the Advancer (always 2 per two-player map, 4 on "Cross of Fire").
/// Confirmed from the manual ("two Headquarters and two Advancers" per side on that map) and from the
/// exe: the loader skips kind 40 when a per-player flag is set, and the unit constructor gives it 800 HP.
pub const MARKER_ADVANCER: i32 = crate::kinds::ADVANCER;

/// What a [`Marker`] spawns. Decoded from the exe's script loader (`0x41da00`, which calls the unit
/// constructor `0x46bd30` with `(cell_x, cell_y, player, kind)`) and its per-kind tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    /// Kinds 1..=15: five classes of three tiers. `class = (kind-1)/3` (0..5), `tier = (kind-1)%3 + 1`.
    /// Classes in order: anti-air, artillery, cannon, flamer, special (see [`crate::units`]);
    /// levels place only tier 1 and 2 markers.
    Ground {
        /// Weapon class, 0..5: anti-air, artillery, cannon, flamer, special.
        class: u8,
        /// Tier, 1..=3.
        tier: u8,
    },
    /// Kind 40: "the most important unit in the game" (manual): repairs/destroys turrets and captures buildings.
    Advancer,
    /// Kinds 41..=45 are the five walker robots (`5W1`..`5W5` tops on the `chW` legs); 46 is a bare `chN`
    /// pod (1000 HP, 17 placed in campaign levels; role unknown).
    Special(u8),
    /// Kinds 109..=113 exist in the exe but the script loader never spawns them (walkers/specials created at runtime).
    RuntimeOnly(i32),
    /// Any other kind number, kept as read.
    Unknown(i32),
}

impl UnitKind {
    /// The kind a unit's kind number stands for.
    pub fn from_id(id: i32) -> UnitKind {
        match id {
            1..=15 => UnitKind::Ground { class: ((id - 1) / 3) as u8, tier: ((id - 1) % 3 + 1) as u8 },
            kinds::ADVANCER => UnitKind::Advancer,
            41..=46 => UnitKind::Special(id as u8),
            109..=113 => UnitKind::RuntimeOnly(id),
            other => UnitKind::Unknown(other),
        }
    }

    /// Maximum hit points from the exe's constructor table (`0x46bd30`, jump table `0x46c23c`).
    pub fn max_hp(id: i32) -> Option<i32> {
        const GROUND: [i32; 15] = [180, 400, 550, 180, 360, 450, 180, 360, 500, 240, 430, 600, 210, 400, 550];
        match id {
            1..=15 => Some(GROUND[(id - 1) as usize]),
            40 => Some(800),
            41 | 44 => Some(1100),
            42 => Some(900),
            43 | 46 => Some(1000),
            45 => Some(1200),
            109..=113 => Some(200),
            _ => None,
        }
    }

    /// English name for listings and the viewer.
    pub fn name(self) -> String {
        match self {
            UnitKind::Ground { class, tier } => format!("Ground unit class {class} tier {tier}"),
            UnitKind::Advancer => "Advancer".into(),
            UnitKind::Special(k) => format!("Special unit {k}"),
            UnitKind::RuntimeOnly(k) => format!("Runtime unit {k}"),
            UnitKind::Unknown(k) => format!("Unknown unit {k}"),
        }
    }
}

/// A unit placed on the map (a marker record of the level file); the loader spawns it when the level starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// Unit kind number (marker word 1), see [`UnitKind`].
    pub kind: i32,
    /// Player index passed to the unit constructor (0 = first player/blue, 1 = second/green).
    pub player: i32,
    /// Horizontal position in pixels (32 per cell).
    pub x: i32,
    /// Vertical position in pixels (32 per cell).
    pub y: i32,
}

impl Marker {
    /// The marker's unit kind as an enum.
    pub fn unit_kind(&self) -> UnitKind {
        UnitKind::from_id(self.kind)
    }

    /// Whether this marker spawns an Advancer.
    pub fn is_advancer(&self) -> bool {
        self.kind == MARKER_ADVANCER
    }

    /// Cell the unit is created in (the exe uses `pixel >> 5`).
    pub fn cell(&self) -> (i32, i32) {
        (self.x >> 5, self.y >> 5)
    }
}

/// A parsed level file (`.edt` campaign mission or `.net` multiplayer map): header, objects and unit markers.
#[derive(Debug, Clone)]
pub struct Level {
    /// File format version, the lower half of header word 0.
    pub version: u16,
    /// Header words; word 0 packs `version | magic << 16`.
    pub header: [i32; HEADER_WORDS],
    /// The buildings and turrets, in file order.
    pub objects: Vec<Object>,
    /// The units to spawn, in file order.
    pub markers: Vec<Marker>,
}

impl Level {
    /// Header word 1: level number (`.edt`: 1..22, `.net`: odd ids 15..19 seen).
    pub fn level_id(&self) -> i32 {
        self.header[1]
    }

    /// Header words 2 and 3. Probably the two players' starting resources
    /// (2500/5000, 3000/3000, ... seen); unconfirmed.
    pub fn start_resources(&self) -> [i32; 2] {
        [self.header[2], self.header[3]]
    }

    /// Starting air-force stocks for group `g` in [`HEADER_ORDER`]: `counts` = `[player, enemy, unused]` = header words
    /// `5+3g`, `6+3g` and `4+3g`. The exe's level loader (`0x41d88e..0x41d9bd`) queues the type code `header[5+3g]` times for the
    /// player block `0x70ba50` and `header[6+3g]` times for the enemy block `0x70c6b8`; word `4+3g` is never read (and is 0 in
    /// every shipped level). Multiplayer maps set both players' words (Island of Fate: 12 fighters each).
    pub fn air_stock(&self) -> [AirStock; 5] {
        std::array::from_fn(|g| AirStock { force: HEADER_ORDER[g], counts: [self.header[5 + 3 * g], self.header[6 + 3 * g], self.header[4 + 3 * g]] })
    }

    /// A level file exactly: nothing may follow the footer.
    pub fn parse(data: &[u8]) -> Result<Level> {
        let (level, used) = Level::parse_prefix(data)?;
        if used != data.len() {
            return Err(Error::Invalid(format!("level tail: {} bytes after the footer", data.len() - used)));
        }
        Ok(level)
    }

    /// The level at the start of `data` and how many bytes it took. The exe's loader (`0x41da00`, an MFC `CArchive` read) stops at
    /// the footer tag `0x1de8` and never reads further, so bytes after it are ignored by the original: custom maps keep the
    /// port's own data there (`custommap.rs`).
    pub fn parse_prefix(data: &[u8]) -> Result<(Level, usize)> {
        let mut r = Reader::new(data);
        let version = r.u16("level version")?;
        let magic = r.u16("level magic")?;
        if magic != MAGIC {
            return Err(Error::Invalid(format!("level magic {magic}, expected {MAGIC}")));
        }
        let mut header = [0i32; HEADER_WORDS];
        header[0] = i32::from(version) | i32::from(magic) << 16;
        for w in &mut header[1..] {
            *w = r.i32("level header")?;
        }

        let mut objects = Vec::new();
        while peek_i32(&r) == Some(OBJECT_TAG) {
            let mut raw = [0i32; 19];
            for w in &mut raw {
                *w = r.i32("object")?;
            }
            objects.push(Object { kind: raw[1], cell_x: raw[16], cell_y: raw[17], raw });
        }
        let mut markers = Vec::new();
        while peek_i32(&r) == Some(MARKER_TAG) {
            let w: [i32; 5] = std::array::from_fn(|_| r.i32("marker").unwrap_or(0));
            markers.push(Marker { kind: w[1], player: w[2], x: w[3], y: w[4] });
        }
        let footer = r.u32("level footer")?;
        if footer != FOOTER {
            return Err(Error::Invalid(format!("level tail: footer {footer}, {} bytes left (record layout mismatch at {:#x})", r.remaining(), r.pos())));
        }
        Ok((Level { version, header, objects, markers }, r.pos()))
    }

    /// The level file: the inverse of [`Level::parse`] (byte-identical for every shipped `.edt` / `.net`, `tests/real_data.rs`).
    /// Object records are written from `raw` with the tag, kind and cell taken from the fields.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(HEADER_WORDS * 4 + self.objects.len() * 76 + self.markers.len() * 20 + 4);
        d.extend(self.version.to_le_bytes());
        d.extend(MAGIC.to_le_bytes());
        for w in &self.header[1..] {
            d.extend(w.to_le_bytes());
        }
        for o in &self.objects {
            let mut raw = o.raw;
            (raw[0], raw[1], raw[16], raw[17]) = (OBJECT_TAG, o.kind, o.cell_x, o.cell_y);
            d.extend(raw.iter().flat_map(|v| v.to_le_bytes()));
        }
        for m in &self.markers {
            d.extend([MARKER_TAG, m.kind, m.player, m.x, m.y].iter().flat_map(|v| v.to_le_bytes()));
        }
        d.extend(FOOTER.to_le_bytes());
        d
    }
}

fn peek_i32(r: &Reader<'_>) -> Option<i32> {
    r.clone().i32("peek").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(4u16.to_le_bytes());
        d.extend(MAGIC.to_le_bytes());
        for i in 1..HEADER_WORDS as i32 {
            d.extend(if i == 1 { 7i32 } else { 0 }.to_le_bytes());
        }
        let mut obj = [0i32; 19];
        (obj[0], obj[1], obj[16], obj[17]) = (OBJECT_TAG, 26, 5, 9);
        d.extend(obj.iter().flat_map(|v| v.to_le_bytes()));
        d.extend([MARKER_TAG, 7, 1, 160, 288].iter().flat_map(|v| v.to_le_bytes()));
        d.extend(FOOTER.to_le_bytes());
        d
    }

    #[test]
    fn parses_sections() {
        let l = Level::parse(&sample()).unwrap();
        assert_eq!((l.version, l.level_id()), (4, 7));
        assert_eq!(l.objects.len(), 1);
        assert_eq!((l.objects[0].kind, l.objects[0].cell_x, l.objects[0].cell_y), (26, 5, 9));
        assert_eq!(l.markers, [Marker { kind: 7, player: 1, x: 160, y: 288 }]);
    }

    #[test]
    fn kind_mapping() {
        assert_eq!(ObjectKind::from_id(25), ObjectKind::Turret(1));
        assert_eq!(ObjectKind::from_id(29), ObjectKind::Turret(5));
        assert_eq!(ObjectKind::from_id(100), ObjectKind::Headquarters);
        assert_eq!(ObjectKind::from_id(108), ObjectKind::WalkerFactory);
        assert!(ObjectKind::from_id(108).produces_units() && !ObjectKind::from_id(104).produces_units());
        assert!(ObjectKind::from_id(105).is_building() && !ObjectKind::from_id(26).is_building());
        assert_eq!(ObjectKind::from_id(7), ObjectKind::Unknown(7));
    }

    #[test]
    fn air_stock_groups() {
        let mut l = Level::parse(&sample()).unwrap();
        l.header[5 + 3 * 3] = 4; // group 3 (tripler), the player's word
        l.header[6 + 3 * 4] = 9; // group 4 (fighter), the enemy's word
        let st = l.air_stock();
        assert_eq!((st[3].force, st[3].counts), (crate::air::AirForce::Tripler, [4, 0, 0]));
        assert_eq!((st[4].force, st[4].counts), (crate::air::AirForce::Fighter, [0, 9, 0]));
        assert_eq!(st[0].counts, [0, 0, 0]);
    }

    #[test]
    fn unit_kinds() {
        assert_eq!(UnitKind::from_id(1), UnitKind::Ground { class: 0, tier: 1 });
        assert_eq!(UnitKind::from_id(14), UnitKind::Ground { class: 4, tier: 2 });
        assert_eq!(UnitKind::from_id(40), UnitKind::Advancer);
        assert_eq!(UnitKind::max_hp(40), Some(800));
        assert_eq!(UnitKind::max_hp(15), Some(550));
        assert_eq!(UnitKind::max_hp(7), Some(180));
        assert_eq!(UnitKind::max_hp(100), None);
        let m = Marker { kind: 40, player: 0, x: 1872, y: 1776 };
        assert_eq!((m.cell(), m.is_advancer()), ((58, 55), true));
    }

    #[test]
    fn writes_what_it_reads_and_keeps_a_tail_apart() {
        let d = sample();
        assert_eq!(Level::parse(&d).unwrap().to_bytes(), d);
        let mut tailed = d.clone();
        tailed.extend(b"OSOW tail");
        assert!(Level::parse(&tailed).is_err(), "a plain level ends at its footer");
        let (l, used) = Level::parse_prefix(&tailed).unwrap();
        assert_eq!((used, l.objects.len()), (d.len(), 1));
    }

    #[test]
    fn rejects_bad_footer_and_magic() {
        let mut d = sample();
        *d.last_mut().unwrap() ^= 1;
        assert!(Level::parse(&d).is_err());
        let mut d = sample();
        d[2] = 0;
        assert!(Level::parse(&d).is_err());
    }
}
