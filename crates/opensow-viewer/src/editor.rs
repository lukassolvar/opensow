//! Skirmish map editor (`opensow-viewer --edit MAP`, docs/map-editor.md): moves, adds, removes and re-owns the buildings,
//! turrets and units of a `.net` map and saves the result as edits of the shipped file (`formats::maptweak`, the text file
//! `data/skirmish-maps.txt` the game compiles in). Pure state and CPU drawing; the window lives in `main.rs`.

use crate::render::{blend, pack, rect, View};
use crate::scene::CELL;
use opensow_formats::buildings::footprint;
use opensow_formats::custommap::ObjectExt;
use opensow_formats::font::Font;
use opensow_formats::kinds;
use opensow_formats::level::Level;
use opensow_formats::map::Map;
use opensow_formats::maptweak::{self, object_name, unit_name, Edit, Production, Side, Thing, Tracked, MAX_LINES};

/// What is selected: an object (building or turret) or a unit marker, by index in the edited level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sel {
    /// Object `i` of the edited level.
    Object(usize),
    /// Unit marker `i` of the edited level.
    Unit(usize),
}

/// What can be added, in palette order: buildings, turrets, the Advancer, ground units (class by class, tier 1-3), walkers, pod.
pub fn palette() -> Vec<Thing> {
    let mut v: Vec<Thing> = [100, 101, 102, 103, 108, 104, 105, 106, 107, 25, 26, 27, 28, 29].into_iter().map(Thing::Object).collect();
    v.push(Thing::Unit(40));
    v.extend((1..=15).map(Thing::Unit));
    v.extend((41..=46).map(Thing::Unit));
    v
}

/// The name shown for a building, turret or unit.
pub fn thing_name(t: Thing) -> String {
    match t {
        Thing::Object(k) => object_name(k),
        Thing::Unit(k) => format!("unit {}", unit_name(k)),
    }
}

/// The name the editor shows for a kind (`Gold mine`, `Cannon 2`, `Advancer`).
pub fn display_name(t: Thing) -> String {
    match t {
        Thing::Object(k @ 25..=29) => ["Guard cannon", "A.A. defence", "Plasma mob", "LedStorm", "Artillery turret"][(k - 25) as usize].to_string(),
        Thing::Object(k) => opensow_formats::level::ObjectKind::from_id(k).name().replace("Wind (energy?)", "Windtrap"),
        Thing::Unit(k @ 1..=15) => format!("{} {}", ["Anti-air", "Artillery", "Cannon", "Flamer", "Special"][((k - 1) / 3) as usize], (k - 1) % 3 + 1),
        Thing::Unit(40) => "Advancer".into(),
        Thing::Unit(k @ 41..=45) => format!("Walker {}", k - 40),
        Thing::Unit(46) => "Pod".into(),
        Thing::Unit(k) => format!("Unit {k}"),
    }
}

/// How much a building is worth to take, as the skirmish Advancer weighs it (`sim/advancer_ai.rs` `worth`).
pub fn worth(kind: i32) -> f32 {
    match kind {
        100 => 10.0,
        105 => 9.0,
        103 | 108 => 8.0,
        102 => 7.0,
        106 => 6.0,
        101 | 104 => 5.0,
        107 | 26 => 4.0,
        25 | 27..=29 => 3.0,
        _ => 2.0,
    }
}

/// What a building's production lines can hold (docs/production.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    /// Light, medium and heavy factories: ground units 1-15 (5 classes x 3 tiers), each line with the tiers research may add.
    Ground,
    /// Air base: air forces 30-34.
    Air,
    /// Walker factory: walkers 41-45 and the pod 46.
    Walkers,
    /// Gold mine, research lab, windtrap: their upgrade levels `base`, `base + 1`, `base + 2` (16 gold, 19 research, 22 energy).
    Levels(i32),
}

/// What kind of production lines a building kind has; `None` for buildings without production.
pub fn lines_of(kind: i32) -> Option<Lines> {
    match kind {
        101..=103 => Some(Lines::Ground),
        104 => Some(Lines::Air),
        108 => Some(Lines::Walkers),
        105 => Some(Lines::Levels(16)),
        106 => Some(Lines::Levels(19)),
        107 => Some(Lines::Levels(22)),
        _ => None,
    }
}

/// The tier ranges a ground line can have: (start tier, top tier).
const TIER_RANGES: [(i32, i32); 6] = [(1, 1), (1, 2), (1, 3), (2, 2), (2, 3), (3, 3)];

/// What a production line is called in the editor (`Cannon T1-2`, `Fighter`, `Walker 3`, `Gold 2`).
pub fn line_label(kind: i32, rise: i32) -> String {
    match kind {
        1..=15 => {
            let (class, tier) = ((kind - 1) / 3, (kind - 1) % 3 + 1);
            let name = ["Anti-air", "Artillery", "Cannon", "Flamer", "Special"][class as usize];
            if rise > 0 {
                format!("{name} T{tier}-{}", (tier + rise).min(3))
            } else {
                format!("{name} T{tier}")
            }
        }
        16..=24 => format!("{} {}", ["Gold", "Research", "Energy"][((kind - 16) / 3) as usize], (kind - 16) % 3 + 1),
        30..=34 => ["Bomber", "Carryall", "Fighter", "Tripler", "Asteroid"][(kind - 30) as usize].to_string(),
        41..=45 => format!("Walker {}", kind - 40),
        46 => "Pod".into(),
        k => format!("Kind {k}"),
    }
}

/// A change of the selected building's production lines (`Editor::change_lines`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChange {
    /// Lines (or levels) running from the start, up or down.
    Active(bool),
    /// Next / previous kind of line `i` (ground: the class, keeping the tiers).
    Kind(usize, bool),
    /// Next tier range of ground line `i` (T1, T1-2, T1-3, T2, T2-3, T3).
    Tiers(usize),
    /// Remove line `i`.
    Remove(usize),
    /// Add a line.
    Add,
    /// One upgrade level more / less (gold mine, lab, windtrap).
    Levels(bool),
    /// A click on level slot `i` (0-2) of a mine, lab or windtrap: a missing level is added (with the ones before it); an
    /// existing one becomes the last unlocked at the start, or, when it already is, locked.
    LevelSlot(usize),
    /// The lock of line `i`: an unlocked line becomes the first locked one (with the lines after it), a locked one the last
    /// unlocked at the start (with the lines before it). Lines unlock in order, so the unlocked ones always come first.
    Lock(usize),
}

/// The map editor's state: the shipped level, the edited copy, the selection and the undo history.
pub struct Editor {
    /// The `.net` file stem (`08-Island of Fate`).
    pub map: String,
    /// The level as shipped, before any edit.
    pub original: Level,
    /// The edited level, with where each object and marker came from.
    pub t: Tracked,
    /// What is selected.
    pub sel: Option<Sel>,
    /// Index into [`palette`] of what a click on the map places while [`Editor::placing`].
    pub palette: usize,
    /// A palette item is armed: clicks on the map place it (else they select and drag).
    pub placing: bool,
    /// Owner of what is added.
    pub side: Side,
    /// A short note on the last action (refusals, saves).
    pub message: String,
    /// Per object (same index as `level().objects`): what a custom map adds (docs/map-editor-ingame.md); defaults for the shipped
    /// maps' tweaks, which do not use it.
    pub ext: Vec<ObjectExt>,
    width: i32,
    height: i32,
    blocked: Vec<bool>,
    no_build: Vec<bool>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    saved: Vec<Edit>,
}

type Snapshot = (Tracked, Option<Sel>, Vec<ObjectExt>);

impl Editor {
    /// `edits` = what the tweaks file holds for the map now (they count as saved).
    pub fn new(map: &str, original: Level, edits: &[Edit], terrain: &Map) -> Editor {
        let (t, skipped) = maptweak::apply(&original, edits);
        let message = if skipped.is_empty() { String::new() } else { format!("{} saved edit(s) name nothing in the map and were dropped", skipped.len()) };
        let saved = maptweak::diff(&original, &t);
        Editor {
            map: map.to_string(),
            original,
            t,
            sel: None,
            palette: palette().iter().position(|&t| t == Thing::Object(105)).unwrap_or(0),
            placing: false,
            side: Side::Neutral,
            message,
            ext: Vec::new(),
            width: terrain.width as i32,
            height: terrain.height as i32,
            blocked: terrain.cells.iter().map(|c| c.blocked).collect(),
            no_build: terrain.cells.iter().map(|c| c.no_build).collect(),
            undo: Vec::new(),
            redo: Vec::new(),
            saved,
        }
        .with_ext_len()
    }

    /// The editor on a custom map: `level` is its own starting point (no shipped original), `ext` its additions.
    pub fn custom(name: &str, level: Level, ext: Vec<ObjectExt>, terrain: &Map) -> Editor {
        let mut e = Editor::new(name, level, &[], terrain);
        e.ext = ext;
        e.with_ext_len()
    }

    fn with_ext_len(mut self) -> Editor {
        self.ext.resize(self.t.level.objects.len(), ObjectExt::default());
        self
    }

    /// The edited level.
    pub fn level(&self) -> &Level {
        &self.t.level
    }

    /// The edits that turn the shipped map into the edited one.
    pub fn edits(&self) -> Vec<Edit> {
        maptweak::diff(&self.original, &self.t)
    }

    /// Whether there are edits that are not saved.
    pub fn dirty(&self) -> bool {
        self.edits() != self.saved
    }

    /// Call after the edits were written to the file.
    pub fn mark_saved(&mut self) {
        self.saved = self.edits();
    }

    fn snapshot(&mut self) {
        self.undo.push((self.t.clone(), self.sel, self.ext.clone()));
        self.redo.clear();
    }

    /// Takes back the last edit; false when there is none.
    pub fn undo(&mut self) -> bool {
        let Some((t, sel, ext)) = self.undo.pop() else { return false };
        self.redo.push((std::mem::replace(&mut self.t, t), self.sel, std::mem::replace(&mut self.ext, ext)));
        self.sel = sel;
        true
    }

    /// Repeats the edit that was last taken back; false when there is none.
    pub fn redo(&mut self) -> bool {
        let Some((t, sel, ext)) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.t, t), self.sel, std::mem::replace(&mut self.ext, ext)));
        self.sel = sel;
        true
    }

    /// Replaces the level header (starting resources of the sides, `custommap::Sides`); undoable.
    pub fn set_header(&mut self, header: [i32; opensow_formats::level::HEADER_WORDS]) -> bool {
        if header == self.t.level.header {
            return false;
        }
        self.snapshot();
        self.t.level.header = header;
        true
    }

    /// Replaces every object's extension at once (the Advancer switches of a side); undoable.
    pub fn set_ext(&mut self, ext: Vec<ObjectExt>) -> bool {
        let mut ext = ext;
        ext.resize(self.t.level.objects.len(), ObjectExt::default());
        if ext == self.ext {
            return false;
        }
        self.snapshot();
        self.ext = ext;
        true
    }

    /// The start hit points of object `i` in percent (object word 18), 5..=100; undoable.
    pub fn set_start_hp(&mut self, i: usize, pct: i32) -> bool {
        let pct = pct.clamp(5, 100);
        if self.t.level.objects.get(i).is_none_or(|o| o.raw[18] == pct) {
            return false;
        }
        self.snapshot();
        self.t.level.objects[i].raw[18] = pct;
        true
    }

    /// Satellite protection of object `i` (object word 15, `[b+0x114]`: the exe loads it for every building and turret,
    /// `0x41dffc`, and its satellite check `0x409f50` kills foreign Advancers attacking it); undoable. A headquarters is always
    /// protected (its constructor sets the flag, `0x40a47a`), so it is not changed.
    pub fn set_protected(&mut self, i: usize, on: bool) -> bool {
        if self.t.level.objects.get(i).is_none_or(|o| o.kind == kinds::HEADQUARTERS || (o.raw[15] != 0) == on) {
            return false;
        }
        self.snapshot();
        self.t.level.objects[i].raw[15] = i32::from(on);
        true
    }

    /// Makes the selected turret one of type `tier` (1-5, kinds 25-29) when the new type's footprint fits; undoable.
    pub fn set_turret_type(&mut self, tier: i32) -> bool {
        let Some(Sel::Object(i)) = self.sel else { return false };
        let o = &self.t.level.objects[i];
        let kind = 24 + tier.clamp(1, 5);
        if !kinds::TURRETS.contains(&o.kind) || o.kind == kind {
            return false;
        }
        if let Err(e) = self.can_place(Thing::Object(kind), (o.cell_x, o.cell_y), Some(Sel::Object(i))) {
            self.message = format!("Cannot change the turret: {e}");
            return false;
        }
        self.snapshot();
        let o = &mut self.t.level.objects[i];
        o.kind = kind;
        o.raw[1] = kind;
        o.raw[13] = [900, 1250, 1540, 2100, 2600][(kind - 25) as usize];
        true
    }

    /// Sets the upgrade points of line `k` of the selected building (0 = the game's own); undoable.
    pub fn set_unlock(&mut self, k: usize, points: i32) -> bool {
        let Some(Sel::Object(i)) = self.sel else { return false };
        if k >= opensow_formats::custommap::LINES || self.ext.get(i).is_none_or(|e| e.unlock[k] == points.max(0)) {
            return false;
        }
        self.snapshot();
        self.ext[i].unlock[k] = points.max(0);
        true
    }

    /// The topmost unit (within 12 px of its position) or else object (its footprint) at map pixel `(mx, my)`.
    pub fn pick(&self, mx: f32, my: f32) -> Option<Sel> {
        let l = &self.t.level;
        if let Some(i) = (0..l.markers.len()).rev().find(|&i| ((l.markers[i].x as f32 - mx).abs()).max((l.markers[i].y as f32 - my).abs()) <= 12.0) {
            return Some(Sel::Unit(i));
        }
        let (cx, cy) = ((mx / CELL as f32).floor() as i32, (my / CELL as f32).floor() as i32);
        (0..l.objects.len())
            .rev()
            .find(|&i| {
                let o = &l.objects[i];
                let (w, h, _) = footprint(o.kind).unwrap_or((1, 1, 0));
                (o.cell_x..o.cell_x + w).contains(&cx) && (o.cell_y..o.cell_y + h).contains(&cy)
            })
            .map(Sel::Object)
    }

    /// The kind of thing a selection is.
    pub fn thing_of(&self, s: Sel) -> Thing {
        match s {
            Sel::Object(i) => Thing::Object(self.t.level.objects[i].kind),
            Sel::Unit(i) => Thing::Unit(self.t.level.markers[i].kind),
        }
    }

    /// The cell a selected object or marker stands on.
    pub fn cell_of(&self, s: Sel) -> (i32, i32) {
        match s {
            Sel::Object(i) => (self.t.level.objects[i].cell_x, self.t.level.objects[i].cell_y),
            Sel::Unit(i) => self.t.level.markers[i].cell(),
        }
    }

    /// The owner of a selected object or marker.
    pub fn side_of(&self, s: Sel) -> Side {
        let code = match s {
            Sel::Object(i) => self.t.level.objects[i].raw[14],
            Sel::Unit(i) => self.t.level.markers[i].player,
        };
        Side::from_code(code).unwrap_or(Side::Neutral)
    }

    fn terrain(&self, x: i32, y: i32) -> Option<(bool, bool)> {
        (x >= 0 && y >= 0 && x < self.width && y < self.height).then(|| {
            let i = (y * self.width + x) as usize;
            (self.blocked[i], self.no_build[i])
        })
    }

    /// Whether `thing` may stand at `cell` (its top-left footprint cell; a unit's cell), `ignore` being the one that moves:
    /// a building or turret needs its whole footprint inside the map, on passable, buildable cells and off every other
    /// object's footprint; a unit a passable cell off every building's footprint (their open top row aside).
    pub fn can_place(&self, thing: Thing, cell: (i32, i32), ignore: Option<Sel>) -> Result<(), String> {
        let l = &self.t.level;
        let objects = l.objects.iter().enumerate().filter(|(i, _)| ignore != Some(Sel::Object(*i)));
        match thing {
            Thing::Object(k) => {
                let (w, h, _) = footprint(k).ok_or("unknown building")?;
                for y in cell.1..cell.1 + h {
                    for x in cell.0..cell.0 + w {
                        match self.terrain(x, y) {
                            None => return Err("outside the map".into()),
                            Some((true, _)) => return Err("blocked terrain".into()),
                            Some((_, true)) => return Err("no building allowed here".into()),
                            _ => {}
                        }
                    }
                }
                for (_, o) in objects {
                    let (ow, oh, _) = footprint(o.kind).unwrap_or((1, 1, 0));
                    if o.cell_x < cell.0 + w && cell.0 < o.cell_x + ow && o.cell_y < cell.1 + h && cell.1 < o.cell_y + oh {
                        return Err(format!("on the {} at {},{}", object_name(o.kind), o.cell_x, o.cell_y));
                    }
                }
            }
            Thing::Unit(_) => {
                match self.terrain(cell.0, cell.1) {
                    None => return Err("outside the map".into()),
                    Some((true, _)) => return Err("blocked terrain".into()),
                    _ => {}
                }
                for (_, o) in objects {
                    let (ow, oh, _) = footprint(o.kind).unwrap_or((1, 1, 0));
                    let top = i32::from(o.kind >= 100);
                    if (o.cell_x..o.cell_x + ow).contains(&cell.0) && (o.cell_y + top..o.cell_y + oh).contains(&cell.1) {
                        return Err(format!("inside the {} at {},{}", object_name(o.kind), o.cell_x, o.cell_y));
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether a carryall can land a whole squad at `cell` (`custommap::landing_block`, 4x2 cells): every cell inside the map,
    /// on passable ground and off every building's and turret's footprint.
    pub fn can_land(&self, cell: (i32, i32)) -> Result<(), String> {
        for (x, y) in opensow_formats::custommap::landing_block(cell) {
            match self.terrain(x, y) {
                None => return Err("outside the map".into()),
                Some((true, _)) => return Err("blocked terrain".into()),
                _ => {}
            }
            for o in &self.t.level.objects {
                let (ow, oh, _) = footprint(o.kind).unwrap_or((1, 1, 0));
                if (o.cell_x..o.cell_x + ow).contains(&x) && (o.cell_y..o.cell_y + oh).contains(&y) {
                    return Err(format!("on the {} at {},{}", object_name(o.kind), o.cell_x, o.cell_y));
                }
            }
        }
        Ok(())
    }

    /// The map's size in cells.
    pub fn grid_size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Where a ground unit can stand (row-major, [`Editor::grid_size`]): passable terrain off every building's footprint
    /// (their open top row aside) and every turret's (the rule of [`Editor::can_place`] for units).
    pub fn walkable(&self) -> Vec<bool> {
        let mut g: Vec<bool> = self.blocked.iter().map(|b| !b).collect();
        for o in &self.t.level.objects {
            let (ow, oh, _) = footprint(o.kind).unwrap_or((1, 1, 0));
            let top = i32::from(o.kind >= 100);
            for y in o.cell_y + top..o.cell_y + oh {
                for x in o.cell_x..o.cell_x + ow {
                    if x >= 0 && y >= 0 && x < self.width && y < self.height {
                        g[(y * self.width + x) as usize] = false;
                    }
                }
            }
        }
        g
    }

    /// Moves the selection to `cell`; a refusal leaves everything as it was and says why in `message`.
    pub fn move_sel(&mut self, cell: (i32, i32)) -> bool {
        let Some(s) = self.sel else { return false };
        if self.cell_of(s) == cell {
            return false;
        }
        if let Err(e) = self.can_place(self.thing_of(s), cell, Some(s)) {
            self.message = format!("Cannot move there: {e}");
            return false;
        }
        self.snapshot();
        match s {
            Sel::Object(i) => {
                let o = &mut self.t.level.objects[i];
                (o.cell_x, o.cell_y) = cell;
                (o.raw[16], o.raw[17]) = cell;
            }
            Sel::Unit(i) => {
                let m = &mut self.t.level.markers[i];
                m.x = cell.0 * CELL + (m.x & 31);
                m.y = cell.1 * CELL + (m.y & 31);
            }
        }
        self.message = format!("Moved {} to {},{}", display_name(self.thing_of(s)), cell.0, cell.1);
        true
    }

    /// Removes the selection (never a side's last headquarters).
    pub fn remove_sel(&mut self) -> bool {
        self.remove_sel_keeping(&[0, 1])
    }

    /// Removes the selection; the last headquarters of the sides (owner codes) in `keep` stays (a custom escort map lets blue
    /// do without one).
    pub fn remove_sel_keeping(&mut self, keep: &[i32]) -> bool {
        let Some(s) = self.sel else { return false };
        if let Sel::Object(i) = s {
            let o = &self.t.level.objects[i];
            if o.kind == kinds::HEADQUARTERS
                && keep.contains(&o.raw[14])
                && self.t.level.objects.iter().filter(|p| p.kind == kinds::HEADQUARTERS && p.raw[14] == o.raw[14]).count() == 1
            {
                self.message = "Cannot remove a side's last headquarters".into();
                return false;
            }
        }
        self.snapshot();
        let name = display_name(self.thing_of(s));
        match s {
            Sel::Object(i) => {
                self.t.level.objects.remove(i);
                self.t.object_origin.remove(i);
                if i < self.ext.len() {
                    self.ext.remove(i);
                }
            }
            Sel::Unit(i) => {
                self.t.level.markers.remove(i);
                self.t.marker_origin.remove(i);
            }
        }
        self.sel = None;
        self.message = format!("Removed {name}");
        true
    }

    /// The selected building's production lines and what they can hold; `None` for anything else.
    pub fn lines(&self) -> Option<(Lines, Production)> {
        let Some(Sel::Object(i)) = self.sel else { return None };
        let o = &self.t.level.objects[i];
        Some((lines_of(o.kind)?, Production::of(o)))
    }

    /// Changes the selected building's production lines; `false` (and nothing done) when the change does not apply.
    pub fn change_lines(&mut self, c: LineChange) -> bool {
        let (Some(Sel::Object(i)), Some((family, mut p))) = (self.sel, self.lines()) else { return false };
        let cycle = |v: i32, lo: i32, hi: i32, up: bool| {
            if up {
                if v >= hi {
                    lo
                } else {
                    v + 1
                }
            } else if v <= lo {
                hi
            } else {
                v - 1
            }
        };
        match (c, family) {
            (LineChange::Active(up), _) => {
                let n = (p.active + if up { 1 } else { -1 }).clamp(0, p.lines.len() as i32);
                if n == p.active {
                    return false;
                }
                p.active = n;
            }
            (LineChange::Levels(up), Lines::Levels(base)) => {
                let n = (p.lines.len() as i32 + if up { 1 } else { -1 }).clamp(1, 3);
                if n == p.lines.len() as i32 {
                    return false;
                }
                p.lines = (0..n).map(|k| (base + k, 0)).collect();
                p.active = p.active.min(n);
            }
            (LineChange::LevelSlot(k), Lines::Levels(base)) if k < 3 => {
                let n = p.lines.len();
                if k >= n {
                    p.lines = (0..=k as i32).map(|j| (base + j, 0)).collect();
                } else {
                    p.active = if p.active == k as i32 + 1 { k as i32 } else { k as i32 + 1 };
                }
            }
            (LineChange::Lock(k), _) if k < p.lines.len() => {
                p.active = if (k as i32) < p.active { k as i32 } else { k as i32 + 1 };
            }
            (LineChange::Kind(k, up), _) if k < p.lines.len() => {
                let (kind, rise) = p.lines[k];
                p.lines[k].0 = match family {
                    Lines::Ground if kinds::GROUND_UNITS.contains(&kind) => {
                        let (class, tier) = ((kind - 1) / 3, (kind - 1) % 3);
                        cycle(class, 0, 4, up) * 3 + tier + 1
                    }
                    Lines::Air => cycle(kind.clamp(30, 34), 30, 34, up),
                    Lines::Walkers => cycle(kind.clamp(41, 46), 41, 46, up),
                    _ => return false,
                };
                let _ = rise;
            }
            (LineChange::Tiers(k), Lines::Ground) if k < p.lines.len() && kinds::GROUND_UNITS.contains(&p.lines[k].0) => {
                let (kind, rise) = p.lines[k];
                let (class, start) = ((kind - 1) / 3, (kind - 1) % 3 + 1);
                let now = TIER_RANGES.iter().position(|&r| r == (start, (start + rise).min(3))).unwrap_or(0);
                let (s, top) = TIER_RANGES[(now + 1) % TIER_RANGES.len()];
                p.lines[k] = (class * 3 + s, top - s);
            }
            (LineChange::Remove(k), Lines::Ground | Lines::Air | Lines::Walkers) if k < p.lines.len() && p.lines.len() > 1 => {
                p.lines.remove(k);
                p.active = p.active.min(p.lines.len() as i32);
                self.snapshot();
                if let Some(e) = self.ext.get_mut(i) {
                    e.unlock.copy_within(k + 1.., k);
                    e.unlock[opensow_formats::custommap::LINES - 1] = 0;
                }
                p.write(&mut self.t.level.objects[i]);
                self.message.clear();
                return true;
            }
            (LineChange::Add, Lines::Ground | Lines::Air | Lines::Walkers) if p.lines.len() < MAX_LINES => {
                let line = match family {
                    // a cannon line at tier 1 that research may raise as far as the factory's first line
                    Lines::Ground => (7, p.lines.first().map_or(1, |l| l.1)),
                    Lines::Air => ((30..=34).find(|k| !p.lines.iter().any(|l| l.0 == *k)).unwrap_or(30), 0),
                    _ => ((41..=46).find(|k| !p.lines.iter().any(|l| l.0 == *k)).unwrap_or(41), 0),
                };
                p.lines.push(line);
            }
            _ => return false,
        }
        self.snapshot();
        p.write(&mut self.t.level.objects[i]);
        self.message.clear();
        true
    }

    /// Sets the owner of the selection (units and headquarters are never neutral; a side keeps one headquarters).
    pub fn set_owner(&mut self, side: Side) -> bool {
        let Some(s) = self.sel else { return false };
        if self.side_of(s) == side {
            return false;
        }
        if side == Side::Neutral && (matches!(s, Sel::Unit(_)) || self.thing_of(s) == Thing::Object(100)) {
            self.message = "Units and headquarters belong to blue or green".into();
            return false;
        }
        if let Sel::Object(i) = s {
            let o = &self.t.level.objects[i];
            if o.kind == kinds::HEADQUARTERS && self.t.level.objects.iter().filter(|p| p.kind == kinds::HEADQUARTERS && p.raw[14] == o.raw[14]).count() == 1 {
                self.message = "Each side needs a headquarters".into();
                return false;
            }
        }
        self.snapshot();
        match s {
            Sel::Object(i) => maptweak::set_owner(&mut self.t.level.objects[i], side),
            Sel::Unit(i) => self.t.level.markers[i].player = side.code(),
        }
        self.message = format!("{} is now {}", display_name(self.thing_of(s)), side.name());
        true
    }

    /// Adds the palette's thing for [`Editor::side`] at `cell` and selects it.
    pub fn add(&mut self, cell: (i32, i32)) -> bool {
        let thing = palette()[self.palette];
        if matches!(thing, Thing::Unit(_)) && self.side == Side::Neutral {
            self.message = "Units belong to blue or green".into();
            return false;
        }
        if thing == Thing::Object(100) && self.side == Side::Neutral {
            self.message = "A headquarters belongs to blue or green".into();
            return false;
        }
        if let Err(e) = self.can_place(thing, cell, None) {
            self.message = format!("Cannot place it there: {e}");
            return false;
        }
        self.snapshot();
        let (one, _) = maptweak::apply(&self.original, &[Edit::Add(thing, cell, self.side, None)]);
        match thing {
            Thing::Object(_) => {
                self.t.level.objects.push(one.level.objects.last().unwrap().clone());
                self.t.object_origin.push(None);
                self.ext.resize(self.t.level.objects.len(), ObjectExt::default());
                self.sel = Some(Sel::Object(self.t.level.objects.len() - 1));
            }
            Thing::Unit(_) => {
                self.t.level.markers.push(one.level.markers.last().unwrap().clone());
                self.t.marker_origin.push(None);
                self.sel = Some(Sel::Unit(self.t.level.markers.len() - 1));
            }
        }
        self.message = format!("Added {} {} at {},{}", self.side.name(), display_name(thing), cell.0, cell.1);
        true
    }

    /// Next owner of the selection: neutral -> blue -> green -> neutral (units and headquarters skip neutral).
    pub fn cycle_owner(&mut self) -> bool {
        let Some(s) = self.sel else { return false };
        let only_sides = matches!(s, Sel::Unit(_)) || self.thing_of(s) == Thing::Object(100);
        let next = match (self.side_of(s), only_sides) {
            (Side::Neutral, _) => Side::Blue,
            (Side::Blue, _) => Side::Green,
            (Side::Green, true) => Side::Blue,
            (Side::Green, false) => Side::Neutral,
        };
        if let Sel::Object(i) = s {
            let o = &self.t.level.objects[i];
            if o.kind == kinds::HEADQUARTERS && self.t.level.objects.iter().filter(|p| p.kind == kinds::HEADQUARTERS && p.raw[14] == o.raw[14]).count() == 1 {
                self.message = "Each side needs a headquarters".into();
                return false;
            }
        }
        self.snapshot();
        match s {
            Sel::Object(i) => maptweak::set_owner(&mut self.t.level.objects[i], next),
            Sel::Unit(i) => self.t.level.markers[i].player = next.code(),
        }
        self.message = format!("{} is now {}", display_name(self.thing_of(s)), next.name());
        true
    }

    /// Centre cell of each side's first headquarters (blue, green).
    pub fn hq_centres(&self) -> [Option<(f32, f32)>; 2] {
        std::array::from_fn(|side| {
            self.t
                .level
                .objects
                .iter()
                .find(|o| o.kind == kinds::HEADQUARTERS && o.raw[14] == side as i32)
                .map(|o| (o.cell_x as f32 + 2.0, o.cell_y as f32 + 2.0))
        })
    }

    /// Flight distance in cells from each side's headquarters to the centre of `s`.
    pub fn flight(&self, s: Sel) -> [Option<f32>; 2] {
        let (x, y) = match s {
            Sel::Object(i) => {
                let o = &self.t.level.objects[i];
                let (w, h, _) = footprint(o.kind).unwrap_or((1, 1, 0));
                (o.cell_x as f32 + w as f32 / 2.0, o.cell_y as f32 + h as f32 / 2.0)
            }
            Sel::Unit(i) => (self.t.level.markers[i].x as f32 / CELL as f32, self.t.level.markers[i].y as f32 / CELL as f32),
        };
        self.hq_centres().map(|c| c.map(|(hx, hy)| ((hx - x).powi(2) + (hy - y).powi(2)).sqrt()))
    }

    /// How much of the neutral buildings each side's Advancer can reach: the sum of worth / (1 + flight / 16) (the skirmish
    /// Advancer's nearness weighting), blue then green. Close numbers = an even map for captures.
    pub fn reach(&self) -> [f32; 2] {
        let mut s = [0.0f32; 2];
        for (i, o) in self.t.level.objects.iter().enumerate().filter(|(_, o)| o.raw[14] == 2) {
            for (side, d) in self.flight(Sel::Object(i)).iter().enumerate() {
                if let Some(d) = d {
                    s[side] += worth(o.kind) / (1.0 + d / 16.0);
                }
            }
        }
        s
    }

    /// Whether there is something to undo / redo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an edit to repeat.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Back to the shipped map (undoable).
    pub fn revert(&mut self) -> bool {
        if self.edits().is_empty() {
            return false;
        }
        self.snapshot();
        self.t = maptweak::apply(&self.original, &[]).0;
        self.sel = None;
        self.message = "Back to the shipped map".into();
        true
    }

    /// Where an added `thing` goes for the cell under the mouse: a building centred on it.
    pub fn add_cell(thing: Thing, cursor: (i32, i32)) -> (i32, i32) {
        match thing {
            Thing::Object(k) => footprint(k).map_or(cursor, |(w, h, _)| (cursor.0 - (w - 1) / 2, cursor.1 - (h - 1) / 2)),
            Thing::Unit(_) => cursor,
        }
    }

    /// On the map: rings round the headquarters, the frame of what is under the mouse and of the selection, the ghost of a
    /// drag (`drag` = where it would land) or of the palette's thing about to be placed (`place` = the cell under the mouse);
    /// green when it fits, red when it does not.
    pub fn draw_marks(&self, buf: &mut [u32], w: usize, h: usize, view: &View, hover: Option<Sel>, drag: Option<(i32, i32)>, place: Option<(i32, i32)>) {
        let outline = |buf: &mut [u32], s: Sel, at: (i32, i32), c: (u8, u8, u8)| {
            let (fw, fh) = match self.thing_of(s) {
                Thing::Object(k) => footprint(k).map_or((1, 1), |(a, b, _)| (a, b)),
                Thing::Unit(_) => (1, 1),
            };
            let (x0, y0) = view.to_screen((at.0 * CELL) as f32, (at.1 * CELL) as f32);
            let (x1, y1) = view.to_screen(((at.0 + fw) * CELL) as f32, ((at.1 + fh) * CELL) as f32);
            let t = 2;
            rect(buf, w, h, x0, y0, x1, y0 + t, c, 255);
            rect(buf, w, h, x0, y1 - t, x1, y1, c, 255);
            rect(buf, w, h, x0, y0, x0 + t, y1, c, 255);
            rect(buf, w, h, x1 - t, y0, x1, y1, c, 255);
        };
        for (centre, colour) in self.hq_centres().into_iter().zip([(80u8, 130u8, 255u8), (60, 220, 90)]) {
            if let Some((hx, hy)) = centre.map(|(x, y)| view.to_screen(x * CELL as f32, y * CELL as f32)) {
                ring(buf, w, h, hx, hy, (2.6 * CELL as f32 * view.zoom) as i32, colour);
            }
        }
        if let Some(s) = hover.filter(|&s| Some(s) != self.sel) {
            outline(buf, s, self.cell_of(s), (230, 230, 230));
        }
        if let Some(s) = self.sel {
            outline(buf, s, self.cell_of(s), (255, 220, 40));
            if let Some(at) = drag.filter(|&at| at != self.cell_of(s)) {
                let ok = self.can_place(self.thing_of(s), at, Some(s)).is_ok();
                outline(buf, s, at, if ok { (60, 230, 90) } else { (240, 50, 40) });
            }
        }
        if let Some(cursor) = place {
            let thing = palette()[self.palette];
            let at = Editor::add_cell(thing, cursor);
            let (fw, fh) = match thing {
                Thing::Object(k) => footprint(k).map_or((1, 1), |(a, b, _)| (a, b)),
                Thing::Unit(_) => (1, 1),
            };
            let c = if self.can_place(thing, at, None).is_ok() { (60, 230, 90) } else { (240, 50, 40) };
            let (x0, y0) = view.to_screen((at.0 * CELL) as f32, (at.1 * CELL) as f32);
            let (x1, y1) = view.to_screen(((at.0 + fw) * CELL) as f32, ((at.1 + fh) * CELL) as f32);
            rect(buf, w, h, x0, y0, x1, y1, c, 70);
            for (a, b, cc, d) in [(x0, y0, x1, y0 + 2), (x0, y1 - 2, x1, y1), (x0, y0, x0 + 2, y1), (x1 - 2, y0, x1, y1)] {
                rect(buf, w, h, a, b, cc, d, c, 255);
            }
        }
    }
}

fn ring(buf: &mut [u32], w: usize, h: usize, cx: i32, cy: i32, r: i32, c: (u8, u8, u8)) {
    let n = (r * 7).max(24);
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        let (x, y) = (cx + (a.cos() * r as f32) as i32, cy + (a.sin() * r as f32) as i32);
        rect(buf, w, h, x - 1, y - 1, x + 1, y + 1, c, 220);
    }
}

/// Splits `s` at spaces so every line fits `max_w` pixels of `font`.
pub fn wrap(font: &Font, s: &str, max_w: i32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if font.measure(&next, 1) > max_w && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = next;
        }
    }
    out.push(cur);
    out
}

/// Draws `s` with the game's mask font pair (inner fill `c`, black outline) at screen `(x, y)` (line top), each font pixel `scale` px.
pub fn text(buf: &mut [u32], w: usize, h: usize, fonts: &(Font, Font), x: i32, y: i32, scale: i32, s: &str, c: (u8, u8, u8)) {
    let (inner, outer) = fonts;
    let placed = inner.layout(s, 1);
    for pass in [true, false] {
        for p in &placed {
            let (g, col) = if pass {
                match outer.glyph(p.ch) {
                    Some(g) => (g, (0, 0, 0)),
                    None => continue,
                }
            } else {
                (p.glyph, c)
            };
            // both masks at `pen - hot` (docs/fonts.md: the `_i` fill nests in the `_o` outline)
            let (left, top) = (p.x - g.hot_x as i32, -(g.hot_y as i32));
            for gy in 0..g.height as i32 {
                for gx in 0..g.width as i32 {
                    if !g.mask[(gy * g.width as i32 + gx) as usize] {
                        continue;
                    }
                    let (px, py) = (x + (left + gx) * scale, y + (top + gy) * scale);
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let (sx, sy) = (px + dx, py + dy);
                            if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                                let i = sy as usize * w + sx as usize;
                                buf[i] = if pass { blend(buf[i], col.0, col.1, col.2, 255) } else { pack(col.0, col.1, col.2) };
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensow_formats::level::{Marker, Object, HEADER_WORDS};
    use opensow_formats::map::Cell;

    fn obj(kind: i32, x: i32, y: i32, owner: i32) -> Object {
        let mut raw = [0i32; 19];
        (raw[0], raw[1], raw[14], raw[16], raw[17]) = (123, kind, owner, x, y);
        Object { kind, cell_x: x, cell_y: y, raw }
    }

    fn terrain() -> Map {
        let (w, h) = (40u32, 30u32);
        let cells = (0..w * h)
            .map(|i| Cell {
                x: (i % w) as u8,
                y: (i / w) as u8,
                blocked: i % w == 20 && i / w < 10,
                border: false,
                no_build: i % w == 25 && i / w == 5,
                no_marks: false,
            })
            .collect();
        Map { a: 0, b: 0, width: w, height: h, cells }
    }

    impl Editor {
        /// Cells a unit may be placed on (the walkable grid's count, the slow way).
        fn can_place_count(&self) -> i32 {
            (0..self.height).flat_map(|y| (0..self.width).map(move |x| (x, y))).filter(|&c| self.can_place(Thing::Unit(1), c, None).is_ok()).count() as i32
        }
    }

    fn editor() -> Editor {
        let level = Level {
            version: 4,
            header: [0; HEADER_WORDS],
            objects: vec![obj(100, 1, 1, 0), obj(100, 30, 20, 1), obj(105, 10, 10, 2)],
            markers: vec![Marker { kind: 40, player: 0, x: 6 * 32 + 16, y: 2 * 32 + 16 }],
        };
        Editor::new("Test", level, &[], &terrain())
    }

    #[test]
    fn walkable_cells_leave_out_blocked_ground_and_building_bodies() {
        let e = editor();
        let g = e.walkable();
        let at = |x: i32, y: i32| g[(y * 40 + x) as usize];
        assert!(!at(20, 3) && at(20, 12), "the blocked column ends at row 9");
        assert!(at(11, 10) && !at(11, 11) && !at(12, 12) && at(13, 11), "the mine: top row open, the rest not");
        assert_eq!(g.iter().filter(|&&w| w).count() as i32, e.can_place_count());
    }

    #[test]
    fn a_landing_needs_its_four_by_two_cells_open_and_inside() {
        let e = editor();
        assert!(e.can_land((5, 15)).is_ok());
        assert!(e.can_land((18, 3)).is_err(), "x 17..=20 reaches the blocked column 20");
        assert!(e.can_land((16, 3)).is_ok(), "x 15..=18");
        assert!(e.can_land((10, 9)).is_err(), "row 10 is the mine's top row");
        assert!(e.can_land((13, 11)).is_err(), "x 12 is the mine's last column");
        assert!(e.can_land((14, 11)).is_ok(), "x 13..=16 clears it");
        assert!(e.can_land((0, 5)).is_err() && e.can_land((38, 5)).is_err() && e.can_land((5, 29)).is_err(), "outside the 40x30 map");
        assert!(e.can_land((1, 5)).is_ok() && e.can_land((37, 28)).is_ok(), "the edges themselves");
    }

    #[test]
    fn picking_prefers_units_then_footprints() {
        let e = editor();
        assert_eq!(e.pick(6.0 * 32.0 + 20.0, 2.0 * 32.0 + 10.0), Some(Sel::Unit(0)));
        assert_eq!(e.pick(11.5 * 32.0, 12.5 * 32.0), Some(Sel::Object(2)), "anywhere on the mine's 3x3 footprint");
        assert_eq!(e.pick(13.5 * 32.0, 12.5 * 32.0), None);
    }

    #[test]
    fn moves_are_checked_against_terrain_and_other_footprints() {
        let mut e = editor();
        e.sel = Some(Sel::Object(2));
        assert!(!e.move_sel((19, 2)), "the column x=20 is blocked up to y=9");
        assert!(e.message.contains("blocked"));
        assert!(!e.move_sel((24, 4)), "(25,5) is no-build");
        assert!(!e.move_sel((2, 2)), "on the blue headquarters");
        assert!(!e.move_sel((38, 2)), "the footprint would leave the map");
        assert!(e.move_sel((14, 14)));
        assert_eq!(e.edits().len(), 1);
        assert!(e.dirty());
        e.mark_saved();
        assert!(!e.dirty());
    }

    #[test]
    fn add_remove_owner_and_undo() {
        let mut e = editor();
        e.palette = palette().iter().position(|&t| t == Thing::Unit(7)).unwrap();
        assert!(!e.add((5, 20)), "units are never neutral");
        e.side = Side::Green;
        assert!(e.add((5, 20)));
        assert_eq!(e.sel, Some(Sel::Unit(1)));
        assert!(!e.add((1, 3)), "inside the headquarters (below its open top row)");
        e.sel = Some(Sel::Object(2));
        assert!(e.cycle_owner());
        assert_eq!(e.side_of(Sel::Object(2)), Side::Blue);
        e.sel = Some(Sel::Object(0));
        assert!(!e.remove_sel(), "blue's only headquarters stays");
        assert!(!e.cycle_owner(), "and keeps its side");
        e.sel = Some(Sel::Unit(0));
        assert!(e.remove_sel());
        let edits = e.edits();
        assert_eq!(edits.len(), 3, "{edits:?}");
        assert!(e.undo() && e.undo() && e.undo());
        assert!(e.edits().is_empty());
        assert!(e.redo());
        assert_eq!(e.edits().len(), 1);
    }

    #[test]
    fn production_lines_of_a_factory_and_the_levels_of_a_mine() {
        let mut e = editor();
        let mut f = obj(102, 15, 20, 2);
        maptweak::Production { active: 1, lines: vec![(7, 1), (13, 1)] }.write(&mut f);
        e.t.level.objects.push(f);
        e.t.object_origin.push(None);
        e.sel = Some(Sel::Object(3));
        let lines = |e: &Editor| e.lines().unwrap().1;
        assert!(e.change_lines(LineChange::Active(true)));
        assert!(!e.change_lines(LineChange::Active(true)), "no more than the lines");
        assert!(e.change_lines(LineChange::Kind(0, true)));
        assert_eq!(lines(&e).lines[0], (10, 1), "cannon T1 -> flamer T1, same tiers");
        assert!(e.change_lines(LineChange::Kind(1, true)));
        assert_eq!(lines(&e).lines[1], (1, 1), "special wraps round to anti-air");
        assert!(e.change_lines(LineChange::Tiers(0)));
        assert_eq!(lines(&e).lines[0], (10, 2), "T1-2 -> T1-3");
        assert_eq!(line_label(10, 2), "Flamer T1-3");
        assert!(e.change_lines(LineChange::Tiers(0)));
        assert_eq!(lines(&e).lines[0], (11, 0), "T1-3 -> T2");
        assert!(e.change_lines(LineChange::Add));
        assert_eq!(lines(&e).lines.len(), 3);
        assert!(e.change_lines(LineChange::Remove(0)) && e.change_lines(LineChange::Remove(0)));
        assert!(!e.change_lines(LineChange::Remove(0)), "a factory keeps one line");
        assert_eq!(lines(&e).active, 1, "active follows the lines left");
        assert!(!e.change_lines(LineChange::Levels(true)), "levels are for mines");
        // the mine of the test map: no lines in its words yet
        e.sel = Some(Sel::Object(2));
        assert!(e.change_lines(LineChange::Levels(true)));
        assert_eq!(lines(&e).lines, vec![(16, 0)]);
        assert!(e.change_lines(LineChange::Levels(true)) && e.change_lines(LineChange::Levels(true)));
        assert!(!e.change_lines(LineChange::Levels(true)), "three levels at most");
        assert_eq!(lines(&e).lines, vec![(16, 0), (17, 0), (18, 0)]);
        assert!(e.change_lines(LineChange::Active(true)));
        assert!(e.edits().iter().any(|x| matches!(x, Edit::Slots(..))));
        // level slots: a click on a level makes it the last unlocked one, a second click locks it
        assert!(e.change_lines(LineChange::LevelSlot(2)));
        assert_eq!(lines(&e).active, 3);
        assert!(e.change_lines(LineChange::LevelSlot(2)));
        assert_eq!(lines(&e).active, 2);
        assert!(e.change_lines(LineChange::Levels(false)) && e.change_lines(LineChange::Levels(false)));
        assert!(e.change_lines(LineChange::LevelSlot(2)), "a missing level is added with the ones before it");
        assert_eq!(lines(&e).lines.len(), 3);
        e.sel = Some(Sel::Object(0));
        assert!(e.lines().is_none(), "the headquarters has no lines");
    }

    #[test]
    fn custom_map_extensions_follow_the_objects_through_edits_and_undo() {
        let mut level = editor().level().clone();
        let mut f = obj(102, 15, 20, 2);
        maptweak::Production { active: 1, lines: vec![(7, 0), (13, 0), (4, 0)] }.write(&mut f);
        level.objects.push(f);
        let mut ext = vec![ObjectExt::default(); 4];
        ext[3].unlock = [0, 2500, 3500, 0, 0];
        let mut e = Editor::custom("t", level, ext, &terrain());
        e.sel = Some(Sel::Object(2));
        assert!(e.remove_sel());
        assert_eq!(e.ext.len(), 3);
        assert_eq!(e.ext[2].unlock[1], 2500, "the factory's extension moved with it");
        e.sel = Some(Sel::Object(2));
        assert!(e.change_lines(LineChange::Remove(1)));
        assert_eq!(e.ext[2].unlock[..3], [0, 3500, 0], "the removed line's points went with it");
        assert!(e.change_lines(LineChange::Lock(1)));
        assert_eq!(e.lines().unwrap().1.active, 2, "a locked line unlocks with the lines before it");
        assert!(e.change_lines(LineChange::Lock(0)));
        assert_eq!(e.lines().unwrap().1.active, 0);
        assert!(e.set_unlock(0, 900) && !e.set_unlock(0, 900));
        assert!(e.set_start_hp(2, 70) && e.level().objects[2].raw[18] == 70);
        let mut h = e.level().header;
        h[2] = 7000;
        assert!(e.set_header(h));
        e.palette = palette().iter().position(|&t| t == Thing::Object(25)).unwrap();
        e.side = Side::Blue;
        assert!(e.add((5, 25)));
        assert_eq!(e.ext.len(), 4);
        assert!(e.set_turret_type(5));
        assert_eq!(e.level().objects[3].kind, 29);
        for _ in 0..9 {
            assert!(e.undo());
        }
        assert!(!e.undo());
        assert_eq!((e.ext.len(), e.ext[3].unlock[1]), (4, 2500), "undo restores the objects and their extensions together");
        assert_eq!(e.level().header[2], 0);
    }

    #[test]
    fn reach_weighs_neutral_buildings_by_flight_from_each_headquarters() {
        let e = editor();
        let r = e.reach();
        // the mine's centre (11.5, 11.5): blue HQ centre (3,3) 12.0 cells, green (32,22) 23.0 cells
        assert!((r[0] - 9.0 / (1.0 + 12.02 / 16.0)).abs() < 0.05, "{r:?}");
        assert!(r[0] > r[1]);
        let f = e.flight(Sel::Object(2));
        assert!((f[0].unwrap() - 12.02).abs() < 0.05 && (f[1].unwrap() - 23.05).abs() < 0.05, "{f:?}");
    }

    #[test]
    fn long_status_lines_wrap() {
        let glyphs = (0..94)
            .map(|_| opensow_formats::sprite::Frame {
                width: 6,
                height: 8,
                hot_x: 0,
                hot_y: 0,
                extra: [0; 4],
                pixels: vec![0; 48],
                mask: vec![true; 48],
                enhanced: Default::default(),
            })
            .collect();
        let f = Font::new(glyphs);
        let lines = wrap(&f, "aaaa bbbb cccc dddd", 60);
        assert!(lines.len() > 1 && lines.iter().all(|l| f.measure(l, 1) <= 60 || !l.contains(' ')), "{lines:?}");
    }
}
