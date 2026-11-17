//! `opensow-viewer [LEVEL] [--screenshot out.png] [--size WxH] [--zoom Z] [--center CX,CY] [--facing 0..31] [--aim 0..31] [--walk TICKS] [--fire TICK] [--shot x1,y1,x2,y2,type]... [--range CX,CY,MIN,MAX]... [--overlay blocked,grid,objects,markers,sprites,muzzles,minimap] [--pieces]`
//!
//! Keys: arrows/WASD or left-drag pan, wheel or +/- zoom, B blocked cells, G grid,
//! O objects (squares for things without art), M markers, X sprites, F muzzle points, [ ] turn all units, K play the walk cycle, V loop the fire/recoil sequences, N/P next/previous level, Home fit map, Esc quit.
//!
//! `opensow-viewer --edit [MAP] [--screenshot out.png [--size WxH] [--select CX,CY] [--tool NAME]]`: the skirmish map editor
//! (docs/map-editor.md) on a `.net` map (a name or its start; no name opens the first). Everything is on its sidebar (map
//! switch, save, undo/redo, revert, the selection's owner and delete, the palette of buildings and units to place); on the map a
//! click selects, a drag moves, and with a palette item armed a click places it (right click disarms). The edits are saved to
//! `data/skirmish-maps.txt`, which the game compiles in. Optional keys: Del, Esc, Ctrl+S / Ctrl+Z / Ctrl+Y.

use opensow_formats::level::Level;
use opensow_formats::maptweak::{self, Side, Tweaks};
use opensow_formats::GameDir;
use opensow_viewer::editor::{self, Editor, Sel};
use opensow_viewer::editor_ui::{panel_width, Hit, Ui};
use opensow_viewer::render::{object_at, render, Overlays, View};
use opensow_viewer::scene::Scene;
use opensow_viewer::screenshot::write_png;
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

struct Args {
    level: String,
    screenshot: Option<String>,
    size: (usize, usize),
    zoom: Option<f32>,
    center: Option<(f32, f32)>,
    facing: usize,
    aim: Option<usize>,
    walk: Option<u32>,
    fire: Option<u32>,
    shots: Vec<(i32, i32, i32, i32, u8)>,
    ranges: Vec<(i32, i32, usize, usize)>,
    overlays: Overlays,
    /// `--edit MAP`: the map editor on a `.net` map (empty = list the maps).
    edit: Option<String>,
    /// `--select CX,CY`: headless editor screenshots select what stands on that cell (or, with `--tool`, place there).
    select: Option<(i32, i32)>,
    /// `--tool NAME`: headless editor screenshots arm that palette item (`mine`, `turret2`, `unit cannon1`, ...).
    tool: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        level: "M01".into(),
        screenshot: None,
        size: (1280, 800),
        zoom: None,
        center: None,
        facing: 0,
        aim: None,
        walk: None,
        fire: None,
        shots: Vec::new(),
        ranges: Vec::new(),
        overlays: Overlays::default(),
        edit: None,
        select: None,
        tool: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--pieces" => {}
            "--screenshot" => a.screenshot = Some(value("a path")?),
            "--edit" => a.edit = Some(it.next().filter(|v| !v.starts_with("--")).unwrap_or_default()),
            "--tool" => a.tool = Some(value("a palette item")?),
            "--select" => {
                let v = value("CX,CY (cells)")?;
                let (x, y) = v.split_once(',').ok_or("--select expects CX,CY")?;
                a.select = Some((x.trim().parse().map_err(|_| "bad select x")?, y.trim().parse().map_err(|_| "bad select y")?));
            }
            "--zoom" => a.zoom = Some(value("a number")?.parse().map_err(|_| "bad --zoom")?),
            "--aim" => a.aim = Some(value("0..31")?.parse::<usize>().map_err(|_| "bad --aim")? % 32),
            "--walk" => a.walk = Some(value("a tick count")?.parse().map_err(|_| "bad --walk")?),
            "--shot" => {
                let v = value("x1,y1,x2,y2,type (map pixels)")?;
                let n: Vec<i32> = v.split(',').map(|t| t.trim().parse().map_err(|_| "bad --shot")).collect::<Result<_, _>>()?;
                if n.len() != 5 {
                    return Err("--shot expects x1,y1,x2,y2,type".into());
                }
                a.shots.push((n[0], n[1], n[2], n[3], n[4] as u8));
            }
            "--range" => {
                let v = value("CX,CY,MIN,MAX (cells, rings)")?;
                let n: Vec<i32> = v.split(',').map(|t| t.trim().parse().map_err(|_| "bad --range")).collect::<Result<_, _>>()?;
                if n.len() != 4 || n[2] < 0 || n[3] < n[2] || n[3] > 63 {
                    return Err("--range expects CX,CY,MIN,MAX with 0 <= MIN <= MAX <= 63".into());
                }
                a.ranges.push((n[0], n[1], n[2] as usize, n[3] as usize));
            }
            "--fire" => a.fire = Some(value("a tick (1 = first tick of the fire sequence)")?.parse().map_err(|_| "bad --fire")?),
            "--facing" => a.facing = value("0..31")?.parse::<usize>().map_err(|_| "bad --facing")? % 32,
            "--center" => {
                let v = value("CX,CY (cells)")?;
                let (x, y) = v.split_once(',').ok_or("--center expects CX,CY")?;
                a.center = Some((x.parse().map_err(|_| "bad center x")?, y.parse().map_err(|_| "bad center y")?));
            }
            "--size" => {
                let v = value("WxH")?;
                let (w, h) = v.split_once('x').ok_or("--size expects WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--overlay" => {
                let v = value("a list")?;
                a.overlays = Overlays {
                    blocked: false,
                    grid: false,
                    objects: false,
                    markers: false,
                    sprites: false,
                    muzzles: false,
                    walk: None,
                    fire: None,
                    foreground: false,
                    minimap: false,
                };
                for o in v.split(',').filter(|s| !s.is_empty()) {
                    match o {
                        "blocked" => a.overlays.blocked = true,
                        "grid" => a.overlays.grid = true,
                        "objects" => a.overlays.objects = true,
                        "markers" => a.overlays.markers = true,
                        "sprites" => {
                            a.overlays.sprites = true;
                            a.overlays.foreground = true;
                        }
                        "muzzles" => a.overlays.muzzles = true,
                        "minimap" => a.overlays.minimap = true,
                        _ => return Err(format!("unknown overlay {o}")),
                    }
                }
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => a.level = s.to_ascii_uppercase(),
        }
    }
    Ok(a)
}

fn fit(scene: &Scene, w: usize, h: usize) -> View {
    let mut v = View { x: 0.0, y: 0.0, zoom: (w as f32 / scene.bg_w as f32).min(h as f32 / scene.bg_h as f32) };
    v.clamp(scene, w, h);
    v
}

fn main() {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("error: {e}\n\n{}", include_str!("main.rs").lines().take(4).collect::<Vec<_>>().join("\n"));
        std::process::exit(2);
    });
    let game = GameDir::locate().unwrap_or_else(|| {
        eprintln!("error: game not found; set OPENSOW_GAME_DIR");
        std::process::exit(1);
    });
    if let Some(wanted) = &args.edit {
        return run_editor(game, wanted, &args);
    }
    // `--pieces`: what a map plan can place from this level (docs/generated-maps.md), then exit
    if std::env::args().any(|a| a == "--pieces") {
        let ground = opensow_viewer::scene::load_ground(&game, &args.level).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        });
        let warmonger = args.level.starts_with(['W', 'w']);
        let t = opensow_formats::custommap::Terrain { warmonger, mission: args.level[1..].parse().unwrap_or(0) };
        for (seed, (x, y, w, h), edges, n, soil) in opensow_viewer::layout::catalogue(t, &ground) {
            let e: Vec<&str> = ["top", "bottom", "left", "right"].iter().zip(edges).filter(|e| e.1).map(|e| *e.0).collect();
            let soil = soil.map_or("-".to_string(), |c| format!("{:.0},{:.0},{:.0}", c[0], c[1], c[2]));
            println!("{},{} box {x},{y} {w}x{h} cells {n} edges {} ground {soil}", seed.0, seed.1, if e.is_empty() { "-".into() } else { e.join("+") });
        }
        return;
    }
    let mut scene = Scene::load(&game, &args.level, args.facing, args.aim).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    scene.shots = args.shots.iter().filter_map(|&(x1, y1, x2, y2, t)| opensow_formats::projectile::build_path(t, x1, y1, x2, y2, 0)).collect();

    scene.ranges = args.ranges.clone();

    if let Some(path) = &args.screenshot {
        let (w, h) = args.size;
        let mut view = fit(&scene, w, h);
        if let Some(z) = args.zoom {
            view.zoom = z;
            view.clamp(&scene, w, h);
        }
        if let Some((cx, cy)) = args.center {
            view.x = (cx + 0.5) * 32.0 - w as f32 / view.zoom / 2.0;
            view.y = (cy + 0.5) * 32.0 - h as f32 / view.zoom / 2.0;
            view.clamp(&scene, w, h);
        }
        let mut buf = vec![0u32; w * h];
        let mut overlays = args.overlays;
        overlays.walk = args.walk;
        overlays.fire = args.fire.map(|t| t.saturating_sub(1));
        render(&scene, &view, overlays, &mut buf, w, h);
        write_png(path, &buf, w, h);
        println!("{} ({}x{} cells) -> {path}", scene.name, scene.map.width, scene.map.height);
        return;
    }

    let levels = Scene::list(&game);
    let idx = levels.iter().position(|l| *l == scene.name).unwrap_or(0);
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        game,
        levels,
        idx,
        scene,
        view: View { x: 0.0, y: 0.0, zoom: 1.0 },
        ov: args.overlays,
        window: None,
        surface: None,
        drag: None,
        cursor: PhysicalPosition::new(0.0, 0.0),
        needs_fit: true,
        autoplay: false,
        fire_play: false,
        ticks: 0,
        last_tick: std::time::Instant::now(),
        edit: None,
        mods: ModifiersState::empty(),
    };
    event_loop.run_app(&mut app).expect("run");
}

/// `data/skirmish-maps.txt` of this checkout (the game compiles it in).
fn tweaks_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/skirmish-maps.txt")
}

/// The map editor's state next to the viewer's: the editor, the install the map's terrain comes from, the sidebar and the drag.
struct EditSession {
    editor: Editor,
    game: GameDir,
    ui: Ui,
    /// Every `.net` map, for the sidebar's map switch.
    maps: Vec<String>,
    /// While dragging the selection: the grab offset (cursor cell - the selection's cell) and where it would land.
    grab: Option<((i32, i32), (i32, i32))>,
    quit_armed: bool,
}

fn net_maps(game: &GameDir) -> Vec<String> {
    let mut maps: Vec<String> = game.level_files().into_iter().filter(|f| f.to_lowercase().ends_with(".net")).map(|f| f[..f.len() - 4].to_string()).collect();
    maps.sort_by_key(|m| m.to_lowercase());
    maps.dedup_by_key(|m| m.to_lowercase());
    maps
}

fn open_editor(game: &GameDir, wanted: &str) -> Result<(Scene, EditSession), String> {
    let maps = net_maps(game);
    let low = wanted.to_lowercase();
    let map = maps.iter().find(|m| m.to_lowercase() == low).or_else(|| maps.iter().find(|m| m.to_lowercase().starts_with(&low)));
    let Some(map) = map.cloned() else {
        return Err(format!("no map \"{wanted}\"; skirmish maps: {}", maps.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(", ")));
    };
    let original = game.resolve(&format!("Data\\Levels\\{map}.net")).and_then(|p| std::fs::read(p).ok()).ok_or(format!("cannot read {map}.net"))?;
    let original = Level::parse(&original).map_err(|e| format!("{map}.net: {e}"))?;
    let text = std::fs::read_to_string(tweaks_path()).unwrap_or_default();
    let tweaks = Tweaks::parse(&text).map_err(|e| format!("{}: {e}", tweaks_path().display()))?;
    let (tg, mission) = game.net_terrain(&map, original.level_id());
    let mut scene = Scene::load_with(&tg, &mission, 0, None, Some(original.clone()))?;
    let editor = Editor::new(&map, original, tweaks.edits(&map), &scene.map);
    scene.set_level(&tg, editor.level().clone());
    scene.name = map;
    let mut art = opensow_viewer::art::Art::open(&tg).ok_or("cannot open sprites.data")?;
    let ui = Ui::new(&mut art);
    Ok((scene, EditSession { editor, game: tg, ui, maps, grab: None, quit_armed: false }))
}

/// Writes the editor's edits into the tweaks file, keeping the other maps' sections and the file's leading comments.
fn save_edits(editor: &Editor) -> Result<(), String> {
    let path = tweaks_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut tweaks = Tweaks::parse(&text).map_err(|e| e.to_string())?;
    tweaks.set(&editor.map, editor.edits());
    let head: String = text.lines().take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#')).map(|l| format!("{l}\n")).collect();
    let head = if head.trim().is_empty() { maptweak::HEADER.to_string() } else { head.trim_end().to_string() + "\n" };
    std::fs::write(&path, tweaks.to_text(&head)).map_err(|e| format!("{}: {e}", path.display()))
}

fn run_editor(game: GameDir, wanted: &str, args: &Args) {
    let (scene, mut session) = open_editor(&game, wanted).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if let Some(path) = &args.screenshot {
        let (w, h) = args.size;
        let mw = w - panel_width(w, h);
        let mut view = fit(&scene, mw, h);
        if let Some(z) = args.zoom {
            view.zoom = z;
            view.clamp(&scene, mw, h);
        }
        if let Some((cx, cy)) = args.center {
            view.x = (cx + 0.5) * 32.0 - mw as f32 / view.zoom / 2.0;
            view.y = (cy + 0.5) * 32.0 - h as f32 / view.zoom / 2.0;
            view.clamp(&scene, mw, h);
        }
        let ed = &mut session.editor;
        let tool = args.tool.as_ref().and_then(|t| editor::palette().iter().position(|&p| editor::thing_name(p).eq_ignore_ascii_case(t)));
        let place = match tool {
            Some(i) => {
                (ed.palette, ed.placing) = (i, true);
                session.ui.tab = opensow_viewer::editor_ui::TABS.iter().position(|(_, r)| r.contains(&i)).unwrap_or(0);
                args.select
            }
            None => {
                ed.sel = args.select.and_then(|(x, y)| ed.pick((x as f32 + 0.5) * 32.0, (y as f32 + 0.5) * 32.0));
                session.ui.production = ed.lines().is_some();
                None
            }
        };
        let mut buf = vec![0u32; w * h];
        draw_editor(&scene, &view, args.overlays, &session, &mut buf, w, h, None, None, place, (-1, -1));
        write_png(path, &buf, w, h);
        println!("{} ({}x{} cells, {} edits) -> {path}", scene.name, scene.map.width, scene.map.height, session.editor.edits().len());
        return;
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        game,
        levels: Vec::new(),
        idx: 0,
        scene,
        view: View { x: 0.0, y: 0.0, zoom: 1.0 },
        ov: args.overlays,
        window: None,
        surface: None,
        drag: None,
        cursor: PhysicalPosition::new(0.0, 0.0),
        needs_fit: true,
        autoplay: false,
        fire_play: false,
        ticks: 0,
        last_tick: std::time::Instant::now(),
        edit: Some(session),
        mods: ModifiersState::empty(),
    };
    event_loop.run_app(&mut app).expect("run");
}

/// One editor frame into `buf` (`w` x `h`): the map with the editor's marks on the left, the sidebar on the right.
fn draw_editor(
    scene: &Scene,
    view: &View,
    ov: Overlays,
    s: &EditSession,
    buf: &mut [u32],
    w: usize,
    h: usize,
    hover: Option<Sel>,
    drag: Option<(i32, i32)>,
    place: Option<(i32, i32)>,
    mouse: (i32, i32),
) {
    let mw = w - panel_width(w, h);
    let mut map = vec![0u32; mw * h];
    render(scene, view, ov, &mut map, mw, h);
    s.editor.draw_marks(&mut map, mw, h, view, hover, drag, place);
    for y in 0..h {
        buf[y * w..y * w + mw].copy_from_slice(&map[y * mw..(y + 1) * mw]);
    }
    s.ui.draw(buf, w, h, &s.editor, &s.editor.map, s.maps.len() > 1, mouse);
}

struct App {
    game: GameDir,
    levels: Vec<String>,
    idx: usize,
    scene: Scene,
    view: View,
    ov: Overlays,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    drag: Option<PhysicalPosition<f64>>,
    cursor: PhysicalPosition<f64>,
    needs_fit: bool,
    autoplay: bool,
    fire_play: bool,
    ticks: u32,
    last_tick: std::time::Instant,
    /// The map editor (`--edit`).
    edit: Option<EditSession>,
    mods: ModifiersState,
}

impl App {
    fn window_size(&self) -> (usize, usize) {
        self.window.as_ref().map(|w| w.inner_size()).map_or((1, 1), |s| (s.width.max(1) as usize, s.height.max(1) as usize))
    }

    /// The map's part of the window (the editor's sidebar takes the right).
    fn size(&self) -> (usize, usize) {
        let (w, h) = self.window_size();
        match &self.edit {
            Some(_) => ((w - panel_width(w, h)).max(1), h),
            None => (w, h),
        }
    }

    fn in_panel(&self) -> bool {
        self.edit.is_some() && self.cursor.x as usize >= self.size().0
    }

    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn switch_level(&mut self, step: isize) {
        if self.levels.is_empty() {
            return;
        }
        let n = self.levels.len() as isize;
        let mut i = self.idx as isize;
        for _ in 0..n {
            i = (i + step).rem_euclid(n);
            match Scene::load(&self.game, &self.levels[i as usize], self.scene.facing, self.scene.aim) {
                Ok(s) => {
                    self.idx = i as usize;
                    self.scene = s;
                    self.needs_fit = true;
                    if let Some(w) = &self.window {
                        w.set_title(&format!("OpenSOW - {}", self.scene.name));
                    }
                    return;
                }
                Err(e) => eprintln!("skipping {}: {e}", self.levels[i as usize]),
            }
        }
    }

    fn map_cursor(&self) -> (f32, f32) {
        (self.view.x + self.cursor.x as f32 / self.view.zoom, self.view.y + self.cursor.y as f32 / self.view.zoom)
    }

    fn cursor_cell(&self) -> (i32, i32) {
        let (mx, my) = self.map_cursor();
        ((mx / 32.0).floor() as i32, (my / 32.0).floor() as i32)
    }

    fn hover(&self) -> Option<Sel> {
        let (mx, my) = self.map_cursor();
        self.edit.as_ref().and_then(|e| e.editor.pick(mx, my))
    }

    /// After an edit: the scene shows the edited level, the title its save state.
    fn edited(&mut self) {
        if let Some(e) = &mut self.edit {
            e.quit_armed = false;
            let level = e.editor.level().clone();
            self.scene.set_level(&e.game, level);
        }
        self.update_title();
        self.redraw();
    }

    /// A sidebar button.
    fn ui_click(&mut self, hit: Hit) {
        let Some(e) = self.edit.as_mut() else { return };
        let ed = &mut e.editor;
        let changed = match hit {
            Hit::MapPrev | Hit::MapNext => return self.switch_map(if hit == Hit::MapNext { 1 } else { -1 }),
            Hit::Save => {
                match save_edits(ed) {
                    Ok(()) => {
                        ed.mark_saved();
                        ed.message = "Saved to data/skirmish-maps.txt; the game uses it from its next build".into();
                    }
                    Err(err) => ed.message = format!("Save failed: {err}"),
                }
                false
            }
            Hit::Undo => ed.undo(),
            Hit::Redo => ed.redo(),
            Hit::Revert => ed.revert(),
            Hit::SelectTool => {
                ed.placing = false;
                false
            }
            Hit::Tool(i) => {
                // a second click on the armed picture disarms it; units and headquarters are never neutral
                ed.placing = !(ed.placing && ed.palette == i);
                ed.palette = i;
                let thing = editor::palette()[i];
                if ed.side == Side::Neutral && (matches!(thing, maptweak::Thing::Unit(_)) || thing == maptweak::Thing::Object(100)) {
                    ed.side = Side::Blue;
                }
                ed.message.clear();
                false
            }
            Hit::PlaceSide(sd) => {
                ed.side = sd;
                false
            }
            Hit::SelOwner(sd) => ed.set_owner(sd),
            Hit::Delete => ed.remove_sel(),
            Hit::ScrollUp => {
                e.ui.scroll = e.ui.scroll.saturating_sub(1);
                false
            }
            Hit::ScrollDown => {
                e.ui.scroll += 1;
                false
            }
            Hit::Tab(t) => {
                (e.ui.tab, e.ui.scroll) = (t, 0);
                false
            }
            Hit::Lower(production) => {
                e.ui.production = production;
                false
            }
            Hit::Line(c) => ed.change_lines(c),
        };
        if changed {
            self.edited();
        } else {
            self.update_title();
            self.redraw();
        }
    }

    /// The sidebar's map switch: the next / previous `.net` map (refused while there are unsaved edits).
    fn switch_map(&mut self, step: isize) {
        let Some(e) = self.edit.as_mut() else { return };
        if e.editor.dirty() {
            e.editor.message = "Save or revert this map before opening another".into();
            self.redraw();
            return;
        }
        let n = e.maps.len() as isize;
        let i = e.maps.iter().position(|m| *m == e.editor.map).unwrap_or(0) as isize;
        let next = e.maps[(i + step).rem_euclid(n) as usize].clone();
        match open_editor(&self.game, &next) {
            Ok((scene, session)) => {
                self.scene = scene;
                self.edit = Some(session);
                self.needs_fit = true;
            }
            Err(err) => e.editor.message = err,
        }
        self.update_title();
        self.redraw();
    }

    /// Optional editor keys (everything also has a button); `true` when the key was the editor's.
    fn edit_key(&mut self, key: &Key) -> bool {
        let ctrl = self.mods.control_key() || self.mods.super_key();
        let shift = self.mods.shift_key();
        let hit = match key {
            Key::Named(NamedKey::Delete | NamedKey::Backspace) => Hit::Delete,
            Key::Named(NamedKey::Escape) => {
                if let Some(e) = self.edit.as_mut() {
                    if e.editor.placing {
                        e.editor.placing = false;
                    } else {
                        e.editor.sel = None;
                    }
                }
                self.redraw();
                return true;
            }
            Key::Character(c) => match (c.to_ascii_lowercase().as_str(), ctrl) {
                ("s", true) => Hit::Save,
                ("z", true) if shift => Hit::Redo,
                ("z", true) => Hit::Undo,
                ("y", true) => Hit::Redo,
                // the viewer's level keys would leave the map
                ("n", false) | ("p", false) | ("q", false) => return true,
                _ => return false,
            },
            _ => return false,
        };
        self.ui_click(hit);
        true
    }

    fn update_title(&self) {
        let Some(w) = &self.window else { return };
        if let Some(e) = &self.edit {
            w.set_title(&format!("OpenSOW map editor - {}{}", e.editor.map, if e.editor.dirty() { " *" } else { "" }));
            return;
        }
        let (mx, my) = (self.view.x + self.cursor.x as f32 / self.view.zoom, self.view.y + self.cursor.y as f32 / self.view.zoom);
        let mut t = format!("OpenSOW - {}  cell ({},{})", self.scene.name, (mx / 32.0).floor() as i32, (my / 32.0).floor() as i32);
        if let Some(l) = &self.scene.level {
            let stock: Vec<String> =
                l.air_stock().iter().filter(|s| s.counts.iter().any(|&c| c != 0)).map(|s| format!("{} {:?}", s.force.name(), s.counts)).collect();
            if !stock.is_empty() {
                t += &format!("  |  air: {}", stock.join(", "));
            }
        }
        if let Some(o) = object_at(&self.scene, mx, my) {
            t += &format!("  |  {} {:?} hp {} ({},{})", o.object_kind().name(), o.owner(), o.hp(), o.cell_x, o.cell_y);
        }
        w.set_title(&t);
    }

    fn pan(&mut self, dx: f32, dy: f32) {
        let (w, h) = self.size();
        self.view.x += dx / self.view.zoom;
        self.view.y += dy / self.view.zoom;
        self.view.clamp(&self.scene, w, h);
        self.redraw();
    }

    fn zoom_at(&mut self, factor: f32, sx: f32, sy: f32) {
        let (w, h) = self.size();
        let (mx, my) = (self.view.x + sx / self.view.zoom, self.view.y + sy / self.view.zoom);
        self.view.zoom *= factor;
        self.view.zoom = self.view.zoom.clamp(0.1, 4.0);
        self.view.x = mx - sx / self.view.zoom;
        self.view.y = my - sy / self.view.zoom;
        self.view.clamp(&self.scene, w, h);
        self.redraw();
    }
}

/// Game ticks per second used for the walk preview. The real tick length was not traced; this is an assumption.
const PREVIEW_TICKS_PER_SECOND: u32 = 20;

impl ApplicationHandler for App {
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if !self.autoplay && !self.fire_play {
            el.set_control_flow(winit::event_loop::ControlFlow::Wait);
            return;
        }
        let tick = std::time::Duration::from_millis(1000 / PREVIEW_TICKS_PER_SECOND as u64);
        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick) >= tick {
            self.last_tick = now;
            self.ticks = self.ticks.wrapping_add(1);
            if self.autoplay {
                self.ov.walk = Some(self.ticks);
            }
            if self.fire_play {
                self.ov.fire = Some(self.ticks);
            }
            self.redraw();
        }
        el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(self.last_tick + tick));
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title(format!("OpenSOW - {}", self.scene.name)).with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(el.create_window(attrs).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(e) = self.edit.as_mut().filter(|e| e.editor.dirty() && !e.quit_armed) {
                    e.quit_armed = true;
                    e.editor.message = "Unsaved changes: Save keeps them, closing again discards them".into();
                    self.redraw();
                } else {
                    el.exit();
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Resized(_) => {
                self.needs_fit |= self.view.zoom == 1.0 && self.view.x == 0.0;
                self.redraw();
            }
            WindowEvent::RedrawRequested => {
                let (w, h) = self.size();
                let full_w = self.window_size().0;
                if self.needs_fit {
                    self.view = fit(&self.scene, w, h);
                    self.needs_fit = false;
                }
                self.view.clamp(&self.scene, w, h);
                let (hover, cell) = (self.hover(), self.cursor_cell());
                let Some(surface) = self.surface.as_mut() else { return };
                if surface.resize(NonZeroU32::new(full_w as u32).unwrap(), NonZeroU32::new(h as u32).unwrap()).is_err() {
                    return;
                }
                if let Ok(mut buf) = surface.buffer_mut() {
                    match &self.edit {
                        Some(e) => {
                            let in_map = (self.cursor.x as usize) < w;
                            let place = (e.editor.placing && in_map && e.grab.is_none()).then_some(cell);
                            let mouse = (self.cursor.x as i32, self.cursor.y as i32);
                            draw_editor(
                                &self.scene,
                                &self.view,
                                self.ov,
                                e,
                                &mut buf,
                                full_w,
                                h,
                                hover.filter(|_| in_map),
                                e.grab.map(|(_, at)| at),
                                place,
                                mouse,
                            );
                        }
                        None => render(&self.scene, &self.view, self.ov, &mut buf, w, h),
                    }
                    let _ = buf.present();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } if self.in_panel() => {
                let (w, h) = self.window_size();
                let (x, y) = (self.cursor.x as i32, self.cursor.y as i32);
                let e = self.edit.as_ref().unwrap();
                if let Some(hit) = e.ui.hit(w, h, &e.editor, &e.editor.map, e.maps.len() > 1, x, y) {
                    self.ui_click(hit);
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if self.edit.is_some() => {
                let (hover, cell) = (self.hover(), self.cursor_cell());
                let e = self.edit.as_mut().unwrap();
                if state == ElementState::Pressed && e.editor.placing {
                    let at = Editor::add_cell(editor::palette()[e.editor.palette], cell);
                    if e.editor.add(at) {
                        self.edited();
                    } else {
                        self.redraw();
                    }
                } else if state == ElementState::Pressed {
                    e.editor.sel = hover;
                    // a building with production lines opens them; anything else shows the palette
                    e.ui.production = e.editor.lines().is_some();
                    e.grab = hover.map(|s| {
                        let at = e.editor.cell_of(s);
                        ((cell.0 - at.0, cell.1 - at.1), at)
                    });
                    self.drag = hover.is_none().then_some(self.cursor);
                    self.redraw();
                } else {
                    self.drag = None;
                    if let Some((_, at)) = e.grab.take() {
                        if e.editor.move_sel(at) {
                            self.edited();
                        }
                    }
                    self.redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.edit.is_some() => {
                let e = self.edit.as_mut().unwrap();
                if e.editor.placing {
                    e.editor.placing = false;
                } else {
                    e.editor.sel = None;
                }
                self.redraw();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.drag = (state == ElementState::Pressed).then_some(self.cursor);
            }
            WindowEvent::CursorMoved { position, .. } if self.edit.is_some() => {
                let before = (self.hover(), self.cursor_cell());
                if let Some(last) = self.drag {
                    self.pan((last.x - position.x) as f32, (last.y - position.y) as f32);
                    self.drag = Some(position);
                }
                self.cursor = position;
                let cell = self.cursor_cell();
                if let Some(e) = self.edit.as_mut() {
                    if let Some((off, at)) = e.grab.as_mut() {
                        *at = (cell.0 - off.0, cell.1 - off.1);
                    }
                }
                // hover highlights on the map and the sidebar, the drag and placing ghosts follow the mouse
                if before != (self.hover(), cell) || self.in_panel() || self.edit.as_ref().is_some_and(|e| e.grab.is_some() || e.editor.placing) {
                    self.redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(last) = self.drag {
                    self.pan((last.x - position.x) as f32, (last.y - position.y) as f32);
                    self.drag = Some(position);
                }
                self.cursor = position;
                self.update_title();
            }
            WindowEvent::MouseWheel { delta, .. } if self.in_panel() => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if dy != 0.0 {
                    self.ui_click(if dy > 0.0 { Hit::ScrollUp } else { Hit::ScrollDown });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.zoom_at(1.15f32.powf(dy), self.cursor.x as f32, self.cursor.y as f32);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if self.edit.is_some() && self.edit_key(&event.key_without_modifiers()) {
                    return;
                }
                let (w, h) = self.size();
                let step = 96.0;
                match event.key_without_modifiers() {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::ArrowLeft) => self.pan(-step, 0.0),
                    Key::Named(NamedKey::ArrowRight) => self.pan(step, 0.0),
                    Key::Named(NamedKey::ArrowUp) => self.pan(0.0, -step),
                    Key::Named(NamedKey::ArrowDown) => self.pan(0.0, step),
                    Key::Named(NamedKey::Home) => {
                        self.needs_fit = true;
                        self.redraw();
                    }
                    Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                        "q" => el.exit(),
                        "a" => self.pan(-step, 0.0),
                        "d" => self.pan(step, 0.0),
                        "w" => self.pan(0.0, -step),
                        "s" => self.pan(0.0, step),
                        "+" | "=" => self.zoom_at(1.25, w as f32 / 2.0, h as f32 / 2.0),
                        "-" => self.zoom_at(0.8, w as f32 / 2.0, h as f32 / 2.0),
                        "b" => {
                            self.ov.blocked ^= true;
                            self.redraw();
                        }
                        "g" => {
                            self.ov.grid ^= true;
                            self.redraw();
                        }
                        "o" => {
                            self.ov.objects ^= true;
                            self.redraw();
                        }
                        "m" => {
                            self.ov.markers ^= true;
                            self.redraw();
                        }
                        "n" => {
                            self.ov.minimap ^= true;
                            self.redraw();
                        }
                        "[" | "]" => {
                            let f = self.scene.facing;
                            let next = if c.as_str() == "]" { (f + 1) % 32 } else { (f + 31) % 32 };
                            self.scene.set_facing(&self.game, next);
                            if let Some(w) = &self.window {
                                w.set_title(&format!("OpenSOW - {}  facing {next}", self.scene.name));
                            }
                            self.redraw();
                        }
                        "v" => {
                            self.fire_play ^= true;
                            self.ticks = 0;
                            self.ov.fire = self.fire_play.then_some(0);
                            self.last_tick = std::time::Instant::now();
                            self.redraw();
                        }
                        "k" => {
                            self.autoplay ^= true;
                            self.ticks = 0;
                            self.ov.walk = self.autoplay.then_some(0);
                            self.last_tick = std::time::Instant::now();
                            self.redraw();
                        }
                        "f" => {
                            self.ov.muzzles ^= true;
                            self.redraw();
                        }
                        "x" => {
                            self.ov.sprites ^= true;
                            self.redraw();
                        }
                        "p" => {
                            self.switch_level(-1);
                            self.redraw();
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
