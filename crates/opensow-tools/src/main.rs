//! `opensow` - inspect and extract assets from a State of War installation.

use opensow_formats::{aim, font, lang, level, map, megafile::MegaFile, projectile, recoil, sprite, srf, GameDir};
mod art;
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
  sparks <19|20|21|23> <variant> <rand values...>   spark arrays of a new beam bullet (rand(n) = value % n), or the dim end frame
  curvebullet <generic.file> <13|14> <rng index> x1 y1   speed, flag, steps, then the path of a new random-curve bullet
  curves <Data dir>                  the 200 random bullet curves (types 13/14): index, then x y lift per point
  path <type> x1 y1 x2 y2 delay      print a bullet's flight path points (x y lift), one per line
  launch <code> <tx> <ty> <side> <cells> <counter>   fields of a launched aircraft (code 30..=34)
  squad <0..17>                      units a carryall lands: dx dy kind per line
  cloud roll W H r1 r2 r3 r4 | scatter W H r1..r6 | step X SPEED W T0 N   ambient cloud: new fields / the position after N ticks from tick T0
  rings                              print the range index table and the ring offset list (dx dy ...)
  weapons                            table of every weapon: range, reload, volley, projectile, speed, strength
  hover                              the Advancer's hover bob table (0x89cf58)
  scanlines banner|banner555|pause W H HEXPIXELS..   the pause / mission-end frame effects (0x45d3a0 / 0x45d400)
  beamtick STATE COUNTER RELOAD      one tick of the Advancer beam cooldown (0x47c6b0): prints new state and counter
  export <out_dir>                   every colour sprite frame (PNG, flat dir) + background (JPG) + manifest.csv, for upscaling
  import <art_dir> <out_dir> [--quality Q] [--alpha A]   encode edited PNG/JPG back (drop-in Data overlay; see docs/art-import.md)
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
        ["export", out] => art::export(&game()?, Path::new(out))?,
        ["import", src, out, opts @ ..] => {
            let (mut quality, mut alpha) = (95u8, 128u8);
            let mut it = opts.iter();
            while let Some(o) = it.next() {
                let v = it.next().ok_or("missing option value")?;
                match *o {
                    "--quality" => quality = v.parse()?,
                    "--alpha" => alpha = v.parse()?,
                    _ => return Err(format!("unknown option {o}").into()),
                }
            }
            art::import(&game()?, Path::new(src), Path::new(out), quality, alpha)?
        }
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
        ["savestate", speech, hexfile] => {
            // parse a whole decompressed state stream (hex text file; `speech` = [0x575720] + 1), print a summary and whether
            // State::to_bytes reproduces it byte for byte (scripts/verify_save_state.py compares the summary with the emulated memory)
            use opensow_formats::save::State;
            let text = std::fs::read_to_string(hexfile)?;
            let t = text.trim();
            let bytes: Vec<u8> = (0..t.len() / 2).map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>()?;
            let st = State::parse(&bytes, speech.parse()?)?;
            let again = st.to_bytes();
            println!("rewrite {}", if again.as_deref() == Some(&bytes[..]) { "ok" } else { "DIFFERS" });
            let h = &st.head;
            println!(
                "head {} {} {} {} {} {} {} {} {} {}",
                h.level, h.rng_index, h.v_5baafc, h.flag_49a339 as u8, h.v_5baad8, h.v_5baadc, h.v_5baac4, h.v_5baac8, h.flag_5baa99 as u8, h.v_89cf50
            );
            let hex = |v: &[u8]| v.iter().map(|b| format!("{b:02x}")).collect::<String>();
            for p in 0..3 {
                println!("pw {p} {} {} {}", st.player_words[p][0], st.player_words[p][1], st.player_words[p][2]);
                for k in 0..4 {
                    println!("parr {p} {k} {}", hex(&st.player_arrays[p][k]));
                }
            }
            for (r, pair) in st.lists.iter().enumerate() {
                for (k, l) in pair.iter().enumerate() {
                    println!("list {r} {k} {}", l.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
                }
            }
            let m = &st.map;
            println!("map {} {} {} {} {} {}", m.version, m.scroll_x, m.scroll_y, m.width, m.height, m.items.len());
            for (k, c) in m.cameras.iter().enumerate() {
                println!("cam {k} {} {} {} {} {} {} {}", c.scroll_to as u8, c.target_x, c.target_y, c.quake_left, c.quake_timer, c.quake_x, c.quake_y);
            }
            for (i, c) in m.cells.iter().enumerate().take(40) {
                println!("cell {i} {} {} {} {} {} {} {}", c.coord, c.ground, c.air, c.reserved, c.no_marks, c.no_build, c.building);
            }
            for (name, groups) in [("units", &st.units), ("buildings", &st.buildings), ("bullets", &st.bullets), ("squads", &st.squads)] {
                for (p, g) in groups.iter().enumerate() {
                    println!("slots {name} {p} {} {}", g.0, g.1.iter().map(|x| x.0.to_string()).collect::<Vec<_>>().join(" "));
                }
            }
            println!("decalflags {}", hex(&st.decal_flags));
            println!(
                "decals {} clouds {} speech {} players {} stocks {} params {}",
                st.decals.len(),
                st.clouds.len(),
                st.speech.len(),
                st.players.len(),
                st.stocks.len(),
                st.params.len()
            );
            for (i, sp) in st.speech.iter().enumerate() {
                println!("speechitems {i} {}", sp.orders.iter().map(|o| o.strings[0].clone()).collect::<Vec<_>>().join("|"));
            }
        }
        ["saverec", class, hexfile] => {
            // parse one record (hex text file), print every field as `path offset value` and whether write(parse(x)) == x
            use opensow_formats::{
                saveobj::{Class, Object},
                Reader,
            };
            let text = std::fs::read_to_string(hexfile)?;
            let t = text.trim();
            let bytes: Vec<u8> = (0..t.len() / 2).map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>()?;
            let c = Class::ALL.iter().copied().find(|c| format!("{c:?}") == *class).ok_or("unknown class")?;
            let mut r = Reader::new(&bytes);
            let o = Object::parse(&mut r, c)?;
            let mut again = Vec::new();
            o.write(&mut again);
            println!("consumed {} of {} rewrite {}", bytes.len() - r.remaining(), bytes.len(), if again == bytes { "ok" } else { "DIFFERS" });
            let mut v = Vec::new();
            o.flatten("", &mut v);
            for (p, off, val) in v {
                println!("{p}|{off}|{val}");
            }
        }
        ["savelayout"] => {
            use opensow_formats::saveobj::Class;
            for c in [
                Class::Unit,
                Class::Top,
                Class::Chassis,
                Class::Order,
                Class::Weapon,
                Class::Building,
                Class::BuildingPart,
                Class::Bullet,
                Class::Aircraft,
                Class::Decal,
                Class::Cloud,
                Class::PlayerState,
                Class::Stock,
                Class::Params,
                Class::Speech,
                Class::SpeechItem,
                Class::MapItem,
            ] {
                let (v, m) = c.header();
                let f: Vec<String> = c.scalars().iter().map(|&(o, b)| format!("{}{:#x}", if b { "b" } else { "u" }, o)).collect();
                println!("{c:?} {v} {m:#x} {}", f.join(" "));
                if std::env::var_os("NAMES").is_some() {
                    for &(o, b) in c.scalars().iter() {
                        println!("    {}{o:#x} {}", if b { "b" } else { "u" }, c.field_name(o));
                    }
                }
            }
        }
        ["netsync", what, hex, bits] => {
            // decode a state-sync record given as hex bytes and a bit count (docs/network.md, `formats::netsync`)
            use opensow_formats::{net::Bits, netsync::*};
            let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>()?;
            let mut r = Bits::new(&bytes, bits.parse()?);
            match *what {
                "building" => {
                    let b = BuildingSync::parse(&mut r)?;
                    println!("{} {} {} {} {}", b.kind, b.word90, b.cell.0, b.cell.1, b.owner);
                }
                "bullet" => {
                    let b = BulletSync::parse(&mut r)?;
                    let (xi, xv) = match b.extra {
                        BulletExtra::None => (5, 0),
                        BulletExtra::Handle(v) => (0, v),
                        BulletExtra::Delay(v) => (1, v),
                        BulletExtra::Speed(v) => (2, v),
                        BulletExtra::Variant(v) => (3, v),
                        BulletExtra::Variant2(v) => (4, v),
                    };
                    println!(
                        "{} {} {} {} {} {} {} {} {} {} {xi} {xv} {}",
                        b.ty,
                        b.owner,
                        b.points[0],
                        b.points[1],
                        b.points[2],
                        b.points[3],
                        b.word2698,
                        b.strength,
                        b.word26b0,
                        b.word26b4,
                        b.flag.map_or(-1, |f| f as i32)
                    );
                }
                "air" => {
                    let a = AirSync::parse(&mut r)?;
                    println!("{} {} {} {} {} {} {} {} {}", a.code, a.handle, a.target.0, a.target.1, a.word98, a.word9c, a.word20, a.worda0, a.worda4);
                }
                "unit" => {
                    let u = UnitSync::parse(&mut r)?;
                    let mut line = format!("{} {} {} {} {} {} {}", u.kind, u.handle, u.player, u.cell.0, u.cell.1, u.home.0, u.home.1);
                    match u.chassis {
                        Some(c) => {
                            line += &format!(
                                " C {} {} {} {} {} {} {} {} {}",
                                c.steps[0], c.steps[1], c.steps[2], c.steps[3], c.goal.0, c.goal.1, c.words.0, c.words.1, c.approaching as u8
                            )
                        }
                        None => line += " N",
                    }
                    match u.top {
                        Some((f, w)) => line += &format!(" T {f} {}", w.map_or(-1i64, |v| v as i64)),
                        None => line += " N",
                    }
                    println!("{line}");
                }
                _ => return Err("netsync building|bullet|air|unit HEX BITS".into()),
            }
            println!("bits left {}", r.remaining());
        }
        ["netmsg", hex, bits] => {
            // decode a whole frame message (docs/network.md): a client's command queues (type bit 0 = 1) or the host's snapshot
            use opensow_formats::{
                net::{Bits, Header},
                netsync::*,
            };
            let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>()?;
            let n: usize = bits.parse()?;
            let h = Header::parse(&bytes, n)?;
            println!("header {:?}", h.fields);
            if h.fields[0] & 1 == 1 {
                let mut r = Bits::new(&bytes, n);
                r.pos = opensow_formats::net::PAYLOAD_BIT as usize;
                println!("commands from player {}: {:#?}", h.fields[7], Commands::parse_payload(&mut r)?);
            } else {
                println!("{:#?}", Snapshot::parse(&bytes, n)?);
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
                .map(|c| Ok(format!("{:04x}", mode.apply565(u16::from_str_radix(c[0], 16)?, u16::from_str_radix(c.get(1).map_or("0", |s| &s[..]), 16)?))))
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
                match opensow_formats::config::SettingsFile::parse(&bytes) {
                    Ok(f) => println!("{f:#?}"),
                    Err(_) => println!("{:?}", opensow_formats::config::Options::parse(&bytes)?),
                }
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
        // scatter line (0x4378d0): `scatter PX PY CX CY N` prints where it stops; several cases may follow, five numbers each
        ["scatter", nums @ ..] if !nums.is_empty() && nums.len().is_multiple_of(5) => {
            let v: Vec<i32> = nums.iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let out: Vec<String> =
                v.chunks(5).map(|c| opensow_formats::scatter::extend((c[0], c[1]), (c[2], c[3]), c[4])).map(|(x, y)| format!("{x},{y}")).collect();
            println!("{}", out.join(" "));
        }
        ["hover"] => println!("{}", opensow_formats::anim::HOVER_TABLE.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")),
        ["beamtick", state, counter, reload] => {
            let mut t = opensow_formats::advancer::BeamTimer { enabled: true, state: state.parse()?, counter: counter.parse()?, reload: reload.parse()? };
            t.tick();
            println!("{} {}", t.state, t.counter);
        }
        ["scanlines", mode, w, h, px @ ..] => {
            // 0x45d400 ("banner": tint even rows, blank odd rows) / 0x45d3a0 ("pause": blank even rows) on RGB565 pixels given in hex
            use opensow_formats::endscreen;
            let (w, h): (usize, usize) = (w.parse()?, h.parse()?);
            let mut v: Vec<u16> = px.iter().map(|p| u16::from_str_radix(p, 16)).collect::<Result<_, _>>()?;
            if v.len() != w * h {
                return Err("scanlines: need w*h pixels".into());
            }
            match *mode {
                "banner" => endscreen::banner_rows(&mut v, w, h, true),
                "banner555" => endscreen::banner_rows(&mut v, w, h, false),
                "pause" => endscreen::pause_rows(&mut v, w, h),
                _ => return Err("scanlines: banner|banner555|pause".into()),
            }
            println!("{}", v.iter().map(|p| format!("{p:04x}")).collect::<Vec<_>>().join(" "));
        }
        ["speedrow", v] => {
            let v: u32 = v.parse()?;
            println!("{}", (0..60).map(|t| projectile::steps_at(v, t).to_string()).collect::<Vec<_>>().join(" "));
        }
        ["facing", ax, ay, bx, by, steps] => {
            println!("{}", opensow_formats::facing::facing(ax.parse()?, ay.parse()?, bx.parse()?, by.parse()?, steps.parse()?));
        }
        ["spark", px, py, x, y, r] => {
            // `0x443210`: next vertex of a Load/Save page spark (coordinates relative to the screen centre); the scripted generator returns `r % n`
            let r: u32 = r.parse()?;
            match opensow_formats::savespark::next_vertex((px.parse()?, py.parse()?), (x.parse()?, y.parse()?), |n| r % n) {
                Some((nx, ny)) => println!("{nx} {ny}"),
                None => println!("none"),
            }
        }
        ["launch", code, tx, ty, side, cells, counter] => {
            use opensow_formats::{air::AirForce, aircraft::Aircraft};
            let force = AirForce::from_code(code.parse()?).ok_or("unknown air force code (30..=34)")?;
            let a = Aircraft::launch(force, tx.parse()?, ty.parse()?, side.parse()?, cells.parse()?, counter.parse()?);
            let v = [
                a.x,
                a.y,
                a.target.0,
                a.target.1,
                a.altitude,
                a.speed,
                force.code(),
                a.hp,
                a.right as i32,
                a.death_timer,
                a.frame,
                a.frames,
                a.anim_count,
                a.anim_reload,
                a.counter,
                a.state,
                a.reach,
                a.off_x,
                a.off_y,
                a.squad as i32,
            ];
            println!("{}", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" "));
        }
        ["squad", index] => {
            for (dx, dy, kind) in opensow_formats::aircraft::squad(index.parse()?).ok_or("no such squad (0..=17)")? {
                println!("{dx} {dy} {kind}");
            }
        }
        ["cloud", "roll", _w, h, r1, r2, r3, r4] => {
            let mut v = [r1, r2, r3, r4].map(|s| s.parse::<u32>().unwrap()).into_iter();
            let c = opensow_formats::clouds::Cloud::roll(h.parse()?, &mut |n| v.next().unwrap() % n);
            println!("{} {} {} {}", c.x, c.y, c.speed, c.picture);
        }
        ["cloud", "scatter", w, h, r1, r2, r3, r4, r5, r6] => {
            let mut v = [r1, r2, r3, r4, r5, r6].map(|s| s.parse::<u32>().unwrap()).into_iter();
            let c = opensow_formats::clouds::Cloud::scatter(w.parse()?, h.parse()?, &mut |n| v.next().unwrap() % n);
            println!("{} {} {} {}", c.x, c.y, c.speed, c.picture);
        }
        ["cloud", "step", x, speed, w, t0, n] => {
            let mut c = opensow_formats::clouds::Cloud { x: x.parse()?, y: 0, speed: speed.parse()?, picture: 0 };
            let (t0, n): (u32, u32) = (t0.parse()?, n.parse()?);
            let alive = c.advance((0..n).map(|i| (t0 + i) % 60), w.parse()?);
            println!("{} {}", c.x, alive as u8);
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
        ["local", file, ubase, sbase, own, ax, ay, route @ ..] => local_cmd(file, ubase, sbase, None, own, ax, ay, route)?,
        // 0x475c30 / 0x4732f0: the file holds the second step grid, the static grid and the first step grid
        // (`air` before the route selects 0x4732f0, which also asks for the stop)
        ["local2", file, ubase, sbase, fbase, air, own, ax, ay, route @ ..] => local_cmd(file, ubase, sbase, Some((fbase, *air == "air")), own, ax, ay, route)?,
        // 0x4774c0: file = unit grid (16384 u32); prints `found base dx dy checksum` (dx dy = `none` when the destination is kept;
        // checksum = sum of (i+1)*cell mod 2^32 over the grid after the flood)
        ["freecell", file, base, own, dx, dy, ax, ay, layer, mw, mh] => {
            let bytes = std::fs::read(file)?;
            let mut w = opensow_formats::wave::Wave::new(|_, _| false);
            w.cells = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).take(16384).collect();
            w.base = base.parse()?;
            let rings = opensow_formats::rings::Rings::generate();
            let r = opensow_formats::freecell::free_cell_near(
                &mut w,
                &rings,
                own.parse()?,
                (dx.parse()?, dy.parse()?),
                (ax.parse()?, ay.parse()?),
                *layer == "air",
                (mw.parse()?, mh.parse()?),
            );
            let sum = w.cells.iter().enumerate().fold(0u32, |a, (i, &v)| a.wrapping_add((i as u32 + 1).wrapping_mul(v)));
            let d = r.destination.map_or("none".to_string(), |(x, y)| format!("{x} {y}"));
            println!("{} {} {} {}", u8::from(r.found), w.base, d, sum);
        }
        // 0x46fa40 (networked step) with recording stubs for its callees; arguments are key=value (docs/pathfinding.md):
        // air guest route now since pos dest handle row line prev step1 step2 plan1 plan2 planline cd anim kind40 turn routelen stat
        ["netstep", kv @ ..] => netstep_cmd(kv)?,
        ["approach", file, base, w, h, ux, uy, tx, ty, handle, range, layer @ ..] => {
            // file: 16384 little-endian u32 (the static grid); prints `ok <x> <y> <new base>` or `fail <new base>`;
            // a trailing `air` runs the chase for handle type 4 (0x478200, an Advancer) instead of the ground one (0x477630)
            let bytes = std::fs::read(file)?;
            let mut cells: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            let mut b: u32 = base.parse()?;
            let chase = if layer.first().is_some_and(|l| *l == "air") { opensow_formats::wave::approach_air } else { opensow_formats::wave::approach };
            let r =
                chase(&mut cells, &mut b, (w.parse()?, h.parse()?), (ux.parse()?, uy.parse()?), (tx.parse()?, ty.parse()?), handle.parse()?, range.parse()?);
            match r {
                Some((x, y)) => println!("ok {x} {y} {b}"),
                None => println!("fail {b}"),
            }
        }
        ["targets", file, base, w, h, cx, cy, own] => {
            // file: 16384 u32 (static grid, exe handles); prints `unit building turret new-base` (building = non-turret)
            use opensow_formats::wave::Seen;
            let bytes = std::fs::read(file)?;
            let mut cells: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            let mut b: u32 = base.parse()?;
            let own: u32 = own.parse()?;
            let t = opensow_formats::wave::find_targets(&mut cells, &mut b, (w.parse()?, h.parse()?), (cx.parse()?, cy.parse()?), |h| {
                if (h >> 16) & 7 == own {
                    return Seen::Ignore;
                }
                match (h >> 22) & 7 {
                    1 => Seen::Unit,
                    2 if matches!((h >> 19) & 7, 3 | 6) => Seen::Turret,
                    2 => Seen::Building,
                    _ => Seen::Ignore,
                }
            });
            println!("{} {} {} {b}", t.unit, t.building, t.turret);
        }
        ["line", x0, y0, x1, y1] => {
            let l = opensow_formats::walk::pixel_line(x0.parse()?, y0.parse()?, x1.parse()?, y1.parse()?);
            println!("{}", l.iter().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" "));
        }
        ["turn", c, t, n] => println!("{}", opensow_formats::facing::turn_step(c.parse()?, t.parse()?, n.parse()?)),
        ["curvebullet", gen, t, idx, x1, y1] => print_curve_bullet(Path::new(gen), t.parse()?, idx.parse()?, x1.parse()?, y1.parse()?)?,
        ["sparks", t, variant, vals @ ..] => print_sparks(t.parse()?, variant.parse()?, vals)?,
        ["turretspots", kind, x, y] => print_turret_spots(kind.parse()?, x.parse()?, y.parse()?),
        ["turretai", file] => print_turret_ai(Path::new(file))?,
        ["curves", dir] => print_curves(Path::new(dir))?,
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
    // the players' own skirmish maps (docs/map-editor-ingame.md): `maps` in the per-user data folder
    let maps = std::env::var_os("OPENSOW_MAPS_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| opensow_formats::launcher::data_dir().join("maps"));
    let mut custom: Vec<_> = std::fs::read_dir(&maps)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(opensow_formats::custommap::EXTENSION)))
        .collect();
    custom.sort();
    for p in custom {
        report(p.display().to_string(), std::fs::read(&p).map_err(Into::into).and_then(|d| opensow_formats::custommap::CustomMap::parse(&d)).map(drop));
    }
    println!("{ok} ok, {bad} failed");
    if bad > 0 {
        Err("some assets failed to parse".into())
    } else {
        Ok(())
    }
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
        img.extend(match (c.border, c.blocked, c.no_build) {
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
    let data = std::fs::read(src)?;
    // a custom map (docs/map-editor-ingame.md): its level part, then the port's block
    let l = match level::Level::parse(&data) {
        Ok(l) => l,
        Err(_) => {
            let m = opensow_formats::custommap::CustomMap::parse(&data)?;
            println!("custom map \"{}\" on {} mission {}", m.name, if m.warmonger { "Warmonger" } else { "State of War" }, m.level.level_id());
            println!("  mission: win rule {:?}, countdown {:?}", m.mission.win, m.timer());
            if !m.escort.is_empty() {
                println!("  escort: {:?}, pods {}", m.escort, opensow_formats::custommap::pods(&m.level, 0));
            }
            if !m.reinforcement.is_empty() {
                println!("  reinforcement: {:?}", m.reinforcement);
            }
            for (i, e) in m.ext.iter().enumerate().filter(|(_, e)| !e.is_default()) {
                println!("  object {i}: unlock points {:?}{}", e.unlock, if e.no_advancer { ", no Advancer" } else { "" });
            }
            m.level
        }
    };
    println!("version {} id {} resources {:?}", l.version, l.level_id(), l.start_resources());
    let nz: Vec<_> = l.header.iter().enumerate().skip(2).filter(|(_, &v)| v != 0).map(|(i, v)| format!("@{}={v}", i * 4)).collect();
    println!("header: {}", nz.join(" "));
    for st in l.air_stock().iter().filter(|s| s.counts.iter().any(|&c| c != 0)) {
        println!("air force {:<9} stock {:?} (player / enemy / unread word)", st.force.name(), st.counts);
    }
    for o in &l.objects {
        let extra = if o.object_kind().is_building() { format!(" words {:?}", &o.raw[..13]) } else { String::new() };
        println!("{:<14} {:<8?} cell ({:3},{:3}) hp {:5} kind {}{}", o.object_kind().name(), o.owner(), o.cell_x, o.cell_y, o.hp(), o.kind, extra);
    }
    for m in &l.markers {
        println!(
            "unit {:<26} player {} cell {:?} hp {:?} kind {}",
            level::UnitKind::describe(m.kind),
            m.player,
            m.cell(),
            level::UnitKind::max_hp(m.kind),
            m.kind
        );
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
    for (i, s) in f.strings.iter().enumerate().filter(|(i, _)| index.is_none_or(|n| n == *i)) {
        println!("{i:3}{} {}", if damaged.contains(&i) { "!" } else { " " }, s.replace("\r\n", " / "));
    }
    if index.is_none() {
        eprintln!("{} (id {}): {} strings, {} damaged", l.code(), l.id(), f.strings.len(), damaged.len());
    }
    Ok(())
}

fn print_weapons() {
    let units = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 41, 42, 43, 44, 45];
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

/// `opensow curves <Data dir>`: the 200 random curves of bullet types 13/14 (RNG index 0 at game start): `i n`, then n lines `x y lift`.
fn print_curves(data: &Path) -> Res<()> {
    let bytes = std::fs::read(data.join("generic.file"))?;
    let mut rng = opensow_formats::rng::GameRng::from_file(&bytes).ok_or("generic.file too short")?;
    let t = opensow_formats::curves::CurveTable::generate(&mut rng);
    for (i, c) in t.curves.iter().enumerate() {
        let last = c.points.last().copied().unwrap_or((0, 0));
        let peak = c.lift.iter().max().copied().unwrap_or(0);
        let _ = (last, peak);
        println!("{i} {}", c.points.len());
        for (p, l) in c.points.iter().zip(&c.lift) {
            println!("{} {} {l}", p.0, p.1);
        }
    }
    Ok(())
}

/// `opensow curvebullet generic.file type rng_index x1 y1`: what a new type 13/14 bullet draws (the table is built from index 0,
/// then the stream continues at `rng_index`) and the path it gets.
fn print_curve_bullet(file: &Path, t: u8, index: usize, x1: i32, y1: i32) -> Res<()> {
    use opensow_formats::curves::{CurveTable, RandomBullet};
    let bytes = std::fs::read(file)?;
    let mut rng = opensow_formats::rng::GameRng::from_file(&bytes).ok_or("generic.file too short")?;
    let table = CurveTable::generate(&mut rng);
    while rng.index() != index {
        rng.next(1);
    }
    let b = RandomBullet::draw(t, &mut rng).ok_or("type must be 13 or 14")?;
    let p = table.path(b.curve, x1, y1, 0);
    println!("{} {} {}", b.speed, b.flag.map_or(-1, |f| f as i64), p.points.len());
    for (q, l) in p.points.iter().zip(&p.lift) {
        println!("{} {} {}", q.0, q.1, l);
    }
    Ok(())
}

/// `opensow sparks type variant values...`: what the constructor of a beam bullet (types 19-21) draws when the visual generator
/// returns `values[k] % n` for its k-th call, as `x y frame` lines after the delay; type 23 prints the end frame for `variant`.
fn print_sparks(t: u8, variant: i32, vals: &[&str]) -> Res<()> {
    use opensow_formats::sparks::{dim_end_frame, Sparks};
    if t == 23 {
        println!("{}", dim_end_frame(variant));
        return Ok(());
    }
    let vals: Vec<u32> = vals.iter().map(|v| v.parse()).collect::<Result<_, _>>()?;
    let mut k = 0;
    let s = Sparks::draw(t, |n| {
        let v = vals[k % vals.len()];
        k += 1;
        (v % n as u32) as u16
    })
    .ok_or("type must be 19, 20, 21 or 23")?;
    println!("{}", s.delay);
    for (x, y, c) in s.fragments {
        println!("{x} {y} {c}");
    }
    Ok(())
}

fn print_path(t: u8, x1: i32, y1: i32, x2: i32, y2: i32, delay: u32) -> Res<()> {
    let p = projectile::build_path(t, x1, y1, x2, y2, delay).ok_or("this bullet type has no computed path")?;
    match p.facing {
        Some(f) => println!("{} {} {f}", p.points.len(), p.first_step),
        None => println!("{} {}", p.points.len(), p.first_step),
    }
    for (q, l) in p.points.iter().zip(&p.lift) {
        println!("{} {} {}", q.0, q.1, l);
    }
    Ok(())
}

/// `opensow local|local2`: file = 16384 little-endian `u32` unit grid, the static grid (and for `local2` the first step grid);
/// route = x y pairs (the unit's cell first). Prints `stop new-base | next | local route`.
fn local_cmd(
    file: &str,
    ubase: &str,
    sbase: &str,
    fbase: Option<(&str, bool)>,
    own: &str,
    ax: &str,
    ay: &str,
    route: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(file)?;
    let words: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut w = opensow_formats::wave::Wave::new(|_, _| false);
    w.cells = words[..16384].to_vec();
    w.base = ubase.parse()?;
    let nums: Vec<i32> = route.iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
    let route: Vec<(i32, i32)> = nums.chunks(2).map(|c| (c[0], c[1])).collect();
    let at = (ax.parse()?, ay.parse()?);
    let r = match fbase {
        Some((fb, air)) => w.local_step_second(&words[16384..32768], sbase.parse()?, own.parse()?, &route, at, &words[32768..], fb.parse()?, air),
        None => w.local_step(&words[16384..], sbase.parse()?, own.parse()?, &route, at),
    };
    let next = r.next.map_or("none".to_string(), |(x, y)| format!("{x} {y}"));
    let path = r.path.iter().map(|(x, y)| format!("{x} {y}")).collect::<Vec<_>>().join(" ");
    println!("{} {} | {} | {}", u8::from(r.stop), w.base, next, path);
    Ok(())
}

/// `opensow netstep key=value ...`: runs `NetChassis::step` with a host that records its calls and prints the resulting state.
fn netstep_cmd(kv: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    use opensow_formats::netwalk::{NetChassis, NetHost};
    let map: std::collections::HashMap<&str, &str> = kv.iter().filter_map(|s| s.split_once('=')).collect();
    let get = |k: &str| -> &str { map.get(k).copied().unwrap_or("") };
    let nums = |k: &str| -> Result<Vec<i32>, std::num::ParseIntError> { get(k).split(',').filter(|s| !s.is_empty()).map(|s| s.parse()).collect() };
    let pair = |k: &str| -> Result<(i32, i32), Box<dyn std::error::Error>> {
        let v = nums(k)?;
        Ok((v[0], v[1]))
    };
    let pairs = |k: &str| -> Result<Vec<(i32, i32)>, Box<dyn std::error::Error>> { Ok(nums(k)?.chunks(2).map(|c| (c[0], c[1])).collect()) };
    let anim = nums("anim")?;
    let mut c = NetChassis {
        pos: pair("pos")?,
        dest: pair("dest")?,
        since: pair("since")?,
        row: nums("row")?.iter().map(|&v| v as u8).collect(),
        line: pairs("line")?,
        handle: get("handle").parse()?,
        air: get("air") == "1",
        kind40: get("kind40") == "1",
        prev: pair("prev")?,
        step1: pair("step1")?,
        step2: pair("step2")?,
        turn_steps: get("turn").parse()?,
        cooldown: get("cd").parse()?,
        anim_count: anim[0],
        anim_reload: anim[1],
        phase: anim[2],
        phases: anim[3],
        route_len: get("routelen").parse()?,
        ..Default::default()
    };
    /// What the planner answers: the first step cell, the second step cell and the pixel line.
    type Plan = ((i32, i32), (i32, i32), Vec<(i32, i32)>);
    struct Rec {
        calls: Vec<String>,
        guest: bool,
        route_ok: bool,
        plan: Plan,
    }
    impl NetHost for Rec {
        fn is_guest(&self) -> bool {
            self.guest
        }
        fn chase(&mut self, c: &mut NetChassis) {
            self.calls.push(format!("chase({},{})", c.since.0, c.since.1));
        }
        fn route(&mut self, c: &mut NetChassis) -> bool {
            self.calls.push(format!("route({},{})", c.pos.0, c.pos.1));
            self.route_ok
        }
        fn plan(&mut self, c: &mut NetChassis) {
            self.calls.push(format!("plan({},{})", c.pos.0, c.pos.1));
            (c.step1, c.step2, c.line) = self.plan.clone();
        }
        fn build_line(&mut self, c: &mut NetChassis, f: (i32, i32), t: (i32, i32)) {
            self.calls.push(format!("line({},{},{},{})", f.0, f.1, t.0, t.1));
            c.line = vec![f, t];
        }
        fn pop_order(&mut self) {
            self.calls.push("pop".into());
        }
        fn start_order(&mut self) {
            self.calls.push("start".into());
        }
        fn clear_orders(&mut self) {
            self.calls.push("clear".into());
        }
    }
    let mut host = Rec { calls: vec![], guest: get("guest") == "1", route_ok: get("route") == "1", plan: (pair("plan1")?, pair("plan2")?, pairs("planline")?) };
    let bytes = std::fs::read(get("stat"))?;
    let mut stat: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    c.step(pair("now")?, &mut stat, &mut host);
    let sum = stat.iter().enumerate().fold(0u32, |a, (i, &v)| a.wrapping_add((i as u32 + 1).wrapping_mul(v)));
    let line = c.line.iter().map(|(x, y)| format!("{x},{y}")).collect::<Vec<_>>().join(";");
    println!(
        "pos={},{} since={},{} line=[{}] prev={},{} step1={},{} step2={},{} facing={} want={} frame={} cd={} anim={},{} routelen={} stat={} calls={}",
        c.pos.0,
        c.pos.1,
        c.since.0,
        c.since.1,
        line,
        c.prev.0,
        c.prev.1,
        c.step1.0,
        c.step1.1,
        c.step2.0,
        c.step2.1,
        c.facing,
        c.want,
        c.frame,
        c.cooldown,
        c.anim_count,
        c.phase,
        c.route_len,
        sum,
        host.calls.join(" ")
    );
    Ok(())
}

/// `opensow turretspots KIND X Y`: the eight turret spots of a building (x0..x7 then y0..y7, the memory order at `[+0x224]`).
fn print_turret_spots(kind: i32, x: i32, y: i32) {
    let s = opensow_formats::turretai::Building { kind_id: kind, cell: (x, y) }.spots();
    let v: Vec<String> = s.iter().map(|p| p.0.to_string()).chain(s.iter().map(|p| p.1.to_string())).collect();
    println!("{}", v.join(" "));
}

/// `opensow turretai FILE`: runs `turretai::plan` on a description (`me P`, `credits C`, `flags a b c d e`, `b KIND X Y`,
/// `u CX CY OWNER KIND`, `aa CX CY`, `rng v...`) and prints every placement test (`try X Y W H`) and the result. The placement
/// test accepts a cell when `((x*31 + y*17) & 7) < 3`.
fn print_turret_ai(file: &Path) -> Res<()> {
    use opensow_formats::turretai::{plan, Building, Slot};
    let text = std::fs::read_to_string(file)?;
    let (mut me, mut credits, mut flags) = (0, 0, [false; 5]);
    let (mut buildings, mut slots, mut aa, mut rng) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for line in text.lines() {
        let v: Vec<i32> = line.split_whitespace().skip(1).filter_map(|t| t.parse().ok()).collect();
        match line.split_whitespace().next() {
            Some("me") => me = v[0],
            Some("credits") => credits = v[0],
            Some("flags") => (0..5).for_each(|i| flags[i] = v[i] != 0),
            Some("b") => buildings.push(Building { kind_id: v[0], cell: (v[1], v[2]) }),
            Some("u") => slots.push(Slot { cell: (v[0], v[1]), owner: v[2], kind: v[3] }),
            Some("aa") => aa.push((v[0], v[1])),
            Some("rng") => rng = v.iter().map(|&x| x as u32).collect(),
            _ => {}
        }
    }
    let mut next = rng.into_iter();
    let mut tries = Vec::new();
    let res = plan(
        &buildings,
        &slots,
        me,
        credits,
        &flags,
        |c| aa.contains(&c),
        |n| next.next().unwrap_or(0) % n,
        |x, y, w, h| {
            tries.push(format!("try {x} {y} {w} {h}"));
            ((x * 31 + y * 17) & 7) < 3
        },
    );
    for t in tries {
        println!("{t}");
    }
    match res {
        Some(p) => println!("place {} {} {}", p.cell.0, p.cell.1, 0x18 + p.tier),
        None => println!("none"),
    }
    Ok(())
}
