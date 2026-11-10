//! Skirmish map editor (`opensow-viewer --edit MAP`, docs/map-editor.md): moves, adds, removes and re-owns the buildings,
//! turrets and units of a `.net` map and saves the result as edits of the shipped file (`formats::maptweak`, the text file
//! `data/skirmish-maps.txt` the game compiles in). Pure state and CPU drawing; the window lives in `main.rs`.

use crate::render::{blend, pack, rect, View};
use crate::scene::CELL;
use opensow_formats::buildings::footprint;
use opensow_formats::font::Font;
use opensow_formats::level::Level;
use opensow_formats::map::Map;
use opensow_formats::maptweak::{self, object_name, unit_name, Edit, Production, Side, Thing, Tracked, MAX_LINES};

/// What is selected: an object (building or turret) or a unit marker, by index in the edited level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sel {
    Object(usize),
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
            if rise > 0 { format!("{name} T{tier}-{}", (tier + rise).min(3)) } else { format!("{name} T{tier}") }
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
    Remove(usize),
    Add,
    /// One upgrade level more / less (gold mine, lab, windtrap).
    Levels(bool),
    /// A click on level slot `i` (0-2) of a mine, lab or windtrap: a missing level is added (with the ones before it); an
    /// existing one becomes the last unlocked at the start, or, when it already is, locked.
    LevelSlot(usize),
}

pub struct Editor {
    /// The `.net` file stem (`08-Island of Fate`).
    pub map: String,
    pub original: Level,
    pub t: Tracked,
    pub sel: Option<Sel>,
    /// Index into [`palette`] of what a click on the map places while [`Editor::placing`].
    pub palette: usize,
    /// A palette item is armed: clicks on the map place it (else they select and drag).
    pub placing: bool,
    /// Owner of what is added.
    pub side: Side,
    /// A short note on the last action (refusals, saves).
    pub message: String,
    width: i32,
    height: i32,
    blocked: Vec<bool>,
    no_build: Vec<bool>,
    undo: Vec<(Tracked, Option<Sel>)>,
    redo: Vec<(Tracked, Option<Sel>)>,
    saved: Vec<Edit>,
}

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
            width: terrain.width as i32,
            height: terrain.height as i32,
            blocked: terrain.cells.iter().map(|c| c.blocked).collect(),
            no_build: terrain.cells.iter().map(|c| c.no_build).collect(),
            undo: Vec::new(),
            redo: Vec::new(),
            saved,
        }
    }

    pub fn level(&self) -> &Level {
        &self.t.level
    }

    /// The edits that turn the shipped map into the edited one.
    pub fn edits(&self) -> Vec<Edit> {
        maptweak::diff(&self.original, &self.t)
    }

    pub fn dirty(&self) -> bool {
        self.edits() != self.saved
    }

    /// Call after the edits were written to the file.
    pub fn mark_saved(&mut self) {
        self.saved = self.edits();
    }

    fn snapshot(&mut self) {
        self.undo.push((self.t.clone(), self.sel));
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some((t, sel)) = self.undo.pop() else { return false };
        self.redo.push((std::mem::replace(&mut self.t, t), self.sel));
        self.sel = sel;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((t, sel)) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.t, t), self.sel));
        self.sel = sel;
        true
    }

    /// The topmost unit (within 12 px of its position) or else object (its footprint) at map pixel `(mx, my)`.
    pub fn pick(&self, mx: f32, my: f32) -> Option<Sel> {
        let l = &self.t.level;
        if let Some(i) = (0..l.markers.len()).rev().find(|&i| ((l.markers[i].x as f32 - mx).abs()).max((l.markers[i].y as f32 - my).abs()) <= 12.0) {
            return Some(Sel::Unit(i));
        }
        let (cx, cy) = ((mx / CELL as f32).floor() as i32, (my / CELL as f32).floor() as i32);
        (0..l.objects.len()).rev().find(|&i| {
            let o = &l.objects[i];
            let (w, h, _) = footprint(o.kind).unwrap_or((1, 1, 0));
            (o.cell_x..o.cell_x + w).contains(&cx) && (o.cell_y..o.cell_y + h).contains(&cy)
        }).map(Sel::Object)
    }

    pub fn thing_of(&self, s: Sel) -> Thing {
        match s {
            Sel::Object(i) => Thing::Object(self.t.level.objects[i].kind),
            Sel::Unit(i) => Thing::Unit(self.t.level.markers[i].kind),
        }
    }

    pub fn cell_of(&self, s: Sel) -> (i32, i32) {
        match s {
            Sel::Object(i) => (self.t.level.objects[i].cell_x, self.t.level.objects[i].cell_y),
            Sel::Unit(i) => self.t.level.markers[i].cell(),
        }
    }

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
        let Some(s) = self.sel else { return false };
        if let Sel::Object(i) = s {
            let o = &self.t.level.objects[i];
            if o.kind == 100 && self.t.level.objects.iter().filter(|p| p.kind == 100 && p.raw[14] == o.raw[14]).count() == 1 {
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
        let cycle = |v: i32, lo: i32, hi: i32, up: bool| if up { if v >= hi { lo } else { v + 1 } } else if v <= lo { hi } else { v - 1 };
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
            (LineChange::Kind(k, up), _) if k < p.lines.len() => {
                let (kind, rise) = p.lines[k];
                p.lines[k].0 = match family {
                    Lines::Ground if (1..=15).contains(&kind) => {
                        let (class, tier) = ((kind - 1) / 3, (kind - 1) % 3);
                        cycle(class, 0, 4, up) * 3 + tier + 1
                    }
                    Lines::Air => cycle(kind.clamp(30, 34), 30, 34, up),
                    Lines::Walkers => cycle(kind.clamp(41, 46), 41, 46, up),
                    _ => return false,
                };
                let _ = rise;
            }
            (LineChange::Tiers(k), Lines::Ground) if k < p.lines.len() && (1..=15).contains(&p.lines[k].0) => {
                let (kind, rise) = p.lines[k];
                let (class, start) = ((kind - 1) / 3, (kind - 1) % 3 + 1);
                let now = TIER_RANGES.iter().position(|&r| r == (start, (start + rise).min(3))).unwrap_or(0);
                let (s, top) = TIER_RANGES[(now + 1) % TIER_RANGES.len()];
                p.lines[k] = (class * 3 + s, top - s);
            }
            (LineChange::Remove(k), Lines::Ground | Lines::Air | Lines::Walkers) if k < p.lines.len() && p.lines.len() > 1 => {
                p.lines.remove(k);
                p.active = p.active.min(p.lines.len() as i32);
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
            if o.kind == 100 && self.t.level.objects.iter().filter(|p| p.kind == 100 && p.raw[14] == o.raw[14]).count() == 1 {
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
            if o.kind == 100 && self.t.level.objects.iter().filter(|p| p.kind == 100 && p.raw[14] == o.raw[14]).count() == 1 {
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
            self.t.level.objects.iter().find(|o| o.kind == 100 && o.raw[14] == side as i32).map(|o| (o.cell_x as f32 + 2.0, o.cell_y as f32 + 2.0))
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
        let cells = (0..w * h).map(|i| Cell { x: (i % w) as u8, y: (i / w) as u8, blocked: i % w == 20 && i / w < 10, border: false, no_build: i % w == 25 && i / w == 5, no_marks: false }).collect();
        Map { a: 0, b: 0, width: w, height: h, cells }
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
        let glyphs = (0..94).map(|_| opensow_formats::sprite::Frame { width: 6, height: 8, hot_x: 0, hot_y: 0, extra: [0; 4], pixels: vec![0; 48], mask: vec![true; 48], enhanced: Default::default() }).collect();
        let f = Font::new(glyphs);
        let lines = wrap(&f, "aaaa bbbb cccc dddd", 60);
        assert!(lines.len() > 1 && lines.iter().all(|l| f.measure(l, 1) <= 60 || !l.contains(' ')), "{lines:?}");
    }
}
