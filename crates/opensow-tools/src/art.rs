//! `opensow export` / `opensow import`: all colour art as editable PNG/JPG files and back (docs/art-import.md).
//!
//! The export is ONE flat directory: a PNG per sprite frame (`units__1a_blue__0008.png`), a JPG per background
//! (`levels__m01.jpg`) and `manifest.csv`. The import re-encodes whatever PNG/JPG files are present (missing ones keep
//! the original art) into a `Data` overlay that can be copied over the installation; files that come out identical
//! to the install are not written.

use crate::{walk, Res};
use opensow_formats::{megafile, sprite, srf, GameDir};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

const WM_DIR: &str = "State of War Warmonger";

/// Where an art file lives: which game (`""` base, `"wm__"` Warmonger), inside `sprites.data` or loose, and its path under `Data`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    prefix: String,
    archive: bool,
    rel: String,
}

impl Target {
    fn encode(&self) -> String {
        format!("{}:{}:{}", self.prefix, if self.archive { 'a' } else { 'f' }, self.rel)
    }
    fn decode(s: &str) -> Option<Target> {
        let mut p = s.splitn(3, ':');
        let (prefix, kind, rel) = (p.next()?, p.next()?, p.next()?);
        Some(Target { prefix: prefix.into(), archive: kind == "a", rel: rel.into() })
    }
    fn root(&self, game: &GameDir) -> Option<PathBuf> {
        match self.prefix.as_str() {
            "" => Some(game.root.clone()),
            _ => game.root.parent().map(|p| p.join(WM_DIR)).filter(|p| p.is_dir()),
        }
    }
    /// Current bytes of this file in the install.
    fn read(&self, game: &GameDir) -> Res<Vec<u8>> {
        let root = self.root(game).ok_or("game folder missing")?;
        if self.archive {
            let m = megafile::MegaFile::open(&root.join("Data/sprites.info"), &root.join("Data/sprites.data"))?;
            let e = m.find(&self.rel).ok_or_else(|| format!("{} not in sprites.data", self.rel))?;
            Ok(m.read(e)?)
        } else {
            let g = GameDir::new(root);
            let p = g.resolve(&format!("Data/{}", self.rel)).ok_or_else(|| format!("{} not found", self.rel))?;
            Ok(std::fs::read(p)?)
        }
    }
}

struct Item {
    targets: Vec<Target>,
    bytes: Vec<u8>,
}

/// Every colour sprite and background of both games, identical files merged into one item with several targets.
fn collect(game: &GameDir) -> Res<Vec<Item>> {
    let mut raw: Vec<(Target, Vec<u8>)> = Vec::new();
    let mut roots = vec![("".to_string(), game.root.clone())];
    if let Some(wm) = game.root.parent().map(|p| p.join(WM_DIR)).filter(|p| p.is_dir()) {
        roots.push(("wm__".into(), wm));
    }
    for (prefix, root) in &roots {
        if let Ok(m) = megafile::MegaFile::open(&root.join("Data/sprites.info"), &root.join("Data/sprites.data")) {
            for e in &m.entries {
                raw.push((Target { prefix: prefix.clone(), archive: true, rel: e.name.replace('\\', "/") }, m.read(e)?));
            }
        }
        let mut files = Vec::new();
        walk(&root.join("Data"), &mut files);
        files.sort();
        for p in files {
            let rel = p.strip_prefix(root.join("Data"))?.to_string_lossy().replace('\\', "/");
            let low = rel.to_ascii_lowercase();
            if low.starts_with("diffdata/") || low.starts_with("sounds/") {
                continue; // other-language menu variants / audio
            }
            let ext = low.rsplit('.').next().unwrap_or("");
            if matches!(ext, "ps6" | "tsp") || (ext == "srf" && (low.starts_with("pictures/") || low.starts_with("levels/"))) {
                raw.push((Target { prefix: prefix.clone(), archive: false, rel }, std::fs::read(&p)?));
            }
        }
    }
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for (t, bytes) in raw {
        let ext = t.rel.to_ascii_lowercase().rsplit('.').next().unwrap_or("").to_string();
        if !matches!(ext.as_str(), "ps6" | "tsp" | "srf") {
            continue; // po1/po2/msk/sha...
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&ext, &bytes).hash(&mut h);
        match seen.get(&h.finish()) {
            Some(&i) if items[i].bytes == bytes => items[i].targets.push(t),
            _ => {
                seen.insert(h.finish(), items.len());
                items.push(Item { targets: vec![t], bytes });
            }
        }
    }
    Ok(items)
}

fn stem_of(t: &Target) -> String {
    let low = t.rel.to_ascii_lowercase();
    format!("{}{}", t.prefix, low.rsplit_once('.').map_or(low.as_str(), |x| x.0).replace('/', "__"))
}

pub fn export(game: &GameDir, out: &Path) -> Res<()> {
    std::fs::create_dir_all(out)?;
    let mut manifest = std::io::BufWriter::new(std::fs::File::create(out.join("manifest.csv"))?);
    writeln!(manifest, "file,frame,width,height,hot_x,hot_y,targets")?;
    let (mut sprites, mut frames_n, mut bgs, mut dup) = (0, 0, 0, 0);
    for item in collect(game)? {
        let t = &item.targets[0];
        let targets = item.targets.iter().map(Target::encode).collect::<Vec<_>>().join(";");
        let stem = stem_of(t);
        dup += item.targets.len() - 1;
        if t.rel.to_ascii_lowercase().ends_with(".srf") {
            std::fs::write(out.join(format!("{stem}.jpg")), srf::to_jpeg(&item.bytes)?)?;
            writeln!(manifest, "{stem}.jpg,,,,,,{targets}")?;
            bgs += 1;
            continue;
        }
        let Some(kind) = sprite::Kind::from_extension(Path::new(&t.rel)) else { continue };
        sprites += 1;
        for (i, f) in sprite::parse(&item.bytes, kind)?.iter().enumerate() {
            if f.width == 0 || f.height == 0 {
                continue;
            }
            let name = format!("{stem}__{i:04}.png");
            write_png(&out.join(&name), f.width as u32, f.height as u32, &f.to_rgba(None))?;
            writeln!(manifest, "{name},{i},{},{},{},{},{targets}", f.width, f.height, f.hot_x, f.hot_y)?;
            frames_n += 1;
        }
    }
    println!("{sprites} sprite files -> {frames_n} frame PNGs, {bgs} background JPGs ({dup} identical duplicates merged) in {}", out.display());
    Ok(())
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Res<()> {
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Decodes any PNG to RGBA8.
fn read_png(path: &Path) -> Res<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::fs::File::open(path)?);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf)?;
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    Ok((info.width, info.height, rgba))
}

/// RGB565 with rounding (exact inverse of the exporter's expansion, so untouched frames survive bit for bit).
fn to_565(r: u8, g: u8, b: u8) -> u16 {
    let q = |v: u8, max: u32| ((v as u32 * max + 127) / 255) as u16;
    q(r, 31) << 11 | q(g, 63) << 5 | q(b, 31)
}

fn frame_from_png(path: &Path, old: &sprite::Frame, alpha: u8) -> Res<sprite::Frame> {
    let (w, h, rgba) = read_png(path)?;
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err(format!("{}: image too large", path.display()).into());
    }
    let scale = |hot: i16, new: u32, old: u16| if old == 0 { hot } else { (hot as f64 * new as f64 / old as f64).round() as i16 };
    let (mut pixels, mut mask) = (Vec::with_capacity(rgba.len() / 4), Vec::with_capacity(rgba.len() / 4));
    for p in rgba.chunks(4) {
        let on = p[3] >= alpha;
        mask.push(on);
        pixels.push(if on { to_565(p[0], p[1], p[2]) } else { 0 });
    }
    Ok(sprite::Frame {
        width: w as u16,
        height: h as u16,
        hot_x: scale(old.hot_x, w, old.width),
        hot_y: scale(old.hot_y, h, old.height),
        extra: old.extra,
        pixels,
        mask,
        enhanced: Default::default(),
    })
}

fn jpeg_from_png(path: &Path, quality: u8) -> Res<Vec<u8>> {
    let (w, h, rgba) = read_png(path)?;
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err(format!("{}: image too large", path.display()).into());
    }
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    // the originals are baseline 4:4:4
    let mut enc = jpeg_encoder::Encoder::new(&mut out, quality);
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
    enc.encode(&rgb, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)?;
    Ok(out)
}

pub fn import(game: &GameDir, src: &Path, out: &Path, quality: u8, alpha: u8) -> Res<()> {
    let text = std::fs::read_to_string(src.join("manifest.csv")).map_err(|e| format!("{}/manifest.csv: {e}", src.display()))?;
    // one entry per original file: its targets and the edited images present for it
    let mut entries: BTreeMap<String, (Vec<Target>, Vec<(usize, String)>, Option<String>)> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let c: Vec<&str> = line.splitn(7, ',').collect();
        if c.len() < 7 {
            continue;
        }
        let e = entries.entry(c[6].to_string()).or_insert_with(|| (c[6].split(';').filter_map(Target::decode).collect(), Vec::new(), None));
        if c[0].ends_with(".jpg") {
            e.2 = Some(c[0].trim_end_matches(".jpg").to_string());
        } else if let Ok(i) = c[1].parse() {
            e.1.push((i, c[0].to_string()));
        }
    }

    let mut archive_new: BTreeMap<String, HashMap<String, Vec<u8>>> = BTreeMap::new(); // prefix -> lowercase name -> bytes
    let (mut changed_files, mut changed_frames, mut unchanged) = (0, 0, 0);
    for (targets, frames, bg) in entries.values() {
        let Some(primary) = targets.first() else { continue };
        let old_bytes = primary.read(game)?;
        let new_bytes = if let Some(stem) = bg {
            let (png, jpg) = (src.join(format!("{stem}.png")), src.join(format!("{stem}.jpg")));
            let jpeg = if png.exists() {
                jpeg_from_png(&png, quality)?
            } else if jpg.exists() {
                std::fs::read(&jpg)?
            } else {
                continue;
            };
            srf::from_jpeg(&old_bytes, &jpeg).map_err(|e| format!("{stem}: {e}"))?
        } else {
            let kind = sprite::Kind::from_extension(Path::new(&primary.rel)).ok_or("unknown sprite extension")?;
            let mut fr = sprite::parse(&old_bytes, kind)?;
            let mut n = 0;
            for (i, name) in frames {
                let path = src.join(name);
                if path.exists() && *i < fr.len() {
                    fr[*i] = frame_from_png(&path, &fr[*i], alpha).map_err(|e| format!("{name}: {e}"))?;
                    n += 1;
                }
            }
            if n == 0 {
                continue;
            }
            changed_frames += n;
            sprite::encode(&fr, kind)
        };
        if new_bytes == old_bytes {
            unchanged += 1;
            continue;
        }
        changed_files += 1;
        for t in targets {
            if t.archive {
                archive_new.entry(t.prefix.clone()).or_default().insert(t.rel.replace('/', "\\").to_ascii_lowercase(), new_bytes.clone());
            } else {
                let folder = if t.prefix.is_empty() { "State of War" } else { WM_DIR };
                let dst = t.rel.split('/').fold(out.join(folder).join("Data"), |p, s| p.join(s));
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::write(dst, &new_bytes)?;
            }
        }
    }

    // repack the archives that changed: every original entry in order, edited ones replaced
    let prefixes: HashSet<&String> = archive_new.keys().collect();
    for prefix in prefixes {
        let t = Target { prefix: prefix.clone(), archive: true, rel: String::new() };
        let root = t.root(game).ok_or("game folder missing")?;
        let m = megafile::MegaFile::open(&root.join("Data/sprites.info"), &root.join("Data/sprites.data"))?;
        let mut files = Vec::new();
        for e in &m.entries {
            let key = e.name.replace('/', "\\").to_ascii_lowercase();
            files.push((e.name.clone(), archive_new[prefix].get(&key).cloned().map_or_else(|| m.read(e), Ok)?));
        }
        let (info, data) = megafile::build(&files);
        let folder = if prefix.is_empty() { "State of War" } else { WM_DIR };
        let dir = out.join(folder).join("Data");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("sprites.info"), info)?;
        std::fs::write(dir.join("sprites.data"), data)?;
        println!("repacked {}", dir.join("sprites.data").display());
    }
    println!("{changed_files} files changed ({changed_frames} frames read from PNGs), {unchanged} identical to the install; overlay in {}", out.display());
    println!("Back up the game folder, then copy the overlay over it (e.g. `cp -R \"{}/State of War/Data\" <game>/`).", out.display());
    Ok(())
}
