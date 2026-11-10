//! The map editor's sidebar (docs/map-editor.md): cards of buttons with the game's own pictures (building and unit sprites, the
//! HUD's production-line icons `extras\\ikony`), so the editor needs no keys and little text; explanations are hover tips. One
//! widget list ([`Ui::widgets`]) is both drawn and hit-tested, so what is drawn is what is clicked.

use crate::art::Art;
use crate::editor::{display_name, line_label, palette, text, wrap, Editor, LineChange, Lines, Sel};
use crate::render::{blend, pack, rect};
use opensow_formats::font::Font;
use opensow_formats::level::Marker;
use opensow_formats::maptweak::{template, Production, Side, Thing, MAX_LINES};
use opensow_formats::sprite::{rgb565_to_rgb8, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// What a click on a widget does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    MapPrev,
    MapNext,
    Save,
    Undo,
    Redo,
    Revert,
    /// Stop placing (back to select and move).
    SelectTool,
    /// Arm palette item `i`: clicks on the map place it (a second click disarms).
    Tool(usize),
    /// Owner of what gets placed.
    PlaceSide(Side),
    /// Owner of the selection.
    SelOwner(Side),
    Delete,
    ScrollUp,
    ScrollDown,
    /// Palette tab: 0 buildings, 1 turrets, 2 units.
    Tab(usize),
    /// The lower part shows the selection's production lines (`true`) or the palette.
    Lower(bool),
    /// A change of the selected building's production lines.
    Line(LineChange),
}

/// The palette tabs and the palette indices each shows.
pub const TABS: [(&str, std::ops::Range<usize>); 3] = [("Buildings", 0..9), ("Turrets", 9..14), ("Units", 14..36)];

/// Small pictures drawn from lines (the game has no art for them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Undo,
    Redo,
    Save,
    Revert,
    Plus,
    Minus,
    Left,
    Right,
    Cross,
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pic {
    /// Palette item `i` as `side` owns it (its map sprites).
    Item(usize, Side),
    /// `extras\\ikony` frame: the HUD's production-line icon of that kind (1-15 units, 16-24 levels, 30-34 air forces).
    Slot(i32),
    Swatch((u8, u8, u8)),
    Glyph(Glyph),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Style {
    Plain,
    /// Chosen (a blue fill, or the given colour).
    On(Option<(u8, u8, u8)>),
    Danger,
    /// A dashed "add something" button.
    Ghost,
}

enum Look {
    Card,
    Text { s: String, colour: (u8, u8, u8) },
    Button { label: String, pic: Option<Pic>, style: Style, enabled: bool },
    Picture { pic: Pic, dim: bool, frame: Option<(u8, u8, u8)> },
    /// Two bars: blue's and green's share of the Advancer reach.
    Balance([f32; 2]),
}

pub struct Widget {
    pub r: Rect,
    pub hit: Option<Hit>,
    /// Shown next to the mouse while it is over the widget.
    pub tip: Option<String>,
    look: Look,
}

/// A picture composed once: `None` pixels are transparent.
struct Icon {
    w: usize,
    h: usize,
    px: Vec<Option<u32>>,
}

pub struct Ui {
    /// The medium and the big mask font of the game (the big one on windows 900+ px high).
    pub fonts: (Font, Font),
    pub big: (Font, Font),
    /// The palette tab shown.
    pub tab: usize,
    /// The lower part shows the selected building's production lines instead of the palette.
    pub production: bool,
    /// Per side (neutral, blue, green) and palette item.
    icons: [Vec<Option<Icon>>; 3],
    /// `extras\\ikony` frames by index (production-line icons by kind).
    slots: Vec<Option<Icon>>,
    /// First palette row shown.
    pub scroll: usize,
}

/// The sidebar's width for a window `w` x `h`.
pub fn panel_width(w: usize, h: usize) -> usize {
    let (pct, max) = if h >= 900 { (24, 460) } else { (22, 340) };
    (w * pct / 100).clamp(210, max).min(w / 2)
}

fn side_index(s: Side) -> usize {
    match s {
        Side::Neutral => 0,
        Side::Blue => 1,
        Side::Green => 2,
    }
}

const SIDES: [Side; 3] = [Side::Neutral, Side::Blue, Side::Green];
const SIDE_NAMES: [&str; 3] = ["Neutral", "Blue", "Green"];
const SIDE_TINT: [(u8, u8, u8); 3] = [(150, 150, 150), (60, 110, 255), (40, 190, 70)];
const PANEL: (u8, u8, u8) = (17, 20, 26);
const CARD: (u8, u8, u8) = (27, 31, 39);
const EDGE: (u8, u8, u8) = (52, 58, 70);
const GOLD: (u8, u8, u8) = (255, 210, 90);
const DIM: (u8, u8, u8) = (150, 156, 166);
const LIVE: (u8, u8, u8) = (60, 150, 80);

/// Builds the widget list top-down.
struct Layout<'a> {
    out: Vec<Widget>,
    font: &'a Font,
    /// Left edge and width of the content (inside the panel's padding).
    x0: i32,
    inner: i32,
    lh: i32,
    bh: i32,
    gap: i32,
    y: i32,
}

impl Layout<'_> {
    fn push(&mut self, r: Rect, hit: Option<Hit>, tip: Option<&str>, look: Look) {
        self.out.push(Widget { r, hit, tip: tip.map(str::to_string), look });
    }

    fn button(&mut self, r: Rect, hit: Hit, label: &str, pic: Option<Pic>, style: Style, enabled: bool, tip: &str) {
        let tip = (!tip.is_empty()).then_some(tip);
        self.push(r, enabled.then_some(hit), tip, Look::Button { label: label.to_string(), pic, style, enabled });
    }

    /// One line of text at `x`, `y`, cut to `w`.
    fn text_at(&mut self, x: i32, y: i32, w: i32, s: &str, colour: (u8, u8, u8), tip: Option<&str>) {
        let s = fit(self.font, s, w);
        self.push(Rect { x, y, w: self.font.measure(&s, 1).min(w).max(1), h: self.lh }, None, tip, Look::Text { s, colour });
    }

    /// Whether every label of a row of buttons fits its cell next to a picture (else the row shows pictures only, its labels
    /// becoming tips: a row never mixes the two).
    /// `pic`: the kind of picture the buttons carry (its drawn size differs: `Button` drawing in [`Ui::draw`]).
    fn labels_fit(&self, cells: &[Rect], labels: &[&str], pic: Pic) -> bool {
        cells.iter().zip(labels).all(|(r, s)| {
            let ps = match pic {
                Pic::Glyph(_) => (r.h - 12).min(self.lh + 2).max(8),
                Pic::Swatch(_) => (self.lh - 6).max(6),
                _ => (r.h - 6).min(self.lh + 10),
            };
            self.font.measure(s, 1) + ps + 6 + 12 <= r.w
        })
    }

    /// The `k`-th of a row of cells with relative `weights` across `w` from `x`.
    fn cell(&self, x: i32, w: i32, weights: &[i32], k: usize, y: i32, h: i32) -> Rect {
        let total: i32 = weights.iter().sum();
        let room = w - self.gap * (weights.len() as i32 - 1);
        let left = x + weights[..k].iter().map(|wt| room * wt / total + self.gap).sum::<i32>();
        Rect { x: left, y, w: room * weights[k] / total, h }
    }

    /// Opens a card (a framed box) behind what follows; [`Layout::end_card`] sets its height.
    fn card(&mut self) -> usize {
        self.push(Rect { x: self.x0, y: self.y, w: self.inner, h: 0 }, None, None, Look::Card);
        self.y += self.gap;
        self.out.len() - 1
    }

    fn end_card(&mut self, k: usize) {
        self.y += self.gap;
        self.out[k].r.h = self.y - self.out[k].r.y;
        self.y += self.gap + 2;
    }
}

impl Ui {
    /// Loads the fonts and composes the pictures from the install's sprites.
    pub fn new(art: &mut Art) -> Ui {
        let mut font = |n: &str| Font::new(art.load(&format!("extras\\fonts\\{n}.msk")).map(|f| (*f).clone()).unwrap_or_default());
        let fonts = (font("medium_i"), font("medium_o"));
        let big = (font("big_i"), font("big_o"));
        let icons = SIDES.map(|side| palette().into_iter().map(|t| compose(art, t, side)).collect());
        let slots = art.load("extras\\ikony.ps6").map_or(Vec::new(), |set| set.iter().map(|f| Some(frame_icon(f))).collect());
        Ui { fonts, big, tab: 0, production: false, icons, slots, scroll: 0 }
    }

    /// The font pair for a window `h` px high.
    fn font(&self, h: usize) -> &(Font, Font) {
        if h >= 900 && self.big.0.glyph_count() > 0 { &self.big } else { &self.fonts }
    }

    /// The sidebar's widgets for a window `w` x `h` (the panel is the right `panel_width(w, h)` columns).
    pub fn widgets(&self, w: usize, h: usize, ed: &Editor, map_name: &str, can_switch: bool) -> Vec<Widget> {
        let big = h >= 900;
        let font = &self.font(h).0;
        let pw = panel_width(w, h) as i32;
        let pad = if big { 16 } else { 10 };
        let lh = font.line_height().max(10) + 4;
        let mut l = Layout { out: Vec::new(), font, x0: w as i32 - pw + pad, inner: pw - 2 * pad, lh, bh: lh + if big { 16 } else { 10 }, gap: if big { 10 } else { 6 }, y: pad };
        let (x0, inner, bh, gap) = (l.x0, l.inner, l.bh, l.gap);
        let message: Vec<String> = if ed.message.is_empty() { Vec::new() } else { wrap(font, &ed.message, inner) };
        let bottom = h as i32 - pad - message.len() as i32 * lh - if message.is_empty() { 0 } else { gap };

        // header: title and save state
        let dirty = ed.dirty();
        let n = ed.edits().len();
        let (state, colour) = match (dirty, n) {
            (true, _) => ("Unsaved".to_string(), (255, 190, 80)),
            (false, 0) => ("As shipped".to_string(), DIM),
            (false, _) => (format!("Saved, {n} edit{}", if n == 1 { "" } else { "s" }), (140, 200, 140)),
        };
        let sw = font.measure(&state, 1).min(inner / 2);
        l.text_at(x0, l.y, inner - sw - gap, "MAP EDITOR", GOLD, None);
        l.text_at(x0 + inner - sw, l.y, sw, &state, colour, Some("This map's edits against the shipped file (data/skirmish-maps.txt)"));
        l.y += lh + gap;

        // map switch and the file actions
        l.button(Rect { x: x0, y: l.y, w: bh, h: bh }, Hit::MapPrev, "", Some(Pic::Glyph(Glyph::Left)), Style::Plain, can_switch, "Previous map");
        l.button(Rect { x: x0 + inner - bh, y: l.y, w: bh, h: bh }, Hit::MapNext, "", Some(Pic::Glyph(Glyph::Right)), Style::Plain, can_switch, "Next map");
        let name = fit(font, map_name, inner - 2 * bh - 2 * gap);
        let nw = font.measure(&name, 1);
        l.push(Rect { x: x0 + (inner - nw) / 2, y: l.y + (bh - lh) / 2 + 2, w: nw.max(1), h: lh }, None, None, Look::Text { s: name, colour: (240, 240, 240) });
        l.y += bh + gap;
        let wts = [4, 4, 2, 2];
        let (rs, rr) = (l.cell(x0, inner, &wts, 0, l.y, bh), l.cell(x0, inner, &wts, 1, l.y, bh));
        let named = l.labels_fit(&[rs, rr], &["Save", "Revert"], Pic::Glyph(Glyph::Save));
        l.button(rs, Hit::Save, if named { "Save" } else { "" }, Some(Pic::Glyph(Glyph::Save)), if dirty { Style::On(None) } else { Style::Plain }, dirty, "Save this map's edits to data/skirmish-maps.txt");
        l.button(rr, Hit::Revert, if named { "Revert" } else { "" }, Some(Pic::Glyph(Glyph::Revert)), Style::Plain, n > 0, "Back to the shipped map (can be undone)");
        let r = l.cell(x0, inner, &wts, 2, l.y, bh);
        l.button(r, Hit::Undo, "", Some(Pic::Glyph(Glyph::Undo)), Style::Plain, ed.can_undo(), "Undo");
        let r = l.cell(x0, inner, &wts, 3, l.y, bh);
        l.button(r, Hit::Redo, "", Some(Pic::Glyph(Glyph::Redo)), Style::Plain, ed.can_redo(), "Redo");
        l.y += bh + gap + 4;

        // balance: blue's reach | bar | green's reach
        let c = l.card();
        let reach = ed.reach();
        let tip = "Advancer reach: the neutral buildings each side's Advancer has within reach (worth / (1 + flight / 16)). Close numbers: an even map.";
        let (b, g) = (format!("{:.1}", reach[0]), format!("{:.1}", reach[1]));
        let (bw, gw) = (font.measure(&b, 1), font.measure(&g, 1));
        let sq = (lh - 4).max(6);
        let y = l.y;
        l.push(Rect { x: x0 + gap, y: y + 2, w: sq, h: sq }, None, Some(tip), Look::Picture { pic: Pic::Swatch(SIDE_TINT[1]), dim: false, frame: None });
        l.text_at(x0 + gap + sq + 6, y, bw, &b, (230, 230, 230), Some(tip));
        l.push(Rect { x: x0 + inner - gap - sq, y: y + 2, w: sq, h: sq }, None, Some(tip), Look::Picture { pic: Pic::Swatch(SIDE_TINT[2]), dim: false, frame: None });
        l.text_at(x0 + inner - gap - sq - 6 - gw, y, gw, &g, (230, 230, 230), Some(tip));
        let (bx0, bx1) = (x0 + gap + sq + 14 + bw, x0 + inner - gap - sq - 14 - gw);
        l.push(Rect { x: bx0, y: y + lh / 2 - 3, w: (bx1 - bx0).max(10), h: 7 }, None, Some(tip), Look::Balance(reach));
        l.y += lh;
        l.end_card(c);

        // the selection: its picture, name, cell, flight distances; owner and delete
        let c = l.card();
        let left = x0 + gap;
        let width = inner - 2 * gap;
        match ed.sel {
            Some(s) => {
                let (thing, cell, f, side) = (ed.thing_of(s), ed.cell_of(s), ed.flight(s), ed.side_of(s));
                let pic = if big { 72 } else { 48 };
                if let Some(i) = palette().iter().position(|&p| p == thing) {
                    let shown = if side == Side::Neutral && matches!(thing, Thing::Unit(_)) { Side::Blue } else { side };
                    l.push(Rect { x: left, y: l.y, w: pic, h: pic }, None, None, Look::Picture { pic: Pic::Item(i, shown), dim: false, frame: Some(EDGE) });
                }
                let del = bh - 4;
                l.button(Rect { x: left + width - del, y: l.y, w: del, h: del }, Hit::Delete, "", Some(Pic::Glyph(Glyph::Trash)), Style::Danger, true, "Delete it");
                let tx = left + pic + gap;
                let tw = left + width - del - gap - tx;
                let ty = l.y + (pic - 3 * lh) / 2;
                l.text_at(tx, ty, tw, &display_name(thing), GOLD, None);
                l.text_at(tx, ty + lh, tw, &format!("Cell {},{}", cell.0, cell.1), DIM, None);
                let tip = "Flight distance in cells from the blue and the green HQ";
                let mut x = tx;
                for (k, d) in f.iter().enumerate() {
                    let v = d.map_or("-".into(), |v| format!("{v:.0}"));
                    l.push(Rect { x, y: ty + 2 * lh + 3, w: sq - 2, h: sq - 2 }, None, Some(tip), Look::Picture { pic: Pic::Swatch(SIDE_TINT[k + 1]), dim: false, frame: None });
                    let vw = font.measure(&v, 1);
                    l.text_at(x + sq + 2, ty + 2 * lh, vw, &v, (225, 225, 225), Some(tip));
                    x += sq + vw + 2 * gap;
                }
                l.y += pic + gap;
                let sides_only = matches!(s, Sel::Unit(_)) || thing == Thing::Object(100);
                let cells: Vec<Rect> = (0..3).map(|k| l.cell(left, width, &[1, 1, 1], k, l.y, bh)).collect();
                let named = l.labels_fit(&cells, &SIDE_NAMES, Pic::Swatch(PANEL));
                for k in 0..3 {
                    let style = if side == SIDES[k] { Style::On(Some(SIDE_TINT[k])) } else { Style::Plain };
                    l.button(cells[k], Hit::SelOwner(SIDES[k]), if named { SIDE_NAMES[k] } else { "" }, Some(Pic::Swatch(SIDE_TINT[k])), style, !(sides_only && k == 0), &format!("Make it {}", SIDE_NAMES[k].to_lowercase()));
                }
                l.y += bh;
            }
            None => {
                l.text_at(left, l.y, width, "Nothing selected", DIM, Some("Click a building, turret or unit on the map"));
                l.y += lh;
            }
        }
        l.end_card(c);

        // lower part: place new | production
        let lines = ed.lines();
        let show_lines = self.production && lines.is_some();
        let (rp, rl) = (l.cell(x0, inner, &[1, 1], 0, l.y, bh), l.cell(x0, inner, &[1, 1], 1, l.y, bh));
        let named = l.labels_fit(&[rp, rl], &["Place", "Production"], Pic::Slot(7));
        l.button(rp, Hit::Lower(false), if named { "Place" } else { "" }, Some(Pic::Glyph(Glyph::Plus)), if show_lines { Style::Plain } else { Style::On(None) }, true, "Place new buildings, turrets and units");
        l.button(rl, Hit::Lower(true), if named { "Production" } else { "" }, Some(Pic::Slot(7)), if show_lines { Style::On(None) } else { Style::Plain }, lines.is_some(), "The selected building's production lines");
        l.y += bh + gap + 4;
        match (show_lines, lines) {
            (true, Some((family, p))) => production_view(&mut l, family, &p, bottom),
            _ => self.place_view(&mut l, ed, bottom),
        }
        let mut my = bottom + gap;
        for s in message {
            l.text_at(x0, my, inner, &s, (255, 170, 120), None);
            my += lh;
        }
        l.out
    }

    fn place_view(&self, l: &mut Layout, ed: &Editor, bottom: i32) {
        let (x0, inner, bh, gap) = (l.x0, l.inner, l.bh, l.gap);
        let cells: Vec<Rect> = (0..3).map(|k| l.cell(x0, inner, &[1, 1, 1], k, l.y, bh)).collect();
        let named = l.labels_fit(&cells, &SIDE_NAMES, Pic::Swatch(PANEL));
        for k in 0..3 {
            let style = if ed.side == SIDES[k] { Style::On(Some(SIDE_TINT[k])) } else { Style::Plain };
            l.button(cells[k], Hit::PlaceSide(SIDES[k]), if named { SIDE_NAMES[k] } else { "" }, Some(Pic::Swatch(SIDE_TINT[k])), style, true, &format!("New things are {}", SIDE_NAMES[k].to_lowercase()));
        }
        l.y += bh + gap;
        let unit_side = if ed.side == Side::Neutral { Side::Blue } else { ed.side };
        let tab_pics = [Pic::Item(0, ed.side), Pic::Item(9, ed.side), Pic::Item(14 + 7, unit_side)];
        let th = bh + 8;
        let cells: Vec<Rect> = (0..3).map(|k| l.cell(x0, inner, &[1, 1, 1], k, l.y, th)).collect();
        let names: Vec<&str> = TABS.iter().map(|t| t.0).collect();
        let named = l.labels_fit(&cells, &names, Pic::Slot(0));
        for (k, name) in names.iter().enumerate() {
            l.button(cells[k], Hit::Tab(k), if named { name } else { "" }, Some(tab_pics[k]), if self.tab == k { Style::On(None) } else { Style::Plain }, true, name);
        }
        l.y += th + gap;
        if ed.placing {
            let thing = palette()[ed.palette];
            l.text_at(x0, l.y + (bh - l.lh) / 2 + 2, inner - bh - gap, &format!("Placing {}", display_name(thing)), (120, 230, 120), None);
            l.button(Rect { x: x0 + inner - bh, y: l.y, w: bh, h: bh }, Hit::SelectTool, "", Some(Pic::Glyph(Glyph::Cross)), Style::Plain, true, "Stop placing (or right click)");
            l.y += bh + gap;
        }
        // the palette grid of the tab fills what is left
        let icon = if bh > 40 { 76 } else { 54 };
        let cols = (inner / icon).max(3);
        let cell = inner / cols;
        let items: Vec<usize> = TABS[self.tab.min(2)].1.clone().collect();
        let rows_total = (items.len() as i32 + cols - 1) / cols;
        let rows_fit = ((bottom - l.y) / cell).max(1);
        let overflow = rows_total > rows_fit;
        let rows_shown = if overflow { ((bottom - l.y - bh - gap) / cell).max(1) } else { rows_total };
        let scroll = (self.scroll as i32).min((rows_total - rows_shown).max(0));
        for r in 0..rows_shown {
            for c in 0..cols {
                let Some(&i) = items.get(((scroll + r) * cols + c) as usize) else { continue };
                let side = if matches!(palette()[i], Thing::Unit(_)) { unit_side } else { ed.side };
                let rr = Rect { x: x0 + c * cell, y: l.y + r * cell, w: cell - 4, h: cell - 4 };
                let on = ed.placing && ed.palette == i;
                let tip = format!("{}: pick it, then click the map", display_name(palette()[i]));
                l.button(rr, Hit::Tool(i), "", Some(Pic::Item(i, side)), if on { Style::On(Some(LIVE)) } else { Style::Plain }, true, &tip);
            }
        }
        l.y += rows_shown * cell;
        if overflow {
            let r = l.cell(x0, inner, &[1, 1], 0, l.y, bh);
            l.button(r, Hit::ScrollUp, "", Some(Pic::Glyph(Glyph::Left)), Style::Plain, scroll > 0, "Previous page");
            let r = l.cell(x0, inner, &[1, 1], 1, l.y, bh);
            l.button(r, Hit::ScrollDown, "", Some(Pic::Glyph(Glyph::Right)), Style::Plain, scroll + rows_shown < rows_total, "Next page");
        }
    }

    /// The widget hit at screen `(x, y)`, if any (only enabled buttons answer).
    pub fn hit(&self, w: usize, h: usize, ed: &Editor, map_name: &str, can_switch: bool, x: i32, y: i32) -> Option<Hit> {
        self.widgets(w, h, ed, map_name, can_switch).into_iter().find(|wd| wd.hit.is_some() && wd.r.contains(x, y)).and_then(|wd| wd.hit)
    }

    #[allow(clippy::too_many_arguments)]
    fn picture(&self, buf: &mut [u32], w: usize, h: usize, pic: Pic, r: Rect, dim: bool, colour: (u8, u8, u8)) {
        match pic {
            Pic::Item(i, side) => {
                if let Some(Some(icon)) = self.icons[side_index(side)].get(i) {
                    draw_icon(buf, w, h, icon, r, dim);
                }
            }
            Pic::Slot(k) => match self.slots.get(k as usize) {
                Some(Some(icon)) => draw_icon(buf, w, h, icon, r, dim),
                _ => rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + r.h, (60, 60, 60), 255),
            },
            Pic::Swatch(c) => {
                rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + r.h, c, 255);
                frame(buf, w, h, r, (0, 0, 0));
            }
            Pic::Glyph(g) => glyph(buf, w, h, g, r, colour),
        }
    }

    /// Draws the sidebar into the window buffer; `mouse` = the cursor, for hover highlights and tips.
    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize, ed: &Editor, map_name: &str, can_switch: bool, mouse: (i32, i32)) {
        let pw = panel_width(w, h) as i32;
        let x0 = w as i32 - pw;
        rect(buf, w, h, x0, 0, w as i32, h as i32, PANEL, 255);
        rect(buf, w, h, x0, 0, x0 + 1, h as i32, EDGE, 255);
        let fonts = self.font(h);
        let lh = fonts.0.line_height().max(10) + 4;
        let widgets = self.widgets(w, h, ed, map_name, can_switch);
        for wd in &widgets {
            let r = wd.r;
            let hover = wd.hit.is_some() && r.contains(mouse.0, mouse.1);
            match &wd.look {
                Look::Card => {
                    rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + r.h, CARD, 255);
                    frame(buf, w, h, r, EDGE);
                }
                Look::Text { s, colour } => text(buf, w, h, fonts, r.x, r.y + 1, 1, s, *colour),
                Look::Balance(v) => {
                    let total = (v[0] + v[1]).max(0.01);
                    let split = r.x + (r.w as f32 * v[0] / total) as i32;
                    rect(buf, w, h, r.x, r.y, split, r.y + r.h, SIDE_TINT[1], 255);
                    rect(buf, w, h, split, r.y, r.x + r.w, r.y + r.h, SIDE_TINT[2], 255);
                    rect(buf, w, h, r.x + r.w / 2, r.y - 3, r.x + r.w / 2 + 1, r.y + r.h + 3, (255, 255, 255), 255);
                }
                Look::Picture { pic, dim, frame: edge } => {
                    if let Some(c) = edge {
                        rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + r.h, (12, 14, 18), 255);
                        frame(buf, w, h, r, *c);
                    }
                    let inset = if edge.is_some() { 3 } else { 0 };
                    self.picture(buf, w, h, *pic, Rect { x: r.x + inset, y: r.y + inset, w: r.w - 2 * inset, h: r.h - 2 * inset }, *dim, (230, 230, 230));
                }
                Look::Button { label, pic, style, enabled } => {
                    let (fill, edge) = match style {
                        Style::On(Some(c)) => (mix(*c, PANEL, 110), *c),
                        Style::On(None) => ((44, 92, 170), (90, 140, 230)),
                        Style::Danger => ((58, 30, 32), (170, 70, 60)),
                        Style::Ghost => (PANEL, (80, 90, 104)),
                        Style::Plain => ((36, 41, 51), EDGE),
                    };
                    rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + r.h, fill, 255);
                    if hover {
                        rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + r.h, (255, 255, 255), 30);
                    }
                    if *style == Style::Ghost {
                        dashed(buf, w, h, r, if hover { (200, 200, 200) } else { edge });
                    } else {
                        frame(buf, w, h, r, if hover && *enabled { (200, 200, 200) } else { edge });
                    }
                    let colour = if *enabled { (238, 238, 238) } else { (100, 104, 112) };
                    let tw = if label.is_empty() { 0 } else { fonts.0.measure(label, 1) };
                    // a picture square at the left of the label; only the picture when the label does not fit
                    let ps = match pic {
                        Some(Pic::Glyph(_)) => (r.h - 12).min(lh + 2).max(8),
                        Some(Pic::Swatch(_)) => (lh - 6).max(6),
                        Some(_) => (r.h - 6).min(lh + 10),
                        None => 0,
                    };
                    let with_label = tw > 0 && (ps == 0 || ps + 6 + tw + 12 <= r.w);
                    if let Some(p) = pic {
                        let alone = !with_label && matches!(p, Pic::Item(..) | Pic::Slot(_));
                        let size = if alone { r.h.min(r.w) - 8 } else { ps };
                        let content = if with_label { ps + 6 + tw } else { size };
                        let pr = Rect { x: r.x + (r.w - content) / 2, y: r.y + (r.h - size) / 2, w: size, h: size };
                        self.picture(buf, w, h, *p, pr, !enabled, colour);
                        if with_label {
                            text(buf, w, h, fonts, pr.x + ps + 6, r.y + (r.h - lh) / 2 + 3, 1, label, colour);
                        }
                    } else if with_label {
                        text(buf, w, h, fonts, r.x + (r.w - tw) / 2, r.y + (r.h - lh) / 2 + 3, 1, label, colour);
                    }
                }
            }
        }
        // the tip of the widget under the mouse, left of it (kept inside the window)
        if let Some(tip) = widgets.iter().rev().find(|wd| wd.tip.is_some() && wd.r.contains(mouse.0, mouse.1)).and_then(|wd| wd.tip.clone()) {
            let lines = wrap(&fonts.0, &tip, (pw - 24).max(140));
            let tw = lines.iter().map(|l| fonts.0.measure(l, 1)).max().unwrap_or(0) + 14;
            let th = lines.len() as i32 * lh + 10;
            let mx = (mouse.0 - tw - 12).max(0);
            let my = if mouse.1 + 18 + th <= h as i32 { mouse.1 + 18 } else { (mouse.1 - th - 8).max(0) };
            rect(buf, w, h, mx, my, mx + tw, my + th, (8, 9, 12), 235);
            frame(buf, w, h, Rect { x: mx, y: my, w: tw, h: th }, (110, 116, 128));
            for (k, l) in lines.iter().enumerate() {
                text(buf, w, h, fonts, mx + 7, my + 5 + k as i32 * lh, 1, l, (255, 255, 255));
            }
        }
    }
}

/// The selected building's production: lines (picture, tiers, kind arrows, remove) or, for a mine / lab / windtrap, its three
/// level slots.
fn production_view(l: &mut Layout, family: Lines, p: &Production, bottom: i32) {
    let (x0, inner, bh, gap, lh) = (l.x0, l.inner, l.bh, l.gap, l.lh);
    let n = p.lines.len() as i32;
    // `label   [-] n [+]`
    let stepper = |l: &mut Layout, s: &str, value: i32, (down, up): (LineChange, LineChange), (can_down, can_up): (bool, bool), tip: &str| {
        let vw = l.font.measure("00", 1) + 10;
        let x_up = x0 + inner - bh;
        let x_down = x_up - vw - bh;
        l.text_at(x0, l.y + (bh - lh) / 2 + 2, x_down - gap - x0, s, (225, 225, 225), Some(tip));
        l.button(Rect { x: x_down, y: l.y, w: bh, h: bh }, Hit::Line(down), "", Some(Pic::Glyph(Glyph::Minus)), Style::Plain, can_down, "Fewer");
        let v = value.to_string();
        let tw = l.font.measure(&v, 1);
        l.text_at(x_down + bh + (vw - tw) / 2, l.y + (bh - lh) / 2 + 2, tw, &v, (255, 255, 255), Some(tip));
        l.button(Rect { x: x_up, y: l.y, w: bh, h: bh }, Hit::Line(up), "", Some(Pic::Glyph(Glyph::Plus)), Style::Plain, can_up, "More");
        l.y += bh + gap;
    };
    match family {
        Lines::Levels(base) => {
            let tip = "Bright: unlocked at the start. Dim: the Advancer unlocks it. Click a level to make it the last unlocked one (again: lock it); click + to add levels.";
            let size = ((inner - 2 * gap) / 3).min(if bh > 40 { 96 } else { 64 });
            let left = x0 + (inner - 3 * size - 2 * gap) / 2;
            for k in 0..3 {
                let r = Rect { x: left + k as i32 * (size + gap), y: l.y, w: size, h: size };
                let (pic, style) = if (k as i32) < n {
                    (Pic::Slot(base + k as i32), if (k as i32) < p.active { Style::On(Some(LIVE)) } else { Style::Plain })
                } else {
                    (Pic::Glyph(Glyph::Plus), Style::Ghost)
                };
                l.push(r, Some(Hit::Line(LineChange::LevelSlot(k))), Some(tip), Look::Button { label: String::new(), pic: Some(pic), style, enabled: true });
            }
            l.y += size + gap;
            stepper(l, "Unlocked", p.active, (LineChange::Active(false), LineChange::Active(true)), (p.active > 0, p.active < n), "Levels producing from the start; the Advancer unlocks the others");
            if n > 1 {
                l.button(Rect { x: x0, y: l.y, w: inner, h: bh }, Hit::Line(LineChange::Levels(false)), "Remove last level", Some(Pic::Glyph(Glyph::Minus)), Style::Plain, true, "");
                l.y += bh + gap;
            }
        }
        _ => {
            stepper(l, "Active", p.active, (LineChange::Active(false), LineChange::Active(true)), (p.active > 0, p.active < n), "Lines producing from the start (bright); the Advancer unlocks the others (dim)");
            let what = match family {
                Lines::Air => "air force",
                Lines::Walkers => "walker",
                _ => "unit",
            };
            // tall rows (the line's HUD icon, its name over the buttons) when they all fit, else one compact row each
            let add = if (n as usize) < MAX_LINES { bh + gap } else { 0 };
            let small = bh - 4;
            let room = (bottom - l.y - add) / n.max(1) - gap;
            let tall = room >= lh + small + 6;
            let pic = if tall { room.min(2 * bh - 6) } else { small };
            for (k, &(kind, rise)) in p.lines.iter().enumerate() {
                let on = (k as i32) < p.active;
                let name = line_label(kind, rise);
                let ground = family == Lines::Ground && (1..=15).contains(&kind);
                let tip = format!("{name}{}", if on { ": produces from the start" } else { ": the Advancer unlocks it" });
                l.push(Rect { x: x0, y: l.y, w: pic, h: pic }, None, Some(&tip), Look::Picture { pic: Pic::Slot(kind), dim: !on, frame: Some(if on { (90, 190, 110) } else { EDGE }) });
                let rx = x0 + pic + gap;
                let rw = x0 + inner - rx;
                if tall {
                    // the tiers are on their own button
                    let short = if ground { name.split(" T").next().unwrap_or(&name).to_string() } else { name.clone() };
                    l.text_at(rx, l.y, rw, &short, if on { (235, 235, 235) } else { DIM }, None);
                }
                let by = l.y + pic - small;
                let mut x = rx;
                if ground {
                    let t = (kind - 1) % 3 + 1;
                    let tiers = if rise > 0 { format!("T{t}-{}", (t + rise).min(3)) } else { format!("T{t}") };
                    let cw = l.font.measure("T1-3", 1) + 16;
                    l.button(Rect { x, y: by, w: cw, h: small }, Hit::Line(LineChange::Tiers(k)), &tiers, None, Style::Plain, true, "Start tier - highest tier research can raise it to. Click to change.");
                    x += cw + gap;
                }
                l.button(Rect { x, y: by, w: small, h: small }, Hit::Line(LineChange::Kind(k, false)), "", Some(Pic::Glyph(Glyph::Left)), Style::Plain, true, &format!("Previous {what}"));
                l.button(Rect { x: x + small + 4, y: by, w: small, h: small }, Hit::Line(LineChange::Kind(k, true)), "", Some(Pic::Glyph(Glyph::Right)), Style::Plain, true, &format!("Next {what}"));
                l.button(Rect { x: x0 + inner - small, y: by, w: small, h: small }, Hit::Line(LineChange::Remove(k)), "", Some(Pic::Glyph(Glyph::Cross)), Style::Danger, n > 1, "Remove this line");
                l.y += pic + gap;
            }
            if (n as usize) < MAX_LINES {
                l.button(Rect { x: x0, y: l.y, w: inner, h: bh }, Hit::Line(LineChange::Add), "Add line", Some(Pic::Glyph(Glyph::Plus)), Style::Ghost, true, "A new production line (five at most)");
                l.y += bh + gap;
            }
        }
    }
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: u32) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| ((x as u32 * t + y as u32 * (255 - t)) / 255) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn frame(buf: &mut [u32], w: usize, h: usize, r: Rect, c: (u8, u8, u8)) {
    rect(buf, w, h, r.x, r.y, r.x + r.w, r.y + 1, c, 255);
    rect(buf, w, h, r.x, r.y + r.h - 1, r.x + r.w, r.y + r.h, c, 255);
    rect(buf, w, h, r.x, r.y, r.x + 1, r.y + r.h, c, 255);
    rect(buf, w, h, r.x + r.w - 1, r.y, r.x + r.w, r.y + r.h, c, 255);
}

fn dashed(buf: &mut [u32], w: usize, h: usize, r: Rect, c: (u8, u8, u8)) {
    for x in (r.x..r.x + r.w).step_by(8) {
        rect(buf, w, h, x, r.y, (x + 4).min(r.x + r.w), r.y + 1, c, 255);
        rect(buf, w, h, x, r.y + r.h - 1, (x + 4).min(r.x + r.w), r.y + r.h, c, 255);
    }
    for y in (r.y..r.y + r.h).step_by(8) {
        rect(buf, w, h, r.x, y, r.x + 1, (y + 4).min(r.y + r.h), c, 255);
        rect(buf, w, h, r.x + r.w - 1, y, r.x + r.w, (y + 4).min(r.y + r.h), c, 255);
    }
}

/// A thick line from `a` to `b` (screen px).
fn line(buf: &mut [u32], w: usize, h: usize, a: (f32, f32), b: (f32, f32), t: i32, c: (u8, u8, u8)) {
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as i32).max(1);
    for k in 0..=steps {
        let f = k as f32 / steps as f32;
        let (x, y) = ((a.0 + (b.0 - a.0) * f).round() as i32, (a.1 + (b.1 - a.1) * f).round() as i32);
        rect(buf, w, h, x - t / 2, y - t / 2, x - t / 2 + t, y - t / 2 + t, c, 255);
    }
}

/// The drawn pictures, in a unit square scaled to `r`.
fn glyph(buf: &mut [u32], w: usize, h: usize, g: Glyph, r: Rect, c: (u8, u8, u8)) {
    let s = r.w.min(r.h) as f32;
    let (ox, oy) = (r.x as f32 + (r.w as f32 - s) / 2.0, r.y as f32 + (r.h as f32 - s) / 2.0);
    let t = ((s / 9.0).round() as i32).max(2);
    let mut poly = |pts: &[(f32, f32)]| {
        for k in 1..pts.len() {
            let (a, b) = (pts[k - 1], pts[k]);
            line(buf, w, h, (ox + a.0 * s, oy + a.1 * s), (ox + b.0 * s, oy + b.1 * s), t, c);
        }
    };
    let arc = |cx: f32, cy: f32, rr: f32, a0: f32, a1: f32| -> Vec<(f32, f32)> {
        (0..=14).map(|k| {
            let a = (a0 + (a1 - a0) * k as f32 / 14.0).to_radians();
            (cx + rr * a.cos(), cy + rr * a.sin())
        }).collect()
    };
    match g {
        Glyph::Plus => {
            poly(&[(0.2, 0.5), (0.8, 0.5)]);
            poly(&[(0.5, 0.2), (0.5, 0.8)]);
        }
        Glyph::Minus => poly(&[(0.2, 0.5), (0.8, 0.5)]),
        Glyph::Left => poly(&[(0.62, 0.2), (0.35, 0.5), (0.62, 0.8)]),
        Glyph::Right => poly(&[(0.38, 0.2), (0.65, 0.5), (0.38, 0.8)]),
        Glyph::Cross => {
            poly(&[(0.25, 0.25), (0.75, 0.75)]);
            poly(&[(0.75, 0.25), (0.25, 0.75)]);
        }
        Glyph::Undo | Glyph::Redo => {
            let m = |pts: Vec<(f32, f32)>| -> Vec<(f32, f32)> { if g == Glyph::Redo { pts.into_iter().map(|(x, y)| (1.0 - x, y)).collect() } else { pts } };
            poly(&m(arc(0.52, 0.55, 0.26, -90.0, 90.0)));
            poly(&m(vec![(0.52, 0.81), (0.3, 0.81)]));
            poly(&m(vec![(0.52, 0.29), (0.22, 0.29)]));
            poly(&m(vec![(0.36, 0.14), (0.2, 0.29), (0.36, 0.44)]));
        }
        Glyph::Revert => {
            // a circle open at the top-left with an arrowhead at its end: back round to the start
            let (cx, cy, rr) = (0.52, 0.54, 0.3);
            poly(&arc(cx, cy, rr, -150.0, 160.0));
            let a = (-150f32).to_radians();
            let tip = (cx + rr * a.cos(), cy + rr * a.sin());
            poly(&[(tip.0 + 0.02, tip.1 - 0.22), tip, (tip.0 + 0.22, tip.1 - 0.02)]);
        }
        Glyph::Save => {
            poly(&[(0.5, 0.12), (0.5, 0.6)]);
            poly(&[(0.3, 0.42), (0.5, 0.62), (0.7, 0.42)]);
            poly(&[(0.18, 0.6), (0.18, 0.84), (0.82, 0.84), (0.82, 0.6)]);
        }
        Glyph::Trash => {
            poly(&[(0.2, 0.28), (0.8, 0.28)]);
            poly(&[(0.4, 0.28), (0.4, 0.16), (0.6, 0.16), (0.6, 0.28)]);
            poly(&[(0.27, 0.34), (0.32, 0.86), (0.68, 0.86), (0.73, 0.34)]);
            poly(&[(0.43, 0.42), (0.43, 0.76)]);
            poly(&[(0.57, 0.42), (0.57, 0.76)]);
        }
    }
}

/// `s` cut with ".." so it fits `max_w` px.
fn fit(font: &Font, s: &str, max_w: i32) -> String {
    if font.measure(s, 1) <= max_w {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while !t.is_empty() && font.measure(&format!("{t}.."), 1) > max_w {
        t.pop();
    }
    format!("{t}..")
}

/// A sprite frame as it is (its tile included: the HUD's production icons are drawn that way in the game).
fn frame_icon(f: &Frame) -> Icon {
    let (w, h) = (f.width as usize, f.height as usize);
    let px = (0..w * h)
        .map(|i| {
            f.mask[i].then(|| {
                let [r, g, b] = rgb565_to_rgb8(f.pixels[i]);
                pack(r, g, b)
            })
        })
        .collect();
    Icon { w: w.max(1), h: h.max(1), px }
}

/// The picture of palette item `t` as `side` owns it: its sprites as the map draws them (building, or turret base and gun,
/// or unit chassis and top), cut to their extent.
fn compose(art: &mut Art, t: Thing, side: Side) -> Option<Icon> {
    let placed = match t {
        Thing::Object(k) => art.place(0, &template(k, (2, 2), side)),
        Thing::Unit(k) => art.place_marker(0, &Marker { kind: k, player: if side == Side::Green { 1 } else { 0 }, x: 80, y: 80 }, 12, None),
    };
    let frames: Vec<_> = placed.iter().filter_map(|p| p.set.get(p.frame).map(|f| (f, p.ax - f.hot_x as i32, p.ay - f.hot_y as i32))).collect();
    if frames.is_empty() {
        return None;
    }
    let (x0, y0) = (frames.iter().map(|f| f.1).min()?, frames.iter().map(|f| f.2).min()?);
    let (x1, y1) = (frames.iter().map(|f| f.1 + f.0.width as i32).max()?, frames.iter().map(|f| f.2 + f.0.height as i32).max()?);
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut px = vec![None; w * h];
    for (f, fx, fy) in frames {
        for y in 0..f.height as usize {
            for x in 0..f.width as usize {
                let i = y * f.width as usize + x;
                if f.mask[i] {
                    let [r, g, b] = rgb565_to_rgb8(f.pixels[i]);
                    px[(y + (fy - y0) as usize) * w + x + (fx - x0) as usize] = Some(pack(r, g, b));
                }
            }
        }
    }
    Some(Icon { w, h, px })
}

/// Draws `icon` scaled to fit `r` (aspect kept, centred), nearest neighbour; `dim` darkens it (a locked line).
fn draw_icon(buf: &mut [u32], w: usize, h: usize, icon: &Icon, r: Rect, dim: bool) {
    let k = (r.w as f32 / icon.w as f32).min(r.h as f32 / icon.h as f32);
    let (dw, dh) = ((icon.w as f32 * k) as i32, (icon.h as f32 * k) as i32);
    let (ox, oy) = (r.x + (r.w - dw) / 2, r.y + (r.h - dh) / 2);
    for y in 0..dh {
        for x in 0..dw {
            let (sx, sy) = (((x as f32 / k) as usize).min(icon.w - 1), ((y as f32 / k) as usize).min(icon.h - 1));
            if let Some(c) = icon.px[sy * icon.w + sx] {
                let (px, py) = (ox + x, oy + y);
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    buf[py as usize * w + px as usize] = if dim { blend(c, PANEL.0, PANEL.1, PANEL.2, 150) } else { c };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_keeps_most_of_the_window_for_the_map() {
        assert_eq!(panel_width(640, 480), 210);
        assert_eq!(panel_width(1280, 800), 281);
        assert_eq!(panel_width(1920, 1080), 460);
        assert!(panel_width(300, 300) <= 150);
    }

    fn ui_and_editor() -> (Ui, Editor) {
        use opensow_formats::level::{Level, Object, HEADER_WORDS};
        use opensow_formats::map::{Cell, Map};
        let glyph = || Frame { width: 6, height: 9, hot_x: 0, hot_y: 0, extra: [0; 4], pixels: vec![0; 54], mask: vec![true; 54], enhanced: Default::default() };
        let font = || Font::new((0..94).map(|_| glyph()).collect());
        let ui = Ui { fonts: (font(), font()), big: (font(), font()), tab: 2, production: false, icons: [Vec::new(), Vec::new(), Vec::new()], slots: Vec::new(), scroll: 0 };
        let mut raw = [0i32; 19];
        (raw[0], raw[1], raw[14], raw[16], raw[17]) = (123, 100, 0, 1, 1);
        let hq = |owner: i32, x: i32| Object { kind: 100, cell_x: x, cell_y: 1, raw: { let mut r = raw; (r[14], r[16]) = (owner, x); r } };
        let building = |kind: i32, x: i32, p: Production| {
            let mut o = hq(2, x);
            (o.kind, o.raw[1]) = (kind, kind);
            p.write(&mut o);
            o
        };
        let factory = building(102, 10, Production { active: 2, lines: vec![(7, 1), (13, 1), (4, 1), (1, 0), (10, 2)] });
        let mine = building(105, 20, Production { active: 1, lines: vec![(16, 0), (17, 0)] });
        let level = Level { version: 4, header: [0; HEADER_WORDS], objects: vec![hq(0, 1), hq(1, 30), factory, mine], markers: Vec::new() };
        let cells = (0..40 * 30).map(|i| Cell { x: (i % 40) as u8, y: (i / 40) as u8, blocked: false, border: false, no_build: false, no_marks: false }).collect();
        let mut ed = Editor::new("Test", level, &[], &Map { a: 0, b: 0, width: 40, height: 30, cells });
        ed.message = "A long message that wraps over more than one line of the narrow panel".into();
        (ui, ed)
    }

    fn check(ws: &[Widget], w: usize, h: usize, what: &str) {
        let x0 = (w - panel_width(w, h)) as i32;
        for wd in ws {
            assert!(wd.r.x >= x0 && wd.r.x + wd.r.w <= w as i32 && wd.r.y >= 0 && wd.r.y + wd.r.h <= h as i32, "{w}x{h} {what}: {:?} {:?} outside the panel", wd.r, wd.hit);
        }
        let clickable: Vec<&Widget> = ws.iter().filter(|wd| wd.hit.is_some()).collect();
        for (i, a) in clickable.iter().enumerate() {
            for b in &clickable[i + 1..] {
                let apart = a.r.x + a.r.w <= b.r.x || b.r.x + b.r.w <= a.r.x || a.r.y + a.r.h <= b.r.y || b.r.y + b.r.h <= a.r.y;
                assert!(apart, "{w}x{h} {what}: {:?} and {:?} overlap", a.hit, b.hit);
            }
        }
    }

    #[test]
    fn every_size_keeps_the_buttons_inside_the_panel_apart_and_every_palette_item_reachable() {
        let (mut ui, mut ed) = ui_and_editor();
        for (w, h) in [(640, 480), (800, 600), (1280, 800), (1920, 1080), (2560, 1440)] {
            // the production view of a factory with five lines and of a mine, then the palette
            ui.production = true;
            ed.sel = Some(Sel::Object(2));
            let ws = ui.widgets(w, h, &ed, "08-Island of Fate", true);
            assert!(ws.iter().any(|wd| wd.hit == Some(Hit::Line(LineChange::Tiers(4)))), "{w}x{h}: the fifth line has its buttons");
            check(&ws, w, h, "factory");
            ed.sel = Some(Sel::Object(3));
            let ws = ui.widgets(w, h, &ed, "08-Island of Fate", true);
            assert!(ws.iter().any(|wd| wd.hit == Some(Hit::Line(LineChange::LevelSlot(2)))), "{w}x{h}: the third level slot");
            check(&ws, w, h, "mine");
            ui.production = false;
            for tab in 0..TABS.len() {
                ui.tab = tab;
                let mut seen = std::collections::BTreeSet::new();
                for scroll in 0..12 {
                    ui.scroll = scroll;
                    let ws = ui.widgets(w, h, &ed, "08-Island of Fate", true);
                    check(&ws, w, h, "palette");
                    seen.extend(ws.iter().filter_map(|wd| match wd.hit {
                        Some(Hit::Tool(i)) => Some(i),
                        _ => None,
                    }));
                }
                assert_eq!(seen.into_iter().collect::<Vec<_>>(), TABS[tab].1.clone().collect::<Vec<_>>(), "{w}x{h} tab {tab}");
            }
        }
    }

    #[test]
    fn the_tabs_cover_the_whole_palette_once() {
        let all: Vec<usize> = TABS.iter().flat_map(|(_, r)| r.clone()).collect();
        assert_eq!(all, (0..palette().len()).collect::<Vec<_>>());
        assert!(TABS[0].1.clone().all(|i| matches!(palette()[i], Thing::Object(100..=108))));
        assert!(TABS[1].1.clone().all(|i| matches!(palette()[i], Thing::Object(25..=29))));
        assert!(TABS[2].1.clone().all(|i| matches!(palette()[i], Thing::Unit(_))));
        assert_eq!(palette()[14 + 7], Thing::Unit(7), "the units tab shows a cannon");
    }

    #[test]
    fn rect_contains_its_inside_only() {
        let r = Rect { x: 10, y: 20, w: 5, h: 4 };
        assert!(r.contains(10, 20) && r.contains(14, 23));
        assert!(!r.contains(15, 20) && !r.contains(10, 24) && !r.contains(9, 21));
    }
}
