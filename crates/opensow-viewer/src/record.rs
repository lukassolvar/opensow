//! Draw-command recording for the GPU renderer.
//!
//! While a recording is active (`begin` .. `end`, per thread) the drawing primitives of [`crate::render`] (and the game's `ui`
//! / `gdi` helpers) do not touch the picture: each one appends a [`Cmd`] instead, with everything the pixel loop needs already
//! worked out on the CPU exactly as the software loop does it (the clipped screen rectangle and, per screen column and row, the
//! source column and row it samples). The GPU then replays the commands per pixel, in order, with the same integer arithmetic
//! (the exe's RGB565 blends included), so a recorded frame comes out identical to the software one; [`replay`] is the CPU
//! reference of that replay (tests compare it with the direct drawing).
//!
//! Sprite frames are copied once into the [`Atlas`] (kept for the whole run; the sets live in the art caches), the map picture
//! into the [`Ground`]. The screen mask of the exe (docs/effects.md) is bit 24 of a picture word on the GPU.
//!
//! A primitive that cannot be recorded (a full-screen backdrop, a direct pixel loop) calls [`taint`]: the frame is then drawn
//! on the CPU instead.

use crate::render::View;
use crate::scene::GroundRef;
use opensow_formats::effects::Blit;
use opensow_formats::sprite::{rgb565_to_rgb8, Frame};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Tile edge of the GPU replay (one workgroup per tile): every tile gets the list of the commands that touch it.
pub const TILE: usize = 16;
/// The screen-mask bit of a picture word on the GPU.
pub const MASK_BIT: u32 = 1 << 24;

/// Command codes (the `op` word); `p` holds the parameters noted.
pub mod op {
    /// `blend(dst, p0 = 0xRRGGBB, p1 = alpha)` over the rectangle (`render::rect`, `ui::fill`, `ui::dim`); `p2 = 1`: even
    /// screen rows only (the pause stripes).
    pub const FILL: u32 = 0;
    /// Every channel minus `p0`, clamped at 0 (`ui::dim_sub`).
    pub const DIM_SUB: u32 = 1;
    /// The map picture (source = the ground; a column or row of -1 is off the map); `p0 = 1`: only its foreground pixels.
    pub const GROUND: u32 = 2;
    /// Marks the screen mask where the map pixel is foreground.
    pub const STAMP_GROUND: u32 = 3;
    /// Plain copy of the sprite pixel (`render::blit`).
    pub const PLAIN: u32 = 4;
    /// `blend(dst, black, p0)`; `p1 = 1`: not on masked pixels.
    pub const SHADOW: u32 = 5;
    /// An exe blend (`p0` = [`super::blit_code`]); `p1 = 1`: not on masked pixels.
    pub const BLEND: u32 = 6;
    /// The lens: brightens the picture pixel `2 * p0` right and `3 * p0` down (as it was before this command); `p1` as above.
    pub const LENS: u32 = 7;
    /// Plain copy where the mask frame (`p0` = its atlas base, `p1` = width | height << 16) is set at the same map pixel
    /// (`p2`, `p3` = the column / row offset into it, signed).
    pub const MASKED: u32 = 8;
    /// Marks the screen mask under the sprite's set pixels.
    pub const STAMP: u32 = 9;
    /// The sprite's colour moved `p0`/255 towards white (`ui::sprite_lit`).
    pub const LIT: u32 = 10;
    /// `ui::sprite_glow`.
    pub const GLOW: u32 = 11;
    /// 8-bit additive (`ui::sprite_add`); `p0 = 1`: the source halved.
    pub const ADD8: u32 = 12;
    /// `blend(dst, black, p0)` under the sprite's set pixels (`ui::sprite_dim`).
    pub const DIM: u32 = 13;
    /// Colour `p0` under the glyph's set pixels (`ui::text`).
    pub const GLYPH: u32 = 14;
    /// Anti-aliased text (`gdi`): `blend(dst, p0, alpha)` with the alpha from a grid in the frame's own data (0 = nothing).
    pub const COVER: u32 = 15;
    /// A picture word `0xRRGGBB` (`p0 = 0`: from the atlas, 1: from the frame's own data; `p1 = 1`: every channel halved);
    /// a column or row of -1 shows nothing.
    pub const PICTURE: u32 = 16;
    /// Every channel plus `p0`'s, clamped at 255 (rectangle).
    pub const ADD_RGB: u32 = 17;
    /// The end banner's rows (`endscreen::banner_rows`): odd screen rows black, even ones tinted (`tint565`) (rectangle).
    pub const TINT_ROWS: u32 = 18;
}

/// The command codes that cover their rectangle without a source.
fn is_rect(op: u32) -> bool {
    matches!(op, op::FILL | op::DIM_SUB | op::ADD_RGB | op::TINT_ROWS)
}

/// The [`op::BLEND`] mode code of an exe blit.
pub fn blit_code(b: Blit) -> u32 {
    match b {
        Blit::Plain => 0,
        Blit::Additive => 1,
        Blit::AddHalf => 2,
        Blit::Average => 3,
        Blit::Smoke => 4,
        Blit::Darken => 5,
        Blit::Shade => 6,
        Blit::Lens => 7,
    }
}

fn blit_of(code: u32) -> Blit {
    [Blit::Plain, Blit::Additive, Blit::AddHalf, Blit::Average, Blit::Smoke, Blit::Darken, Blit::Shade, Blit::Lens][code as usize & 7]
}

/// One command, 16 words: the layout the shader reads. The rectangle `xs..xe` x `ys..ye` is clipped to the screen; screen pixel
/// `(x, y)` samples source column `maps[cols + x - xs]` and row `maps[rows + y - ys]`, i.e. source word `base + row * stride + col`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Cmd {
    /// The operation, one of the `op` constants.
    pub op: u32,
    /// Left edge of the rectangle (screen x).
    pub xs: i32,
    /// Top edge of the rectangle (screen y).
    pub ys: i32,
    /// Right edge of the rectangle (exclusive).
    pub xe: i32,
    /// Bottom edge of the rectangle (exclusive).
    pub ye: i32,
    /// Start of the command's column map in `maps`.
    pub cols: u32,
    /// Start of the command's row map in `maps`.
    pub rows: u32,
    /// First source word.
    pub base: u32,
    /// Source words per source row.
    pub stride: u32,
    /// Seven parameter words whose meaning depends on the operation.
    pub p: [u32; 7],
}

/// Every sprite frame drawn so far, as words `rgb565 | mask << 16`.
#[derive(Default)]
pub struct Atlas {
    /// The frames' words, one frame after the other.
    pub data: Vec<u32>,
    index: HashMap<FrameKey, u32>,
    /// Pictures (`0xRRGGBB` words) by their RGB8 source.
    pictures: HashMap<(usize, usize, u32), u32>,
    /// Changes when the atlas is emptied (it outgrew [`Atlas::limit`]): the GPU copy must then be made again from the start.
    pub generation: u64,
    /// Most words the GPU buffer can hold (0 = no limit).
    pub limit: usize,
}

/// A frame's identity: its address, its pixel buffer and a sample of its content (a short-lived frame whose memory is reused
/// by another one is not taken for it).
type FrameKey = (usize, usize, usize, u32, u32);

fn frame_key(f: &Frame) -> FrameKey {
    let n = f.pixels.len();
    let mut h = 0x811c_9dc5u32;
    for i in (0..n).step_by((n / 61).max(1)) {
        h = (h ^ (f.pixels[i] as u32 | (f.mask[i] as u32) << 16)).wrapping_mul(16_777_619);
    }
    (f as *const Frame as usize, f.pixels.as_ptr() as usize, n, f.width as u32 | (f.height as u32) << 16, h)
}

impl Atlas {
    /// The atlas word index of `f`'s first pixel, copying it in at first sight; `None` when it does not fit (the atlas is emptied).
    fn add(&mut self, f: &Frame) -> Option<u32> {
        let key = frame_key(f);
        if let Some(&b) = self.index.get(&key) {
            return Some(b);
        }
        if !self.room(f.pixels.len()) {
            return None;
        }
        let base = self.data.len() as u32;
        self.data.extend(f.pixels.iter().zip(&f.mask).map(|(&p, &m)| p as u32 | (m as u32) << 16));
        self.index.insert(key, base);
        Some(base)
    }

    /// `n` more words fit; otherwise the atlas is emptied (and `false`).
    fn room(&mut self, n: usize) -> bool {
        if self.limit > 0 && self.data.len() + n > self.limit {
            self.data.clear();
            self.index.clear();
            self.pictures.clear();
            self.generation += 1;
            return false;
        }
        true
    }

    /// An RGB8 picture as `0xRRGGBB` words, copied in at first sight.
    fn add_rgb8(&mut self, rgb: &[u8]) -> Option<u32> {
        let n = rgb.len() / 3;
        let mut h = 0x811c_9dc5u32;
        for i in (0..rgb.len()).step_by((rgb.len() / 97).max(1)) {
            h = (h ^ rgb[i] as u32).wrapping_mul(16_777_619);
        }
        let key = (rgb.as_ptr() as usize, rgb.len(), h);
        if let Some(&b) = self.pictures.get(&key) {
            return Some(b);
        }
        if !self.room(n) {
            return None;
        }
        let base = self.data.len() as u32;
        self.data.extend(rgb.chunks_exact(3).map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32));
        self.pictures.insert(key, base);
        Some(base)
    }
}

/// The map picture as words `0xRRGGBB | foreground << 24` (`bg_w` per row).
#[derive(Default)]
pub struct Ground {
    /// The picture's words, row by row.
    pub data: Vec<u32>,
    /// Words per row.
    pub w: usize,
    key: (usize, usize, usize),
    /// Changes with the map.
    pub generation: u64,
}

impl Ground {
    fn set(&mut self, g: &GroundRef) {
        let key = (g.bg.as_ptr() as usize, g.bg.len(), g.fg.as_ptr() as usize);
        if key == self.key && !self.data.is_empty() {
            return;
        }
        let n = g.w * g.h;
        self.data = (0..n)
            .map(|i| (g.bg[i * 3] as u32) << 16 | (g.bg[i * 3 + 1] as u32) << 8 | g.bg[i * 3 + 2] as u32 | (g.fg.get(i).copied().unwrap_or(false) as u32) << 24)
            .collect();
        self.w = g.w;
        self.key = key;
        self.generation += 1;
    }
}

/// One recorded frame.
#[derive(Default)]
pub struct Recording {
    /// Frame width in pixels.
    pub w: usize,
    /// Frame height in pixels.
    pub h: usize,
    /// The commands, in drawing order.
    pub cmds: Vec<Cmd>,
    /// The sample maps of every command (source column / row per screen column / row, -1 = nothing).
    pub maps: Vec<i32>,
    /// Per-frame source data ([`op::COVER`] grids); `base` of such a command indexes it.
    pub local: Vec<u32>,
    /// Something was drawn that cannot be replayed: draw this frame on the CPU.
    pub tainted: bool,
    /// The screen mask in use (its address): one per frame.
    mask: Option<usize>,
}

#[derive(Default)]
struct State {
    rec: Option<Recording>,
    atlas: Atlas,
    ground: Ground,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Starts recording a `w` x `h` frame on this thread.
pub fn begin(w: usize, h: usize) {
    STATE.with_borrow_mut(|s| s.rec = Some(Recording { w, h, ..Recording::default() }));
    ACTIVE.set(true);
}

/// Ends the recording (`None` when none was running).
pub fn end() -> Option<Recording> {
    ACTIVE.set(false);
    STATE.with_borrow_mut(|s| s.rec.take())
}

/// A recording is running: the primitives record instead of drawing.
#[inline]
pub fn active() -> bool {
    ACTIVE.get()
}

/// The frame cannot be replayed (see the module notes).
pub fn taint() {
    with_rec(|r, _, _| r.tainted = true);
}

/// Runs `f` with the recording paused: drawing into a CPU canvas (a menu's 640x480 page) while a frame is being recorded.
pub fn cpu<R>(f: impl FnOnce() -> R) -> R {
    let was = ACTIVE.replace(false);
    let r = f();
    ACTIVE.set(was);
    r
}

/// The GPU's buffer size for the atlas, in words.
pub fn set_atlas_limit(words: usize) {
    STATE.with_borrow_mut(|s| s.atlas.limit = words);
}

/// Read access to the atlas and the ground (for the upload).
pub fn with_sources<R>(f: impl FnOnce(&Atlas, &Ground) -> R) -> R {
    STATE.with_borrow(|s| f(&s.atlas, &s.ground))
}

fn with_rec(f: impl FnOnce(&mut Recording, &mut Atlas, &mut Ground)) {
    STATE.with_borrow_mut(|s| {
        let State { rec, atlas, ground } = s;
        if let Some(r) = rec.as_mut() {
            f(r, atlas, ground)
        }
    })
}

/// The screen mask `ptr` is used: a second mask in the same frame is not supported.
fn use_mask(r: &mut Recording, ptr: usize) {
    match r.mask {
        None => r.mask = Some(ptr),
        Some(p) if p != ptr => r.tainted = true,
        _ => {}
    }
}

/// Clips `x0..x1` x `y0..y1` to the screen; `None` when nothing is left.
fn clip(r: &Recording, x0: i32, y0: i32, x1: i32, y1: i32) -> Option<(i32, i32, i32, i32)> {
    let (xs, ys, xe, ye) = (x0.max(0), y0.max(0), x1.min(r.w as i32), y1.min(r.h as i32));
    (xs < xe && ys < ye).then_some((xs, ys, xe, ye))
}

fn push_maps(r: &mut Recording, xs: i32, xe: i32, ys: i32, ye: i32, col: impl Fn(i32) -> i32, row: impl Fn(i32) -> i32) -> (u32, u32) {
    let cols = r.maps.len() as u32;
    r.maps.extend((xs..xe).map(col));
    let rows = r.maps.len() as u32;
    r.maps.extend((ys..ye).map(row));
    (cols, rows)
}

/// [`op::FILL`] (also `DIM_SUB` with `op`).
pub fn fill(op: u32, x0: i32, y0: i32, x1: i32, y1: i32, p0: u32, p1: u32) {
    with_rec(|r, _, _| {
        let Some((xs, ys, xe, ye)) = clip(r, x0, y0, x1, y1) else { return };
        r.cmds.push(Cmd { op, xs, ys, xe, ye, p: [p0, p1, 0, 0, 0, 0, 0], ..Cmd::default() });
    });
}

/// The pause stripes: every even screen row of the rectangle black.
pub fn stripes(x0: i32, y0: i32, x1: i32, y1: i32) {
    with_rec(|r, _, _| {
        let Some((xs, ys, xe, ye)) = clip(r, x0, y0, x1, y1) else { return };
        r.cmds.push(Cmd { op: op::FILL, xs, ys, xe, ye, p: [0, 255, 1, 0, 0, 0, 0], ..Cmd::default() });
    });
}

/// A sprite command: screen rectangle `x0..x1` x `y0..y1` (unclipped), `col(dx)` / `row(dy)` the frame column / row a screen
/// column / row shows (called only inside the clipped rectangle).
pub fn sprite(
    op: u32,
    f: &Frame,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    col: impl Fn(i32) -> usize,
    row: impl Fn(i32) -> usize,
    p: [u32; 7],
    mask: Option<usize>,
) {
    with_rec(|r, atlas, _| {
        let Some((xs, ys, xe, ye)) = clip(r, x0, y0, x1, y1) else { return };
        if let Some(m) = mask {
            use_mask(r, m);
        }
        let Some(base) = atlas.add(f) else {
            r.tainted = true;
            return;
        };
        let (cols, rows) = push_maps(r, xs, xe, ys, ye, |x| col(x) as i32, |y| row(y) as i32);
        r.cmds.push(Cmd { op, xs, ys, xe, ye, cols, rows, base, stride: f.width as u32, p });
    });
}

/// [`op::MASKED`]: `f` drawn where the mask frame `m` is set (at the same map pixel).
pub fn sprite_masked(f: &Frame, m: &Frame, x0: i32, y0: i32, x1: i32, y1: i32, col: impl Fn(i32) -> usize, row: impl Fn(i32) -> usize) {
    let mb = STATE.with_borrow_mut(|s| s.atlas.add(m));
    let Some(mb) = mb else {
        taint();
        return;
    };
    let (dx, dy) = (m.hot_x as i32 - f.hot_x as i32, m.hot_y as i32 - f.hot_y as i32);
    sprite(op::MASKED, f, x0, y0, x1, y1, col, row, [mb, m.width as u32 | (m.height as u32) << 16, dx as u32, dy as u32, 0, 0, 0], None);
}

/// [`op::PICTURE`] from a static RGB8 picture `src_w` wide (kept in the atlas): screen rectangle `x0..x1` x `y0..y1`,
/// `col(x)` / `row(y)` the picture column / row (-1 = nothing there).
pub fn picture_rgb8(rgb: &[u8], src_w: usize, x0: i32, y0: i32, x1: i32, y1: i32, col: impl Fn(i32) -> i32, row: impl Fn(i32) -> i32, halve: bool) {
    with_rec(|r, atlas, _| {
        let Some((xs, ys, xe, ye)) = clip(r, x0, y0, x1, y1) else { return };
        let Some(base) = atlas.add_rgb8(rgb) else {
            r.tainted = true;
            return;
        };
        let (cols, rows) = push_maps(r, xs, xe, ys, ye, col, row);
        r.cmds.push(Cmd { op: op::PICTURE, xs, ys, xe, ye, cols, rows, base, stride: src_w as u32, p: [0, halve as u32, 0, 0, 0, 0, 0] });
    });
}

/// [`op::PICTURE`] from this frame's own `0xRRGGBB` words (a menu page, a movie frame), uploaded with the frame.
pub fn picture_words(words: &[u32], src_w: usize, x0: i32, y0: i32, x1: i32, y1: i32, col: impl Fn(i32) -> i32, row: impl Fn(i32) -> i32) {
    with_rec(|r, _, _| {
        let Some((xs, ys, xe, ye)) = clip(r, x0, y0, x1, y1) else { return };
        let base = r.local.len() as u32;
        r.local.extend_from_slice(words);
        let (cols, rows) = push_maps(r, xs, xe, ys, ye, col, row);
        r.cmds.push(Cmd { op: op::PICTURE, xs, ys, xe, ye, cols, rows, base, stride: src_w as u32, p: [1, 0, 0, 0, 0, 0, 0] });
    });
}

/// The ground column of every screen column (-1 off the map), as `render::column_map`.
fn ground_cols(view: &View, w: usize, bg_w: usize) -> Vec<i32> {
    let inv = 1.0 / view.zoom;
    (0..w)
        .map(|sx| {
            let mx = view.x + sx as f32 * inv;
            if mx >= 0.0 && (mx as usize) < bg_w {
                mx as i32
            } else {
                -1
            }
        })
        .collect()
}

/// The ground row of screen row `sy` (-1 off the map), as `render::render`.
fn ground_row(view: &View, bg_h: usize, sy: i32) -> i32 {
    let my = view.y + sy as f32 * (1.0 / view.zoom);
    if my < 0.0 || my >= bg_h as f32 {
        -1
    } else {
        my as i32
    }
}

fn ground_cmd(g: &GroundRef, view: &View, op: u32, ys: i32, ye: i32, row: impl Fn(i32) -> i32, p0: u32, mask: Option<usize>) {
    with_rec(|r, _, ground| {
        ground.set(g);
        if let Some(m) = mask {
            use_mask(r, m);
        }
        let Some((xs, ys, xe, ye)) = clip(r, 0, ys, r.w as i32, ye) else { return };
        let cols = ground_cols(view, r.w, g.w);
        let c0 = r.maps.len() as u32;
        r.maps.extend(cols);
        let r0 = r.maps.len() as u32;
        r.maps.extend((ys..ye).map(row));
        r.cmds.push(Cmd { op, xs, ys, xe, ye, cols: c0, rows: r0, base: 0, stride: g.w as u32, p: [p0, 0, 0, 0, 0, 0, 0] });
    });
}

/// `render::render`'s backdrop: the dark fill and the map picture.
pub fn background(g: &GroundRef, view: &View, w: usize, h: usize) {
    fill(op::FILL, 0, 0, w as i32, h as i32, 12 << 16 | 12 << 8 | 16, 255);
    ground_cmd(g, view, op::GROUND, 0, h as i32, |sy| ground_row(view, g.h, sy), 0, None);
}

/// `render::stamp_foreground`.
pub fn stamp_ground(g: &GroundRef, view: &View, h: usize, mask: usize) {
    if g.fg.is_empty() {
        return;
    }
    ground_cmd(g, view, op::STAMP_GROUND, 0, h as i32, |sy| ground_row(view, g.h, sy), 0, Some(mask));
}

/// `render::foreground_rows`: the foreground of map rows `y0..y1` copied over the picture (screen rows `s0..s1`).
pub fn foreground_rows(g: &GroundRef, view: &View, s0: usize, s1: usize, y0: usize, y1: usize) {
    if g.fg.is_empty() || s0 >= s1 {
        return;
    }
    let inv = 1.0 / view.zoom;
    let row = |sy: i32| {
        let my = view.y + sy as f32 * inv;
        if my < y0 as f32 || my >= y1.min(g.h) as f32 {
            -1
        } else {
            my as i32
        }
    };
    ground_cmd(g, view, op::GROUND, s0 as i32, s1 as i32, row, 1, None);
}

/// [`op::COVER`]: an alpha grid `bw` wide (0..=255, 0 = none) with its top-left at screen `(bx, by)`.
pub fn cover(bx: i32, by: i32, bw: usize, grid: &[u32], rgb: u32) {
    cover_masked(bx, by, bw, grid, rgb, None);
}

/// [`cover`] that skips the screen pixels set in the screen mask `mask` (its address, as for the masked sprite commands).
pub fn cover_masked(bx: i32, by: i32, bw: usize, grid: &[u32], rgb: u32, mask: Option<usize>) {
    if bw == 0 || grid.is_empty() {
        return;
    }
    let bh = grid.len() / bw;
    with_rec(|r, _, _| {
        let Some((xs, ys, xe, ye)) = clip(r, bx, by, bx + bw as i32, by + bh as i32) else { return };
        if let Some(m) = mask {
            use_mask(r, m);
        }
        let base = r.local.len() as u32;
        r.local.extend_from_slice(grid);
        let (cols, rows) = push_maps(r, xs, xe, ys, ye, |x| x - bx, |y| y - by);
        r.cmds.push(Cmd { op: op::COVER, xs, ys, xe, ye, cols, rows, base, stride: bw as u32, p: [rgb, mask.is_some() as u32, 0, 0, 0, 0, 0] });
    });
}

/// The per-tile command lists of a recording, in segments: a new segment starts at every [`op::LENS`] (which reads the picture
/// as it was before it). Tile `t` of segment `s` has the command indices `indices[offsets[s * tiles + t] .. offsets[s * tiles + t + 1]]`, ascending.
pub struct Bins {
    /// Tiles per row.
    pub tiles_x: usize,
    /// Tile rows.
    pub tiles_y: usize,
    /// First command of every segment.
    pub segments: Vec<usize>,
    /// Where each tile's command list starts in `indices` (see the struct docs).
    pub offsets: Vec<u32>,
    /// The command indices of all tiles, one list after the other.
    pub indices: Vec<u32>,
}

impl Recording {
    /// Sorts the frame's commands into screen tiles, so a pixel only looks at the commands that can touch it.
    pub fn bins(&self) -> Bins {
        let (tiles_x, tiles_y) = (self.w.div_ceil(TILE), self.h.div_ceil(TILE));
        let tiles = tiles_x * tiles_y;
        let mut segments = vec![0usize];
        let mut seg_of = Vec::with_capacity(self.cmds.len());
        for (i, c) in self.cmds.iter().enumerate() {
            if c.op == op::LENS && i > 0 {
                segments.push(i);
            }
            seg_of.push(segments.len() - 1);
        }
        let span = |c: &Cmd| (c.xs as usize / TILE, (c.xe as usize - 1) / TILE, c.ys as usize / TILE, (c.ye as usize - 1) / TILE);
        let mut counts = vec![0u32; segments.len() * tiles + 1];
        for (c, &s) in self.cmds.iter().zip(&seg_of) {
            let (tx0, tx1, ty0, ty1) = span(c);
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    counts[s * tiles + ty * tiles_x + tx] += 1;
                }
            }
        }
        // exclusive prefix sum -> start offsets
        let mut total = 0u32;
        for v in counts.iter_mut() {
            let n = *v;
            *v = total;
            total += n;
        }
        let offsets = counts;
        let mut cursor = offsets.clone();
        let mut indices = vec![0u32; total as usize];
        for (i, (c, &s)) in self.cmds.iter().zip(&seg_of).enumerate() {
            let (tx0, tx1, ty0, ty1) = span(c);
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    let k = s * tiles + ty * tiles_x + tx;
                    indices[cursor[k] as usize] = i as u32;
                    cursor[k] += 1;
                }
            }
        }
        Bins { tiles_x, tiles_y, segments, offsets, indices }
    }
}

/// `endscreen::tint565` (red kept, green and blue / 4).
#[inline]
fn tint565(p: u16) -> u16 {
    opensow_formats::endscreen::tint565(p)
}

#[inline]
fn pack(r: u32, g: u32, b: u32) -> u32 {
    r << 16 | g << 8 | b
}

#[inline]
fn blend(d: u32, s: u32, a: u32) -> u32 {
    let mix = |sh: u32| ((d >> sh & 255) * (255 - a) + (s >> sh & 255) * a) / 255;
    pack(mix(16), mix(8), mix(0))
}

#[inline]
fn to565(q: u32) -> u16 {
    ((q >> 19 & 31) << 11 | (q >> 10 & 63) << 5 | (q >> 3 & 31)) as u16
}

#[inline]
fn from565(o: u16) -> u32 {
    let [r, g, b] = rgb565_to_rgb8(o);
    pack(r as u32, g as u32, b as u32)
}

/// The CPU reference of the GPU replay: draws `rec` into `out` (`w * h`, row-major, no stride) in command order.
pub fn replay(rec: &Recording, out: &mut [u32]) {
    let (w, h) = (rec.w, rec.h);
    let mut mask = vec![false; w * h];
    STATE.with_borrow(|s| {
        let (atlas, ground) = (&s.atlas.data, &s.ground.data);
        for c in &rec.cmds {
            let p = c.p;
            for y in c.ys..c.ye {
                let row = if is_rect(c.op) { 0 } else { rec.maps[(c.rows + (y - c.ys) as u32) as usize] };
                for x in c.xs..c.xe {
                    let i = y as usize * w + x as usize;
                    let q = out[i];
                    match c.op {
                        op::FILL => {
                            if p[2] == 0 || y % 2 == 0 {
                                out[i] = blend(q, p[0], p[1]);
                            }
                            continue;
                        }
                        op::DIM_SUB => {
                            let sub = |sh: u32| (q >> sh & 255).saturating_sub(p[0]);
                            out[i] = pack(sub(16), sub(8), sub(0));
                            continue;
                        }
                        op::ADD_RGB => {
                            let add = |sh: u32| ((q >> sh & 255) + (p[0] >> sh & 255)).min(255);
                            out[i] = pack(add(16), add(8), add(0));
                            continue;
                        }
                        op::TINT_ROWS => {
                            out[i] = if y % 2 == 1 { 0 } else { from565(tint565(to565(q))) };
                            continue;
                        }
                        _ => {}
                    }
                    let col = rec.maps[(c.cols + (x - c.xs) as u32) as usize];
                    if c.op == op::GROUND || c.op == op::STAMP_GROUND {
                        if col < 0 || row < 0 {
                            continue;
                        }
                        let g = ground[row as usize * c.stride as usize + col as usize];
                        if c.op == op::STAMP_GROUND {
                            mask[i] |= g & MASK_BIT != 0;
                        } else if p[0] == 0 || g & MASK_BIT != 0 {
                            out[i] = g & 0xff_ffff;
                        }
                        continue;
                    }
                    if c.op == op::PICTURE {
                        if col < 0 || row < 0 {
                            continue;
                        }
                        let si = c.base as usize + row as usize * c.stride as usize + col as usize;
                        let v = if p[0] == 0 { atlas[si] } else { rec.local[si] };
                        out[i] = if p[1] != 0 { v >> 1 & 0x7f7f7f } else { v & 0xff_ffff };
                        continue;
                    }
                    let si = c.base as usize + row as usize * c.stride as usize + col as usize;
                    if c.op == op::COVER {
                        let a = rec.local[si];
                        if a > 0 && !(p[1] != 0 && mask[i]) {
                            out[i] = blend(q, p[0], a);
                        }
                        continue;
                    }
                    let s = atlas[si];
                    if s & 1 << 16 == 0 {
                        continue;
                    }
                    let px = s as u16;
                    match c.op {
                        op::PLAIN => out[i] = from565(px),
                        op::SHADOW => {
                            if !(p[1] != 0 && mask[i]) {
                                out[i] = blend(q, 0, p[0]);
                            }
                        }
                        op::BLEND => {
                            if !(p[1] != 0 && mask[i]) {
                                out[i] = from565(blit_of(p[0]).apply565(to565(q), px));
                            }
                        }
                        op::LENS => {
                            if !(p[1] != 0 && mask[i]) {
                                let (qx, qy) = ((x as usize + 2 * p[0] as usize).min(w - 1), (y as usize + 3 * p[0] as usize).min(h - 1));
                                out[i] = from565(Blit::Lens.apply565(to565(out[qy * w + qx]), px));
                            }
                        }
                        op::MASKED => {
                            let (mw, mh) = ((p[1] & 0xffff) as i32, (p[1] >> 16) as i32);
                            let (mx, my) = (col + p[2] as i32, row + p[3] as i32);
                            if mx >= 0 && my >= 0 && mx < mw && my < mh && atlas[p[0] as usize + (my * mw + mx) as usize] & 1 << 16 != 0 {
                                out[i] = from565(px);
                            }
                        }
                        op::STAMP => mask[i] = true,
                        op::LIT => {
                            let [r, g, b] = rgb565_to_rgb8(px).map(|c| c as u32 + (255 - c as u32) * p[0] / 255);
                            out[i] = pack(r, g, b);
                        }
                        op::GLOW => {
                            let [r, g, b] = rgb565_to_rgb8(px).map(|c| c as u32);
                            let a = (g.saturating_sub(r.max(b)) * 2).min(255);
                            if a != 0 {
                                out[i] = blend(q, pack(r, g, b), a);
                            }
                        }
                        op::ADD8 => {
                            let src = rgb565_to_rgb8(px).map(|c| if p[0] != 0 { c as u32 / 2 } else { c as u32 });
                            let add = |sh: u32, s: u32| ((q >> sh & 255) + s).min(255);
                            out[i] = pack(add(16, src[0]), add(8, src[1]), add(0, src[2]));
                        }
                        op::DIM => out[i] = blend(q, 0, p[0]),
                        op::GLYPH => out[i] = p[0],
                        _ => {}
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn frame(w: u16, h: u16, seed: u16) -> Frame {
        let n = (w * h) as usize;
        Frame {
            width: w,
            height: h,
            hot_x: (w / 3) as i16,
            hot_y: (h / 2) as i16,
            extra: [0; 4],
            pixels: (0..n).map(|i| (i as u16).wrapping_mul(2654).wrapping_add(seed.wrapping_mul(40503))).collect(),
            mask: (0..n).map(|i| !(i * 7 + seed as usize).is_multiple_of(5)).collect(),
            enhanced: Default::default(),
        }
    }

    /// Draws with `f` once directly and once recorded + replayed; the pictures must be the same.
    fn same(w: usize, h: usize, f: impl Fn(&mut [u32])) {
        let start: Vec<u32> = (0..w * h).map(|i| (i as u32).wrapping_mul(2_654_435_761) & 0xff_ffff).collect();
        let mut direct = start.clone();
        f(&mut direct);
        begin(w, h);
        let mut dummy = start.clone();
        f(&mut dummy);
        let rec = end().unwrap();
        assert!(!rec.tainted);
        assert_eq!(dummy, start, "recording leaves the buffer alone");
        let mut replayed = start;
        replay(&rec, &mut replayed);
        let bad = direct.iter().zip(&replayed).filter(|(a, b)| a != b).count();
        assert_eq!(bad, 0, "{bad} pixels differ");
    }

    #[test]
    fn recorded_blits_match_the_direct_ones() {
        let (w, h) = (97usize, 61usize);
        let (a, b, m) = (frame(23, 17, 1), frame(40, 30, 2), frame(30, 22, 3));
        for zoom in [1.0f32, 1.37, 2.0, 2.5, 0.8] {
            let view = View { x: -3.3, y: 4.1, zoom };
            same(w, h, |buf| {
                let mut mask = vec![false; w * h];
                render::rect(buf, w, h, -5, 3, 40, 70, (200, 10, 99), 77);
                render::blit(buf, w, h, &view, &a, 10, 12, false);
                render::blit(buf, w, h, &view, &b, 30, 20, true);
                render::stamp_frame(&mut mask, w, h, &view, &m, 25, 25);
                render::blit_shadow_masked(buf, w, h, &view, &a, 28, 26, &mask);
                for (k, mode) in
                    [Blit::Plain, Blit::Additive, Blit::AddHalf, Blit::Average, Blit::Smoke, Blit::Darken, Blit::Shade, Blit::Lens].into_iter().enumerate()
                {
                    let masked = (k % 2 == 1).then_some(&mask[..]);
                    render::blit_blend_masked(buf, w, h, &view, &b, 5 + 6 * k as i32, 8 + 4 * k as i32, mode, masked);
                }
                render::blit_masked(buf, w, h, &view, &b, &m, 40, 30, (3, 2, 90, 50));
                render::blit_clip(buf, w, h, &view, &a, 60, 40, false, (50, 30, 80, 55));
            });
        }
    }

    #[test]
    fn bins_cover_every_command_in_order() {
        begin(40, 20);
        fill(op::FILL, 0, 0, 40, 20, 0, 255);
        fill(op::FILL, 17, 3, 18, 4, 0, 255);
        let f = frame(4, 4, 0);
        sprite(op::LENS, &f, 30, 15, 34, 19, |x| (x - 30) as usize, |y| (y - 15) as usize, [1, 0, 0, 0, 0, 0, 0], None);
        fill(op::FILL, 0, 0, 1, 1, 0, 255);
        let rec = end().unwrap();
        let b = rec.bins();
        assert_eq!((b.tiles_x, b.tiles_y), (3, 2));
        assert_eq!(b.segments, vec![0, 2]);
        let tiles = 6;
        let list = |s: usize, t: usize| &b.indices[b.offsets[s * tiles + t] as usize..b.offsets[s * tiles + t + 1] as usize];
        assert_eq!(list(0, 0), &[0]);
        assert_eq!(list(0, 1), &[0, 1]);
        assert_eq!(list(1, 0), &[3]);
        assert_eq!(list(1, 5), &[2]);
        assert_eq!(list(1, 1), &[2]);
        assert!(list(1, 3).is_empty());
    }
}
