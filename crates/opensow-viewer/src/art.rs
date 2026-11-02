//! Resolves level objects to sprite frames from the game's `sprites.data` archive.

use opensow_formats::level::{Marker, Object, ObjectKind, Owner};
use opensow_formats::anim::{phase_after, walk_params, WalkParams};
use opensow_formats::fire::FireTables;
use opensow_formats::megafile::MegaFile;
use opensow_formats::recoil::{fire_sequence, FireSequence, FLASH_SETS};
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_formats::units::unit_art;
use opensow_formats::GameDir;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

/// Walk-cycle parameters of a chassis sprite: `frame = phase * steps + facing` (`facing` already scaled to `steps`).
#[derive(Clone, Copy)]
pub struct Walk {
    pub params: WalkParams,
    pub facing: usize,
}

impl Walk {
    pub fn frame_at(&self, ticks: u32) -> usize {
        phase_after(self.params, ticks) as usize * self.params.steps as usize + self.facing
    }
}

/// Fire/recoil animation of a weapon top.
#[derive(Clone)]
pub struct Recoil {
    pub seq: FireSequence,
    /// Facings per phase of the top's sprite set (32, or 16 for the walker tops).
    pub steps: usize,
    /// Facing already scaled to `steps`.
    pub facing: usize,
    pub flash: Option<Rc<Vec<Frame>>>,
}

/// One sprite to draw: `frame` of `set`, anchored (hotspot) at map pixel `ax,ay`.
pub enum Source {
    Object(usize),
    Marker(usize),
}

pub struct Placed {
    pub source: Source,
    /// Draw order within the same row: chassis before top.
    pub layer: u8,
    pub set: Rc<Vec<Frame>>,
    pub frame: usize,
    pub shadow: Option<(Rc<Vec<Frame>>, usize)>,
    pub ax: i32,
    pub ay: i32,
    /// Muzzle positions in map pixels (one per barrel) for weapon tops.
    pub muzzles: Vec<(i32, i32)>,
    /// Set for chassis that animate while walking.
    pub walk: Option<Walk>,
    pub recoil: Option<Recoil>,
}

pub struct Art {
    mega: MegaFile,
    cache: HashMap<String, Option<Rc<Vec<Frame>>>>,
}

fn building_stem(kind: ObjectKind) -> Option<&'static str> {
    Some(match kind {
        ObjectKind::Headquarters => "_base",
        ObjectKind::LightFactory => "_light",
        ObjectKind::MediumFactory => "_med",
        ObjectKind::HeavyFactory => "_heavy",
        ObjectKind::AirBase => "_air",
        ObjectKind::GoldMine => "_mine",
        ObjectKind::ResearchLab => "_rlab",
        ObjectKind::Wind => "_wind",
        ObjectKind::WalkerFactory => "_walker",
        _ => return None,
    })
}

impl Art {
    pub fn open(game: &GameDir) -> Option<Art> {
        let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).ok()?;
        Some(Art { mega, cache: HashMap::new() })
    }

    /// Sprite file `name` (e.g. `units\\chA_blue.ps6`) from `sprites.data`, cached.
    pub fn load(&mut self, name: &str) -> Option<Rc<Vec<Frame>>> {
        if let Some(c) = self.cache.get(name) {
            return c.clone();
        }
        let kind = Kind::from_extension(Path::new(name))?;
        let parsed = self
            .mega
            .find(name)
            .and_then(|e| self.mega.read(e).ok())
            .and_then(|d| sprite::parse(&d, kind).ok())
            .filter(|f| !f.is_empty())
            .map(Rc::new);
        self.cache.insert(name.to_owned(), parsed.clone());
        parsed
    }

    /// Sprite for an object, or `None` when the game has no art for it (e.g. turret 4).
    pub fn place(&mut self, index: usize, o: &Object) -> Option<Placed> {
        let (cx, cy) = (o.cell_x * 32 + 16, o.cell_y * 32 + 16);
        let kind = o.object_kind();
        if let ObjectKind::Turret(n) = kind {
            let team = if o.owner() == Owner::Enemy { "gren" } else { "blue" };
            let set = self.load(&format!("units\\tur_0{n}_{team}.ps6"))?;
            return Some(Placed { source: Source::Object(index), layer: 0, set, frame: 0, shadow: None, ax: cx, ay: cy, muzzles: vec![], walk: None, recoil: None });
        }
        let stem = building_stem(kind)?;
        let team = if o.owner() == Owner::Enemy { "green" } else { "blue" };
        let set = self.load(&format!("builds\\{team}\\{stem}.ps6"))?;
        // Frame layout seen on every set: 0 scaffold, 1 construction, 2.. idle animation,
        // then damaged, destroyed and (last) a grey "neutral" look; the HQ set is shorter.
        let idle = 4.min(set.len() - 1);
        let frame = match o.owner() {
            Owner::Neutral => {
                let blue = if team == "blue" { set.clone() } else { self.load(&format!("builds\\blue\\{stem}.ps6"))? };
                return self.neutral(index, stem, blue, cx, cy);
            }
            _ => idle,
        };
        let shadow = self.load(&format!("builds\\shadow\\{stem}.sha")).map(|s| {
            let i = frame.min(s.len() - 1);
            (s, i)
        });
        Some(Placed { source: Source::Object(index), layer: 0, set, frame, shadow, ax: cx, ay: cy, muzzles: vec![], walk: None, recoil: None })
    }

    /// Chassis + weapon top for a unit marker. Markers carry no facing, so the caller picks one of 32
    /// (0 = north, 8 = east, ... clockwise; see `opensow_formats::facing`); `aim` turns the weapon top independently of
    /// the chassis, as the game does (the top keeps its own facing). Sets with fewer facings
    /// (16-facing walker tops, 1-row sets) are indexed proportionally.
    pub fn place_marker(&mut self, index: usize, m: &Marker, facing: usize, aim: Option<usize>) -> Vec<Placed> {
        let Some(art) = unit_art(m.kind) else { return vec![] };
        let team = if m.player == 0 { "blue" } else { "gren" };
        let mut out = Vec::new();
        for (layer, name) in [Some(art.chassis), art.top].into_iter().flatten().enumerate() {
            let Some(set) = self.load(&format!("units\\{name}_{team}.ps6")) else { continue };
            // `1I`, the Advancer's top, is a 16x2 placeholder with nothing to draw.
            if set[0].width <= 16 && set[0].height <= 2 {
                continue;
            }
            let params = (layer == 0).then(|| walk_params(m.kind)).flatten();
            // The chassis follows the unit's facing; the weapon top (layer 1) follows its own aim facing.
            let face = if layer == 1 { aim.unwrap_or(facing) } else { facing } % 32;
            let steps = params.map_or(set.len().min(32), |p| p.steps as usize).max(1);
            let frame = (face * steps / 32).min(set.len() - 1);
            let recoil = if layer == 1 {
                fire_sequence(m.kind).map(|seq| {
                    let flash = seq.flash_set.and_then(|f| self.load(&format!("units\\{}_fire.ps6", FLASH_SETS[f as usize])));
                    Recoil { seq, steps, facing: face * steps / 32, flash }
                })
            } else {
                None
            };
            let walk = params.filter(|p| p.phases > 0).map(|params| Walk { params, facing: face * steps / 32 });
            let muzzles = if layer == 1 { self.muzzles(name, frame, m.x, m.y) } else { Vec::new() };
            let shadow = self.load(&format!("units\\{name}_{team}.sha")).map(|s| {
                let i = frame.min(s.len() - 1);
                (s, i)
            });
            // Marker pixels are the unit centre (cell*32+16).
            out.push(Placed { source: Source::Marker(index), layer: layer as u8, set, frame, shadow, ax: m.x, ay: m.y, muzzles, walk, recoil });
        }
        out
    }

    /// `frame` is the top's own frame index: fire-point tables of 16-facing tops only fill their first 16 entries.
    fn muzzles(&mut self, top: &str, frame: usize, x: i32, y: i32) -> Vec<(i32, i32)> {
        let Some(e) = self.mega.find(&format!("units\\{top}.po1")) else { return Vec::new() };
        let Some(t) = self.mega.read(e).ok().and_then(|d| FireTables::parse(&d).ok()) else { return Vec::new() };
        (0..t.variants()).filter_map(|v| t.muzzle(v, frame)).map(|(dx, dy)| (x + dx, y + dy)).collect()
    }

    fn neutral(&mut self, index: usize, stem: &str, blue: Rc<Vec<Frame>>, cx: i32, cy: i32) -> Option<Placed> {
        let frame = blue.len() - 1;
        let shadow = self.load(&format!("builds\\shadow\\{stem}.sha")).map(|s| {
            let i = 4.min(s.len() - 1);
            (s, i)
        });
        Some(Placed { source: Source::Object(index), layer: 0, set: blue, frame, shadow, ax: cx, ay: cy, muzzles: vec![], walk: None, recoil: None })
    }
}
