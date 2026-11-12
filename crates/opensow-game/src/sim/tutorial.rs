//! The tutorial script of mission M00 (`0x4325b0`, cases `0x432e5c..0x433b3f`, jump table `0x433be0`, docs/tutorial.md).
//!
//! `[0x70f2cc]` is the step. Every frame the script (1) halts while the arrow button of the instruction panel waits for a
//! click, (2) on the first frame of a step shows its text (`lang[146 + step]`) and plays `Tutorial\NN.wav`, (3) runs the
//! step's case: a case either moves on at once (information steps), waits for its condition (tasks) or, on its first frame,
//! wipes the map and builds the scene of the lesson. Entering a step clears the key flags and raises the arrow button,
//! except for steps 1 and 17.

use super::*;
use opensow_formats::kinds;

/// What the instruction panel has to do (drained by the front end).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TutEvent {
    /// Show the text of this step and play its voice file `Tutorial\NN.wav`.
    Step(u32),
    /// The arrow button appears (sound 29).
    Arrow,
}

/// The state of the script and the input flags it reads.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Tutorial {
    /// `[0x70f2cc]`.
    pub step: u32,
    /// `[0x70f2e2]`: first frame of the step.
    pub entered: bool,
    /// `[0x70baf4]`: the arrow button waits for a click.
    pub arrow: bool,
    /// `[0x70f2d8..db]`: the four scroll keys seen in step 3.
    pub keys: [bool; 4],
    /// `[0x6bed20]`: the map was scrolled with the right mouse button.
    pub rmb_scroll: bool,
    /// `[0x6beef0]`: an air force icon was clicked.
    pub air_icon: bool,
    /// The Advancer was selected and given a go-to order (pointer 3 + left click, step 5).
    pub advancer_moved: bool,
    /// `[0x70f2f9]`: the HQ built a replacement Advancer (set by `0x42e41d`).
    pub advancer_replaced: bool,
    /// The click on the arrow button happened (`[0x6bed13]` over pointer 0x7a).
    pub arrow_click: bool,
    /// Events for the instruction panel; the front end drains them.
    pub events: Vec<TutEvent>,
}

/// Language string of a step (`lang[146 + step]`: 148 "scroll with the right mouse button" is step 2).
pub fn text_index(step: u32) -> usize {
    146 + step as usize
}

impl World {
    /// Starts the script (the init block `0x432bf1`, run in step 0).
    pub fn start_tutorial(&mut self) {
        self.tutorial_step = Some(0);
        self.tutorial = Some(Tutorial::default());
    }

    /// The player clicked the arrow button.
    pub fn tutorial_arrow_clicked(&mut self) {
        if let Some(t) = &mut self.tutorial {
            if t.arrow {
                t.arrow_click = true;
            }
        }
    }

    /// 0x43a640 / 0x43a7e0 for every player: all buildings and units vanish silently.
    fn tutorial_wipe(&mut self) {
        let cells: Vec<(i32, i32)> = self.structures.iter().flat_map(|s| s.blocked_cells()).collect();
        for (cx, cy) in cells {
            if let Some(b) = self.cell_index(cx, cy) {
                self.blocked[b] = false;
            }
        }
        self.structures.clear();
        self.units.clear();
        self.beams.clear();
        self.has_enemy_hq = false;
        self.occ = None;
    }

    /// `0x439e30(x, y, owner, kind, 1, 0)` + `0x40d870(slots)`: a finished building; `slots` = (kind, level limit) pairs.
    fn tutorial_building(&mut self, cell: (i32, i32), owner: Owner, kind_id: i32, slots: &[(i32, u32)]) -> usize {
        let kind = ObjectKind::from_id(kind_id);
        let max_hp = building_max_hp(kind_id);
        let weapon = if let ObjectKind::Turret(n) = kind { turret_weapon(n as u32) } else { 0 };
        let aim = (weapon != 0).then(|| TopAim::new(weapon, 0)).flatten();
        let slots: Vec<SlotState> = slots
            .iter()
            .filter_map(|&(k, m)| {
                slot_target(k).filter(|_| k != 0).map(|target| SlotState { kind: k, progress: 0, target, level: 0, max_level: m, waiting: 0, unlock: 0 })
            })
            .collect();
        let active = 1.min(slots.len());
        self.add_structure(Structure {
            object: usize::MAX,
            kind,
            owner,
            cell,
            hp: max_hp,
            max_hp,
            aim,
            weapon,
            target: None,
            slots,
            active,
            kind_id,
            tally: [0; 3],
            heal: 0,
            upgrade: 0,
            protected: kind_id == kinds::HEADQUARTERS,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        });
        self.structures.len() - 1
    }

    /// `0x43a420(x, y, player, kind, 0)`: a unit on a cell.
    fn tutorial_unit(&mut self, cell: (i32, i32), player: i32, kind: i32) -> u32 {
        self.add_unit(kind, player, cell.0 * CELL + CELL / 2, cell.1 * CELL + CELL / 2)
    }

    fn tutorial_face(&mut self, id: u32, facing: u32) {
        if let Some(c) = self.units.iter_mut().find(|u| u.id == id).and_then(|u| u.chassis.as_mut()) {
            c.facing = facing;
            c.target = facing;
        }
    }

    /// `0x4588c0(x, y, player)`: the view jumps to this map pixel.
    fn tutorial_camera(&mut self, x: i32, y: i32) {
        self.camera_request = Some((x, y));
    }

    /// Player 0's buildings in the exe's array order (cases that compute `step = index + base` use the index).
    pub(super) fn tut_buildings(&self) -> Vec<usize> {
        (0..self.structures.len()).filter(|&i| self.structures[i].alive() && self.structures[i].owner == Owner::Player).collect()
    }

    /// Whether every unit that does not belong to the player is gone (steps 13 and 17: the `[+0x2c]` flag of each is set).
    fn tut_foreign_units_gone(&self) -> bool {
        !self.units.iter().any(|u| u.player != 0 && u.hp > 0)
    }

    /// One frame of the script.
    pub(super) fn step_tutorial(&mut self) {
        let Some(mut t) = self.tutorial.take() else { return };
        self.tutorial_frame(&mut t);
        self.tutorial_step = Some(t.step);
        self.tutorial = Some(t);
    }

    fn tutorial_frame(&mut self, t: &mut Tutorial) {
        // the arrow button halts the script until it is clicked
        if t.arrow {
            if !t.arrow_click {
                return;
            }
            t.arrow = false;
        }
        t.arrow_click = false;
        if t.entered {
            t.events.push(TutEvent::Step(t.step));
        }
        let (before, al) = (t.step, t.entered);
        let player_hq = |s: &Structure| s.kind_id == kinds::HEADQUARTERS;
        match before {
            0 | 1 | 4 | 6 | 11 | 14 | 18 | 23 | 26 => t.step = before + 1,
            2 => {
                if t.rmb_scroll {
                    t.step = 3;
                }
            }
            3 => {
                // the four scroll keys (flags 0x6bee34, 0x6bee3c, 0x6bee37, 0x6bee39), remembered in 0x70f2d8..db
                if t.keys.iter().all(|&k| k) {
                    t.step = 4;
                }
            }
            5 => {
                if al {
                    self.tutorial_unit((9, 46), 0, 40);
                    self.tutorial_camera(0x120, 0x5c0);
                } else if t.advancer_moved {
                    // `step = index + 6` of the selected Advancer: it is the first unit, index 0
                    let idx = self.units.iter().filter(|u| u.player == 0 && u.hp > 0).position(|u| u.kind == kinds::ADVANCER && u.selected).unwrap_or(0);
                    t.step = 6 + idx as u32;
                }
            }
            7 => {
                if al {
                    self.tutorial_wipe();
                    self.tutorial_building((8, 8), Owner::Neutral, 101, &[(7, 0), (4, 0)]);
                    self.tutorial_unit((5, 9), 0, 40);
                    self.tutorial_camera(0x100, 0x100);
                } else {
                    // any building of the player that is not the HQ: step = index + 8
                    for (i, &b) in self.tut_buildings().iter().enumerate() {
                        if !player_hq(&self.structures[b]) {
                            t.step = 8 + i as u32;
                        }
                    }
                }
            }
            8 => {
                // the last unit of the player that is not the Advancer: step = index + 9
                let units: Vec<&Unit> = self.units.iter().filter(|u| u.player == 0 && u.hp > 0).collect();
                for (i, u) in units.iter().enumerate() {
                    if u.kind != kinds::ADVANCER {
                        t.step = 9 + i as u32;
                    }
                }
            }
            9 => {
                for (i, &b) in self.tut_buildings().iter().enumerate() {
                    let s = &self.structures[b];
                    if !player_hq(s) && s.active >= 2 {
                        t.step = 10 + i as u32;
                    }
                }
            }
            10 => {
                if al {
                    self.tutorial_wipe();
                    self.tutorial_unit((44, 39), 0, 40);
                    let k = self.tutorial_building((46, 39), Owner::Player, 101, &[(7, 0), (4, 0)]);
                    self.structures[k].hp = self.structures[k].max_hp / 2 - 100;
                    self.tutorial_camera(0x500, 0x5c0);
                } else if self
                    .tut_buildings()
                    .iter()
                    .any(|&b| self.structures[b].kind_id == kinds::LIGHT_FACTORY && self.structures[b].hp >= self.structures[b].max_hp)
                {
                    t.step = 11;
                }
            }
            12 => {
                if al {
                    self.tutorial_wipe();
                    self.tutorial_unit((40, 6), 0, 40);
                    let a = self.tutorial_unit((39, 8), 1, 7);
                    self.tutorial_face(a, 6);
                    let b = self.tutorial_unit((40, 9), 1, 7);
                    self.tutorial_face(b, 8);
                    self.tutorial_building((44, 5), Owner::Player, 105, &[(16, 0), (17, 0), (18, 0)]);
                    self.tutorial_camera(0x5c0, 0xa0);
                    self.credits[0] = 120;
                } else if self.credits[0] >= 200 {
                    t.step = 13;
                }
            }
            13 => {
                if self.tut_foreign_units_gone() {
                    t.step = 14;
                }
            }
            15 => {
                if al {
                    self.tutorial_wipe();
                    self.credits = [0, 0];
                    self.tutorial_building((22, 51), Owner::Player, 104, &[(32, 0)]);
                    self.tutorial_unit((27, 53), 0, 40);
                    for c in [(31, 55), (32, 55), (31, 57), (32, 57)] {
                        self.tutorial_unit(c, 1, 7);
                    }
                    self.tutorial_camera(0x340, 0x6a0);
                } else {
                    let fighter = HEADER_ORDER.iter().position(|&h| h == AirForce::Fighter).unwrap_or(4);
                    if self.tut_buildings().iter().any(|&b| self.structures[b].kind_id == kinds::AIR_BASE) && self.air_stock[0][fighter] > 0 {
                        t.step = 16;
                    }
                }
            }
            16 => {
                if t.air_icon {
                    t.step = 17;
                }
            }
            17 => {
                if self.tut_foreign_units_gone() {
                    t.step = 18;
                    self.air_stock[0] = [0; 5]; // 0x455670: the air force icons are cleared
                }
            }
            19 => {
                if al {
                    self.tutorial_wipe();
                    self.research[0] = 400;
                    self.tutorial_building((12, 13), Owner::Neutral, 106, &[(19, 0), (20, 0)]);
                    self.tutorial_building((6, 6), Owner::Player, 102, &[(7, 1)]);
                    self.tutorial_unit((5, 9), 0, 40);
                    self.tutorial_camera(0x100, 0x100);
                } else {
                    t.step = 20;
                }
            }
            20 => {
                for (i, &b) in self.tut_buildings().iter().enumerate() {
                    if self.structures[b].kind_id == kinds::RESEARCH_LAB {
                        t.step = 21 + i as u32;
                    }
                }
            }
            21 => {
                if self.research[0] >= 500 {
                    t.step = 22;
                }
            }
            22 => {
                for (i, &b) in self.tut_buildings().iter().enumerate() {
                    let s = &self.structures[b];
                    if s.kind_id == kinds::MEDIUM_FACTORY && s.slots.first().is_some_and(|sl| sl.level == 1) {
                        t.step = 23 + i as u32;
                    }
                }
            }
            24 => {
                if al {
                    self.tutorial_wipe();
                    self.research = [0, 0];
                    let adv = self.tutorial_unit((41, 17), 0, 40);
                    let hq = self.tutorial_building((41, 17), Owner::Player, 100, &[]);
                    self.structures[hq].advancer = Some(adv);
                    let ehq = self.tutorial_building((56, 18), Owner::Enemy, 100, &[]);
                    self.structures[ehq].hp = self.structures[ehq].max_hp / 2 + 10;
                    self.tutorial_building((49, 20), Owner::Neutral, 101, &[(7, 0), (4, 0)]);
                    self.tutorial_building((56, 12), Owner::Neutral, 101, &[(7, 0), (4, 0)]);
                    self.tutorial_camera(0x640, 0x240);
                    t.advancer_replaced = false;
                } else {
                    if t.advancer_replaced {
                        t.step = 25;
                    }
                    if !self.structures.iter().any(|s| s.alive() && s.owner == Owner::Enemy && s.kind_id == kinds::HEADQUARTERS) {
                        t.step = 26;
                    }
                }
            }
            25 => {
                if !self.structures.iter().any(|s| s.alive() && s.owner == Owner::Enemy && s.kind_id == kinds::HEADQUARTERS) {
                    t.step = 26;
                }
            }
            27 => {
                // 0x42c260(0x1c): the tutorial is over
                self.outcome = Some(Outcome::Victory);
            }
            _ => {}
        }
        t.advancer_moved = false;
        if t.step == before {
            t.entered = false;
        } else {
            t.keys = [false; 4];
            t.entered = true;
            if t.step != 1 && t.step != 17 {
                t.arrow = true;
                t.events.push(TutEvent::Arrow);
            }
        }
    }
}
