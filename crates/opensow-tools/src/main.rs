//! `opensow` - inspect and extract assets from a State of War installation.

use opensow_formats::{aim, font, lang, level, projectile, recoil, map, megafile::MegaFile, sprite, srf, GameDir};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
opensow <command> [args]

  where                              show the detected game directory
  check                              parse every known asset and report failures
  unpack <out_dir>                   extract sprites.data into <out_dir>
  srf <file.srf|.srfa> <out.jpg>     restore a JPEG from an SRF image
  sprite <file> <out.png> [cols]     render all frames of a .ps6/.tsp/.sha as a sheet
  text <font> <text> <out.png> [fg] [outline]   render text; font = small|medium|big|msg (tinted, hex colours)
                                     or font02_dark|font02_light|font_msg2|font_briefing_big|font_briefing_small
  lang <EN|SK|CZ|PL|HU|RU|CS|CT> [index]   print the language's strings (damaged ones are flagged)
  path <type> x1 y1 x2 y2 delay      print a bullet's flight path points (x y lift), one per line
  launch <code> <tx> <ty> <side> <cells> <counter>   fields of a launched aircraft (code 30..=34)
  squad <0..17>                      units a carryall lands: dx dy kind per line
  rings                              print the range index table and the ring offset list (dx dy ...)
  weapons                            table of every weapon: range, reload, volley, projectile, speed, strength
  hover                              the Advancer's hover bob table (0x89cf58)
  frames <sprite file>               list every frame's size and hotspot
  level <file.edt|.net>              print header values, objects and markers of a level script
  map <Mxx.map> <out.png>            render blocked/border/flag cells as a 1px-per-cell image

The game is found via $OPENSOW_GAME_DIR or the default Steam depot path.";

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn game() -> Res<GameDir> {
    GameDir::locate().ok_or_else(|| "game not found; set OPENSOW_GAME_DIR".into())
}

fn open_megafile(game: &GameDir) -> Res<MegaFile> {
    Ok(MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data"))?)
}

fn run(args: &[String]) -> Res<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["where"] => println!("{}", game()?.root.display()),
        ["check"] => check(&game()?)?,
        ["unpack", out] => unpack(&game()?, Path::new(out))?,
        ["srf", src, dst] => std::fs::write(dst, srf::to_jpeg(&std::fs::read(src)?)?)?,
        ["sprite", src, dst] => sprite_sheet(Path::new(src), Path::new(dst), None)?,
        ["sprite", src, dst, cols] => sprite_sheet(Path::new(src), Path::new(dst), Some(cols.parse()?))?,
        ["text", name, text, dst] => render_text(&game()?, name, text, Path::new(dst), "ffffff", "000000")?,
        ["text", name, text, dst, fg] => render_text(&game()?, name, text, Path::new(dst), fg, "000000")?,
        ["text", name, text, dst, fg, outline] => render_text(&game()?, name, text, Path::new(dst), fg, outline)?,
        ["lang", code] => print_lang(&game()?, code, None)?,
        ["lang", code, index] => print_lang(&game()?, code, Some(index.parse()?))?,
        ["save", path] => {
            // savegame_N.info prints the header; savegame_N.data is decompressed and its scalar head printed
            let bytes = std::fs::read(path)?;
            if path.to_ascii_lowercase().ends_with(".info") {
                let i = opensow_formats::save::SaveInfo::parse(&bytes)?;
                println!("{:?} {} {} {} stamp {} extra {}", i.name, i.time, i.date, i.weekday, i.stamp, i.extra);
            } else {
                let state = opensow_formats::save::huff_decode(&bytes)?;
                println!("{} bytes; {:?}", state.len(), opensow_formats::save::StateHead::parse(&state)?);
            }
        }
        ["savelayout"] => {
            use opensow_formats::saveobj::Class;
            for c in [Class::Unit, Class::Top, Class::Chassis, Class::Order, Class::Weapon, Class::Building, Class::BuildingPart, Class::Bullet, Class::Aircraft, Class::Decal, Class::Cloud, Class::PlayerState, Class::Stock, Class::Params, Class::Speech, Class::SpeechItem, Class::MapItem] {
                let (v, m) = c.header();
                let f: Vec<String> = c.scalars().iter().map(|&(o, b)| format!("{}{:#x}", if b { "b" } else { "u" }, o)).collect();
                println!("{c:?} {v} {m:#x} {}", f.join(" "));
            }
        }
        ["netpkg", hex, bits] => {
            // decode an input package given as hex bytes and a bit count
            let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>()?;
            let p = opensow_formats::net::InputPackage::parse(&bytes, bits.parse()?)?;
            let keys: Vec<String> = p.keys.iter().map(|k| format!("{}:{}", k.code, k.down as u8)).collect();
            match p.mouse {
                Some(m) => println!("keys {} mouse {} {} {} {}", keys.join(","), m.x, m.y, m.a as u8, m.b as u8),
                None => println!("keys {} mouse none", keys.join(",")),
            }
        }
        ["blend", mode, rest @ ..] => {
            // blend <mode> <dst> <src> [<dst> <src> ...] (RGB565 words in hex): the exe's blit arithmetic
            use opensow_formats::effects::Blit;
            let mode = match &mode[..] {
                "plain" => Blit::Plain,
                "add" => Blit::Additive,
                "addhalf" => Blit::AddHalf,
                "average" => Blit::Average,
                "smoke" => Blit::Smoke,
                "darken" => Blit::Darken,
                "shade" => Blit::Shade,
                "lens" => Blit::Lens,
                _ => return Err("blend modes: plain add addhalf average smoke darken shade lens".into()),
            };
            let out: Vec<String> = rest
                .chunks(2)
                .map(|c| Ok(format!("{:04x}", mode.apply565(u16::from_str_radix(&c[0], 16)?, u16::from_str_radix(c.get(1).map_or("0", |s| &s[..]), 16)?))))
                .collect::<Result<_, std::num::ParseIntError>>()?;
            println!("{}", out.join(" "));
        }
        ["cfg", path] => {
            // State of War_1.cfg (options) or savegame/player.cfg (progress)
            let bytes = std::fs::read(path)?;
            if bytes.len() > 100 {
                let p = opensow_formats::config::PassedLevels::parse(&bytes)?;
                println!("level states {:?}", p.states);
            } else {
                println!("{:?}", opensow_formats::config::Options::parse(&bytes)?);
            }
        }
        ["sgv", path] => println!("{:#?}", opensow_formats::video::parse(&std::fs::read(path)?)?),
        ["sgvyuv", path, out] => {
            // decode an MPEG-1 .sgv to raw yuv420p frames (for comparing with other decoders)
            let es = opensow_formats::video::video_stream(&std::fs::read(path)?)?;
            let frames = opensow_formats::mpeg1::decode(&es)?;
            let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
            for fr in &frames {
                std::io::Write::write_all(&mut f, &fr.to_yuv420())?;
            }
            println!("{} frames of {}x{}", frames.len(), frames[0].width, frames[0].height);
        }
        ["sgvframe", path, n, out] => {
            let es = opensow_formats::video::video_stream(&std::fs::read(path)?)?;
            let frames = opensow_formats::mpeg1::decode(&es)?;
            let f = frames.get(n.parse::<usize>()?).ok_or("frame out of range")?;
            let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(out)?), f.width as u32, f.height as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&f.to_rgb())?;
        }
        ["sgvwav", path, out] => {
            let es = opensow_formats::video::audio_stream(&std::fs::read(path)?)?;
            let audio = opensow_formats::mp2::decode(&es)?;
            std::fs::write(out, audio.to_wav())?;
            println!("{:.2} s, {} Hz, {} channels", audio.seconds(), audio.sample_rate, audio.channels);
        }
        ["sgvavi", path, n, out] => {
            // one frame of an AVI .sgv (mB: MS Video 1) as a PNG
            use opensow_formats::{msvideo1, video};
            let data = std::fs::read(path)?;
            let video::Video::Avi(avi) = video::parse(&data)? else { return Err("not an AVI".into()) };
            let s = &avi.streams[0];
            if &s.handler == b"IV50" || &s.handler == b"iv50" {
                // mA: Indeo 5, every frame is intra so the wanted one decodes alone
                let chunks = video::avi_chunks(&data, 0)?;
                let f = opensow_formats::indeo5::decode(chunks.get(n.parse::<usize>()?).ok_or("no such frame")?)?;
                let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(out)?), f.width as u32, f.height as u32);
                enc.set_color(png::ColorType::Rgb);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header()?.write_image_data(&f.to_rgb())?;
                return Ok(());
            }
            let mut dec = msvideo1::Decoder::new(s.format[0] as usize, s.format[1] as usize, msvideo1::Decoder::palette_from_strf(&s.strf));
            let chunks = video::avi_chunks(&data, 0)?;
            for c in chunks.iter().take(n.parse::<usize>()? + 1) {
                dec.decode(c)?;
            }
            let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(out)?), dec.width as u32, dec.height as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&dec.to_rgb())?;
        }
        ["sgvaviyuv", path, out] => {
            // all frames of mB as raw RGB for comparing with other decoders
            use opensow_formats::{msvideo1, video};
            let data = std::fs::read(path)?;
            let video::Video::Avi(avi) = video::parse(&data)? else { return Err("not an AVI".into()) };
            let s = &avi.streams[0];
            let mut dec = msvideo1::Decoder::new(s.format[0] as usize, s.format[1] as usize, msvideo1::Decoder::palette_from_strf(&s.strf));
            let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
            for c in video::avi_chunks(&data, 0)? {
                dec.decode(c)?;
                std::io::Write::write_all(&mut f, &dec.to_rgb())?;
            }
        }
        ["cfglayout"] => println!("Options 1 0x1d6c u0xc u0x14 u0x18 u0x1c u0x20 b0x8 b0x9 b0x10 b0x11 u0x24"),
        ["weapons"] => print_weapons(),
        ["hover"] => println!("{}", opensow_formats::anim::HOVER_TABLE.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")),
        ["speedrow", v] => {
            let v: u32 = v.parse()?;
            println!("{}", (0..60).map(|t| projectile::steps_at(v, t).to_string()).collect::<Vec<_>>().join(" "));
        }
        ["facing", ax, ay, bx, by, steps] => {
            println!("{}", opensow_formats::facing::facing(ax.parse()?, ay.parse()?, bx.parse()?, by.parse()?, steps.parse()?));
        }
        ["launch", code, tx, ty, side, cells, counter] => {
            use opensow_formats::{air::AirForce, aircraft::Aircraft};
            let force = AirForce::from_code(code.parse()?).ok_or("unknown air force code (30..=34)")?;
            let a = Aircraft::launch(force, tx.parse()?, ty.parse()?, side.parse()?, cells.parse()?, counter.parse()?);
            let v = [
                a.x, a.y, a.target.0, a.target.1, a.altitude, a.speed, force.code(), a.hp, a.right as i32, a.death_timer, a.frame, a.frames,
                a.anim_count, a.anim_reload, a.counter, a.state, a.reach, a.off_x, a.off_y, a.squad as i32,
            ];
            println!("{}", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" "));
        }
        ["squad", index] => {
            for (dx, dy, kind) in opensow_formats::aircraft::squad(index.parse()?).ok_or("no such squad (0..=17)")? {
                println!("{dx} {dy} {kind}");
            }
        }
        ["rings"] => {
            let r = opensow_formats::rings::Rings::generate();
            let words = r.index_words();
            println!("{}", words[..64].iter().map(|w| w.to_string()).collect::<Vec<_>>().join(" "));
            let n = words[63] / 2;
            println!("{}", r.offsets[..n].iter().map(|(a, b)| format!("{a} {b}")).collect::<Vec<_>>().join(" "));
        }
        ["wave", grid, sx, sy, tx, ty, drop, base] => {
            // grid: 128 lines of 128 characters, '1' = blocked; prints `ok|fail <new base> <route cells...>`
            let text = std::fs::read_to_string(grid)?;
            let rows: Vec<&[u8]> = text.lines().map(|l| l.as_bytes()).collect();
            let mut w = opensow_formats::wave::Wave::new(|x, y| rows.get(x as usize).and_then(|r| r.get(y as usize)) == Some(&b'1'));
            w.base = base.parse()?;
            let r = w.find((sx.parse()?, sy.parse()?), (tx.parse()?, ty.parse()?), drop.parse()?);
            let cells = r.as_ref().map(|r| r.iter().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            println!("{} {} {}", if r.is_some() { "ok" } else { "fail" }, w.base, cells);
        }
        ["local", file, ubase, sbase, own, ax, ay, route @ ..] => {
            // file: 16384 little-endian u32 unit grid, then 16384 static grid; route: x y pairs (the unit's cell first)
            let bytes = std::fs::read(file)?;
            let words: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            let mut w = opensow_formats::wave::Wave::new(|_, _| false);
            w.cells = words[..16384].to_vec();
            w.base = ubase.parse()?;
            let nums: Vec<i32> = route.iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let route: Vec<(i32, i32)> = nums.chunks(2).map(|c| (c[0], c[1])).collect();
            let r = w.local_step(&words[16384..], sbase.parse()?, own.parse()?, &route, (ax.parse()?, ay.parse()?));
            let next = r.next.map_or("none".to_string(), |(x, y)| format!("{x} {y}"));
            let path = r.path.iter().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" ");
            println!("{} {} | {} | {}", u8::from(r.stop), w.base, next, path);
        }
        ["approach", file, base, w, h, ux, uy, tx, ty, handle, range] => {
            // file: 16384 little-endian u32 (the static grid); prints `ok <x> <y> <new base>` or `fail <new base>`
            let bytes = std::fs::read(file)?;
            let mut cells: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            let mut b: u32 = base.parse()?;
            let r = opensow_formats::wave::approach(&mut cells, &mut b, (w.parse()?, h.parse()?), (ux.parse()?, uy.parse()?), (tx.parse()?, ty.parse()?), handle.parse()?, range.parse()?);
            match r {
                Some((x, y)) => println!("ok {x} {y} {b}"),
                None => println!("fail {b}"),
            }
        }
        ["line", x0, y0, x1, y1] => {
            let l = opensow_formats::walk::pixel_line(x0.parse()?, y0.parse()?, x1.parse()?, y1.parse()?);
            println!("{}", l.iter().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" "));
        }
        ["turn", c, t, n] => println!("{}", opensow_formats::facing::turn_step(c.parse()?, t.parse()?, n.parse()?)),
        ["path", t, x1, y1, x2, y2, delay] => print_path(t.parse()?, x1.parse()?, y1.parse()?, x2.parse()?, y2.parse()?, delay.parse()?)?,
        ["frames", src] => frame_table(Path::new(src))?,
        ["level", src] => print_level(Path::new(src))?,
        ["map", src, dst] => map_image(Path::new(src), Path::new(dst))?,
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn unpack(game: &GameDir, out: &Path) -> Res<()> {
    let mega = open_megafile(game)?;
    for e in &mega.entries {
        let dst = e.name.split('\\').fold(out.to_path_buf(), |p, s| p.join(s));
        std::fs::create_dir_all(dst.parent().unwrap())?;
        std::fs::write(&dst, mega.read(e)?)?;
    }
    println!("unpacked {} files into {}", mega.entries.len(), out.display());
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn check(game: &GameDir) -> Res<()> {
    let (mut ok, mut bad) = (0, 0);
    let mut report = |what: String, r: Result<(), opensow_formats::Error>| match r {
        Ok(()) => ok += 1,
        Err(e) => {
            bad += 1;
            println!("FAIL {what}: {e}");
        }
    };

    let mut files = Vec::new();
    walk(&game.data(), &mut files);
    files.sort();
    for p in &files {
        let name = p.strip_prefix(&game.root).unwrap_or(p).display().to_string();
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "ps6" | "tsp" | "sha" => {
                let kind = sprite::Kind::from_extension(p).unwrap();
                report(name, std::fs::read(p).map_err(Into::into).and_then(|d| sprite::parse(&d, kind)).map(drop));
            }
            "map" => report(name, std::fs::read(p).map_err(Into::into).and_then(|d| map::Map::parse(&d)).map(drop)),
            "edt" | "net" => report(name, std::fs::read(p).map_err(Into::into).and_then(|d| level::Level::parse(&d)).map(drop)),
            "tmi" => report(name, std::fs::read(p).map_err(Into::into).and_then(|d| map::TileMap::parse(&d)).map(drop)),
            "til" => report(name, std::fs::read(p).map_err(Into::into).and_then(|d| sprite::parse(&d, sprite::Kind::Shadow)).map(drop)),
            "sgv" => report(name, std::fs::read(p).map_err(Into::into).and_then(|d| opensow_formats::video::parse(&d)).map(drop)),
            "srf" | "srfa" => report(name, std::fs::read(p).map_err(Into::into).and_then(|d| srf::to_jpeg(&d)).map(drop)),
            _ => {}
        }
    }

    let mega = open_megafile(game)?;
    for e in &mega.entries {
        let Some(kind) = sprite::Kind::from_extension(Path::new(&e.name)) else { continue };
        report(format!("sprites.data:{}", e.name), mega.read(e).and_then(|d| sprite::parse(&d, kind)).map(drop));
    }
    println!("{ok} ok, {bad} failed");
    if bad > 0 { Err("some assets failed to parse".into()) } else { Ok(()) }
}

fn sprite_sheet(src: &Path, dst: &Path, cols: Option<usize>) -> Res<()> {
    let kind = sprite::Kind::from_extension(src).ok_or("unknown sprite extension")?;
    let frames = sprite::parse(&std::fs::read(src)?, kind)?;
    if frames.is_empty() {
        return Err("no frames".into());
    }
    let cols = cols.unwrap_or(frames.len().min(16)).max(1);
    let rows = frames.len().div_ceil(cols);
    let cw = frames.iter().map(|f| f.width as usize).max().unwrap() + 1;
    let ch = frames.iter().map(|f| f.height as usize).max().unwrap() + 1;
    let (w, h) = (cols * cw, rows * ch);
    let mut img = vec![0u8; w * h * 4];
    let alpha = (kind == sprite::Kind::Shadow).then_some(128);
    for (i, f) in frames.iter().enumerate() {
        let (ox, oy) = ((i % cols) * cw, (i / cols) * ch);
        let rgba = f.to_rgba(alpha);
        for y in 0..f.height as usize {
            let s = y * f.width as usize * 4;
            let d = ((oy + y) * w + ox) * 4;
            img[d..d + f.width as usize * 4].copy_from_slice(&rgba[s..s + f.width as usize * 4]);
        }
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(dst)?), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img)?;
    println!("{} frames -> {}", frames.len(), dst.display());
    Ok(())
}

fn map_image(src: &Path, dst: &Path) -> Res<()> {
    let m = map::Map::parse(&std::fs::read(src)?)?;
    let mut img = Vec::with_capacity(m.cells.len() * 3);
    for c in &m.cells {
        img.extend(match (c.border, c.blocked, c.flag) {
            (true, _, _) => [255, 255, 255],
            (_, true, _) => [200, 40, 40],
            (_, _, true) => [40, 200, 40],
            _ => [20, 20, 20],
        });
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(dst)?), m.width, m.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img)?;
    println!("{}x{} cells -> {}", m.width, m.height, dst.display());
    Ok(())
}

fn print_level(src: &Path) -> Res<()> {
    let l = level::Level::parse(&std::fs::read(src)?)?;
    println!("version {} id {} resources {:?}", l.version, l.level_id(), l.start_resources());
    let nz: Vec<_> = l.header.iter().enumerate().skip(2).filter(|(_, &v)| v != 0).map(|(i, v)| format!("@{}={v}", i * 4)).collect();
    println!("header: {}", nz.join(" "));
    for st in l.air_stock().iter().filter(|s| s.counts.iter().any(|&c| c != 0)) {
        println!("air force {:<9} stock {:?} (side 0/1/2)", st.force.name(), st.counts);
    }
    for o in &l.objects {
        let extra = if o.object_kind().is_building() { format!(" words {:?}", &o.raw[..13]) } else { String::new() };
        println!("{:<14} {:<8?} cell ({:3},{:3}) hp {:5} kind {}{}", o.object_kind().name(), o.owner(), o.cell_x, o.cell_y, o.hp(), o.kind, extra);
    }
    for m in &l.markers {
        println!("unit {:<26} player {} cell {:?} hp {:?} kind {}", level::UnitKind::describe(m.kind), m.player, m.cell(), level::UnitKind::max_hp(m.kind), m.kind);
    }
    Ok(())
}

/// One line per frame: size and hotspot (used to study glyph tables).
fn frame_table(src: &Path) -> Res<()> {
    let kind = sprite::Kind::from_extension(src).ok_or("unknown sprite extension")?;
    let frames = sprite::parse(&std::fs::read(src)?, kind)?;
    for (i, f) in frames.iter().enumerate() {
        println!("{i:4} {:3}x{:<3} hot ({:4},{:4}) extra {:?}", f.width, f.height, f.hot_x, f.hot_y, f.extra);
    }
    Ok(())
}

fn load_font(mega: &MegaFile, name: &str) -> Res<font::Font> {
    let file = format!("extras\\fonts\\{name}.msk");
    let e = mega.find(&file).ok_or_else(|| format!("{file} not in sprites.data"))?;
    Ok(font::Font::new(sprite::parse(&mega.read(e)?, sprite::Kind::Shadow)?))
}

fn parse_hex(s: &str) -> Res<[u8; 3]> {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16)?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

const COLOUR_FONTS: [&str; 5] = ["font02_dark", "font02_light", "font_msg2", "font_briefing_big", "font_briefing_small"];

/// Coloured fonts keep their own pixels (no tint, no outline pass).
fn render_colour_text(mega: &MegaFile, name: &str, text: &str, dst: &Path) -> Res<()> {
    let file = format!("extras\\fonts\\{name}.ps6");
    let e = mega.find(&file).ok_or_else(|| format!("{file} not in sprites.data"))?;
    let f = font::Font::new(sprite::parse(&mega.read(e)?, sprite::Kind::Color)?);
    let placed = f.layout(text, 1);
    // Coloured fonts use a different hotspot convention, so size the canvas from the glyph extents.
    let top = placed.iter().map(|p| p.y).min().unwrap_or(0);
    let bottom = placed.iter().map(|p| p.y + p.glyph.height as i32).max().unwrap_or(0);
    let (w, h) = ((f.measure(text, 1) + 8).max(8) as usize, (bottom - top + 8) as usize);
    let mut img = vec![0u8; w * h * 4];
    for p in placed {
        let rgba = p.glyph.to_rgba(None);
        for gy in 0..p.glyph.height as i32 {
            for gx in 0..p.glyph.width as i32 {
                let k = (gy * p.glyph.width as i32 + gx) as usize;
                let (x, y) = (p.x + gx + 4, p.y - top + gy + 4);
                if p.glyph.mask[k] && x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                    let i = (y as usize * w + x as usize) * 4;
                    img[i..i + 4].copy_from_slice(&rgba[k * 4..k * 4 + 4]);
                }
            }
        }
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(dst)?), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img)?;
    println!("{name}: {w}x{h} -> {}", dst.display());
    Ok(())
}

fn render_text(game: &GameDir, name: &str, text: &str, dst: &Path, fg: &str, outline: &str) -> Res<()> {
    let mega = open_megafile(game)?;
    if COLOUR_FONTS.contains(&name) {
        return render_colour_text(&mega, name, text, dst);
    }
    let (inner, outer) = (load_font(&mega, &format!("{name}_i"))?, load_font(&mega, &format!("{name}_o"))?);
    let (fg, outline) = (parse_hex(fg)?, parse_hex(outline)?);
    let spacing = 1;
    let (w, h) = ((inner.measure(text, spacing) + 8).max(8) as usize, (inner.line_height() + 8) as usize);
    let mut img = vec![0u8; w * h * 4];
    // Both masks share one pen (the inner font's advances) and are placed at `pen - hot`.
    let placed = inner.layout(text, spacing);
    for outline_pass in [true, false] {
        for p in &placed {
            let (glyph, color) = if outline_pass {
                match outer.glyph(p.ch) {
                    Some(g) => (g, outline),
                    None => continue,
                }
            } else {
                (p.glyph, fg)
            };
            let (left, top) = (p.x - glyph.hot_x as i32, -(glyph.hot_y as i32));
            for gy in 0..glyph.height as i32 {
                for gx in 0..glyph.width as i32 {
                    if !glyph.mask[(gy * glyph.width as i32 + gx) as usize] {
                        continue;
                    }
                    let (x, y) = (left + gx + 4, top + gy + 4);
                    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                        let i = (y as usize * w + x as usize) * 4;
                        img[i..i + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
                    }
                }
            }
        }
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(dst)?), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img)?;
    println!("{name}: {w}x{h} -> {}", dst.display());
    Ok(())
}

fn print_lang(game: &GameDir, code: &str, index: Option<usize>) -> Res<()> {
    let l = lang::Language::from_code(code).ok_or("unknown language code")?;
    let path = game.resolve(&format!("Data\\Language\\{}", l.file_name())).ok_or("language file not found")?;
    let f = lang::LangFile::parse(&std::fs::read(path)?)?;
    let damaged = f.damaged();
    for (i, s) in f.strings.iter().enumerate().filter(|(i, _)| index.map_or(true, |n| n == *i)) {
        println!("{i:3}{} {}", if damaged.contains(&i) { "!" } else { " " }, s.replace("\r\n", " / "));
    }
    if index.is_none() {
        eprintln!("{} (id {}): {} strings, {} damaged", l.code(), l.id(), f.strings.len(), damaged.len());
    }
    Ok(())
}

fn print_weapons() {
    let units = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 41, 42, 43, 44, 45,
    ];
    println!("kind  weapon              range  reload(+-jitter)  volley ticks  shots  bullet  speed/60t  strength  damage/volley");
    for kind in units {
        let (Some(a), Some(seq), Some((bt, strength)), Some(art)) =
            (aim::aim_params(kind), recoil::fire_sequence(kind), projectile::weapon_shot(kind), opensow_formats::units::unit_art(kind))
        else {
            continue;
        };
        let shots: u32 = seq.steps.iter().filter(|s| s.barrel >= 0).map(|s| s.shots as u32).sum();
        let speed = projectile::bullet_type(bt).and_then(|b| b.speed).map_or("-".to_string(), |v| v.to_string());
        println!(
            "{kind:>4}  {:<18}  {:>2}-{:<3}  {:>6} +-{:<6}   {:>10}  {:>5}  {:>6}  {:>9}  {:>8}  {:>13}",
            level::UnitKind::describe(kind),
            a.min_range,
            a.max_range,
            a.reload,
            a.jitter / 2,
            seq.duration(),
            shots,
            bt,
            speed,
            strength,
            shots * strength,
        );
        let _ = art;
    }
    println!("\nranges in cells; ticks are game ticks (a bullet's speed is path steps per 60 ticks). Damage ignores splash on neighbours.");
}

fn print_path(t: u8, x1: i32, y1: i32, x2: i32, y2: i32, delay: u32) -> Res<()> {
    let p = projectile::build_path(t, x1, y1, x2, y2, delay).ok_or("this bullet type has no computed path")?;
    println!("{} {}", p.points.len(), p.first_step);
    for (q, l) in p.points.iter().zip(&p.lift) {
        println!("{} {} {}", q.0, q.1, l);
    }
    Ok(())
}
