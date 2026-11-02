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
use crate::{Error, Reader, Result};

pub const MAGIC: u16 = 9870;
pub const OBJECT_TAG: i32 = 123;
pub const MARKER_TAG: i32 = 224;
pub const FOOTER: u32 = 7656;
pub const HEADER_WORDS: usize = 94;

/// What an [`Object`] is, from its kind number.
///
/// Evidence: the exe's building-name table (`_base _light _med _heavy _walker _air _mine _rlab _wind`),
/// `Data/Sprites2/InfoBox/2/{Base,Light,Med,Heavy,Walk,Air,Mine,Rlab,Wind,Tur1..5}.tsp`, the exe treating
/// kinds {101,102,103,108} as unit producers, and the manual's per-map descriptions
/// (heavy factories first appear on map 06, walker factories only on maps 04/05/07/08,
/// gold mines dominate "Gold Rush", air bases are absent from "First Spark").
/// Kind 107 (`Wind`) is assumed to be the energy source (InfoBox has energy meters); unconfirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// Kinds 25..=29 = `tur_01`..`tur_05` (HP 900/1250/1540/2100/2600); `1..=5`.
    Turret(u8),
    Headquarters,
    LightFactory,
    MediumFactory,
    HeavyFactory,
    AirBase,
    GoldMine,
    ResearchLab,
    Wind,
    WalkerFactory,
    Unknown(i32),
}

impl ObjectKind {
    pub fn from_id(id: i32) -> ObjectKind {
        match id {
            25..=29 => ObjectKind::Turret((id - 24) as u8),
            100 => ObjectKind::Headquarters,
            101 => ObjectKind::LightFactory,
            102 => ObjectKind::MediumFactory,
            103 => ObjectKind::HeavyFactory,
            104 => ObjectKind::AirBase,
            105 => ObjectKind::GoldMine,
            106 => ObjectKind::ResearchLab,
            107 => ObjectKind::Wind,
            108 => ObjectKind::WalkerFactory,
            other => ObjectKind::Unknown(other),
        }
    }

    pub fn is_building(self) -> bool {
        !matches!(self, ObjectKind::Turret(_) | ObjectKind::Unknown(_))
    }

    /// Whether the exe's factory logic applies (kinds 101, 102, 103, 108).
    pub fn produces_units(self) -> bool {
        matches!(self, ObjectKind::LightFactory | ObjectKind::MediumFactory | ObjectKind::HeavyFactory | ObjectKind::WalkerFactory)
    }

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Player,
    Enemy,
    Neutral,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub kind: i32,
    pub cell_x: i32,
    pub cell_y: i32,
    pub raw: [i32; 19],
}

impl Object {
    pub fn object_kind(&self) -> ObjectKind {
        ObjectKind::from_id(self.kind)
    }

    pub fn owner(&self) -> Owner {
        match self.raw[14] {
            0 => Owner::Player,
            1 => Owner::Enemy,
            2 => Owner::Neutral,
            o => Owner::Unknown(o),
        }
    }

    /// Current hit points (word 13); buildings top out at ~13536.
    pub fn hp(&self) -> i32 {
        self.raw[13]
    }
}

/// Marker kind of the Advancer (always 2 per two-player map, 4 on "Cross of Fire").
/// Confirmed from the manual ("two Headquarters and two Advancers" per side on that map) and from the
/// exe: the loader skips kind 40 when a per-player flag is set, and the unit constructor gives it 800 HP.
pub const MARKER_ADVANCER: i32 = 40;

/// What a [`Marker`] spawns. Decoded from the exe's script loader (`0x41da00`, which calls the unit
/// constructor `0x46bd30` with `(cell_x, cell_y, player, kind)`) and its per-kind tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    /// Kinds 1..=15: five classes of three tiers. `class = (kind-1)/3` (0..5), `tier = (kind-1)%3 + 1`.
    /// Classes in order: anti-air, artillery, cannon, flamer, special (see [`crate::units`]);
    /// levels place only tier 1 and 2 markers.
    Ground { class: u8, tier: u8 },
    /// Kind 40: "the most important unit in the game" (manual): repairs/destroys turrets and captures buildings.
    Advancer,
    /// Kinds 41..=45 are the five walker robots (`5W1`..`5W5` tops on the `chW` legs); 46 is a bare `chN`
    /// pod (1000 HP, 17 placed in campaign levels; role unknown).
    Special(u8),
    /// Kinds 109..=113 exist in the exe but the script loader never spawns them (walkers/specials created at runtime).
    RuntimeOnly(i32),
    Unknown(i32),
}

impl UnitKind {
    pub fn from_id(id: i32) -> UnitKind {
        match id {
            1..=15 => UnitKind::Ground { class: ((id - 1) / 3) as u8, tier: ((id - 1) % 3 + 1) as u8 },
            40 => UnitKind::Advancer,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub kind: i32,
    /// Player index passed to the unit constructor (0 = first player/blue, 1 = second/green).
    pub player: i32,
    pub x: i32,
    pub y: i32,
}

impl Marker {
    pub fn unit_kind(&self) -> UnitKind {
        UnitKind::from_id(self.kind)
    }

    pub fn is_advancer(&self) -> bool {
        self.kind == MARKER_ADVANCER
    }

    /// Cell the unit is created in (the exe uses `pixel >> 5`).
    pub fn cell(&self) -> (i32, i32) {
        (self.x >> 5, self.y >> 5)
    }
}

#[derive(Debug, Clone)]
pub struct Level {
    pub version: u16,
    /// Header words; word 0 packs `version | magic << 16`.
    pub header: [i32; HEADER_WORDS],
    pub objects: Vec<Object>,
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

    /// Starting air-force stocks: header words `4+3g ..= 6+3g` for group `g` in [`HEADER_ORDER`].
    /// The exe's level loader (`0x41d88e..0x41d9bd`) pushes each type code onto a per-side queue that many times.
    pub fn air_stock(&self) -> [AirStock; 5] {
        std::array::from_fn(|g| {
            let w = &self.header[4 + 3 * g..7 + 3 * g];
            AirStock { force: HEADER_ORDER[g], counts: [w[0], w[1], w[2]] }
        })
    }

    pub fn parse(data: &[u8]) -> Result<Level> {
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
        if footer != FOOTER || r.remaining() != 0 {
            return Err(Error::Invalid(format!(
                "level tail: footer {footer}, {} bytes left (record layout mismatch at {:#x})",
                r.remaining(),
                r.pos()
            )));
        }
        Ok(Level { version, header, objects, markers })
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
        l.header[5 + 3 * 3] = 4; // group 3 (fighter), side 1
        l.header[6 + 3 * 4] = 9; // group 4 (tripler), side 2
        let st = l.air_stock();
        assert_eq!((st[3].force, st[3].counts), (crate::air::AirForce::Tripler, [0, 4, 0]));
        assert_eq!((st[4].force, st[4].counts), (crate::air::AirForce::Fighter, [0, 0, 9]));
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
    fn rejects_bad_footer_and_magic() {
        let mut d = sample();
        *d.last_mut().unwrap() ^= 1;
        assert!(Level::parse(&d).is_err());
        let mut d = sample();
        d[2] = 0;
        assert!(Level::parse(&d).is_err());
    }
}
