//! Composed grounds (docs/generated-maps.md, docs/map-design.md; not in the exe): a new layout made only of the player's own
//! mission art, following a written [`Plan`].
//!
//! [`ground`] quilts open ground from patches of the base mission's picture (image quilting: each patch overlaps the ones
//! before it by [`OVERLAP`] pixels and joins them along the cheapest seam). [`paste`] then lays whole pieces of missions over
//! it: a cliff ridge, a lake with its shore, a forest block, a whole structure. A piece brings its cells, so the grid always
//! matches the picture. [`generate`] builds a plan's layout and measures it; the file keeps only the result ([`Layout`]), and
//! every load rebuilds the same ground from the install.

use crate::scene::Ground;
use opensow_formats::custommap::{Feature, Layout, Terrain, TrailStamp};

/// Pixels per map cell.
const CELL: usize = 32;
/// Side of a ground patch (pixels).
pub const PATCH: usize = 128;
/// How far a patch overlaps the ones above and left of it (pixels).
pub const OVERLAP: usize = 40;
const STEP: usize = PATCH - OVERLAP;
/// Cells of a feature's rectangle around its own cells that are blended (each side).
const PAD: usize = 4;
/// Width of a feature's fade into the ground (pixels) and how far the fade's line wanders.
const FADE: f32 = 24.0;
const RAGGED: f32 = 10.0;

/// Whether cell `(x, y)` of `g` has no foreground pixel (tree and rock tops).
fn open_cell(g: &Ground, x: usize, y: usize) -> bool {
    let (x0, y0) = (x * CELL, y * CELL);
    !(y0..(y0 + CELL).min(g.h)).any(|py| (x0..(x0 + CELL).min(g.w)).any(|px| g.fg[py * g.w + px]))
}

fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

/// Smooth value noise in 0..1 with lattice spacing `scale` pixels (fixed: the same layout always blends the same way).
fn noise(x: usize, y: usize, scale: f32, seed: u32) -> f32 {
    let (fx, fy) = (x as f32 / scale, y as f32 / scale);
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let c = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, seed) >> 8) as f32 / 16_777_216.0;
    let top = c(0, 0) + (c(1, 0) - c(0, 0)) * sx;
    let bottom = c(0, 1) + (c(1, 1) - c(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Distance in pixels (chamfer 3-4, divided by 3) from each pixel to the nearest `true` one, capped at `cap`.
fn distance(mask: &[bool], w: usize, h: usize, cap: u16) -> Vec<u16> {
    let big = cap.saturating_mul(3).saturating_add(4);
    let mut d: Vec<u16> = mask.iter().map(|&m| if m { 0 } else { big }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1].saturating_add(3));
            }
            if y > 0 {
                v = v.min(d[i - w].saturating_add(3));
                if x > 0 {
                    v = v.min(d[i - w - 1].saturating_add(4));
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1].saturating_add(4));
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1].saturating_add(3));
            }
            if y + 1 < h {
                v = v.min(d[i + w].saturating_add(3));
                if x + 1 < w {
                    v = v.min(d[i + w + 1].saturating_add(4));
                }
                if x > 0 {
                    v = v.min(d[i + w - 1].saturating_add(4));
                }
            }
            d[i] = v;
        }
    }
    d.iter().map(|&v| (v / 3).min(cap)).collect()
}

/// The top-left pixel of each patch of a `w` x `h` picture, row by row.
fn grid(w: usize, h: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..h).step_by(STEP).flat_map(move |y| (0..w).step_by(STEP).map(move |x| (x, y)))
}

/// The cheapest path down `err` (`rows` x `cols`, each step to the same or a neighbouring column): the column per row.
fn cut(err: &[f32], rows: usize, cols: usize) -> Vec<usize> {
    let mut cost = err.to_vec();
    for r in 1..rows {
        for c in 0..cols {
            let mut best = cost[(r - 1) * cols + c];
            if c > 0 {
                best = best.min(cost[(r - 1) * cols + c - 1]);
            }
            if c + 1 < cols {
                best = best.min(cost[(r - 1) * cols + c + 1]);
            }
            cost[r * cols + c] += best;
        }
    }
    let mut path = vec![0; rows];
    let last = &cost[(rows - 1) * cols..rows * cols];
    path[rows - 1] = (0..cols).fold(0, |b, c| if last[c] < last[b] { c } else { b });
    for r in (0..rows - 1).rev() {
        let j = path[r + 1];
        let (lo, hi) = (j.saturating_sub(1), (j + 2).min(cols));
        path[r] = (lo..hi).fold(lo, |b, c| if cost[r * cols + c] < cost[r * cols + b] { c } else { b });
    }
    path
}

/// Whether the source window at `(sx, sy)` lies inside `src`.
fn fits(src: &Ground, sx: usize, sy: usize) -> bool {
    sx + PATCH <= src.w && sy + PATCH <= src.h
}

/// Joins the patch of `src` at `(sx, sy)` into `out` at `(x, y)` along the cheapest seams through its overlaps.
fn join(out: &mut Ground, src: &Ground, (sx, sy): (usize, usize), (x, y): (usize, usize)) {
    if !fits(src, sx, sy) {
        return;
    }
    let (pw, ph) = (PATCH.min(out.w - x), PATCH.min(out.h - y));
    let diff = |r: usize, c: usize| {
        let (o, s) = (((y + r) * out.w + x + c) * 3, ((sy + r) * src.w + sx + c) * 3);
        (0..3).map(|k| (f32::from(out.bg[o + k]) - f32::from(src.bg[s + k])).powi(2)).sum::<f32>()
    };
    // the first column of the patch taken from the patch itself, per row (left seam) and per column (top seam)
    let left: Vec<usize> = if x > 0 {
        let cols = OVERLAP.min(pw);
        let err: Vec<f32> = (0..ph).flat_map(|r| (0..cols).map(move |c| (r, c))).map(|(r, c)| diff(r, c)).collect();
        cut(&err, ph, cols)
    } else {
        vec![0; ph]
    };
    let top: Vec<usize> = if y > 0 {
        let rows = OVERLAP.min(ph);
        let err: Vec<f32> = (0..pw).flat_map(|c| (0..rows).map(move |r| (r, c))).map(|(r, c)| diff(r, c)).collect();
        cut(&err, pw, rows)
    } else {
        vec![0; pw]
    };
    for r in 0..ph {
        for c in left[r]..pw {
            if r < top[c] {
                continue;
            }
            let (o, s) = ((y + r) * out.w + x + c, (sy + r) * src.w + sx + c);
            out.bg[o * 3..o * 3 + 3].copy_from_slice(&src.bg[s * 3..s * 3 + 3]);
            out.fg[o] = src.fg[s];
        }
    }
}

/// An empty ground of `base`'s size: no picture yet, every cell open except the outer ring (blocked, and the air border).
fn blank(base: &Ground) -> Ground {
    let mut map = base.map.clone();
    let (w, h) = (map.width as usize, map.height as usize);
    for (i, c) in map.cells.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        let ring = x == 0 || y == 0 || x + 1 == w || y + 1 == h;
        c.blocked = ring;
        c.border = ring;
        c.no_build = false;
        c.no_marks = false;
    }
    Ground { bg: vec![0; base.w * base.h * 3], fg: vec![false; base.w * base.h], map, w: base.w, h: base.h }
}

/// The open ground of a layout: `patches` of `base`'s picture joined row by row (missing ones leave the rest black).
pub fn ground(base: &Ground, patches: &[(u16, u16)]) -> Ground {
    let mut out = blank(base);
    for (&(sx, sy), at) in patches.iter().zip(grid(base.w, base.h)) {
        join(&mut out, base, (usize::from(sx), usize::from(sy)), at);
    }
    out
}

/// Pastes feature `f`, cut from `src`, onto `dst`: its pixels within a ragged fade around its cells (ground-like source pixels
/// shifted to the destination's ground colour), its foreground, and its cells' flags.
pub fn paste(dst: &mut Ground, src: &Ground, f: &Feature) {
    let (w, h) = (usize::from(f.w), usize::from(f.h));
    let (gw, gh) = (w + 2 * PAD, h + 2 * PAD);
    // the feature's cells, and those grown by one cell (its overhang, and the shore flags)
    let own = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && f.cells[y as usize * w + x as usize];
    let core: Vec<bool> = (0..gh * gw)
        .map(|i| {
            let (x, y) = ((i % gw) as i32 - PAD as i32, (i / gw) as i32 - PAD as i32);
            (-1..=1).any(|dy| (-1..=1).any(|dx| own(x + dx, y + dy)))
        })
        .collect();
    let (pw, ph) = (gw * CELL, gh * CELL);
    let cell_of = |i: usize| (i / pw / CELL) * gw + (i % pw) / CELL;
    let own_px: Vec<bool> = (0..pw * ph).map(|i| own((cell_of(i) % gw) as i32 - PAD as i32, (cell_of(i) / gw) as i32 - PAD as i32)).collect();
    let outside: Vec<bool> = own_px.iter().map(|&o| !o).collect();
    let (d_out, d_in) = (distance(&own_px, pw, ph, 160), distance(&outside, pw, ph, 64));
    // pixel (px, py) of the padded rectangle in the source and in the map
    let (sx0, sy0) = (i64::from(f.x) * CELL as i64 - (PAD * CELL) as i64, i64::from(f.y) * CELL as i64 - (PAD * CELL) as i64);
    let (dx0, dy0) = (i64::from(f.dx) * CELL as i64 - (PAD * CELL) as i64, i64::from(f.dy) * CELL as i64 - (PAD * CELL) as i64);
    let at = |px: usize, py: usize| -> Option<(usize, usize)> {
        let (sx, sy, dx, dy) = (sx0 + px as i64, sy0 + py as i64, dx0 + px as i64, dy0 + py as i64);
        let inside = |x: i64, y: i64, w: usize, h: usize| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h;
        (inside(sx, sy, src.w, src.h) && inside(dx, dy, dst.w, dst.h)).then(|| (sy as usize * src.w + sx as usize, dy as usize * dst.w + dx as usize))
    };
    // the source's ground and the map's, measured in the padding (40-63 px out from the feature's cells)
    let (mut from, mut to, mut n) = ([0f64; 3], [0f64; 3], 0f64);
    for i in (0..pw * ph).filter(|&i| (40..64).contains(&d_out[i])) {
        if let Some((s, o)) = at(i % pw, i / pw) {
            for k in 0..3 {
                from[k] += f64::from(src.bg[s * 3 + k]);
                to[k] += f64::from(dst.bg[o * 3 + k]);
            }
            n += 1.0;
        }
    }
    let n = n.max(1.0);
    let from = from.map(|v| (v / n) as f32);
    // snow cliffs are plateaus: only the face is blocked, the snowy top above it looks like ground; on snow (blue above red)
    // the source's surroundings are kept whole for three cells, so the tops come along
    let keep = if from[2] > from[0] { 96.0 } else { 0.0 };
    let shift: Vec<f32> = (0..3).map(|k| (to[k] / n) as f32 - from[k]).collect();
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // a whole structure: its picture over the map's own ground; the source's ground (by hue) gives way to the map's, darkened
    // where it was in shadow
    let structure = !f.source.warmonger && STRUCTURES.iter().any(|s| s.mission == f.source.mission && s.rect == (usize::from(f.x), usize::from(f.y), w, h));
    let chroma = |c: [f32; 3]| -> [f32; 3] {
        let t = (c[0] + c[1] + c[2]).max(1.0);
        [c[0] / t, c[1] / t, c[2] / t]
    };
    let (from_c, from_l) = (chroma(from), (from[0] + from[1] + from[2]).max(1.0));
    // the hue difference from the source ground, averaged over 9 x 9 pixels: soil is noisy, a structure is a smooth area
    let hue: Vec<f32> = if structure {
        let raw: Vec<f32> = (0..pw * ph)
            .map(|i| match at(i % pw, i / pw) {
                Some((s, _)) => {
                    let pc = chroma([f32::from(src.bg[s * 3]), f32::from(src.bg[s * 3 + 1]), f32::from(src.bg[s * 3 + 2])]);
                    (0..3).map(|k| (pc[k] - from_c[k]).abs()).sum()
                }
                None => 0.0,
            })
            .collect();
        let blur = |v: &[f32], horizontal: bool| -> Vec<f32> {
            (0..pw * ph)
                .map(|i| {
                    let (x, y) = ((i % pw) as i64, (i / pw) as i64);
                    let (mut t, mut n) = (0.0, 0.0);
                    for d in -4..=4i64 {
                        let (xx, yy) = if horizontal { (x + d, y) } else { (x, y + d) };
                        if xx >= 0 && yy >= 0 && (xx as usize) < pw && (yy as usize) < ph {
                            t += v[yy as usize * pw + xx as usize];
                            n += 1.0;
                        }
                    }
                    t / n
                })
                .collect()
        };
        blur(&blur(&raw, true), false)
    } else {
        Vec::new()
    };
    // the source ground's brightness, averaged the same way: its shading (shadows) carries over, its texture does not
    let shade: Vec<f32> = if structure {
        let raw: Vec<f32> = (0..pw * ph)
            .map(|i| at(i % pw, i / pw).map_or(1.0, |(s, _)| (f32::from(src.bg[s * 3]) + f32::from(src.bg[s * 3 + 1]) + f32::from(src.bg[s * 3 + 2])) / from_l))
            .collect();
        let mut v = raw;
        for horizontal in [true, false, true, false] {
            v = (0..pw * ph)
                .map(|i| {
                    let (x, y) = ((i % pw) as i64, (i / pw) as i64);
                    let (mut t, mut n) = (0.0, 0.0);
                    for d in -3..=3i64 {
                        let (xx, yy) = if horizontal { (x + d, y) } else { (x, y + d) };
                        if xx >= 0 && yy >= 0 && (xx as usize) < pw && (yy as usize) < ph {
                            t += v[yy as usize * pw + xx as usize];
                            n += 1.0;
                        }
                    }
                    t / n
                })
                .collect();
        }
        v
    } else {
        Vec::new()
    };
    for i in 0..pw * ph {
        let Some((s, o)) = at(i % pw, i / pw) else { continue };
        let p = [f32::from(src.bg[s * 3]), f32::from(src.bg[s * 3 + 1]), f32::from(src.bg[s * 3 + 2])];
        if structure {
            if !own_px[i] {
                continue;
            }
            let lit = (p[0] + p[1] + p[2]) / from_l;
            // ground-like: the source ground's hue (averaged), no brighter than it (a lit structure part of that hue stays);
            // reddish rocks of the source count as ground too
            // a foreground pixel (what units walk behind: the structure's tall parts) is never ground
            let ground = if src.fg[s] { 0.0 } else { (1.0 - smooth(0.17, 0.26, hue[i])) * (1.0 - smooth(1.3, 1.6, lit)) };
            // the shadows the structure casts, fading out towards the rectangle's edge
            let dark = 1.0 + (shade[i].clamp(0.2, 1.0) - 1.0) * smooth(0.0, 24.0, f32::from(d_in[i]));
            for c in 0..3 {
                let old = f32::from(dst.bg[o * 3 + c]);
                let v = p[c] * (1.0 - ground) + old * dark * ground;
                dst.bg[o * 3 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
            dst.fg[o] = src.fg[s];
            continue;
        }
        // how much the source pixel looks like its open ground (soil, grass) rather than the feature (rock, water, trees, shadow)
        let ground = 1.0 - smooth(40.0, 100.0, (0..3).map(|k| (p[k] - from[k]).abs()).sum());
        let wander = (noise(o % dst.w, o / dst.w, 23.0, 0x6c61_796f) - 0.5) * 2.0 * RAGGED;
        let (dout, din) = (f32::from(d_out[i]), f32::from(d_in[i]));
        let (a, k) = if own_px[i] {
            // the feature's cells whole, except ground near their edge, which fades along a ragged line (no square halos)
            (if keep > 0.0 { 1.0 } else { 1.0 - ground * (1.0 - smooth(0.0, 16.0, din + wander * 0.6)) }, ground * (1.0 - smooth(8.0, 48.0, din)))
        } else {
            // around them: ground fades out; the feature's overhang (cliff faces, shadows, tree tops) within a cell stays
            let fade = (1.0 - ((dout - keep).max(0.0) + wander) / FADE).clamp(0.0, 1.0);
            let overhang = (1.0 - ground) * (1.0 - smooth(6.0, 28.0, (dout - keep).max(0.0) + wander));
            ((fade * ground).max(overhang), ground)
        };
        if a <= 0.0 {
            continue;
        }
        for c in 0..3 {
            let v = p[c] + shift[c] * k;
            let old = f32::from(dst.bg[o * 3 + c]);
            dst.bg[o * 3 + c] = (old + (v - old) * a).round().clamp(0.0, 255.0) as u8;
        }
        if a >= 0.5 {
            dst.fg[o] = src.fg[s];
        }
    }
    // cells: the feature's own take the source's flags; the grown ring takes its no-build and no-marks (shores)
    let (mw, mh) = (dst.map.width as i64, dst.map.height as i64);
    for cy in 0..gh {
        for cx in 0..gw {
            if !core[cy * gw + cx] {
                continue;
            }
            let (x, y) = (cx as i64 - PAD as i64, cy as i64 - PAD as i64);
            let (sx, sy, tx, ty) = (i64::from(f.x) + x, i64::from(f.y) + y, i64::from(f.dx) + x, i64::from(f.dy) + y);
            if sx < 0 || sy < 0 || sx >= i64::from(src.map.width) || sy >= i64::from(src.map.height) || tx <= 0 || ty <= 0 || tx >= mw - 1 || ty >= mh - 1 {
                continue;
            }
            let s = src.map.cells[(sy * i64::from(src.map.width) + sx) as usize];
            let d = &mut dst.map.cells[(ty * mw + tx) as usize];
            if own(x as i32, y as i32) {
                d.blocked = s.blocked;
            }
            d.no_build = s.no_build || own(x as i32, y as i32) && s.blocked;
            d.no_marks = s.no_marks;
        }
    }
}

/// A painted structure that is pasted whole (docs/generated-maps.md): its rectangle of cells holds the whole picture and has
/// open ground all around its edge.
#[derive(Debug, Clone, Copy)]
pub struct Structure {
    /// The name a plan uses.
    pub name: &'static str,
    /// The base-game mission it is painted in.
    pub mission: i32,
    /// The rectangle (cells): x, y, w, h.
    pub rect: (usize, usize, usize, usize),
}

/// The structures a plan may place (base-game missions; checked by eye on a plain ground). All stand on orange ground, so
/// only orange maps can use them ([`SAME_TERRAIN`]).
pub const STRUCTURES: [Structure; 3] = [
    Structure { name: "temple", mission: 11, rect: (35, 38, 28, 22) },
    Structure { name: "tower", mission: 4, rect: (35, 16, 17, 23) },
    Structure { name: "tower2", mission: 4, rect: (17, 31, 16, 21) },
];

/// Regions never cut into pieces: structures that cannot be pasted whole (M10's dome by the river, M17's in its lake, M05's
/// wreck by its lake, M22's Overmind base, M03's dam and bridge, M02's walker wrecks) and the ones of [`STRUCTURES`]. Mission, x, y, w, h (cells).
const WHOLE: [(i32, usize, usize, usize, usize); 8] = [
    (10, 0, 33, 56, 47),
    (17, 69, 22, 27, 21),
    (5, 13, 26, 72, 27),
    (22, 18, 1, 55, 56),
    (3, 30, 10, 16, 14),
    (3, 62, 50, 22, 16),
    (2, 37, 18, 14, 13),
    (2, 50, 10, 12, 10),
];

/// One placement of a plan: the piece of `source` that holds cell `seed` (a cliff, rock group, forest or lake, as the generator
/// cuts them), its bounding box's top-left put at `at`. `whole` pastes the whole box (a plateau with the ground its cliffs ring).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub source: Terrain,
    pub seed: (usize, usize),
    pub at: (usize, usize),
    pub whole: bool,
}

/// A map's design (docs/generated-maps.md, `docs/map-plans/`): the pieces of original missions, where each goes, the whole
/// structures, and the start areas the map is designed around.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The map's name.
    pub name: String,
    /// The mission whose size and open ground the map takes.
    pub base: Terrain,
    /// Fixes the open ground's patches.
    pub seed: u64,
    /// The pieces, pasted in this order.
    pub pieces: Vec<Placement>,
    /// Structures: index into [`STRUCTURES`] and the top-left cell of its rectangle.
    pub structures: Vec<(usize, (usize, usize))>,
    /// The start areas (cells: x, y, w, h); they must stay open. The first two are measured against each other.
    pub starts: Vec<(usize, usize, usize, usize)>,
    /// Worn trails: cells the trail passes, in order (routed between them over open ground).
    pub trails: Vec<Vec<(usize, usize)>>,
    /// After the placed pieces: fill the open fields with pieces of these missions up to this share of blocked cells.
    pub fill: Option<(f32, Vec<Terrain>)>,
    /// The narrowest passage the fill leaves between two pieces (cells; `fill 60% gap 2 ..`, default 3).
    pub gap: usize,
    /// The fewest building spots ([`Generated::building_spots`]) the fill may leave (`fill 60% spots 45 ..`, default 0).
    pub spots: usize,
    /// The narrowest passage the fill may make (cells; `fill 60% lane 3 ..`, default 2): every open cell stays inside an
    /// open `lane` x `lane` square, and the front between the first two start areas stays wider than a lane.
    pub lane: usize,
    /// The open ways (docs/generated-maps.md "Flows"): the primary flow, then the branches off it; each a width and the cells
    /// it runs through.
    pub ways: Vec<(usize, Vec<(usize, usize)>)>,
    /// Pockets at the ends of branches (cells: x, y, w, h), kept open.
    pub rooms: Vec<(usize, usize, usize, usize)>,
    /// Wall everything else: pieces of these missions cover the ground outside the ways, rooms and start areas until this
    /// share of it is blocked.
    pub wall: Option<(f32, Vec<Terrain>)>,
}

fn terrain(v: &str) -> Option<Terrain> {
    let warmonger = v.starts_with(['W', 'w']);
    v.get(1..).and_then(|n| n.parse::<i32>().ok()).map(|mission| Terrain { warmonger, mission })
}

fn pair(v: &str, sep: char) -> Option<(usize, usize)> {
    let (a, b) = v.split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

impl Plan {
    /// Reads a plan: `name TEXT`, `base M18`, `seed N`, `piece M18 X,Y at X,Y [whole]`, `structure NAME at X,Y`,
    /// `start X,Y WxH`, `trail X,Y X,Y ..` (cells), `fill 60% M18 M21 ..`; `;` starts a comment.
    pub fn parse(text: &str) -> Result<Plan, String> {
        let mut plan = Plan {
            name: String::new(),
            base: Terrain { warmonger: false, mission: 0 },
            seed: 1,
            pieces: Vec::new(),
            structures: Vec::new(),
            starts: Vec::new(),
            trails: Vec::new(),
            fill: None,
            gap: 3,
            spots: 0,
            lane: 2,
            ways: Vec::new(),
            rooms: Vec::new(),
            wall: None,
        };
        let mut has_base = false;
        for (n, line) in text.lines().enumerate() {
            let t = line.split(';').next().unwrap_or("").trim();
            if t.is_empty() {
                continue;
            }
            let bad = || format!("line {}: {t}", n + 1);
            let words: Vec<&str> = t.split_whitespace().collect();
            match words[0] {
                "name" => plan.name = t["name".len()..].trim().to_string(),
                "base" => {
                    plan.base = words.get(1).and_then(|v| terrain(v)).ok_or_else(bad)?;
                    has_base = true;
                }
                "seed" => plan.seed = words.get(1).and_then(|v| v.parse().ok()).ok_or_else(bad)?,
                "piece" => {
                    let (Some(src), Some(seed), Some(&"at"), Some(at)) =
                        (words.get(1).and_then(|v| terrain(v)), words.get(2).and_then(|v| pair(v, ',')), words.get(3), words.get(4).and_then(|v| pair(v, ',')))
                    else {
                        return Err(bad());
                    };
                    plan.pieces.push(Placement { source: src, seed, at, whole: words.get(5) == Some(&"whole") });
                }
                "structure" => {
                    let k = words.get(1).and_then(|v| STRUCTURES.iter().position(|s| s.name == *v)).ok_or_else(bad)?;
                    let at = (words.get(2) == Some(&"at")).then(|| words.get(3).and_then(|v| pair(v, ','))).flatten().ok_or_else(bad)?;
                    plan.structures.push((k, at));
                }
                "flow" | "branch" => {
                    let width = words.get(1).and_then(|v| v.parse::<usize>().ok()).ok_or_else(bad)?;
                    let points: Option<Vec<(usize, usize)>> = words[2..].iter().map(|v| pair(v, ',')).collect();
                    let points = points.filter(|p| p.len() >= 2).ok_or_else(bad)?;
                    if words[0] == "flow" {
                        // the primary flow: a worn trail runs along it
                        plan.trails.insert(0, points.clone());
                        plan.ways.insert(0, (width, points));
                    } else {
                        plan.ways.push((width, points));
                    }
                }
                "room" => {
                    let (Some(at), Some(size)) = (words.get(1).and_then(|v| pair(v, ',')), words.get(2).and_then(|v| pair(v, 'x'))) else { return Err(bad()) };
                    plan.rooms.push((at.0, at.1, size.0, size.1));
                }
                "wall" => {
                    let share = words.get(1).and_then(|v| v.trim_end_matches('%').parse::<f32>().ok()).ok_or_else(bad)?;
                    let from: Vec<Terrain> = words[2..].iter().filter_map(|v| terrain(v)).collect();
                    plan.wall = Some((if share > 1.0 { share / 100.0 } else { share }, from));
                }
                "fill" => {
                    let share = words.get(1).and_then(|v| v.trim_end_matches('%').parse::<f32>().ok()).ok_or_else(bad)?;
                    if let Some(i) = words.iter().position(|&v| v == "gap") {
                        plan.gap = words.get(i + 1).and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                    }
                    if let Some(i) = words.iter().position(|&v| v == "lane") {
                        plan.lane = words.get(i + 1).and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                    }
                    if let Some(i) = words.iter().position(|&v| v == "spots") {
                        plan.spots = words.get(i + 1).and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                    }
                    let from: Vec<Terrain> = words[2..].iter().filter_map(|v| terrain(v)).collect();
                    plan.fill = Some((if share > 1.0 { share / 100.0 } else { share }, from));
                }
                "trail" => {
                    let points: Option<Vec<(usize, usize)>> = words[1..].iter().map(|v| pair(v, ',')).collect();
                    plan.trails.push(points.filter(|p| p.len() >= 2).ok_or_else(bad)?);
                }
                "start" => {
                    let (Some(at), Some(size)) = (words.get(1).and_then(|v| pair(v, ',')), words.get(2).and_then(|v| pair(v, 'x'))) else { return Err(bad()) };
                    plan.starts.push((at.0, at.1, size.0, size.1));
                }
                _ => return Err(bad()),
            }
        }
        if !has_base {
            return Err("no base mission".into());
        }
        Ok(plan)
    }

    /// Every mission the plan needs loaded: the base, the pieces' and the structures' missions.
    pub fn missions(&self) -> Vec<Terrain> {
        let mut v = vec![self.base];
        for t in self
            .pieces
            .iter()
            .map(|p| p.source)
            .chain(self.fill.iter().flat_map(|f| f.1.iter().copied()))
            .chain(self.wall.iter().flat_map(|f| f.1.iter().copied()))
            .chain(self.structures.iter().map(|s| Terrain { warmonger: false, mission: STRUCTURES[s.0].mission }))
        {
            if !v.contains(&t) {
                v.push(t);
            }
        }
        v
    }
}

/// Side of the blocks the trail mask is measured in (pixels).
const TRAIL_BLOCK: usize = 16;

/// The base mission's worn trails (docs/generated-maps.md), per [`TRAIL_BLOCK`] block: open ground whose green-to-red ratio
/// is at least 0.18 below the ground's median (the reddish-brown tracks and plazas of the forest maps). Brown and orange
/// grounds have no such contrast and give few or none.
fn trail_blocks(g: &Ground) -> (Vec<bool>, usize, usize) {
    let (bw, bh) = (g.w / TRAIL_BLOCK, g.h / TRAIL_BLOCK);
    let gw = g.map.width as usize;
    let mut ratio = vec![f32::NAN; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let c = g.map.cells[(by * TRAIL_BLOCK / CELL) * gw + bx * TRAIL_BLOCK / CELL];
            if c.blocked || c.no_build {
                continue;
            }
            let (mut r, mut gr, mut fg) = (0f32, 0f32, false);
            for py in (by * TRAIL_BLOCK..(by + 1) * TRAIL_BLOCK).step_by(2) {
                for px in (bx * TRAIL_BLOCK..(bx + 1) * TRAIL_BLOCK).step_by(2) {
                    let o = py * g.w + px;
                    r += f32::from(g.bg[o * 3]);
                    gr += f32::from(g.bg[o * 3 + 1]);
                    fg |= g.fg[o];
                }
            }
            if !fg {
                ratio[by * bw + bx] = gr / r.max(1.0);
            }
        }
    }
    let mut known: Vec<f32> = ratio.iter().copied().filter(|v| !v.is_nan()).collect();
    if known.is_empty() {
        return (vec![false; bw * bh], bw, bh);
    }
    known.sort_by(|a, b| a.total_cmp(b));
    let median = known[known.len() / 2];
    (ratio.iter().map(|&v| !v.is_nan() && v < median - 0.18).collect(), bw, bh)
}

/// Lays trail stamps of `base`'s picture onto `dst` (the open ground, before the features): each a ragged round dab that
/// fades out over its outer third.
pub fn paint_trails(dst: &mut Ground, base: &Ground, stamps: &[TrailStamp]) {
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    for t in stamps {
        let r = f32::from(t.radius.max(1));
        let reach = (r * 1.3) as i64;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (x, y) = (i64::from(t.at.0) + dx, i64::from(t.at.1) + dy);
                let (sx, sy) = (i64::from(t.from.0) + dx, i64::from(t.from.1) + dy);
                if x < 0 || y < 0 || x as usize >= dst.w || y as usize >= dst.h || sx < 0 || sy < 0 || sx as usize >= base.w || sy as usize >= base.h {
                    continue;
                }
                let (o, s) = (y as usize * dst.w + x as usize, sy as usize * base.w + sx as usize);
                // ragged at the edge, and worn unevenly: in places only half the trail texture shows
                let ragged = (noise(x as usize, y as usize, 11.0, 0x7472_6169) - 0.5) * 0.9;
                let worn = 0.6 + 0.4 * noise(x as usize, y as usize, 47.0, 0x776f_726e);
                let a = (1.0 - smooth(0.3, 1.0, ((dx * dx + dy * dy) as f32).sqrt() / r + ragged)) * worn;
                if a <= 0.0 {
                    continue;
                }
                for c in 0..3 {
                    let old = f32::from(dst.bg[o * 3 + c]);
                    dst.bg[o * 3 + c] = (old + (f32::from(base.bg[s * 3 + c]) - old) * a).round() as u8;
                }
            }
        }
    }
}

/// What [`generate`] made: the layout to store and how the map plays.
#[derive(Debug, Clone)]
pub struct Generated {
    /// The choices, as the map file keeps them.
    pub layout: Layout,
    /// Share of the cells blocked.
    pub blocked: f32,
    /// Narrowest cross-section between the first two start areas (cells): the number of cell-disjoint paths.
    pub front: usize,
    /// Where it is: the cells of the cut.
    pub cut: Vec<(usize, usize)>,
    /// Walking distance between their centres (cells), and the straight (Manhattan) distance.
    pub path: (usize, usize),
    /// The widest open space outside the start areas: the most cells from an open cell to the nearest blocked one, and where.
    pub widest: (usize, (usize, usize)),
    /// Every open space of 6 cells or more to the nearest blocked cell, greatest first (one per 12-cell neighbourhood).
    pub open_spaces: Vec<(usize, (usize, usize))>,
    /// Room for buildings: how many separate open, buildable 6 x 6 squares fit outside the start areas (a 4 x 4 building with
    /// a cell around it to drive past, or a carryall's landing).
    pub building_spots: usize,
    /// Open cells left outside the ways, rooms and start areas by a wall (gaps between its pieces).
    pub leaks: usize,
    /// Per room, then per start area: how many separate openings its edge has (the openings in the ring of cells one and
    /// two cells outside it that lead somewhere: more than eight open cells beyond), and how many open cells touch its edge
    /// (the openings' total width). One opening as wide as its branch means one entrance.
    pub entrances: Vec<(usize, usize)>,
    /// Per room: whether its branch is its only way in (blocking the branch halfway cuts the room off from the first start
    /// area); `None` for a room no branch ends at.
    pub sealed: Vec<Option<bool>>,
    /// The seed the layout was made with (the plan's, or a later one tried for sealed pockets).
    pub seed: u64,
}

/// SplitMix64: a small, fixed random generator (the same seed always gives the same map).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// A number in `lo..hi` (`lo` when the range is empty).
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            lo
        } else {
            lo + (self.next() % (hi - lo) as u64) as usize
        }
    }
}

/// A piece the generator can place: a component of a mission's cells, with the map edges it touches.
struct Piece {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    cells: Vec<bool>,
    /// Top, bottom, left, right.
    touches: [bool; 4],
}

/// Components (4-connected) of `mask` (`w` x `h`), each as its cells.
fn components(mask: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let (mut stack, mut comp) = (vec![start], Vec::new());
        seen[start] = true;
        while let Some(i) = stack.pop() {
            comp.push(i);
            for j in neighbours(i, w, h) {
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        out.push(comp);
    }
    out
}

fn neighbours(i: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % w, i / w);
    [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)].into_iter().flatten()
}

/// Cells within `r` (chessboard) of a `true` one.
fn grow(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let big = usize::MAX / 2;
    let mut d: Vec<usize> = mask.iter().map(|&m| if m { 0 } else { big }).collect();
    for _ in 0..2 {
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                for (dx, dy) in [(-1i64, 0i64), (-1, -1), (0, -1), (1, -1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w {
                        d[i] = d[i].min(d[ny as usize * w + nx as usize] + 1);
                    }
                }
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let i = y * w + x;
                for (dx, dy) in [(1i64, 0i64), (1, 1), (0, 1), (-1, 1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && (nx as usize) < w && (ny as usize) < h {
                        d[i] = d[i].min(d[ny as usize * w + nx as usize] + 1);
                    }
                }
            }
        }
    }
    d.iter().map(|&v| v <= r).collect()
}

/// The pieces of one mission's ground: lakes (water with its shore rocks) and blocked land (cliffs, rocks, forests), split
/// at the map's outer ring; nothing that touches a region of [`WHOLE`] or a [`STRUCTURES`] rectangle.
fn pieces(t: Terrain, g: &Ground) -> Vec<Piece> {
    let (w, h) = (g.map.width as usize, g.map.height as usize);
    let blocked: Vec<bool> = g.map.cells.iter().map(|c| c.blocked).collect();
    let water: Vec<bool> = (0..w * h)
        .map(|i| {
            if !blocked[i] {
                return false;
            }
            let (cx, cy) = (i % w, i / w);
            let mut m = [0f64; 3];
            for py in (cy * CELL..(cy + 1) * CELL).step_by(4) {
                for px in (cx * CELL..(cx + 1) * CELL).step_by(4) {
                    let o = (py.min(g.h - 1) * g.w + px.min(g.w - 1)) * 3;
                    for c in 0..3 {
                        m[c] += f64::from(g.bg[o + c]);
                    }
                }
            }
            m[2] > m[0] + 25.0 * 64.0 && m[2] > m[1] + 10.0 * 64.0
        })
        .collect();
    let mut whole = vec![false; w * h];
    let regions = WHOLE.iter().copied().chain(STRUCTURES.iter().map(|s| (s.mission, s.rect.0, s.rect.1, s.rect.2, s.rect.3)));
    if !t.warmonger {
        for (m, x, y, rw, rh) in regions.filter(|r| r.0 == t.mission) {
            for cy in y..(y + rh).min(h) {
                for cx in x..(x + rw).min(w) {
                    whole[cy * w + cx] = true;
                }
            }
            let _ = m;
        }
    }
    let near_water = grow(&water, w, h, 2);
    let ring = |i: usize| i.is_multiple_of(w) || i / w == 0 || i % w + 1 == w || i / w + 1 == h;
    let rock: Vec<bool> = (0..w * h).map(|i| blocked[i] && !near_water[i] && !ring(i)).collect();
    let mut big = vec![false; w * h];
    for c in components(&water, w, h).into_iter().filter(|c| c.len() >= 20) {
        for i in c {
            big[i] = true;
        }
    }
    let shore = grow(&big, w, h, 2);
    let lake: Vec<bool> = (0..w * h).map(|i| big[i] || shore[i] && blocked[i]).collect();
    let mut out = Vec::new();
    for (mask, is_lake) in [(lake, true), (rock, false)] {
        for comp in components(&mask, w, h) {
            if comp.len() < if is_lake { 20 } else { 2 } || comp.iter().any(|&i| whole[i]) {
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
            for &i in &comp {
                (x0, y0, x1, y1) = (x0.min(i % w), y0.min(i / w), x1.max(i % w + 1), y1.max(i / w + 1));
            }
            let touches = [y0 <= 1, y1 + 1 >= h, x0 <= 1, x1 + 1 >= w];
            if touches[0] {
                y0 = 0;
            }
            if touches[1] {
                y1 = h;
            }
            if touches[2] {
                x0 = 0;
            }
            if touches[3] {
                x1 = w;
            }
            let (pw, ph) = (x1 - x0, y1 - y0);
            if pw > 255 || ph > 255 {
                continue;
            }
            let mut cells = vec![false; pw * ph];
            for &i in &comp {
                cells[(i / w - y0) * pw + i % w - x0] = true;
            }
            out.push(Piece { x: x0, y: y0, w: pw, h: ph, cells, touches });
        }
    }
    out
}

/// Mean colour of the open ground among `cells` of `g` (not blocked, not no-build, no foreground), sampled every 4 pixels.
fn soil(g: &Ground, cells: impl Iterator<Item = (usize, usize)>) -> Option<[f32; 3]> {
    let gw = g.map.width as usize;
    let (mut sum, mut n) = ([0f64; 3], 0f64);
    for (x, y) in cells {
        let c = g.map.cells[y * gw + x];
        if c.blocked || c.no_build || !open_cell(g, x, y) {
            continue;
        }
        for py in (y * CELL..(y + 1) * CELL).step_by(4) {
            for px in (x * CELL..(x + 1) * CELL).step_by(4) {
                let o = (py * g.w + px) * 3;
                for k in 0..3 {
                    sum[k] += f64::from(g.bg[o + k]);
                }
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| sum.map(|v| (v / n) as f32))
}

/// How far apart two soils are in hue: the summed difference of their colour shares (grass and brown soil differ by about
/// 0.2, two grass maps by under 0.05).
fn soil_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let share = |c: [f32; 3]| {
        let t = (c[0] + c[1] + c[2]).max(1.0);
        [c[0] / t, c[1] / t, c[2] / t]
    };
    let (a, b) = (share(a), share(b));
    (0..3).map(|k| (a[k] - b[k]).abs()).sum()
}

/// The largest [`soil_distance`] at which a piece's ground counts as the map's kind of terrain: a forest map takes only
/// pieces from forest maps, an orange map only from orange maps (docs/generated-maps.md).
pub const SAME_TERRAIN: f32 = 0.10;

/// The open ground within two cells of a piece's own cells (`cells`, `w` x `h`, its box at `x`, `y`) in its mission `g`.
fn soil_around(g: &Ground, x: usize, y: usize, w: usize, h: usize, cells: &[bool]) -> Option<[f32; 3]> {
    let (gw, gh) = (g.map.width as usize, g.map.height as usize);
    let near = |cx: usize, cy: usize| {
        (cy.saturating_sub(2)..(cy + 3).min(gh))
            .any(|yy| (cx.saturating_sub(2)..(cx + 3).min(gw)).any(|xx| xx >= x && yy >= y && xx < x + w && yy < y + h && cells[(yy - y) * w + xx - x]))
    };
    let x0 = x.saturating_sub(2);
    let y0 = y.saturating_sub(2);
    let around: Vec<(usize, usize)> =
        (y0..(y + h + 2).min(gh)).flat_map(|cy| (x0..(x + w + 2).min(gw)).map(move |cx| (cx, cy))).filter(|&(cx, cy)| near(cx, cy)).collect();
    soil(g, around.into_iter())
}

/// One piece of [`catalogue`]: seed cell, box (x, y, w, h), edges (top, bottom, left, right), size in cells, and the mean
/// colour of the open ground around it.
pub type Listed = ((usize, usize), (usize, usize, usize, usize), [bool; 4], usize, Option<[f32; 3]>);

/// What a plan can place from mission `t` (docs/generated-maps.md): per piece a seed cell (its first cell in reading order),
/// its box (x, y, w, h), the edges it runs into (top, bottom, left, right) and its size in cells.
pub fn catalogue(t: Terrain, g: &Ground) -> Vec<Listed> {
    pieces(t, g)
        .into_iter()
        .map(|p| {
            let first = p.cells.iter().position(|&c| c).unwrap_or(0);
            let around = soil_around(g, p.x, p.y, p.w, p.h, &p.cells);
            ((p.x + first % p.w, p.y + first / p.w), (p.x, p.y, p.w, p.h), p.touches, p.cells.iter().filter(|&&c| c).count(), around)
        })
        .collect()
}

/// Walking distance (4-connected, cells) from `from` over `open`, `usize::MAX` where unreachable.
fn walk(open: &[bool], w: usize, h: usize, from: usize) -> Vec<usize> {
    let mut d = vec![usize::MAX; open.len()];
    let mut q = std::collections::VecDeque::from([from]);
    d[from] = 0;
    while let Some(i) = q.pop_front() {
        for j in neighbours(i, w, h) {
            if open[j] && d[j] == usize::MAX {
                d[j] = d[i] + 1;
                q.push_back(j);
            }
        }
    }
    d
}

/// The number of cell-disjoint paths over `open` from cells of `a` to cells of `b` (the narrowest cross-section), at most 64.
fn front(open: &[bool], a: &[bool], b: &[bool], w: usize, h: usize) -> (usize, Vec<(usize, usize)>) {
    // node i: in = 2i, out = 2i + 1; source 2n, sink 2n + 1; residual capacities in a map
    let n = w * h;
    let (src, sink) = (2 * n, 2 * n + 1);
    let mut cap: std::collections::HashMap<(usize, usize), i32> = std::collections::HashMap::new();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); 2 * n + 2];
    let mut edge = |u: usize, v: usize, c: i32, cap: &mut std::collections::HashMap<(usize, usize), i32>| {
        if !cap.contains_key(&(u, v)) && !cap.contains_key(&(v, u)) {
            adj[u].push(v);
            adj[v].push(u);
        }
        *cap.entry((u, v)).or_insert(0) += c;
        cap.entry((v, u)).or_insert(0);
    };
    for i in (0..n).filter(|&i| open[i]) {
        edge(2 * i, 2 * i + 1, 1, &mut cap);
        for j in neighbours(i, w, h).filter(|&j| open[j]) {
            edge(2 * i + 1, 2 * j, 1, &mut cap);
        }
        if a[i] {
            edge(src, 2 * i, 1000, &mut cap);
        }
        if b[i] {
            edge(2 * i + 1, sink, 1000, &mut cap);
        }
    }
    let mut flow = 0;
    while flow < 64 {
        let mut prev = vec![usize::MAX; 2 * n + 2];
        prev[src] = src;
        let mut q = std::collections::VecDeque::from([src]);
        while let Some(u) = q.pop_front() {
            if u == sink {
                break;
            }
            for &v in &adj[u] {
                if prev[v] == usize::MAX && cap[&(u, v)] > 0 {
                    prev[v] = u;
                    q.push_back(v);
                }
            }
        }
        if prev[sink] == usize::MAX {
            break;
        }
        let mut v = sink;
        while v != src {
            let u = prev[v];
            *cap.get_mut(&(u, v)).unwrap() -= 1;
            *cap.get_mut(&(v, u)).unwrap() += 1;
            v = u;
        }
        flow += 1;
    }
    // the narrowest cross-section: cells whose way in is still reachable from the start but whose way out is not
    let mut seen = vec![false; 2 * n + 2];
    seen[src] = true;
    let mut q = std::collections::VecDeque::from([src]);
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            if !seen[v] && cap[&(u, v)] > 0 {
                seen[v] = true;
                q.push_back(v);
            }
        }
    }
    let cut = (0..n).filter(|&i| open[i] && seen[2 * i] && !seen[2 * i + 1]).map(|i| (i % w, i / w)).collect();
    (flow, cut)
}

/// Builds the layout of `plan` on `base`'s ground from `sources` (`(terrain, ground)`: every mission of [`Plan::missions`]):
/// checks that edge pieces keep their edge, that no two pieces' cells overlap and that the start areas stay open.
pub fn generate(base: &Ground, sources: &[(Terrain, &Ground)], plan: &Plan) -> Result<Generated, String> {
    let mut rng = Rng(plan.seed);
    let (w, h) = (base.map.width as usize, base.map.height as usize);
    let mut blocked: Vec<bool> = (0..w * h).map(|i| i % w == 0 || i / w == 0 || i % w + 1 == w || i / w + 1 == h).collect();
    let mut owner: Vec<Option<usize>> = vec![None; w * h];
    let mut features: Vec<Feature> = Vec::new();
    let ground_of = |t: Terrain| sources.iter().find(|s| s.0 == t).map(|s| s.1).ok_or(format!("{t:?} is not loaded"));
    // a placement's cells: all checked before any is taken, so a refused placement leaves nothing behind
    let mut claim = |cells: &mut dyn Iterator<Item = (usize, usize, bool)>, what: &str, k: usize, blocked: &mut Vec<bool>| -> Result<(), String> {
        let cells: Vec<(usize, usize, bool)> = cells.collect();
        for &(x, y, b) in &cells {
            if x >= w || y >= h {
                return Err(format!("{what} leaves the map at cell {x},{y}"));
            }
            if let Some(o) = owner[y * w + x].filter(|_| b) {
                return Err(format!("{what} overlaps placement {} at cell {x},{y}", o + 1));
            }
        }
        for (x, y, b) in cells.into_iter().filter(|c| c.2) {
            owner[y * w + x] = Some(k);
            blocked[y * w + x] = b;
        }
        Ok(())
    };
    // only the map's own kind of terrain: a piece's surrounding ground must match the map's (docs/generated-maps.md)
    let map_soil = soil(base, (0..w * h).map(|i| (i % w, i / w))).ok_or("the base mission has no open ground")?;
    let same = |what: &str, around: Option<[f32; 3]>| -> Result<(), String> {
        match around {
            Some(c) if soil_distance(c, map_soil) > SAME_TERRAIN => Err(format!(
                "{what} comes from another kind of terrain: its ground {:.0},{:.0},{:.0}, the map's {:.0},{:.0},{:.0}",
                c[0], c[1], c[2], map_soil[0], map_soil[1], map_soil[2]
            )),
            _ => Ok(()),
        }
    };
    let mut cache: Vec<(Terrain, Vec<Piece>)> = Vec::new();
    // every problem of the plan is reported at once (the refused placements are left out)
    let mut errors: Vec<String> = Vec::new();
    for (k, pl) in plan.pieces.iter().enumerate() {
        let mut one = || -> Result<(), String> {
            let what = format!("piece {} ({:?} {},{})", k + 1, pl.source, pl.seed.0, pl.seed.1);
            if !cache.iter().any(|c| c.0 == pl.source) {
                cache.push((pl.source, pieces(pl.source, ground_of(pl.source)?)));
            }
            let list = &cache.iter().find(|c| c.0 == pl.source).unwrap().1;
            let p = list
                .iter()
                .find(|p| {
                    pl.seed.0 >= p.x && pl.seed.1 >= p.y && pl.seed.0 < p.x + p.w && pl.seed.1 < p.y + p.h && p.cells[(pl.seed.1 - p.y) * p.w + pl.seed.0 - p.x]
                })
                .ok_or(format!("{what}: no piece holds that cell"))?;
            let (tx, ty) = pl.at;
            for (edge, at, want) in [(p.touches[0], ty, 0), (p.touches[2], tx, 0), (p.touches[1], ty + p.h, h), (p.touches[3], tx + p.w, w)] {
                if edge && at != want {
                    return Err(format!(
                        "{what} (box {},{} {}x{}) runs into its map's {} edge: it must keep that edge here too",
                        p.x,
                        p.y,
                        p.w,
                        p.h,
                        ["top", "bottom", "left", "right"].iter().zip(p.touches).filter(|t| t.1).map(|t| *t.0).collect::<Vec<_>>().join(" and ")
                    ));
                }
            }
            let g = ground_of(pl.source)?;
            same(&what, soil_around(g, p.x, p.y, p.w, p.h, &p.cells))?;
            let gw = g.map.width as usize;
            let cells: Vec<bool> = if pl.whole { vec![true; p.w * p.h] } else { p.cells.clone() };
            let mut it = (0..p.w * p.h).filter(|&i| cells[i]).map(|i| (tx + i % p.w, ty + i / p.w, g.map.cells[(p.y + i / p.w) * gw + p.x + i % p.w].blocked));
            claim(&mut it, &what, k, &mut blocked)?;
            features.push(Feature { source: pl.source, x: p.x as u8, y: p.y as u8, w: p.w as u8, h: p.h as u8, dx: tx as u8, dy: ty as u8, cells });
            Ok(())
        };
        if let Err(e) = one() {
            errors.push(e);
        }
    }
    for (n, &(k, (tx, ty))) in plan.structures.iter().enumerate() {
        let st = STRUCTURES[k];
        let t = Terrain { warmonger: false, mission: st.mission };
        let g = ground_of(t)?;
        let (sx, sy, sw, sh) = st.rect;
        if let Err(e) = same(&format!("the {}", st.name), soil_around(g, sx, sy, sw, sh, &vec![true; sw * sh])) {
            errors.push(e);
            continue;
        }
        let gw = g.map.width as usize;
        let mut it = (0..sw * sh).map(|i| (tx + i % sw, ty + i / sw, g.map.cells[(sy + i / sw) * gw + sx + i % sw].blocked));
        if let Err(e) = claim(&mut it, &format!("the {}", st.name), plan.pieces.len() + n, &mut blocked) {
            errors.push(e);
            continue;
        }
        features.push(Feature { source: t, x: sx as u8, y: sy as u8, w: sw as u8, h: sh as u8, dx: tx as u8, dy: ty as u8, cells: vec![true; sw * sh] });
    }
    for &(x, y, sw, sh) in &plan.starts {
        if let Some(i) = (y..y + sh).flat_map(|cy| (x..x + sw).map(move |cx| cy * w + cx)).find(|&i| i >= w * h || blocked[i]) {
            errors.push(format!("start area {x},{y} {sw}x{sh} is blocked at cell {},{}", i % w, i / w));
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if let Some((share, from)) = &plan.fill {
        fill(plan, (*share, plan.gap), from, sources, map_soil, &mut blocked, &mut features, &mut rng);
    }
    // the ways as promised (no wall piece may touch them) and a cell wider (wall pieces may reach into that fringe)
    let ways = way_mask(plan, w, h, 0.0);
    let fringe = way_mask(plan, w, h, 1.0);
    let mut leaks = 0;
    if let Some((share, from)) = &plan.wall {
        if let Some(i) = (0..w * h).find(|&i| ways[i] && owner[i].is_some() && !plan.pieces.is_empty()) {
            return Err(format!("a placed piece blocks the ways at cell {},{}", i % w, i / w));
        }
        leaks = wall(&ways, &fringe, *share, from, sources, map_soil, &owner, &mut blocked, &mut features, &mut rng, w, h);
        if std::env::var_os("OPENSOW_WAYS_DEBUG").is_some() {
            let row = |y: usize| {
                (0..w)
                    .map(|x| {
                        if ways[y * w + x] {
                            '.'
                        } else if blocked[y * w + x] {
                            '#'
                        } else {
                            'o'
                        }
                    })
                    .collect::<String>()
            };
            eprintln!(
                "ways {:.0} % of the map\n{}",
                100.0 * ways.iter().filter(|&&b| b).count() as f32 / (w * h) as f32,
                (0..h).map(row).collect::<Vec<_>>().join("\n")
            );
        }
    }
    let open: Vec<bool> = blocked.iter().map(|&b| !b).collect();
    let (mut fr, mut cut, mut path) = (0, Vec::new(), (0, 0));
    if let [a, b, ..] = plan.starts[..] {
        let mask = |(x, y, sw, sh): (usize, usize, usize, usize)| {
            (0..w * h).map(|i| (x..x + sw).contains(&(i % w)) && (y..y + sh).contains(&(i / w))).collect::<Vec<_>>()
        };
        (fr, cut) = front(&open, &mask(a), &mask(b), w, h);
        let (ca, cb) = ((a.1 + a.3 / 2) * w + a.0 + a.2 / 2, (b.1 + b.3 / 2) * w + b.0 + b.2 / 2);
        path = (walk(&open, w, h, ca)[cb], (ca % w).abs_diff(cb % w) + (ca / w).abs_diff(cb / w));
    }
    let patches = quilt(base, &mut rng);
    let trails = lay_trails(base, plan, &blocked, &mut rng)?;
    // the widest open space outside the start areas (how open the map is; the shipped maps stay around 6 or under)
    // (the start areas themselves are the bases' own open ground)
    let in_start = |i: usize| plan.starts.iter().any(|&(x, y, sw, sh)| (x..x + sw).contains(&(i % w)) && (y..y + sh).contains(&(i / w)));
    let to_wall = chessboard(&blocked, w, h);
    let widest = (0..w * h).filter(|&i| !blocked[i] && !in_start(i)).map(|i| (to_wall[i], (i % w, i / w))).max().unwrap_or((0, (0, 0)));
    // every open space of 6 or more, greatest first, one per 12-cell neighbourhood
    let mut spaces: Vec<(usize, (usize, usize))> =
        (0..w * h).filter(|&i| !blocked[i] && !in_start(i) && to_wall[i] >= 6).map(|i| (to_wall[i], (i % w, i / w))).collect();
    spaces.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut open_spaces: Vec<(usize, (usize, usize))> = Vec::new();
    for sp in spaces {
        if open_spaces.iter().all(|o| o.1 .0.abs_diff(sp.1 .0).max(o.1 .1.abs_diff(sp.1 .1)) >= 12) {
            open_spaces.push(sp);
        }
    }
    Ok(Generated {
        layout: Layout { patches, features, trails },
        blocked: blocked.iter().filter(|&&b| b).count() as f32 / (w * h) as f32,
        front: fr,
        cut,
        path,
        widest,
        open_spaces,
        building_spots: spots(&blocked, w, h, &in_start),
        leaks,
        entrances: plan.rooms.iter().chain(&plan.starts).map(|&r| entrances(&blocked, w, h, r)).collect(),
        sealed: sealed_rooms(plan, &blocked, w, h),
        seed: plan.seed,
    })
}

/// Greedily counts separate open 6 x 6 squares, in reading order, none touching a start area.
fn spots(blocked: &[bool], w: usize, h: usize, in_start: &dyn Fn(usize) -> bool) -> usize {
    let mut taken = vec![false; w * h];
    let mut n = 0;
    for y in 0..h.saturating_sub(6) {
        for x in 0..w.saturating_sub(6) {
            let cells = || (y..y + 6).flat_map(move |cy| (x..x + 6).map(move |cx| cy * w + cx));
            if cells().all(|i| !blocked[i] && !taken[i] && !in_start(i)) {
                for i in cells() {
                    taken[i] = true;
                }
                n += 1;
            }
        }
    }
    n
}

/// Chessboard distance (cells) to the nearest `true` cell.
fn chessboard(mask: &[bool], w: usize, h: usize) -> Vec<usize> {
    let big = usize::MAX / 2;
    let mut d: Vec<usize> = mask.iter().map(|&m| if m { 0 } else { big }).collect();
    for pass in 0..2 {
        let order: Vec<usize> = if pass == 0 { (0..w * h).collect() } else { (0..w * h).rev().collect() };
        for i in order {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for (dx, dy) in [(-1i64, -1i64), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                    d[i] = d[i].min(d[ny as usize * w + nx as usize] + 1);
                }
            }
        }
    }
    d
}

/// The trail stamps of `plan`: each trail routed between its cells over open ground, keeping to the middle of the passes,
/// smoothed, then dabbed with texture from `base`'s own trails in broken, wandering, uneven stretches (worn, not drawn).
fn lay_trails(base: &Ground, plan: &Plan, blocked: &[bool], rng: &mut Rng) -> Result<Vec<TrailStamp>, String> {
    let (w, h) = (base.map.width as usize, base.map.height as usize);
    let (mask, bw, bh) = trail_blocks(base);
    // where texture can come from: blocks whose 7 x 7 neighbourhood is all trail (a 112-pixel area of pure trail), three cells
    // or more from anything blocked (the reddish soil at a cliff's foot is not a trail)
    let (sw, sh) = (base.map.width as usize, base.map.height as usize);
    let src_blocked: Vec<bool> = base.map.cells.iter().map(|c| c.blocked).collect();
    let src_wall = chessboard(&src_blocked, sw, sh);
    let clear = |bx: usize, by: usize| src_wall[(by * TRAIL_BLOCK / CELL) * sw + bx * TRAIL_BLOCK / CELL] >= 3;
    let core: Vec<(u16, u16)> = (3..bh.saturating_sub(3))
        .flat_map(|by| (3..bw.saturating_sub(3)).map(move |bx| (bx, by)))
        .filter(|&(bx, by)| clear(bx, by) && (by - 3..=by + 3).all(|y| (bx - 3..=bx + 3).all(|x| mask[y * bw + x])))
        .map(|(bx, by)| ((bx * TRAIL_BLOCK + TRAIL_BLOCK / 2) as u16, (by * TRAIL_BLOCK + TRAIL_BLOCK / 2) as u16))
        .collect();
    if core.is_empty() {
        // brown and orange grounds have no worn tracks: their maps get none (the flow's trail is left out too)
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let dab = |x: f32, y: f32, r: f32, rng: &mut Rng, out: &mut Vec<TrailStamp>| {
        let from = core[rng.range(0, core.len())];
        out.push(TrailStamp { at: (x.max(0.0) as u16, y.max(0.0) as u16), from, radius: r.clamp(8.0, 66.0) as u8 });
    };
    let open: Vec<bool> = blocked.iter().map(|&b| !b).collect();
    let to_wall = chessboard(blocked, w, h);
    for (n, trail) in plan.trails.iter().enumerate() {
        let mut cells: Vec<usize> = Vec::new();
        // each waypoint on the nearest open cell within 8 cells
        let snap = |(x, y): (usize, usize)| -> Result<usize, String> {
            (0..=8usize)
                .flat_map(|r| {
                    let (x0, y0) = (x.saturating_sub(r), y.saturating_sub(r));
                    (y0..=(y + r).min(h - 1)).flat_map(move |yy| (x0..=(x + r).min(w - 1)).map(move |xx| (xx, yy)))
                })
                .map(|(xx, yy)| yy * w + xx)
                .find(|&i| open[i])
                .ok_or(format!("trail {}: no open cell near {x},{y}", n + 1))
        };
        for pair in trail.windows(2) {
            let (a, b) = (snap(pair[0])?, snap(pair[1])?);
            // cheapest way: each step costs more near a wall, so the trail keeps to the middle
            let mut cost = vec![u32::MAX; w * h];
            let mut prev = vec![usize::MAX; w * h];
            let mut heap = std::collections::BinaryHeap::from([std::cmp::Reverse((0u32, a))]);
            cost[a] = 0;
            while let Some(std::cmp::Reverse((c, i))) = heap.pop() {
                if i == b {
                    break;
                }
                if c > cost[i] {
                    continue;
                }
                for j in neighbours(i, w, h).filter(|&j| open[j]) {
                    let step = 10 + 40 / (1 + to_wall[j].min(8) as u32);
                    if c + step < cost[j] {
                        cost[j] = c + step;
                        prev[j] = i;
                        heap.push(std::cmp::Reverse((c + step, j)));
                    }
                }
            }
            if cost[b] == u32::MAX {
                return Err(format!("trail {}: no way from {:?} to {:?}", n + 1, pair[0], pair[1]));
            }
            let mut leg = vec![b];
            while *leg.last().unwrap() != a {
                leg.push(prev[*leg.last().unwrap()]);
            }
            leg.reverse();
            if !cells.is_empty() {
                leg.remove(0);
            }
            cells.extend(leg);
        }
        // smoothed: each point the mean of its 9 neighbours along the trail, then a dab every 20 pixels
        let pts: Vec<(f32, f32)> = cells.iter().map(|&i| ((i % w) as f32 * 32.0 + 16.0, (i / w) as f32 * 32.0 + 16.0)).collect();
        let smooth: Vec<(f32, f32)> = (0..pts.len())
            .map(|k| {
                let (lo, hi) = (k.saturating_sub(4), (k + 5).min(pts.len()));
                let n = (hi - lo) as f32;
                (pts[lo..hi].iter().map(|p| p.0).sum::<f32>() / n, pts[lo..hi].iter().map(|p| p.1).sum::<f32>() / n)
            })
            .collect();
        // worn, not drawn: the trail comes and goes (gaps where 300-pixel noise is low), wanders up to a cell and a half to
        // the side, changes width, and thins out over its first and last fifth
        let total: f32 = smooth.windows(2).map(|s| ((s[1].0 - s[0].0).powi(2) + (s[1].1 - s[0].1).powi(2)).sqrt()).sum();
        let seed = 0x7472_6c00 + n as u32;
        let (mut next, mut along) = (0f32, 0f32);
        for seg in smooth.windows(2) {
            let (dx, dy) = (seg[1].0 - seg[0].0, seg[1].1 - seg[0].1);
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            while next < len {
                let d = along + next;
                let wear = noise(d as usize, 0, 300.0, seed);
                let end = (d / (total * 0.2)).min((total - d) / (total * 0.2)).clamp(0.0, 1.0);
                if wear * end > 0.32 {
                    let side = (noise(d as usize, 7, 260.0, seed ^ 0x55) - 0.5) * 112.0;
                    let (x, y) = (seg[0].0 + dx * next / len - dy / len * side, seg[0].1 + dy * next / len + dx / len * side);
                    let r = (26.0 + 40.0 * noise(d as usize, 3, 140.0, seed ^ 0xaa)) * (0.5 + 0.5 * end);
                    dab(x, y, r, rng, &mut out);
                }
                next += 22.0;
            }
            next -= len;
            along += len;
        }
    }
    Ok(out)
}

/// Fills the open fields (docs/generated-maps.md): until `share` of the cells are blocked, the most open spot (the open cell
/// farthest from anything blocked, outside the start areas) gets the piece of `from` that covers most cells there, of the
/// map's own kind of terrain, `gap` or more open cells from everything else (every passage stays that wide), and only
/// if it seals no open cell off from the first start area. A spot no piece fits is left.
#[allow(clippy::too_many_arguments)]
fn fill(
    plan: &Plan,
    (share, gap): (f32, usize),
    from: &[Terrain],
    sources: &[(Terrain, &Ground)],
    map_soil: [f32; 3],
    blocked: &mut [bool],
    features: &mut Vec<Feature>,
    rng: &mut Rng,
) {
    let (w, h) = plan_size(blocked, sources, plan);
    let in_start = |x: usize, y: usize, m: usize| plan.starts.iter().any(|&(sx, sy, sw, sh)| x + m >= sx && x < sx + sw + m && y + m >= sy && y < sy + sh + m);
    // the candidates: pieces of 4 to 600 cells on the map's kind of ground (one running into an edge goes to the same edge)
    let mut cands: Vec<(Terrain, Piece, usize)> = Vec::new();
    for &t in from {
        let Some(g) = sources.iter().find(|s| s.0 == t).map(|s| s.1) else { continue };
        for p in pieces(t, g) {
            let n = p.cells.iter().filter(|&&c| c).count();
            if !(4..=600).contains(&n) || p.w + 4 >= w || p.h + 4 >= h || (p.touches[0] && p.touches[1]) || (p.touches[2] && p.touches[3]) {
                continue;
            }
            if soil_around(g, p.x, p.y, p.w, p.h, &p.cells).is_some_and(|c| soil_distance(c, map_soil) > SAME_TERRAIN) {
                continue;
            }
            cands.push((t, p, n));
        }
    }
    if cands.is_empty() {
        return;
    }
    let mut uses = vec![0usize; cands.len()];
    let mut tried = vec![false; w * h];
    let start_cell = plan.starts.first().map(|&(x, y, sw, sh)| (y + sh / 2) * w + x + sw / 2);
    let total = (w * h) as f32;
    while (blocked.iter().filter(|&&b| b).count() as f32) < share * total {
        let to_wall = chessboard(blocked, w, h);
        let Some(spot) =
            (0..w * h).filter(|&i| !blocked[i] && !tried[i] && !in_start(i % w, i / w, 2) && to_wall[i] > gap.max(1)).max_by_key(|&i| (to_wall[i], i))
        else {
            break;
        };
        let (sx, sy) = (spot % w, spot / w);
        let mut best: Option<(usize, usize, usize, usize)> = None;
        for _ in 0..1500 {
            let k = rng.range(0, cands.len());
            if uses[k] >= 3 {
                continue;
            }
            let p = &cands[k].1;
            // the piece's box centred near the spot
            let (mut tx, mut ty) = ((sx + rng.range(0, 3)).saturating_sub(p.w / 2 + 1), (sy + rng.range(0, 3)).saturating_sub(p.h / 2 + 1));
            if p.touches[0] {
                ty = 0;
            } else if p.touches[1] {
                ty = h - p.h;
            }
            if p.touches[2] {
                tx = 0;
            } else if p.touches[3] {
                tx = w - p.w;
            }
            let inner = |t: usize, len: usize, size: usize, lo: bool, hi: bool| (lo || t >= 1) && (hi || t + len < size) && t + len <= size;
            if !inner(tx, p.w, w, p.touches[2], p.touches[3]) || !inner(ty, p.h, h, p.touches[0], p.touches[1]) {
                continue;
            }
            let cells: Vec<usize> = (0..p.w * p.h).filter(|&i| p.cells[i]).map(|i| (ty + i / p.w) * w + tx + i % p.w).collect();
            let ring = |i: usize| i.is_multiple_of(w) || i / w == 0 || i % w + 1 == w || i / w + 1 == h;
            let on_wall =
                |i: usize| !ring(i) && blocked[i] || to_wall[i] <= gap && !(ring(i) || to_wall[i] >= 1 && p.touches.iter().any(|&e| e) && ring_near(i, w, h));
            if cells.iter().any(|&i| on_wall(i) || in_start(i % w, i / w, 1))
                || !cells.contains(&spot) && cells.iter().all(|&i| (i % w).abs_diff(sx) + (i / w).abs_diff(sy) > 3)
            {
                continue;
            }
            if best.is_none_or(|b| cands[k].2 > b.3) {
                best = Some((k, tx, ty, cands[k].2));
            }
        }
        let Some((k, tx, ty, _)) = best else {
            tried[spot] = true;
            continue;
        };
        let (t, p, _) = &cands[k];
        let g = sources.iter().find(|s| s.0 == *t).map(|s| s.1).unwrap();
        let gw = g.map.width as usize;
        let mut trial = blocked.to_vec();
        for i in (0..p.w * p.h).filter(|&i| p.cells[i]) {
            trial[(ty + i / p.w) * w + tx + i % p.w] |= g.map.cells[(p.y + i / p.w) * gw + p.x + i % p.w].blocked;
        }
        // no new sealed pockets: no more open cells cut off from the first start area than before
        let cut_off = |b: &[bool]| {
            let open: Vec<bool> = b.iter().map(|&x| !x).collect();
            start_cell.map_or(0, |c| {
                let r = walk(&open, w, h, c);
                (0..w * h).filter(|&i| open[i] && r[i] == usize::MAX).count()
            })
        };
        // no new slits: open cells that no fully open `lane` x `lane` square covers (a passage narrower than a lane)
        let lane = plan.lane.max(2);
        let slits = |b: &[bool]| {
            let mut fat = vec![false; w * h];
            for y in 0..=h - lane {
                for x in 0..=w - lane {
                    if (y..y + lane).all(|yy| (x..x + lane).all(|xx| !b[yy * w + xx])) {
                        for yy in y..y + lane {
                            for xx in x..x + lane {
                                fat[yy * w + xx] = true;
                            }
                        }
                    }
                }
            }
            (0..w * h).filter(|&i| !b[i] && !fat[i]).count()
        };
        // and the front between the first two start areas stays at least three cells wide
        let front_of = |b: &[bool]| match plan.starts[..] {
            [a, c, ..] => {
                let mask = |(x, y, sw, sh): (usize, usize, usize, usize)| {
                    (0..w * h).map(|i| (x..x + sw).contains(&(i % w)) && (y..y + sh).contains(&(i / w))).collect::<Vec<_>>()
                };
                let open: Vec<bool> = b.iter().map(|&x| !x).collect();
                front(&open, &mask(a), &mask(c), w, h).0
            }
            _ => usize::MAX,
        };
        // (the fill may not narrow it below a lane and a cell; what the placed pieces made stays as it is)
        let narrow = |b: &[bool]| {
            let after = front_of(b);
            after < lane + 1 && after < front_of(blocked)
        };
        // and room for buildings stays
        let start_area = |i: usize| in_start(i % w, i / w, 0);
        let crowded = plan.spots > 0 && spots(&trial, w, h, &start_area) < plan.spots;
        let sealed = crowded || cut_off(&trial) > cut_off(blocked) || slits(&trial) > slits(blocked) + 2 || narrow(&trial);
        if sealed {
            tried[spot] = true;
            continue;
        }
        blocked.copy_from_slice(&trial);
        uses[k] += 1;
        features.push(Feature { source: *t, x: p.x as u8, y: p.y as u8, w: p.w as u8, h: p.h as u8, dx: tx as u8, dy: ty as u8, cells: p.cells.clone() });
    }
}

/// Whether cell `i` is within 2 cells of the map's outer ring.
fn ring_near(i: usize, w: usize, h: usize) -> bool {
    let (x, y) = (i % w, i / w);
    x <= 2 || y <= 2 || x + 3 >= w || y + 3 >= h
}

fn plan_size(blocked: &[bool], sources: &[(Terrain, &Ground)], plan: &Plan) -> (usize, usize) {
    let g = sources.iter().find(|s| s.0 == plan.base).map(|s| s.1).unwrap_or(sources[0].1);
    let w = g.map.width as usize;
    (w, blocked.len() / w)
}

/// Per room: blocks a disk across the middle of the branch that ends at it and checks that the room is then cut off from
/// the first start area.
fn sealed_rooms(plan: &Plan, blocked: &[bool], w: usize, h: usize) -> Vec<Option<bool>> {
    let Some(&(sx, sy, sw, sh)) = plan.starts.first() else { return vec![None; plan.rooms.len()] };
    let start = (sy + sh / 2) * w + sx + sw / 2;
    plan.rooms
        .iter()
        .map(|&(rx, ry, rw, rh)| {
            let near = |p: (usize, usize)| p.0 + 4 >= rx && p.1 + 4 >= ry && p.0 < rx + rw + 4 && p.1 < ry + rh + 4;
            let (width, pts) = plan.ways.iter().skip(1).find(|(_, pts)| near(*pts.last().unwrap()))?;
            // the middle of the branch, by length
            let len: f32 = pts.windows(2).map(|s| ((s[1].0 as f32 - s[0].0 as f32).powi(2) + (s[1].1 as f32 - s[0].1 as f32).powi(2)).sqrt()).sum();
            let mut left = len / 2.0;
            let mut mid = (pts[0].0 as f32, pts[0].1 as f32);
            for s in pts.windows(2) {
                let (dx, dy) = (s[1].0 as f32 - s[0].0 as f32, s[1].1 as f32 - s[0].1 as f32);
                let l = (dx * dx + dy * dy).sqrt();
                if left <= l {
                    mid = (s[0].0 as f32 + dx * left / l, s[0].1 as f32 + dy * left / l);
                    break;
                }
                left -= l;
            }
            let r = *width as f32 / 2.0 + 2.5;
            let open: Vec<bool> = (0..w * h).map(|i| !blocked[i] && ((i % w) as f32 - mid.0).powi(2) + ((i / w) as f32 - mid.1).powi(2) > r * r).collect();
            let reach = walk(&open, w, h, start);
            let centre = (ry + rh / 2) * w + rx + rw / 2;
            Some(reach[centre] == usize::MAX)
        })
        .collect()
}

/// The openings of a room `(x, y, w, h)`: components of open cells in the two-cell ring around it that reach more than eight
/// open cells outside the room.
fn entrances(blocked: &[bool], w: usize, h: usize, (rx, ry, rw, rh): (usize, usize, usize, usize)) -> (usize, usize) {
    let inside = |x: usize, y: usize| x >= rx && y >= ry && x < rx + rw && y < ry + rh;
    let ring: Vec<bool> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            !blocked[i] && !inside(x, y) && x + 2 >= rx && y + 2 >= ry && x < rx + rw + 2 && y < ry + rh + 2
        })
        .collect();
    let outside: Vec<bool> = (0..w * h).map(|i| !blocked[i] && !inside(i % w, i / w)).collect();
    let width = (0..w * h)
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            !blocked[i]
                && !inside(x, y)
                && x + 1 >= rx
                && y + 1 >= ry
                && x < rx + rw + 1
                && y < ry + rh + 1
                && !(x + 1 == rx || x == rx + rw) | !(y + 1 == ry || y == ry + rh)
        })
        .count();
    let n = components(&ring, w, h)
        .into_iter()
        .filter(|c| {
            // flood the open ground outside the room from this opening, a little way
            let mut seen = std::collections::HashSet::new();
            let mut stack = c.clone();
            while let Some(i) = stack.pop() {
                if seen.len() > c.len() + 8 {
                    return true;
                }
                if !seen.insert(i) {
                    continue;
                }
                stack.extend(neighbours(i, w, h).filter(|&j| outside[j] && !seen.contains(&j)));
            }
            false
        })
        .count();
    (n, width)
}

/// The cells a plan keeps open: its ways (each `width` cells across, its edge wandering a cell with noise), rooms and start
/// areas.
fn way_mask(plan: &Plan, w: usize, h: usize, extra: f32) -> Vec<bool> {
    let mut m = vec![false; w * h];
    for (k, (width, pts)) in plan.ways.iter().enumerate() {
        for seg in pts.windows(2) {
            let (ax, ay, bx, by) = (seg[0].0 as f32, seg[0].1 as f32, seg[1].0 as f32, seg[1].1 as f32);
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = (dx * dx + dy * dy).max(0.001);
            for i in 0..w * h {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                let t = (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0);
                let d = ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt();
                // the edge wanders outward by up to a cell and a half, never inward: the width is a minimum
                let wobble = noise(i % w * 32, i / w * 32, 160.0, 0x7761_7900 + k as u32) * 1.5;
                if d <= *width as f32 / 2.0 + extra + wobble {
                    m[i] = true;
                }
            }
        }
    }
    let e = extra as usize;
    for &(x, y, rw, rh) in plan.rooms.iter().chain(&plan.starts) {
        for cy in y.saturating_sub(e)..(y + rh + e).min(h) {
            for cx in x.saturating_sub(e)..(x + rw + e).min(w) {
                m[cy * w + cx] = true;
            }
        }
    }
    for i in 0..w * h {
        if i % w == 0 || i / w == 0 || i % w + 1 == w || i / w + 1 == h {
            m[i] = false;
        }
    }
    m
}

/// Walls the ground outside `ways` (docs/generated-maps.md "Flows"): until `share` of it is blocked, the uncovered cell
/// deepest in the wall gets the piece of `from` (the map's own kind of terrain) that blocks the most uncovered cells there,
/// with none of its cells on the ways or on a placed piece. Wall pieces may overlap each other, so the mass closes. Returns
/// the open cells left outside the ways.
#[allow(clippy::too_many_arguments)]
fn wall(
    ways: &[bool],
    fringe: &[bool],
    share: f32,
    from: &[Terrain],
    sources: &[(Terrain, &Ground)],
    map_soil: [f32; 3],
    owner: &[Option<usize>],
    blocked: &mut [bool],
    features: &mut Vec<Feature>,
    rng: &mut Rng,
    w: usize,
    h: usize,
) -> usize {
    // per candidate: its mission, the piece, and its own blocked cells (relative to its box)
    #[allow(clippy::type_complexity)]
    let mut cands: Vec<(Terrain, Piece, Vec<(usize, usize)>)> = Vec::new();
    for &t in from {
        let Some(g) = sources.iter().find(|s| s.0 == t).map(|s| s.1) else { continue };
        let gw = g.map.width as usize;
        for p in pieces(t, g) {
            let own: Vec<(usize, usize)> =
                (0..p.w * p.h).filter(|&i| p.cells[i] && g.map.cells[(p.y + i / p.w) * gw + p.x + i % p.w].blocked).map(|i| (i % p.w, i / p.w)).collect();
            if own.len() < 2 || p.w >= w || p.h >= h || (p.touches[0] && p.touches[1]) || (p.touches[2] && p.touches[3]) {
                continue;
            }
            if soil_around(g, p.x, p.y, p.w, p.h, &p.cells).is_some_and(|c| soil_distance(c, map_soil) > SAME_TERRAIN) {
                continue;
            }
            cands.push((t, p, own));
        }
    }
    let target = |i: usize| !fringe[i];
    let total = (0..w * h).filter(|&i| target(i)).count() as f32;
    let mut uses = vec![0usize; cands.len()];
    // where each candidate went: the same rock twice within 14 cells reads as a pattern, not a landscape
    let mut placed_at: Vec<Vec<(usize, usize)>> = vec![Vec::new(); cands.len()];
    let mut tried = vec![false; w * h];
    if cands.is_empty() {
        return 0;
    }
    loop {
        let covered = (0..w * h).filter(|&i| target(i) && blocked[i]).count() as f32;
        if covered >= share * total {
            break;
        }
        // the uncovered wall cell farthest from the ways (the inside of a wall mass first)
        let to_way = chessboard(fringe, w, h);
        let Some(spot) = (0..w * h).filter(|&i| target(i) && !blocked[i] && !tried[i]).max_by_key(|&i| (to_way[i], i)) else { break };
        let (sx, sy) = (spot % w, spot / w);
        let mut best: Option<(usize, usize, usize, usize)> = None;
        // first with copies of a rock kept apart; only a spot nothing else fits takes a repeat
        for strict in [true, false] {
            if best.is_some() {
                break;
            }
            for k in 0..cands.len() {
                let (_, p, own) = &cands[k];
                // small rocks may repeat more than the big, recognisable masses
                if strict && uses[k] >= if own.len() < 40 { 10 } else { 3 } {
                    continue;
                }
                let pick = rng.range(0, 3);
                for (a, &(ox, oy)) in own.iter().enumerate() {
                    if own.len() > 24 && a % 3 != pick {
                        continue;
                    }
                    let (mut tx, mut ty) = (sx as i64 - ox as i64, sy as i64 - oy as i64);
                    if p.touches[0] {
                        ty = 0;
                    } else if p.touches[1] {
                        ty = (h - p.h) as i64;
                    }
                    if p.touches[2] {
                        tx = 0;
                    } else if p.touches[3] {
                        tx = (w - p.w) as i64;
                    }
                    if tx < 0 || ty < 0 || tx as usize + p.w > w || ty as usize + p.h > h {
                        continue;
                    }
                    let (tx, ty) = (tx as usize, ty as usize);
                    if strict && placed_at[k].iter().any(|&(px, py)| px.abs_diff(tx) + py.abs_diff(ty) < 14) {
                        continue;
                    }
                    let mut gain = 0;
                    let mut fine = true;
                    for &(cx, cy) in own {
                        let i = (ty + cy) * w + tx + cx;
                        if ways[i] || owner[i].is_some() {
                            fine = false;
                            break;
                        }
                        gain += usize::from(!blocked[i] && target(i));
                    }
                    if fine && gain > 0 && best.is_none_or(|b| gain > b.3) {
                        best = Some((k, tx, ty, gain));
                    }
                }
            }
        }
        let Some((k, tx, ty, _)) = best else {
            tried[spot] = true;
            continue;
        };
        let (t, p, own) = &cands[k];
        for &(cx, cy) in own {
            blocked[(ty + cy) * w + tx + cx] = true;
        }
        uses[k] += 1;
        placed_at[k].push((tx, ty));
        features.push(Feature { source: *t, x: p.x as u8, y: p.y as u8, w: p.w as u8, h: p.h as u8, dx: tx as u8, dy: ty as u8, cells: p.cells.clone() });
    }
    (0..w * h).filter(|&i| target(i) && !blocked[i]).count()
}

/// The open ground's patches: per patch position the best-joining of up to 150 windows over open cells of `base`.
fn quilt(base: &Ground, rng: &mut Rng) -> Vec<(u16, u16)> {
    let (w, h) = (base.map.width as usize, base.map.height as usize);
    let open: Vec<bool> = (0..w * h)
        .map(|i| {
            let c = base.map.cells[i];
            !c.blocked && !c.no_build && open_cell(base, i % w, i / w)
        })
        .collect();
    let span = PATCH.div_ceil(CELL) + 1;
    // no trail texture in the open ground: trails are laid where the plan routes them
    let (trail, bw, _) = trail_blocks(base);
    let per = CELL / TRAIL_BLOCK;
    let clean =
        |x: usize, y: usize| (y * per..(y + span) * per).all(|by| (x * per..(x + span) * per).all(|bx| !trail.get(by * bw + bx).copied().unwrap_or(false)));
    let all: Vec<(usize, usize)> = (0..h.saturating_sub(span))
        .flat_map(|y| (0..w.saturating_sub(span)).map(move |x| (x, y)))
        .filter(|&(x, y)| (y..y + span).all(|cy| (x..x + span).all(|cx| open[cy * w + cx])))
        .collect();
    let windows: Vec<(usize, usize)> = match all.iter().copied().filter(|&(x, y)| clean(x, y)).collect::<Vec<_>>() {
        v if v.len() >= 20 => v,
        _ => all,
    };
    let mut out = blank(base);
    let mut patches = Vec::new();
    for at in grid(base.w, base.h) {
        let mut best: Option<(f32, (usize, usize))> = None;
        for _ in 0..if windows.is_empty() { 0 } else { 150 } {
            let (cx, cy) = windows[rng.range(0, windows.len())];
            let s = (cx * CELL + rng.range(0, CELL), cy * CELL + rng.range(0, CELL));
            if !fits(base, s.0, s.1) {
                continue;
            }
            let (pw, ph) = (PATCH.min(out.w - at.0), PATCH.min(out.h - at.1));
            let mut e = 0f32;
            for r in (0..ph).step_by(3) {
                for c in (0..pw).step_by(3) {
                    if (at.0 > 0 && c < OVERLAP) || (at.1 > 0 && r < OVERLAP) {
                        let (o, q) = (((at.1 + r) * out.w + at.0 + c) * 3, ((s.1 + r) * base.w + s.0 + c) * 3);
                        e += (0..3).map(|k| (f32::from(out.bg[o + k]) - f32::from(base.bg[q + k])).powi(2)).sum::<f32>();
                    }
                }
            }
            if best.is_none_or(|(b, _)| e < b) {
                best = Some((e, s));
            }
        }
        let s = best.map(|b| b.1).unwrap_or((0, 0));
        join(&mut out, base, s, at);
        patches.push((s.0 as u16, s.1 as u16));
    }
    patches
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensow_formats::map::{Cell, Map};

    /// A `cells` x `cells` ground whose picture is a smooth gradient, nothing blocked.
    fn plain(cells: u32) -> Ground {
        let w = cells as usize * CELL;
        let map = Map {
            a: 0,
            b: 0,
            width: cells,
            height: cells,
            cells: (0..cells)
                .flat_map(|y| (0..cells).map(move |x| Cell { x: x as u8, y: y as u8, blocked: false, border: false, no_build: false, no_marks: false }))
                .collect(),
        };
        let bg = (0..w * w).flat_map(|i| [(i % w / 8) as u8, (i / w / 8) as u8, 90]).collect();
        Ground { bg, fg: vec![false; w * w], map, w, h: w }
    }

    #[test]
    fn the_cheapest_seam_follows_the_low_column() {
        let err: Vec<f32> = (0..4 * 3).map(|i| if i % 3 == 1 { 0.0 } else { 9.0 }).collect();
        assert_eq!(cut(&err, 4, 3), vec![1, 1, 1, 1]);
    }

    #[test]
    fn a_patch_copied_onto_itself_is_the_picture() {
        let base = plain(12);
        let patches: Vec<(u16, u16)> = grid(base.w, base.h).map(|(x, y)| (x.min(base.w - PATCH) as u16, y.min(base.h - PATCH) as u16)).collect();
        let g = ground(&base, &patches);
        // the first patch is copied whole
        assert_eq!(&g.bg[..PATCH * 3], &base.bg[..PATCH * 3]);
        assert!(g.map.cells[0].blocked && g.map.cells[0].border, "the outer ring is blocked");
        assert!(!g.map.cells[13].blocked && !g.map.cells[13].border);
    }

    #[test]
    fn a_feature_brings_its_cells_and_pixels() {
        let mut src = plain(12);
        for (cx, cy) in [(5, 5), (6, 5), (5, 6)] {
            src.map.cells[cy * 12 + cx].blocked = true;
            for py in cy * CELL..(cy + 1) * CELL {
                for px in cx * CELL..(cx + 1) * CELL {
                    src.bg[(py * src.w + px) * 3..][..3].copy_from_slice(&[250, 250, 250]);
                }
            }
        }
        let mut dst = plain(16);
        let f = Feature { source: Terrain { warmonger: false, mission: 1 }, x: 5, y: 5, w: 2, h: 2, dx: 9, dy: 3, cells: vec![true, true, true, false] };
        paste(&mut dst, &src, &f);
        let c = |x: usize, y: usize| dst.map.cells[y * 16 + x];
        assert!(c(9, 3).blocked && c(10, 3).blocked && c(9, 4).blocked && !c(10, 4).blocked);
        assert!(c(10, 4).no_build == src.map.cells[6 * 12 + 6].no_build);
        let p = ((3 * CELL + 16) * dst.w + 9 * CELL + 16) * 3;
        assert_eq!(&dst.bg[p..p + 3], &[250, 250, 250], "inside the feature the source's pixels");
        let far = ((15 * CELL) * dst.w + 15 * CELL) * 3;
        assert_eq!(dst.bg[far + 2], 90, "far away the map's own");
    }

    #[test]
    fn a_plan_places_its_pieces_and_keeps_the_starts_open() {
        let mut src = plain(40);
        // a brown picture (a blue one would read as water) with two rocks: 2x2 cells at 10,10 and 3x1 at 20,25
        src.bg = [120u8, 80, 50].repeat(src.w * src.h);
        for (x, y) in [(10usize, 10usize), (11, 10), (10, 11), (11, 11), (20, 25), (21, 25), (22, 25)] {
            src.map.cells[y * 40 + x].blocked = true;
        }
        let t = Terrain { warmonger: false, mission: 12 };
        let text = "name Test\nbase M12\nseed 3\npiece M12 10,10 at 5,5\npiece M12 21,25 at 30,8 ; a ridge\nstart 2,30 6x6\nstart 30,30 6x6\n";
        let plan = Plan::parse(text).unwrap();
        let a = generate(&src, &[(t, &src)], &plan).unwrap();
        assert_eq!(a.layout, generate(&src, &[(t, &src)], &plan).unwrap().layout, "fixed by the seed");
        assert_eq!(a.layout.features.len(), 2);
        let f = &a.layout.features[1];
        assert_eq!((f.x, f.y, f.w, f.h, f.dx, f.dy), (20, 25, 3, 1, 30, 8));
        assert!(a.front >= 6, "the open map between the two starts: {}", a.front);
        assert_eq!(a.path.1, 28);
        let overlap = Plan::parse("base M12\npiece M12 10,10 at 5,5\npiece M12 10,10 at 6,6\n").unwrap();
        assert!(generate(&src, &[(t, &src)], &overlap).unwrap_err().contains("overlaps"));
        let closed = Plan::parse("base M12\npiece M12 10,10 at 5,5\nstart 4,4 4x4\n").unwrap();
        assert!(generate(&src, &[(t, &src)], &closed).unwrap_err().contains("start area"));
        assert!(Plan::parse("base M12\npiece M12 10 at 5,5").is_err());
    }

    #[test]
    fn fill_packs_pieces_up_to_the_share_and_keeps_lanes_and_starts_open() {
        let mut src = plain(40);
        src.bg = [120u8, 80, 50].repeat(src.w * src.h);
        // a field of 2 x 2 rocks to fill with
        for (x, y) in [(6usize, 6usize), (12, 6), (18, 6), (24, 6), (30, 6), (6, 14), (12, 14), (18, 14), (24, 14), (30, 14)] {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                src.map.cells[(y + dy) * 40 + x + dx].blocked = true;
            }
        }
        let t = Terrain { warmonger: false, mission: 12 };
        let plan = Plan::parse("base M12\nseed 4\nstart 2,2 6x6\nstart 32,32 6x6\nfill 30% gap 0 spots 3 lane 3 M12\n").unwrap();
        assert_eq!((plan.spots, plan.lane), (3, 3));
        let g = generate(&src, &[(t, &src)], &plan).unwrap();
        assert!(g.layout.features.len() >= 10, "pieces were added: {}", g.layout.features.len());
        assert!(g.front >= 3, "the front stays three cells wide: {}", g.front);
        assert!(g.building_spots >= 3, "room for buildings stays: {}", g.building_spots);
        let mut built = ground(&src, &g.layout.patches);
        for f in &g.layout.features {
            paste(&mut built, &src, f);
        }
        for &(x, y, sw, sh) in &plan.starts {
            assert!((y..y + sh).all(|cy| (x..x + sw).all(|cx| !built.map.cells[cy * 40 + cx].blocked)), "start areas stay open");
        }
    }
}
