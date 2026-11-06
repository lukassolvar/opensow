//! Effect sprites, decoded from `State of War.exe` (docs/effects.md).
//!
//! Three pieces make up everything that is drawn besides units and buildings:
//! - the **extras table**: the start-up loader (`0x424700`) reads every `extras\*`, `__extras\*` and `specials\*` sprite
//!   file into a static array of frame pointers at a fixed address ([`EXTRAS`]);
//! - **bullet drawing**: the draw function `0x410a30` switches on the bullet type ([`bullet_draw`]); the frame advance is
//!   in the step function (`0x411c8x`, [`BulletAnim`]);
//! - **animation sprites** (`AnimSprite`, constructor `0x4054a0`): a list of frames played at a fixed period, created at
//!   about forty places for explosions, smoke and markers ([`SPAWNS`], [`blast`]).

/// One sprite file in the loader, in load order. `addr` is the static array of frame pointers the loader fills; `slots` is
/// the frame cap it passes (the real frame count comes from the file; neighbouring arrays may overlap beyond it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extra {
    /// Loader name without extension; the file is `<name>.ps6` inside `sprites.data`.
    pub name: &'static str,
    pub slots: u16,
    pub addr: u32,
}

pub static EXTRAS: &[Extra] = &[
    Extra { name: "extras\\pasy_light", slots: 100, addr: 0x5e57e0 },
    Extra { name: "extras\\pasy_medium", slots: 100, addr: 0x5e5970 },
    Extra { name: "extras\\pasy_heavy", slots: 100, addr: 0x5e5b00 },
    Extra { name: "extras\\radar", slots: 40, addr: 0x5e60a0 },
    Extra { name: "extras\\cross_as", slots: 21, addr: 0x5e5fb0 },
    Extra { name: "extras\\cross_rp", slots: 21, addr: 0x5e6028 },
    Extra { name: "extras\\click", slots: 10, addr: 0x5e6140 },
    Extra { name: "extras\\money", slots: 50, addr: 0x5e6168 },
    Extra { name: "__extras\\gula2", slots: 17, addr: 0x5e6690 },
    Extra { name: "extras\\missile", slots: 40, addr: 0x5e66e8 },
    Extra { name: "extras\\missileSmoke", slots: 40, addr: 0x5e6788 },
    Extra { name: "extras\\MissileFlare", slots: 5, addr: 0x5e66d4 },
    Extra { name: "extras\\blast_verysmall", slots: 40, addr: 0x5e6230 },
    Extra { name: "extras\\blast_small", slots: 40, addr: 0x5e62d0 },
    Extra { name: "extras\\blast_medium", slots: 40, addr: 0x5e6370 },
    Extra { name: "extras\\blast_big", slots: 40, addr: 0x5e6410 },
    Extra { name: "extras\\blast2_defrag", slots: 40, addr: 0x5e64b0 },
    Extra { name: "extras\\blast2_artligh", slots: 40, addr: 0x5e6550 },
    Extra { name: "extras\\blast2_artheav", slots: 40, addr: 0x5e65f0 },
    Extra { name: "extras\\wreck_bu1", slots: 20, addr: 0x5e6828 },
    Extra { name: "extras\\wreck_bu2", slots: 20, addr: 0x5e6878 },
    Extra { name: "extras\\sizer", slots: 9, addr: 0x5e68c8 },
    Extra { name: "extras\\fire_sm1", slots: 10, addr: 0x5e6910 },
    Extra { name: "extras\\fire_sm2", slots: 10, addr: 0x5e6938 },
    Extra { name: "__extras\\pustkrat", slots: 25, addr: 0x5e6960 },
    Extra { name: "extras\\heavylaser", slots: 20, addr: 0x5e6978 },
    Extra { name: "extras\\walker_blast", slots: 5, addr: 0x5e69c8 },
    Extra { name: "extras\\specialfall_light", slots: 29, addr: 0x5e69f0 },
    Extra { name: "extras\\specialfall_medium", slots: 29, addr: 0x5e6a68 },
    Extra { name: "extras\\specialfall_heavy", slots: 29, addr: 0x5e6ae0 },
    Extra { name: "__extras\\plasma", slots: 5, addr: 0x5e68d4 },
    Extra { name: "extras\\bodky_r", slots: 20, addr: 0x5e6b80 },
    Extra { name: "extras\\bodky_b", slots: 20, addr: 0x5e6bd0 },
    Extra { name: "extras\\bodky_y", slots: 20, addr: 0x5e6c20 },
    Extra { name: "extras\\bodky_g", slots: 20, addr: 0x5e6c70 },
    Extra { name: "__extras\\dim1", slots: 21, addr: 0x5e6cc0 },
    Extra { name: "__extras\\dim2", slots: 21, addr: 0x5e6d14 },
    Extra { name: "__extras\\dim3", slots: 21, addr: 0x5e6d68 },
    Extra { name: "__extras\\dim4", slots: 21, addr: 0x5e6dbc },
    Extra { name: "extras\\ikony", slots: 100, addr: 0x5e6e50 },
    Extra { name: "extras\\ikonyaura", slots: 10, addr: 0x5e7170 },
    Extra { name: "extras\\ikonyrpoints", slots: 10, addr: 0x5e7198 },
    Extra { name: "__extras\\panel2nu", slots: 11, addr: 0x5e6e10 },
    Extra { name: "extras\\flare", slots: 12, addr: 0x5e92b8 },
    Extra { name: "extras\\flamerburst_sma", slots: 32, addr: 0x5e9340 },
    Extra { name: "extras\\flamerburst_big", slots: 32, addr: 0x5e93c0 },
    Extra { name: "extras\\satlaser", slots: 10, addr: 0x5e6b58 },
    Extra { name: "extras\\newvrak", slots: 22, addr: 0x5e92e8 },
    Extra { name: "extras\\tutorialgfx", slots: 60, addr: 0x5e9440 },
    Extra { name: "__extras\\dym03", slots: 18, addr: 0x5e9530 },
    Extra { name: "__extras\\compplay", slots: 2, addr: 0x5e95d8 },
    Extra { name: "extras\\elektron", slots: 30, addr: 0x5e95e0 },
    Extra { name: "extras\\kruh", slots: 20, addr: 0x5e9658 },
    Extra { name: "extras\\rallypoint", slots: 48, addr: 0x5eab00 },
    Extra { name: "extras\\walker_teleport", slots: 20, addr: 0x5e96a8 },
    Extra { name: "extras\\bigexplo\\clouds", slots: 10, addr: 0x5e95b0 },
    Extra { name: "extras\\bigexplo\\e_unit2", slots: 100, addr: 0x5e9888 },
    Extra { name: "extras\\bigexplo\\e_turret", slots: 100, addr: 0x5e9ba8 },
    Extra { name: "extras\\bigexplo\\e_air2", slots: 100, addr: 0x5e96f8 },
    Extra { name: "extras\\bigexplo\\e_bubg", slots: 100, addr: 0x5e9a18 },
    Extra { name: "specials\\bomber_l", slots: 10, addr: 0x5e9d38 },
    Extra { name: "specials\\bomber_r", slots: 10, addr: 0x5e9d60 },
    Extra { name: "specials\\bomba_l", slots: 20, addr: 0x5e9d88 },
    Extra { name: "specials\\bomba_r", slots: 20, addr: 0x5e9dd8 },
    Extra { name: "specials\\bomba_ex", slots: 20, addr: 0x5e9e28 },
    Extra { name: "specials\\meteor_medium", slots: 40, addr: 0x5e9e78 },
    Extra { name: "specials\\meteor_medium_aura", slots: 40, addr: 0x5e9f18 },
    Extra { name: "specials\\meteor_medium_smoke", slots: 50, addr: 0x5e9fb8 },
    Extra { name: "specials\\meteor_small", slots: 40, addr: 0x5ea080 },
    Extra { name: "specials\\meteor_small_aura", slots: 40, addr: 0x5ea120 },
    Extra { name: "specials\\meteor_small_smoke", slots: 50, addr: 0x5ea1c0 },
    Extra { name: "specials\\carryAll", slots: 20, addr: 0x5ea288 },
    Extra { name: "specials\\carryAll_down", slots: 10, addr: 0x5ea2d8 },
    Extra { name: "specials\\carryAll_rear", slots: 10, addr: 0x5ea300 },
    Extra { name: "specials\\tripler", slots: 10, addr: 0x5ea328 },
    Extra { name: "specials\\fighter", slots: 10, addr: 0x5ea3a4 },
    Extra { name: "specials\\fighter_smoke", slots: 200, addr: 0x5ea3d0 },
    Extra { name: "specials\\fighter_bomb", slots: 20, addr: 0x5ea6f0 },
    Extra { name: "specials\\fighter_ex", slots: 60, addr: 0x5ea740 },
    Extra { name: "specials\\tripler_ph", slots: 21, addr: 0x5ea350 },
    Extra { name: "extras\\infobox", slots: 100, addr: 0x5ea8d0 },
    Extra { name: "extras\\shortcuts", slots: 5, addr: 0x5e6e3c },
];

/// The extra that owns the sprite array element at `addr` and the frame index of that element (`addr` may point into the
/// array: the shell sprites use `gula2` frames 1 and 2 at `0x5e6694` / `0x5e6698`).
pub fn extra_at(addr: u32) -> Option<(&'static Extra, usize)> {
    // the frame caps overlap (`sizer` has 9 slots and reaches into `plasma`): the array that starts closest below the address owns it
    EXTRAS.iter().filter(|e| addr >= e.addr && addr < e.addr + 4 * e.slots as u32).max_by_key(|e| e.addr).map(|e| (e, ((addr - e.addr) / 4) as usize))
}

/// How a sprite is combined with the picture: the exe's software blitters, all exact (docs/effects.md; emulation-checked by
/// `scripts/verify_blits.py`). Pixels are RGB565; "channel" below means the 5/6/5-bit fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blit {
    /// `0x45edf0`: colour-keyed copy.
    Plain,
    /// `0x45cde0` (and `0x45e560`, `0x45b5e0`): `min(dst + src, max)` - flares, glows, explosions.
    Additive,
    /// `0x45c220` (draw mode 9): `min(dst + src / 2, max)`, the source halved per channel first.
    AddHalf,
    /// `0x4611f0` (draw modes 10, 11): `(dst + src) / 2` per channel, a 50 % blend.
    Average,
    /// `0x45f4d0` (draw mode 14, the fighter's smoke): `max(0, dst - src / 4)` per channel.
    Smoke,
    /// `0x45df40`: `max(0, dst - src / 2)` per channel - the scorch decals.
    Darken,
    /// `0x460f60`: the sprite is only a stencil; every covered destination pixel goes through the shade table `0x73ced8`
    /// (`max(0, channel - 6/12/6)`) - shadows.
    Shade,
    /// `0x461740` (draw mode 13): the covered pixels are replaced by the picture pixel 2 columns right and 3 rows down
    /// (the source row is not clamped, the destination loop stops at row 0x1de), brightened through the table `0x77cee8`
    /// (`min(channel + 1/2/1, max)`). `apply565(dst_from_offset, _)` gives the brightening only; the sprite is a stencil.
    Lens,
}

impl Blit {
    pub fn of_routine(addr: u32) -> Blit {
        match addr {
            0x45edf0 => Blit::Plain,
            0x461740 => Blit::Lens,
            // 0x45b5e0 adds too, but only where a screen mask byte is below `[0x49a33b]` (the smoke puffs)
            0x45cde0 | 0x45e560 | 0x45b5e0 => Blit::Additive,
            0x45df40 => Blit::Darken,
            0x45f4d0 => Blit::Smoke,
            0x4611f0 => Blit::Average,
            0x460f60 => Blit::Shade,
            _ => Blit::AddHalf,
        }
    }

    /// One RGB565 pixel: `dst` combined with the sprite pixel `src`.
    #[inline(always)]
    pub fn apply565(self, dst: u16, src: u16) -> u16 {
        let (dr, dg, db) = ((dst >> 11) as i32, (dst >> 5 & 63) as i32, (dst & 31) as i32);
        let (sr, sg, sb) = ((src >> 11) as i32, (src >> 5 & 63) as i32, (src & 31) as i32);
        let pack = |r: i32, g: i32, b: i32| (r << 11 | g << 5 | b) as u16;
        let sat = |v: i32, max: i32| v.clamp(0, max);
        let (r, g, b) = match self {
            Blit::Plain => (sr, sg, sb),
            Blit::Additive => (sat(dr + sr, 31), sat(dg + sg, 63), sat(db + sb, 31)),
            Blit::AddHalf => (sat(dr + (sr >> 1), 31), sat(dg + (sg >> 1), 63), sat(db + (sb >> 1), 31)),
            Blit::Average => ((dr + sr) >> 1, (dg + sg) >> 1, (db + sb) >> 1),
            Blit::Smoke => (sat(dr - (sr >> 2), 31), sat(dg - (sg >> 2), 63), sat(db - (sb >> 2), 31)),
            Blit::Darken => (sat(dr - (sr >> 1), 31), sat(dg - (sg >> 1), 63), sat(db - (sb >> 1), 31)),
            Blit::Shade => (sat(dr - 6, 31), sat(dg - 12, 63), sat(db - 6, 31)),
            Blit::Lens => (sat(dr + 1, 31), sat(dg + 2, 63), sat(db + 1, 31)),
        };
        pack(r, g, b)
    }
}

/// Which frame of a layer's sprite array is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// A fixed element of the array (`0` for the first).
    Fixed(u16),
    /// The animation counter `[bullet+0x2670]` plus a fixed offset.
    Anim(u16),
    /// The facing `[bullet+0x2654]` (missiles).
    Facing,
    /// `[bullet+0x268c] * stride + anim` (wreck pieces, bombs: the second sprite is the next `stride` frames).
    Variant268c(u16),
    /// `[bullet+0x2680] * stride + anim` (unit boom pieces, the dim effect).
    Variant2680(u16),
}

/// One sprite a bullet type draws, drawn in list order at the bullet position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    /// Sprite array address (element 0 of the array, or the element used when the frame is fixed).
    pub array: u32,
    pub frame: Frame,
    /// The blit routine's address.
    pub routine: u32,
}

const fn l(array: u32, frame: Frame, routine: u32) -> Layer {
    Layer { array, frame, routine }
}

const PLAIN: u32 = 0x45edf0;
const ADD: u32 = 0x45cde0;

/// What bullet types 19 and 21 draw: `[bullet+0x268c]` fragments, each at its own offset with its own frame.
pub const PARTICLE_TYPES: [u8; 2] = [19, 21];

static NONE: [Layer; 0] = [];
static MISSILE: [Layer; 2] = [l(0x5e66e8, Frame::Facing, PLAIN), l(0x5e66d4, Frame::Fixed(0), ADD)];
static GULA_1: [Layer; 1] = [l(0x5e6694, Frame::Fixed(0), PLAIN)];
static GULA_2: [Layer; 1] = [l(0x5e6698, Frame::Fixed(0), PLAIN)];
static GULA_0: [Layer; 1] = [l(0x5e6690, Frame::Fixed(0), PLAIN)];
static PLASMA: [Layer; 1] = [l(0x5e68d4, Frame::Anim(0), ADD)];
static WRECK: [Layer; 1] = [l(0x5e6828, Frame::Variant268c(20), PLAIN)];
static BLUE_BOOM: [Layer; 1] = [l(0x5870b0, Frame::Variant2680(80), PLAIN)];
static GREN_BOOM: [Layer; 1] = [l(0x58a2b0, Frame::Variant2680(80), PLAIN)];
static TRIPLER_POD: [Layer; 1] = [l(0x5ea350, Frame::Anim(0), ADD)];
static BOMB: [Layer; 1] = [l(0x5e9d88, Frame::Variant268c(20), PLAIN)];
static BODKY_B: [Layer; 1] = [l(0x5e6bd0, Frame::Anim(0), ADD)];
static BODKY_R: [Layer; 1] = [l(0x5e6b80, Frame::Anim(0), ADD)];
static FIGHTER_SMOKE: [Layer; 1] = [l(0x5ea3d0, Frame::Anim(0), 0x45f4d0)];
static DIM: [Layer; 1] = [l(0x5e6cc0, Frame::Variant2680(21), 0x460f60)];
static METEOR_MEDIUM: [Layer; 2] = [l(0x5e9e78, Frame::Anim(0), PLAIN), l(0x5e9f18, Frame::Anim(0), 0x45c220)];
static METEOR_SMALL: [Layer; 2] = [l(0x5ea080, Frame::Anim(0), PLAIN), l(0x5ea120, Frame::Anim(0), 0x45c220)];
static FIGHTER_BOMB: [Layer; 1] = [l(0x5ea6f0, Frame::Anim(0), PLAIN)];
static ELEKTRON: [Layer; 1] = [l(0x5e95e4, Frame::Anim(0), ADD)];
static HEAVY_LASER: [Layer; 1] = [l(0x5e6978, Frame::Fixed(0), ADD)];
static LIGHTS: [Layer; 1] = [l(0x71c548, Frame::Fixed(0), ADD)]; // menu\001_lights (a loose Sprites2 file)
static WALKER_BLAST: [Layer; 1] = [l(0x5e69c8, Frame::Fixed(0), ADD)];

/// The sprites bullet type `ty` (1..=34) draws, from the draw function `0x410a30` (jump table `0x410e7c`). Types with no
/// entry draw nothing there: 6-10 and 12, 14, 20, 27, 31 (type 10 is the satellite laser, drawn by the beam code).
pub fn bullet_draw(ty: u8) -> &'static [Layer] {
    match ty {
        1 => &MISSILE,
        2 | 3 => &GULA_1,
        4 => &GULA_2,
        5 => &GULA_0,
        11 => &PLASMA,
        13 => &WRECK,
        15 => &BLUE_BOOM,
        16 => &GREN_BOOM,
        17 => &TRIPLER_POD,
        18 => &BOMB,
        19 => &BODKY_B,
        21 => &BODKY_R,
        22 => &FIGHTER_SMOKE,
        23 => &DIM,
        24 => &METEOR_MEDIUM,
        25 => &METEOR_SMALL,
        26 => &FIGHTER_BOMB,
        28 => &ELEKTRON,
        29 | 30 | 33 => &HEAVY_LASER,
        32 => &LIGHTS,
        34 => &WALKER_BLAST,
        _ => &NONE,
    }
}

/// Bullet types whose frame counter the step function advances on a timer: the other four animation fields
/// ([`crate::projectile::BULLET_TYPES`] `anim`) are the start frame, the frame count, the first countdown and the delay.
/// Types 17 and 18 take their frame from the flight progress instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BulletAnim {
    pub frame: i32,
    count: i32,
    countdown: i32,
    delay: i32,
}

impl BulletAnim {
    /// `[frame, count, countdown, delay]` as `new` takes them (`[bullet+0x2670..0x267c]`, kept by a savegame).
    pub fn parts(&self) -> [i32; 4] {
        [self.frame, self.count, self.countdown, self.delay]
    }

    pub fn new(anim: [i32; 4]) -> BulletAnim {
        BulletAnim { frame: anim[0], count: anim[1], countdown: anim[2], delay: anim[3] }
    }

    /// One logic step (`0x411c95..0x411d07`; the exe runs `[0x552494]` of them per tick). `ty` 17 and 18 set the frame from
    /// the path progress `progress` (`[bullet+0x2644]`, twice the path index) instead. Returns `false` when a one-shot
    /// animation (types 22 and 23) has played its last frame and the bullet is finished.
    pub fn step(&mut self, ty: u8, progress: i32) -> bool {
        match ty {
            17 => {
                self.frame = ((progress - 2) / 20).min(19);
                return true;
            }
            18 => {
                self.frame = ((progress - 2) / 50).min(8);
                return true;
            }
            _ => {}
        }
        self.countdown -= 1;
        if self.countdown > 0 {
            return true;
        }
        self.countdown = self.delay;
        if ty == 22 && self.frame % 3 == 0 {
            self.frame += 1; // the fighter's smoke skips every third frame
        }
        self.frame += 1;
        if self.frame >= self.count {
            self.frame = 0;
            if ty == 22 || ty == 23 {
                return false;
            }
        }
        true
    }
}

/// Sound ids played at a bullet's end point by the impact function `0x40f590` (table `0x410a14` -> `0x4109f0`), see
/// `sfx::SFX`. Type 18 plays its sound only half of the time (`random(2) == 0`).
pub fn impact_sound(ty: u8) -> Option<u8> {
    Some(match ty {
        1 => 20,
        3 | 4 => 21,
        6 | 7 => 22,
        15 | 16 => 28,
        17 => 26,
        18 => 65,
        24 | 25 => 70,
        26 => 67,
        _ => return None,
    })
}

/// The size of the blast sprite (`0x405240`) a bullet type creates when it hits, or `None`: index into
/// `[blast_verysmall, blast_small, blast_medium, blast_big, blast2_defrag, blast2_artligh, blast2_artheav]`.
pub fn impact_blast(ty: u8) -> Option<u8> {
    Some(match ty {
        4 => 6,
        1 | 6 => 1,
        7 | 2 => 0,
        3 => 5,
        15 | 16 | 24 | 25 => 2,
        17 => 1,
        11 => 4,
        _ => return None,
    })
}

/// An animation sprite: frames `from..to` of `array` played `repeat` times, each shown for `period` logic steps
/// (`AnimSprite::update` `0x4050d0`: `[+0xc]` counts down from `[+0x10]`, `[+0x38]` is the frame index, finished when it
/// reaches the list length `[+0x3c]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnimSpec {
    pub array: u32,
    pub from: u16,
    pub to: u16,
    pub period: u8,
    pub repeat: u8,
    /// Draw mode `[obj+0x34]` ([`ANIM_DRAW_MODES`]): 6 for nearly everything, 8 for the smoke puffs, 9 and 14 for smoke trails.
    pub mode: u8,
}

impl AnimSpec {
    /// The same animation drawn with another mode.
    pub const fn with_mode(self, mode: u8) -> AnimSpec {
        AnimSpec { mode, ..self }
    }

    /// The exe's blit routine for this mode (`0x404e30`).
    pub fn routine(&self) -> u32 {
        ANIM_DRAW_MODES.iter().find(|m| m.0 == self.mode).map_or(0x45edf0, |m| m.1)
    }

    /// Number of frames in the list.
    pub fn len(&self) -> usize {
        (self.to - self.from) as usize * self.repeat as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `(frame index into the sprite file, ...)` after `age` logic steps, or `None` once the animation is over.
    pub fn frame_at(&self, age: u32) -> Option<usize> {
        let i = (age / self.period.max(1) as u32) as usize;
        if i >= self.len() {
            return None;
        }
        Some(self.from as usize + i % (self.to - self.from) as usize)
    }
}

/// Size of a blast sprite (`0x405240(x, y, size)`): `blast_*` index `size`, 23 frames for size 4, 30 for sizes 5 and 6, 22
/// otherwise, shown 4 steps each. The exe stores `size + 1` in `[+0x40]`.
pub fn blast(size: u8) -> AnimSpec {
    let to = match size {
        4 => 23,
        5 | 6 => 30,
        _ => 22,
    };
    AnimSpec { array: 0x5e6230 + 0xa0 * size as u32, from: 0, to, period: 4, repeat: 1, mode: 6 }
}

/// One place that creates an [`AnimSprite`](AnimSpec) with constant arguments, from the call sites of `0x4054a0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spawn {
    /// Address of the `call 0x4054a0`.
    pub site: u32,
    pub spec: AnimSpec,
    /// What triggers it (read from the surrounding code).
    pub what: &'static str,
}

/// [`sp`] with a draw mode other than 6.
const fn spm(site: u32, array: u32, from: u16, to: u16, period: u8, mode: u8, what: &'static str) -> Spawn {
    Spawn { site, spec: AnimSpec { array, from, to, period, repeat: 1, mode }, what }
}

const fn sp(site: u32, array: u32, from: u16, to: u16, period: u8, what: &'static str) -> Spawn {
    Spawn { site, spec: AnimSpec { array, from, to, period, repeat: 1, mode: 6 }, what }
}

/// A turret destroyed (`0x409ccb`): 45 frames of `e_turret`, two steps each.
pub const TURRET_DESTROYED: AnimSpec = AnimSpec { array: 0x5e9ba8, from: 0, to: 45, period: 2, repeat: 1, mode: 6 };
/// Any other building destroyed (`0x409d03`): 83 frames of `e_bubg`.
pub const BUILDING_DESTROYED: AnimSpec = AnimSpec { array: 0x5e9a18, from: 0, to: 83, period: 2, repeat: 1, mode: 6 };
/// An air force unit destroyed (`0x467999`): 52 frames of `e_air2`.
pub const AIR_DESTROYED: AnimSpec = AnimSpec { array: 0x5e96f8, from: 0, to: 52, period: 2, repeat: 1, mode: 6 };
/// One cloud of a ground unit's destruction (`0x46a693`...): 29 frames of `e_unit2`, three steps each.
pub const UNIT_CLOUD: AnimSpec = AnimSpec { array: 0x5e9888, from: 0, to: 29, period: 3, repeat: 1, mode: 6 };
/// The wreck that stays after a ground unit's destruction (`0x46a7b1`): `newvrak`.
pub const UNIT_WRECK: AnimSpec = AnimSpec { array: 0x5e92e8, from: 0, to: 20, period: 5, repeat: 1, mode: 6 };

/// The sprite array of the scorch decals, `__extras\pustkrat` (25 frames; variant 0-4 is drawn).
pub const DECAL_ARRAY: u32 = 0x5e6960;

/// Whether a bullet of type `ty` leaves a scorch decal where it lands (the generic tail of the impact function
/// `0x40f590`, `0x410405`): every type except the targeted hits 1, 10 and 31, the Tripler pod 17, the bodky types 19-21
/// and 22. The decal needs a free ground cell inside the map (no building, no terrain block) with no decal yet; its frame
/// is `random(9) / 2` (visual generator) and it is drawn with the darkening blit until a building covers the cell.
pub fn impact_leaves_decal(ty: u8) -> bool {
    !matches!(ty, 1 | 10 | 17 | 19 | 20 | 21 | 22 | 31)
}

/// A smoke puff (`0x405340(x, y)`): `fire_sm1` or `fire_sm2` (`variant` 0/1, chosen with the visual generator), frames 0 to
/// 8, shown 5 steps each, repeated `4 + random(13)` more times after the first pass (`extra`), drawn additively.
pub fn smoke_puff(variant: u8, extra: u8) -> AnimSpec {
    AnimSpec { array: 0x5e6910 + 0x28 * variant as u32, from: 0, to: 9, period: 5, repeat: 5 + extra.min(12), mode: 8 }
}

/// Smoke puffs a landing bullet makes, as `(lockstep, single)` counts, given the lockstep roll `random(3)` of the generic
/// impact tail and whether the cell is free of terrain, buildings and the map's "no marks" layer (`marks_free`; the caller
/// makes none at all on a blocked cell). Type 17 and 18 hits make two puffs behind the `marks_free` test (`0x40fc0a`,
/// `0x40fe18`). In the generic tail (`0x4104da..0x4106fb`): on roll 1 a type 15/16 hit makes three puffs with offsets from the
/// lockstep generator (`0x410510`, `0x41059b`, `0x410629`) and falls through to the single puff below, which every other
/// hit of types 13 to 16 reaches directly (`0x4106ba`: ground free and no-marks clear, offsets from the visual generator).
pub fn impact_smoke(ty: u8, roll_of_three: u32, marks_free: bool) -> (u8, u8) {
    match ty {
        17 | 18 => (0, 2 * marks_free as u8),
        13..=16 => (if roll_of_three == 1 && ty >= 15 { 3 } else { 0 }, marks_free as u8),
        _ => (0, 0),
    }
}

/// Puffs a destroyed ground unit makes (`0x46af25`, `0x46afb6`, `0x46b047`): three, at random offsets of -10..9 px.
pub const UNIT_DEATH_PUFFS: u8 = 3;

/// Steps a destroyed unit stays in its slot (`unit+0x90`, set to 430 at `0x46a5a9`). The dead unit (`unit+0x2c == 1`) re-stamps
/// its cell in the unit grid with the terrain marker `0xFFFFFD` every step (`0x46dfeb` air, `0x46e004` ground), so the corpse
/// blocks its cell, and `0x46b600` counts the timer down; the slot is freed when it goes below 0.
pub const CORPSE_LIFE: i32 = 430;
/// When the timer drops below this the final blasts go off, once (`0x46b649`, flag `unit+0x98`).
pub const CORPSE_BLAST_BELOW: i32 = 30;
/// The final blasts by death variant `unit+0x94` (`0x46b652`: variant 4 enters at `0x46b67a`, 3 at `0x46b6fe`, 2 at `0x46b782`,
/// the chain falls through): `0x405240` blast sizes in order, each at `(x - 15 + rand(30), y - 15 + rand(30))`, the first
/// lockstep draw for y, the second for x. Any other variant (the kinds without debris class) makes none.
pub fn corpse_blasts(variant: u32) -> &'static [u8] {
    match variant {
        2 => &[2, 2],
        3 => &[2, 2, 2],
        4 => &[3, 2, 2, 2],
        _ => &[],
    }
}

/// Clouds ([`UNIT_CLOUD`]) a destroyed unit makes, the Advancer included (`0x46a693`, `a735`, `a84c`, `a8ee` and, for kind 40,
/// `a980`, `aa22`, `aac4`, `ab66`). Each is drawn at `(x - 15 + rand(30), y - 15 + hover + rand(30))` with two lockstep draws,
/// the first for y and the second for x. Only the other kinds leave the [`UNIT_WRECK`] (after the second cloud, at `(x, y + 7)`)
/// and make the puffs; every kind plays sound 27 (`explodes\unit`) at the unit (`0x4077e0`, `0x46aba9`).
pub const UNIT_DEATH_CLOUDS: u8 = 4;
pub const UNIT_DEATH_SOUND: u8 = 27;

/// The flying pieces a destroyed unit throws (`0x46abae..0x46ae3c`): a bullet of type 15 (blue) or 16 (green) with the
/// unit's top part as variant, from `(x, y + top height)` to `(x + sx, y + sy)` where each of `sx`, `sy` is
/// `(2 * rand(2) - 1) * (rand(spread) + spread)`, strength [`DEBRIS_STRENGTH`] (constructor argument 2, `[bullet+0x2660]`; the path delay
/// `[bullet+0x26ac]` stays 0). Only units whose top sprite index is at most 14 ([`debris_variant`]) throw them, although the four
/// lockstep draws are made for every kind with a debris class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debris {
    /// Weapon tier class: 30, 40 or 60 pixels.
    pub spread: i32,
    /// Stored in the bullet at `+0x265c`.
    pub bullet_param: u32,
    /// The corpse's death variant, stored at `unit+0x94` (`0x46b0af`; 2, 3 or 4 by tier) and used by [`corpse_blasts`].
    pub wreck_param: u32,
}

/// A destroyed building's destruction animation (`0x409b60`, called every tick while the building's state `[b+0x104]` is 2).
/// Each call runs [`DESTRUCTION_PASSES`] x ticks-this-frame passes. A pass counts the debris counter `[b+0x110]` (1 at the start) down;
/// when it reaches 0 the animation counter `[b+0x10c]` (= `[b+0x108]`, [`destruction_bursts`]) is decremented, and if that left
/// something the burst is thrown (docs/projectiles.md) and `[b+0x110]` is set to [`BURST_GAP`] + lockstep `rand(BURST_JITTER)`.
/// When the animation counter reaches 0 instead, the animation ends (stats, then the capture or removal `0x43ab40`). So a building
/// bursts four times (about 3 s), a turret once (about 0.75 s), and the building stays on the map, dead, until then.
pub const DESTRUCTION_PASSES: i32 = 2;
pub const BURST_GAP: i32 = 90;
pub const BURST_JITTER: u32 = 15;

/// `[b+0x108]`, set by the building constructor `0x40a330`: 5 for the buildings (kinds 100..=108, default `0x40a381`), 2 for the turrets
/// (kinds 25..=29, cases 0..=2 of the byte table `0x40af64`, `0x40a93f`). Bursts thrown = this minus 1.
pub fn destruction_bursts(kind_id: i32) -> i32 {
    if (25..=29).contains(&kind_id) { 2 } else { 5 }
}

/// Strength of a flying piece (argument 2 of `0x411dc0`, pushed as `0x28` at `0x46ae04`). The generic impact tail (`0x410881`) calls
/// the area damage `0x42f2c0(x, y, strength, owner, type)` for every bullet with strength > 0, so a landing piece hurts ground units
/// within 32 px and the building in its cell like any shell. (An earlier note read this value as the path delay.)
pub const DEBRIS_STRENGTH: i32 = 40;

/// Variant `[bullet+0x2680]` of a flying piece = the unit's top sprite index `[top+0x54]` (`tier * 5 + class`, [`crate::units::unit_art`]),
/// which selects the 80 frames of `blueboom` / `grenboom` (frame `variant * 80 + counter`). The jump table `0x46b140` is the identity
/// for 0..=14; larger indices (the Advancer's `1I`, the walkers' `5W1..5W5` = 20..24) skip the bullet, so walkers throw no pieces.
pub fn debris_variant(kind: i32) -> Option<u32> {
    match kind {
        1..=15 => Some((((kind - 1) % 3) * 5 + (kind - 1) / 3) as u32),
        _ => None,
    }
}

/// Draw modes of an `AnimSprite` (`[obj+0x34]`, set by its creator after the initialiser `0x4054a0`; draw `0x404e30` with the jump
/// table `0x405080` over `mode - 1`): the blit routine each mode calls. Mode 0 and 1..=5 (nothing sets them, the initialiser stores 0)
/// fall into the plain draw. `(mode, blit routine)`; the routines are described in docs/effects.md ("Blit routines").
pub const ANIM_DRAW_MODES: [(u8, u32); 15] = [
    (1, 0x45edf0),
    (2, 0x45edf0),
    (3, 0x45edf0),
    (4, 0x45edf0),
    (5, 0x45edf0),
    (6, 0x45cde0),  // additive: blasts, bodky sparks, most effects
    (7, 0x45e560),  // 6 + (`[0x49a33a] == 0`): building / air-force destruction picks 6 or 7 by that flag
    (8, 0x45b5e0),  // smoke puffs (additive under the screen mask)
    (9, 0x45c220),  // half-strength additive: the fighter's smoke
    (10, 0x4611f0),
    (11, 0x4611f0),
    (12, 0x45cde0), // a column (see [`ANIM_COLUMN`]); no creator ever sets it
    (13, 0x461740),
    (14, 0x45f4d0), // the missile's smoke (`0x41159e`, `0x4117e8`, `0x4118af`)
    (15, 0x45df40),
];

/// Mode 12 (`0x404e51`) draws the current frame 14 times, additively (`0x45cde0`), at `(x, y - 23 - 46 * k)` for `k = 0..14` (the loop
/// runs `edi = 0, 46, .., 644` while `edi < 0x284`). It is the same column as the satellite laser's ([`satlaser_column`]), a leftover of
/// an earlier version of that effect: no code in the exe stores 12 into an `AnimSprite`'s mode (the immediates stored are 0, 6, 7, 8, 9
/// and 14, the register stores are 6 or 7), so it never runs.
pub const ANIM_COLUMN: (usize, i32, i32) = (14, 46, 23);

/// The satellite laser (bullet type 10). A protected building's check `0x409f50` (docs/advancer-actions.md) kills every foreign Advancer
/// above its footprint (`[unit+0x34] = 0`) and fires this bullet at it: strength [`SATLASER_STRENGTH`], from `(x, y + lift - 15)` to
/// `(x, y + lift + 10)` (`lift = [unit+0x88]` = -64), owner = the Advancer's team, target handle `[+0x2688]` = the Advancer's handle,
/// speed 60, animation counter `[+0x2670]` 0..3 (4 steps each). The impact (`0x4108bc`, types 1, 10 and 31) only subtracts the strength
/// from that handle's unit and makes no area damage, decal or sound; the Advancer is already dead, so the bullet is purely visual.
/// The bullet itself draws nothing; its draw (`0x411100`, additive `0x45cde0`) puts a column at the *end* point (see [`satlaser_column`]).
pub const SATLASER_STRENGTH: i32 = 1000;
pub const SATLASER_START_DY: i32 = -15;
pub const SATLASER_END_DY: i32 = 10;

/// Sprites of the satellite laser column at the end point `(x, y_end)` for the animation counter `frame` (0..=3): one `satlaser` frame
/// `frame + 4` at `y_end - 23`, then 13 frames `frame` at 46 px intervals going up (`y_end - 23 - 46 * k`, `k = 1..=13`), each as
/// `(satlaser frame, x, y)`. The exe skips the whole column when the base point is outside the 480-pixel-high view.
pub fn satlaser_column(frame: i32, x: i32, y_end: i32) -> Vec<(i32, i32, i32)> {
    let base = y_end - 23;
    let mut v = vec![(frame + 4, x, base)];
    v.extend((1..=13).map(|k| (frame, x, base - 46 * k)));
    v
}

/// The burning wreck object (`0x45afc0`, 0x1c bytes, vtable `0x490100`, list `[0x49a3e4]`), created for every destroyed unit except the
/// Advancer by `0x45b210(x, y + [chassis+0x2c], 0, 0, 20, 5)`: fields `+4` x, `+8` y, `+0x10` 0, `+0x14` remaining smoke columns (20),
/// `+0x18` countdown (5). Its update `0x45b0a0` runs once per logic step: when `+0x14 <= 0` the object is removed; otherwise the
/// countdown drops by one and at 0 it reloads [`WRECK_SMOKE_PERIOD`], `+0x14` drops by one and a bullet of type [`WRECK_SMOKE_TYPE`]
/// (the fighter-smoke sprite, speed 20, strength 0, owner 0) rises from the wreck to `(x + 80 + rand(80), y - 80 - rand(40))`, the two
/// draws made in this order with the visual generator `0x437cd0`. The first column leaves after 5 steps, the last after 5 + 19 * 20 = 385.
pub const WRECK_SMOKE_COLUMNS: i32 = 20;
pub const WRECK_SMOKE_FIRST: i32 = 5;
pub const WRECK_SMOKE_PERIOD: i32 = 20;
pub const WRECK_SMOKE_TYPE: u8 = 22;

/// Debris class of a unit kind (`0x46b110` byte table over kinds 1..=45 -> `0x46b100` cases): kinds 1 to 15 cycle through the
/// tiers (30, 40, 60), the walkers 41 to 45 use the largest; everything else (the Advancer 40 included) has none.
pub fn debris(kind: i32) -> Option<Debris> {
    let tier = match kind {
        1..=15 => (kind - 1) % 3,
        41..=45 => 2,
        _ => return None,
    };
    Some([Debris { spread: 30, bullet_param: 2, wreck_param: 2 }, Debris { spread: 40, bullet_param: 2, wreck_param: 3 }, Debris { spread: 60, bullet_param: 3, wreck_param: 4 }][tier as usize])
}

/// Animation sprites a bullet type creates when it lands (impact function `0x40f590`): its blast ([`impact_blast`]) and
/// the type's own effect from [`SPAWNS`] (flame bursts for types 8 and 9, the bomb and fighter-bomb explosions for 18
/// and 26, the three "special fall" blasts for 27 to 29).
pub fn impact_anims(ty: u8) -> Vec<AnimSpec> {
    let mut v: Vec<AnimSpec> = impact_blast(ty).map(blast).into_iter().collect();
    let site = match ty {
        8 => 0x4107cf,
        9 => 0x41084f,
        18 => 0x40fbcc,
        26 => 0x40fb48,
        27 => 0x40f6bc,
        28 => 0x40f740,
        29 => 0x40f7c5,
        _ => return v,
    };
    v.extend(SPAWNS.iter().find(|s| s.site == site).map(|s| s.spec));
    v
}

/// The flame stream of the flamer bullets (types 8 and 9, no sprite of their own): in the flight step (`0x411220`, branch
/// `0x4118e2`: `cmp edx, 8 / 9`) every path step the counter `[+0x264c]` counts down; at 0 it is reloaded from `[+0x2650]`
/// (= the constructor's interval 6, `projectile::BULLET_TYPES`) and an AnimSprite `blast_verysmall` frames 16..22 (period 5,
/// additive, call site `0x411952`) is created at the bullet's position. The last path step (the landing) makes none. So the
/// beam is a trail of tail-end blast puffs every 6th pixel of the path, followed by the flame burst of the impact.
pub const FLAME_TRAIL_SITE: u32 = 0x411952;

/// The puff a flamer bullet of type `ty` drops every `projectile::bullet_type(ty).interval` path steps, see [`FLAME_TRAIL_SITE`].
pub fn flame_trail(ty: u8) -> Option<AnimSpec> {
    if !matches!(ty, 8 | 9) {
        return None;
    }
    SPAWNS.iter().find(|s| s.site == FLAME_TRAIL_SITE).map(|s| s.spec)
}

/// The blue streak of bullet type 27 (the special tier 1 and the walker 5W1, no sprite of their own in the draw function): the
/// same per-step counter as the flamers (`0x411bc2: cmp eax, 0x1b`, interval 5) drops an AnimSprite `bodky_b` frames 4..6, period 1,
/// repeated 3 times, additive (call site `0x411c2d`) at the bullet's position. Pushes: flag 1, repeat 3, to 6, from 4, array, period 1.
pub const SPARK_TRAIL_SITE: u32 = 0x411c2d;

/// The spark bullet type `ty` drops every `interval` path steps, see [`SPARK_TRAIL_SITE`].
pub fn spark_trail(ty: u8) -> Option<AnimSpec> {
    if ty != 27 {
        return None;
    }
    SPAWNS.iter().find(|s| s.site == SPARK_TRAIL_SITE).map(|s| s.spec)
}

/// The beam of the Advancer's gun (bullet type 31, speed 600, no sprite in the draw function `0x410a30`): the flight step
/// (`0x411b0c: cmp eax, 0x1f`) counts `[+0x264c]` down once per path step (the constructor starts it at 2 and the shared tail
/// copies it to `[+0x2650]`, the reload) and drops an AnimSprite at the bullet's position: array `0x5e6b80 + 80 * [+0x268c]`
/// (`0x411b6d..0x411b7c`), frames 0..1, period 1, repeated 15 times, additive (call site `0x411b8a`; pushes: flag 1, repeat 15,
/// to 1, from 0, array, period 1). The constructor (`0x412240`, `[+0x268c] = 2 * (player != 0) + 1`) picks the colour: the
/// local player's gun (player 0) draws `bodky_b` (blue), every other side `bodky_g` (green). Chained along the path they form
/// the blue or green streak between the Advancer and its target; the landing step makes none.
pub const GUN_TRAIL_SITE: u32 = 0x411b8a;
/// `extras\bodky_r`, the first of the four colour arrays (red, blue, yellow, green) that follow each other 80 bytes apart.
pub const BODKY_ARRAYS: u32 = 0x5e6b80;
pub const BODKY_STRIDE: u32 = 80;

/// The spark the Advancer gun's bullet (type 31) of `owner` (0 = player 0, anything else = the other sides) drops every
/// `projectile::bullet_type(31).interval` path steps, see [`GUN_TRAIL_SITE`].
pub fn gun_trail(ty: u8, owner: i32) -> Option<AnimSpec> {
    if ty != 31 {
        return None;
    }
    let colour = 2 * (owner != 0) as u32 + 1;
    Some(AnimSpec { array: BODKY_ARRAYS + BODKY_STRIDE * colour, from: 0, to: 1, period: 1, repeat: 15, mode: 6 })
}

/// The smoke a flying shell leaves (flight step `0x411220`, branches `0x411513` type 1, `0x4115d1` type 24, `0x411695` type 25,
/// `0x411759` type 3, `0x41181b` type 4): once per path step the counter `[+0x264c]` (the type's interval at the start: 8 for types
/// 1, 3 and 4, 0 for the meteors, so those drop one at once) is decremented, and at 0 it reloads with [`Trail::reload`] and an
/// `AnimSprite` is created at the step's point plus `offset + rand(jitter)` on each axis (the visual generator `0x437cd0`, y drawn
/// first). None on the landing step. The flamers' puff (types 8 and 9, no jitter) is [`flame_trail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trail {
    pub reload: i32,
    pub spec: AnimSpec,
    pub jitter: u32,
    pub offset: i32,
}

pub fn trail(ty: u8) -> Option<Trail> {
    let find = |site: u32| SPAWNS.iter().find(|s| s.site == site).map(|s| s.spec);
    Some(match ty {
        // type 1 reloads with `[+0x2650] / 2` = 8 / 2, type 4 likewise, type 3 with `[+0x2650]` = 8
        1 => Trail { reload: 4, spec: find(0x411599)?, jitter: 4, offset: -1 },
        3 => Trail { reload: 8, spec: find(0x4117e3)?, jitter: 4, offset: -1 },
        4 => Trail { reload: 4, spec: find(0x4118aa)?, jitter: 4, offset: -1 },
        24 => Trail { reload: 10, spec: find(0x41165d)?, jitter: 10, offset: -5 },
        25 => Trail { reload: 5, spec: find(0x411721)?, jitter: 6, offset: -3 },
        _ => return None,
    })
}

pub static SPAWNS: &[Spawn] = &[
    sp(0x40887d, 0x5e96a8, 0, 16, 3, "walker teleport, leaving"),
    sp(0x408a85, 0x5e96a8, 0, 16, 3, "walker teleport, arriving"),
    Spawn { site: 0x409ccb, spec: TURRET_DESTROYED, what: "a building destroyed while the 0x40a130 test holds (turrets): e_turret" },
    Spawn { site: 0x409d03, spec: BUILDING_DESTROYED, what: "any other building destroyed: e_bubg" },
    sp(0x40f6bc, 0x5e69f0, 0, 24, 2, "bullet type 27 hits: specialfall_light"),
    sp(0x40f740, 0x5e6a68, 0, 16, 2, "bullet type 28 hits: specialfall_medium"),
    sp(0x40f7c5, 0x5e6ae0, 0, 19, 2, "bullet type 29 hits: specialfall_heavy"),
    sp(0x40fb48, 0x5ea740, 0, 45, 4, "bullet type 26 (fighter bomb) hits: fighter_ex"),
    sp(0x40fbcc, 0x5e9e28, 0, 13, 4, "bullet type 18 (bomber bomb) hits: bomba_ex"),
    sp(0x410063, 0x5e6bd0, 6, 9, 4, "bullet type 19 hits: a bodky_b fragment"),
    sp(0x410342, 0x5e6b80, 6, 16, 2, "bullet type 21 hits: a bodky_r fragment"),
    sp(0x4107cf, 0x5e9340, 0, 13, 2, "bullet type 8 hits: flamerburst_sma"),
    sp(0x41084f, 0x5e93c0, 0, 13, 2, "bullet type 9 hits: flamerburst_big"),
    spm(0x411599, 0x5e6788, 0, 32, 1, 14, "missile trail puff (type 1): missileSmoke"),
    sp(0x41165d, 0x5e9fb8, 0, 42, 2, "meteor_medium trail: meteor_medium_smoke"),
    sp(0x411721, 0x5ea1c0, 0, 42, 1, "meteor_small trail: meteor_small_smoke"),
    spm(0x4117e3, 0x5e6788, 4, 32, 2, 14, "missile smoke, later frames"),
    spm(0x4118aa, 0x5e6788, 0, 32, 2, 14, "missile smoke, all frames"),
    Spawn { site: SPARK_TRAIL_SITE, spec: AnimSpec { array: 0x5e6bd0, from: 4, to: 6, period: 1, repeat: 3, mode: 6 }, what: "bullet type 27 (1S, walker 5W1) trail: a bodky_b spark, every interval path steps" },
    sp(0x411952, 0x5e6230, 16, 22, 5, "flamer bullet (types 8, 9) trail puff: blast_verysmall tail frames"),
    sp(0x411ada, 0x71c548, 0, 14, 1, "menu light flash (bullet type 32)"),
    Spawn { site: 0x411c2d, spec: AnimSpec { array: 0x5e6bd0, from: 4, to: 6, period: 1, repeat: 3, mode: 6 }, what: "bodky_b spark (three passes)" },
    sp(0x4171d5, 0x5e5fb0, 0, 20, 3, "order marker cross_as"),
    sp(0x4175e6, 0x5e6028, 0, 20, 3, "order marker cross_rp"),
    sp(0x418457, 0x5e6140, 0, 8, 3, "click marker"),
    spm(0x4676d8, 0x5ea3d0, 7, 178, 1, 9, "fighter smoke trail"),
    spm(0x467776, 0x5ea3d0, 7, 178, 1, 9, "fighter smoke trail"),
    Spawn { site: 0x467999, spec: AIR_DESTROYED, what: "air force unit destroyed: e_air2" },
    sp(0x467ac4, 0x5e96f8, 0, 52, 4, "air force unit destroyed (slow): e_air2"),
    Spawn { site: 0x46a693, spec: UNIT_CLOUD, what: "ground unit destroyed: e_unit2 cloud (several per unit)" },
    sp(0x46a7b1, 0x5e92e8, 0, 20, 5, "ground unit destroyed: newvrak wreck"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_debris_classes() {
        assert_eq!(debris(1).unwrap().spread, 30);
        assert_eq!(debris(5).unwrap().spread, 40);
        assert_eq!(debris(15).unwrap().wreck_param, 4);
        assert_eq!(debris(44).unwrap().spread, 60);
        assert!(debris(40).is_none() && debris(16).is_none() && debris(46).is_none());
    }

    #[test]
    fn blit_arithmetic() {
        // RGB565 words: dst 0x8410 (16, 32, 16), src 0x4208 (8, 16, 8)
        let (d, s) = (0x8410u16, 0x4208u16);
        assert_eq!(Blit::Plain.apply565(d, s), s);
        assert_eq!(Blit::Additive.apply565(d, s), 24 << 11 | 48 << 5 | 24);
        assert_eq!(Blit::AddHalf.apply565(d, s), 20 << 11 | 40 << 5 | 20);
        assert_eq!(Blit::Average.apply565(d, s), 12 << 11 | 24 << 5 | 12);
        assert_eq!(Blit::Smoke.apply565(d, s), 14 << 11 | 28 << 5 | 14);
        assert_eq!(Blit::Darken.apply565(d, s), 12 << 11 | 24 << 5 | 12);
        assert_eq!(Blit::Shade.apply565(d, 0), 10 << 11 | 20 << 5 | 10);
        assert_eq!(Blit::Lens.apply565(d, 0), 17 << 11 | 34 << 5 | 17);
        // saturation at both ends
        assert_eq!(Blit::Additive.apply565(0xFFFF, 0xFFFF), 0xFFFF);
        assert_eq!(Blit::Darken.apply565(0, 0xFFFF), 0);
        assert_eq!(Blit::Shade.apply565(0x0801, 0), 0);
        assert_eq!(Blit::of_routine(0x4611f0), Blit::Average);
        assert_eq!(Blit::of_routine(0x45c220), Blit::AddHalf);
    }


    #[test]
    fn extras_sit_at_distinct_addresses() {
        // `slots` is the loader's frame cap, not the file's frame count, so neighbours may overlap (sizer / plasma); the
        // start addresses are unique.
        for (i, a) in EXTRAS.iter().enumerate() {
            assert!(EXTRAS[i + 1..].iter().all(|b| b.addr != a.addr), "{} shares its address", a.name);
        }
        let (e, i) = extra_at(0x5e68d4).unwrap();
        assert_eq!((e.name, i), ("__extras\\plasma", 0), "the plasma bullets are not frames of sizer");
        let (e, i) = extra_at(0x5e6694).unwrap();
        assert_eq!((e.name, i), ("__extras\\gula2", 1));
    }

    #[test]
    fn blast_sizes() {
        assert_eq!(blast(0).to, 22);
        assert_eq!(blast(4).to, 23);
        assert_eq!(blast(6).to, 30);
        assert_eq!(blast(1).array, 0x5e62d0);
    }

    #[test]
    fn anim_spec_plays_frames_in_order() {
        let a = AnimSpec { array: 0, from: 4, to: 7, period: 2, repeat: 2, mode: 6 };
        let frames: Vec<_> = (0..14).map(|t| a.frame_at(t)).collect();
        assert_eq!(frames, [Some(4), Some(4), Some(5), Some(5), Some(6), Some(6), Some(4), Some(4), Some(5), Some(5), Some(6), Some(6), None, None]);
    }

    #[test]
    fn timer_animations_loop_or_end() {
        let mut a = BulletAnim::new([0, 3, 1, 2]);
        let frames: Vec<i32> = (0..8).map(|_| { a.step(11, 0); a.frame }).collect();
        assert_eq!(frames, [1, 1, 2, 2, 0, 0, 1, 1]);
        let mut a = BulletAnim::new([0, 2, 1, 1]);
        assert!(a.step(23, 0));
        assert!(!a.step(23, 0));
    }

    #[test]
    fn progress_driven_frames() {
        let mut a = BulletAnim::new([0, 21, 1, 1]);
        a.step(17, 62);
        assert_eq!(a.frame, 3);
        a.step(17, 10_000);
        assert_eq!(a.frame, 19);
        a.step(18, 10_000);
        assert_eq!(a.frame, 8);
    }

    #[test]
    fn impacts_create_their_effects() {
        assert_eq!(impact_anims(3), vec![blast(5)]);
        assert_eq!(impact_anims(8).len(), 1);
        assert_eq!(impact_anims(8)[0].array, 0x5e9340);
        assert_eq!(impact_anims(18)[0].array, 0x5e9e28);
        assert_eq!(impact_anims(29)[0].array, 0x5e6ae0);
        assert!(impact_anims(12).is_empty());
        assert!(SPAWNS.iter().any(|s| s.spec == TURRET_DESTROYED) && SPAWNS.iter().any(|s| s.spec == UNIT_CLOUD));
    }

    #[test]
    fn smoke_and_decals() {
        assert_eq!(smoke_puff(1, 0).array, 0x5e6938);
        assert_eq!(smoke_puff(0, 12).len(), 9 * 17);
        assert_eq!(impact_smoke(15, 1, true), (3, 1));
        assert_eq!(impact_smoke(16, 1, false), (3, 0), "the three lockstep puffs ignore the no-marks layer");
        assert_eq!(impact_smoke(15, 0, true), (0, 1));
        assert_eq!(impact_smoke(13, 1, true), (0, 1), "roll 1 only adds the three puffs for types 15 and 16");
        assert_eq!(impact_smoke(14, 2, false), (0, 0));
        assert_eq!(impact_smoke(5, 2, true), (0, 0));
        assert_eq!(impact_smoke(17, 0, true), (0, 2));
        assert_eq!(impact_smoke(18, 0, false), (0, 0));
        assert!(impact_leaves_decal(3) && !impact_leaves_decal(17) && !impact_leaves_decal(1));
        assert_eq!(Blit::of_routine(0x45df40), Blit::Darken);
    }

    #[test]
    fn draw_table_shape() {
        assert_eq!(bullet_draw(1).len(), 2);
        assert!(bullet_draw(10).is_empty());
        assert_eq!(bullet_draw(24)[1].array, 0x5e9f18);
        assert_eq!(impact_sound(18), Some(65));
        assert_eq!(impact_blast(3), Some(5));
    }
}
