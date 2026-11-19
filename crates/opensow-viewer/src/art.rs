//! Resolves level objects to sprite frames from the game's `sprites.data` archive.

use opensow_formats::anim::{phase_after, walk_params, WalkParams};
use opensow_formats::buildings::BuildingAnim;
use opensow_formats::fire::FireTables;
use opensow_formats::level::{Marker, Object, ObjectKind, Owner};
use opensow_formats::megafile::MegaFile;
use opensow_formats::recoil::{fire_sequence, FireSequence, FLASH_SETS};
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_formats::units::{unit_art, SheetSizes};
use opensow_formats::GameDir;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

/// Walk-cycle parameters of a chassis sprite: `frame = phase * steps + facing` (`facing` already scaled to `steps`).
#[derive(Clone, Copy)]
pub struct Walk {
    /// The chassis kind's walking values.
    pub params: WalkParams,
    /// The facing, already scaled to the chassis' number of facings.
    pub facing: usize,
}

impl Walk {
    /// The sprite frame after `ticks` game ticks of walking.
    pub fn frame_at(&self, ticks: u32) -> usize {
        phase_after(self.params, ticks) as usize * self.params.steps as usize + self.facing
    }
}

/// Fire/recoil animation of a weapon top.
#[derive(Clone)]
pub struct Recoil {
    /// The top kind's fire sequence.
    pub seq: FireSequence,
    /// Facings per phase of the top's sprite set (32, or 16 for the walker tops).
    pub steps: usize,
    /// Facing already scaled to `steps`.
    pub facing: usize,
    /// The muzzle-flash sprite set, for tops that have one.
    pub flash: Option<Rc<Vec<Frame>>>,
}

/// What a placed sprite belongs to.
pub enum Source {
    /// Object `i` of the level (a building or turret).
    Object(usize),
    /// Unit marker `i` of the level.
    Marker(usize),
}

/// One sprite to draw: `frame` of `set`, anchored (hotspot) at map pixel `ax,ay`.
pub struct Placed {
    /// The level object or marker the sprite belongs to.
    pub source: Source,
    /// Draw order within the same row: chassis before top.
    pub layer: u8,
    /// The sprite file's frames.
    pub set: Rc<Vec<Frame>>,
    /// Which frame of `set` is shown.
    pub frame: usize,
    /// The shadow sprite's frames and the frame shown, when the sprite has a shadow.
    pub shadow: Option<(Rc<Vec<Frame>>, usize)>,
    /// Map pixel x of the sprite's hotspot.
    pub ax: i32,
    /// Map pixel y of the sprite's hotspot.
    pub ay: i32,
    /// Draw-order key (`scene` sorts by it, then by layer): the object's foot line; a turret's top sits above its base but is drawn after it.
    pub sort_y: i32,
    /// Muzzle positions in map pixels (one per barrel) for weapon tops.
    pub muzzles: Vec<(i32, i32)>,
    /// Set for chassis that animate while walking.
    pub walk: Option<Walk>,
    /// Set for weapon tops that have a fire sequence.
    pub recoil: Option<Recoil>,
    /// Set for buildings with state art (`buildings::anim`).
    pub anim: Option<BuildingAnim>,
    /// The sprite sets of both teams of a building, so a captured one can change colour.
    pub art: Option<Box<BuildArt>>,
}

/// Team art of a building and what it currently shows.
pub struct BuildArt {
    /// The blue team's frames.
    pub blue: Rc<Vec<Frame>>,
    /// The green team's frames.
    pub green: Rc<Vec<Frame>>,
    /// Whose colours the building shows now.
    pub owner: Owner,
    /// `builds\\mask\\{stem}.msk`: the part of each building frame that stands in front of units (the exe draws it with the
    /// sprite, `0x4092f0` / `0x40df80`); the front ramp of a factory is left out, so units come out over it.
    pub mask: Option<Rc<Vec<Frame>>>,
    /// Frame of `mask` that goes with the frame shown.
    pub mask_frame: usize,
    /// Rebuilding after a capture: the exe constructs a new building at 1 hit point (`0x43ab40`), shown as scaffold, construction, finished.
    pub up: bool,
}

impl Placed {
    /// The mask frame of a building: where its sprite hides units standing behind it.
    pub fn mask(&self) -> Option<&Frame> {
        let a = self.art.as_ref()?;
        a.mask.as_ref().and_then(|m| m.get(a.mask_frame))
    }

    /// Shows a building as the exe's state machine does (`0x40e099`..`0x40e0eb`): scaffold stages while it is being built up
    /// after a capture (hit points in thirds), else the idle loop, the damaged frame at half hit points or less, or for a
    /// neutral one its grey look; while its destruction animation runs (`dying`) the destroyed state 5 (`0x40e08c`). `heal` is
    /// the hit points still to be added.
    pub fn set_building_state(&mut self, owner: Owner, hp: i32, max_hp: i32, heal: i32, tick: u32, dying: bool) {
        use opensow_formats::buildings::*;
        let (Some(a), Some(art)) = (self.anim, self.art.as_mut()) else { return };
        if owner != art.owner {
            art.owner = owner;
            art.up = owner != Owner::Neutral && heal > 0;
        }
        if art.up && heal == 0 {
            art.up = false;
        }
        let (set, frame) = if owner == Owner::Neutral {
            // the state code `0x40e0df..0x40e0ee` forces state 6 (neutral) for owner 2 whatever the hit points are
            (art.blue.clone(), a.range(STATE_NEUTRAL).0)
        } else {
            let set = if owner == Owner::Enemy { art.green.clone() } else { art.blue.clone() };
            if art.up {
                let third = max_hp / 3;
                (
                    set,
                    if hp < third {
                        0
                    } else if hp < 2 * third {
                        1
                    } else {
                        2
                    },
                )
            } else if dying {
                // turrets have no frame for state 5 (bounds 5..5): `0x4092f0` still draws its first index, the base's frame 5
                (set, a.range(STATE_DESTROYED).0)
            } else {
                (set, a.frame(hp, max_hp, tick))
            }
        };
        self.frame = frame.min(set.len() - 1);
        art.mask_frame = frame;
        self.set = set;
        let shadow_frame = if art.up { self.frame } else { frame };
        if let Some((s, i)) = &mut self.shadow {
            *i = shadow_frame.min(s.len() - 1);
        }
    }
}

/// Loads sprites from the install's `sprites.data` archive.
pub struct Art {
    mega: MegaFile,
    /// The language pack's own `sprites.data` (`Data\DiffData\<pack>`): its files (HUD frame, pause menu, money, shortcuts ...) replace the base ones.
    local: Option<MegaFile>,
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
    /// Opens the sprite archive of an install; `None` when it cannot be read.
    pub fn open(game: &GameDir) -> Option<Art> {
        let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).ok()?;
        let local = game.language().diff_dir().and_then(|d| {
            let (i, b) = (game.resolve(&format!("Data\\DiffData\\{d}\\sprites.info"))?, game.resolve(&format!("Data\\DiffData\\{d}\\sprites.data"))?);
            MegaFile::open(&i, &b).ok()
        });
        Some(Art { mega, local, cache: HashMap::new() })
    }

    /// Sprite file `name` (e.g. `units\\chA_blue.ps6`) from `sprites.data`, cached.
    pub fn load(&mut self, name: &str) -> Option<Rc<Vec<Frame>>> {
        if let Some(c) = self.cache.get(name) {
            return c.clone();
        }
        let kind = Kind::from_extension(Path::new(name))?;
        let pack = self.local.as_ref().and_then(|m| m.find(name).and_then(|e| m.read(e).ok()));
        let parsed = pack
            .or_else(|| self.mega.find(name).and_then(|e| self.mega.read(e).ok()))
            .and_then(|d| sprite::parse(&d, kind).ok())
            .filter(|f| !f.is_empty())
            .map(Rc::new);
        self.cache.insert(name.to_owned(), parsed.clone());
        parsed
    }

    /// The header words `extra[0]`, `extra[1]` of frame 0 of every unit sheet (`units\\{name}_blue.ps6`), read without decoding the pixels:
    /// the numbers the exe's unit initialiser turns into the sprite size fields of a saved unit (`formats::units::SheetSizes`).
    pub fn sheet_sizes(&mut self) -> SheetSizes {
        let words = |name: &str| -> Option<(u16, u16)> {
            let e = self.mega.find(&format!("units\\{name}_blue.ps6"))?;
            let d = self.mega.read(e).ok()?;
            // u32 frame size, then width, height, hot x, hot y, extra[0], extra[1]
            let w = |at: usize| d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
            Some((w(12)?, w(14)?))
        };
        let mut out = SheetSizes::default();
        for (i, n) in opensow_formats::units::CHASSIS.iter().enumerate() {
            out.chassis[i] = words(n);
        }
        for (i, n) in opensow_formats::units::TOPS.iter().enumerate() {
            out.tops[i] = words(n);
        }
        out
    }

    /// Sprite for an object, or `None` when the game has no art for it (e.g. turret 4).
    pub fn place(&mut self, index: usize, o: &Object) -> Vec<Placed> {
        let turret = opensow_formats::buildings::turret_art(o.kind);
        let Some(base) = self.place_building(index, o, turret.map(|t| t.base)) else { return Vec::new() };
        let sort_y = base.sort_y;
        let mut out = vec![base];
        // a turret is a base plus a gun top that the exe creates as a separate unit (kinds 109-113): drawn over the base
        if let Some(t) = turret {
            let team = if o.owner() == Owner::Enemy { "gren" } else { "blue" };
            if let Some(set) = self.load(&format!("units\\{}_{team}.ps6", t.top)) {
                let (x, y) = opensow_formats::buildings::turret_top_pos(o.kind, (o.cell_x, o.cell_y));
                let shadow = self.load(&format!("units\\{}_{team}.sha", t.top)).map(|s| (s, 0));
                // tops are created facing north (docs/art.md); the game then turns them (aim and idle look-around)
                let frame = 0;
                out.push(Placed {
                    source: Source::Object(index),
                    layer: 1,
                    set,
                    frame,
                    shadow,
                    ax: x,
                    ay: y + t.top_dy,
                    sort_y: sort_y + 1,
                    muzzles: self.muzzles(t.top, frame, x, y + t.top_dy),
                    walk: None,
                    recoil: None,
                    anim: None,
                    art: None,
                });
            }
        }
        out
    }

    fn place_building(&mut self, index: usize, o: &Object, turret_base: Option<&str>) -> Option<Placed> {
        let kind = o.object_kind();
        // the exe draws a building's hotspot at `cell * 32 + (b+0x1c, b+0x20)` (buildings::art_offset), not at the cell centre
        let (ox, oy) = opensow_formats::buildings::art_offset(o.kind);
        let (cx, cy) = (o.cell_x * 32 + ox, o.cell_y * 32 + oy);
        let stem = match turret_base {
            Some(b) => b,
            None => building_stem(kind)?,
        };
        let blue = self.load(&format!("builds\\blue\\{stem}.ps6"))?;
        let green = self.load(&format!("builds\\green\\{stem}.ps6")).unwrap_or_else(|| blue.clone());
        // Frame layout (`buildings::anim`): 0 scaffold, 1 construction, 2 finished, 3.. idle loop,
        // then damaged, destroyed and (last) a grey "neutral" look; the HQ set is shorter (one frame per state: 3 healthy, 4 damaged).
        let anim = opensow_formats::buildings::anim(o.kind);
        let shadow = self.load(&format!("builds\\shadow\\{stem}.sha")).map(|s| (s, 0));
        let owner = o.owner();
        let mask = self.load(&format!("builds\\mask\\{stem}.msk"));
        let art = BuildArt { blue: blue.clone(), green: green.clone(), owner, mask, mask_frame: 0, up: false };
        let mut p = Placed {
            source: Source::Object(index),
            layer: 0,
            set: blue,
            frame: 0,
            shadow,
            ax: cx,
            ay: cy,
            sort_y: cy,
            muzzles: vec![],
            walk: None,
            recoil: None,
            anim,
            art: Some(Box::new(art)),
        };
        p.set_building_state(owner, 1, 1, 0, 0, false);
        Some(p)
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
            out.push(Placed {
                source: Source::Marker(index),
                layer: layer as u8,
                set,
                frame,
                shadow,
                ax: m.x,
                ay: m.y,
                sort_y: m.y,
                muzzles,
                walk,
                recoil,
                anim: None,
                art: None,
            });
        }
        out
    }

    /// The `.po1` muzzle tables of a weapon top (`units\\{top}.po1`).
    pub fn fire_tables(&mut self, top: &str) -> Option<FireTables> {
        let e = self.mega.find(&format!("units\\{top}.po1"))?;
        self.mega.read(e).ok().and_then(|d| FireTables::parse(&d).ok())
    }

    /// The weapon's reference point table `units\\{top}.po2` (one table; the spawner `0x47cf40` ends a shot at the target plus this point minus the barrel's).
    pub fn fire_tables_ref(&mut self, top: &str) -> Option<FireTables> {
        let e = self.mega.find(&format!("units\\{top}.po2"))?;
        self.mega.read(e).ok().and_then(|d| FireTables::parse(&d).ok())
    }

    /// `frame` is the top's own frame index: fire-point tables of 16-facing tops only fill their first 16 entries.
    fn muzzles(&mut self, top: &str, frame: usize, x: i32, y: i32) -> Vec<(i32, i32)> {
        let Some(e) = self.mega.find(&format!("units\\{top}.po1")) else { return Vec::new() };
        let Some(t) = self.mega.read(e).ok().and_then(|d| FireTables::parse(&d).ok()) else { return Vec::new() };
        (0..t.variants()).filter_map(|v| t.muzzle(v, frame)).map(|(dx, dy)| (x + dx, y + dy)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(n: usize, tag: u16) -> Rc<Vec<Frame>> {
        Rc::new(
            (0..n)
                .map(|_| Frame { width: 1, height: 1, hot_x: 0, hot_y: 0, extra: [0; 4], pixels: vec![tag], mask: vec![true], enhanced: Default::default() })
                .collect(),
        )
    }

    fn placed(owner: Owner) -> Placed {
        let (blue, green) = (set(18, 1), set(17, 2));
        let art = BuildArt { blue: blue.clone(), green, owner, mask: None, mask_frame: 0, up: false };
        Placed {
            source: Source::Object(0),
            layer: 0,
            set: blue,
            frame: 0,
            shadow: None,
            ax: 0,
            ay: 0,
            sort_y: 0,
            muzzles: vec![],
            walk: None,
            recoil: None,
            anim: opensow_formats::buildings::anim(106),
            art: Some(Box::new(art)),
        }
    }

    #[test]
    fn neutral_buildings_never_show_damage_and_captured_ones_change_colour() {
        let mut p = placed(Owner::Neutral);
        p.set_building_state(Owner::Neutral, 100, 100, 0, 0, false);
        assert_eq!((p.set[0].pixels[0], p.frame), (1, 17)); // grey neutral frame of the blue set
        p.set_building_state(Owner::Neutral, 49, 100, 0, 0, false);
        assert_eq!(p.frame, 17); // about half hit points is still the regular look
        p.set_building_state(Owner::Neutral, 30, 100, 0, 0, false);
        assert_eq!((p.set[0].pixels[0], p.frame), (1, 17), "the exe shows state 6 for a neutral building at any hit points");
        p.set_building_state(Owner::Neutral, 1, 100, 0, 0, false);
        assert_eq!(p.frame, 17);
        // captured: a new building at 1 hp builds up (scaffold, construction, finished) in the new colour
        p.set_building_state(Owner::Enemy, 1, 100, 99, 5, false);
        assert_eq!((p.set[0].pixels[0], p.frame), (2, 0));
        p.set_building_state(Owner::Enemy, 40, 100, 60, 5, false);
        assert_eq!(p.frame, 1);
        p.set_building_state(Owner::Enemy, 90, 100, 10, 5, false);
        assert_eq!(p.frame, 2);
        p.set_building_state(Owner::Enemy, 100, 100, 0, 5, false);
        assert!((3..15).contains(&p.frame));
    }

    #[test]
    fn dying_structures_show_the_destroyed_state_frame() {
        // kind 106: 4 damaged = 15, 5 destroyed = 16, 6 neutral = 17 (`0x40e08c`: a dead building gets state 5)
        let mut p = placed(Owner::Enemy);
        p.set_building_state(Owner::Enemy, 0, 100, 0, 5, false);
        assert_eq!(p.frame, 15);
        p.set_building_state(Owner::Enemy, 0, 100, 0, 5, true);
        assert_eq!(p.frame, 16);
        // `0x40e0df`: the neutral state wins over the destroyed one
        p.set_building_state(Owner::Neutral, 0, 100, 0, 5, true);
        assert_eq!(p.frame, 17);
        // still building up (`[b+0x68]`): the stage by thirds wins (`0x40e061` comes before the dead check)
        let mut p = placed(Owner::Enemy);
        p.set_building_state(Owner::Player, 1, 100, 99, 5, false);
        p.set_building_state(Owner::Player, 0, 100, 99, 5, true);
        assert_eq!(p.frame, 0);
        // turrets: state 5 is empty (bounds 5..5), so `0x4092f0` draws frame 5 + 0 of the base
        let mut p = placed(Owner::Enemy);
        p.anim = opensow_formats::buildings::anim(25);
        p.set_building_state(Owner::Enemy, 0, 100, 0, 5, true);
        assert_eq!(p.frame, 5);
    }
}
