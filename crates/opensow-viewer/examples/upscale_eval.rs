//! Objective check of the enhancement filters on the user's own files: shrink real art 2x (box filter), enlarge it back with each
//! method and compare with the original (PSNR, higher is better). `cargo run --release -p opensow-viewer --example upscale_eval`
use opensow_formats::{megafile::MegaFile, sprite, srf, GameDir};
use opensow_viewer::upscale;

fn psnr(a: &[u8], b: &[u8], skip: impl Fn(usize) -> bool) -> f64 {
    let (mut se, mut n) = (0f64, 0f64);
    for i in 0..a.len() / 3 {
        if skip(i) {
            continue;
        }
        for k in 0..3 {
            let d = a[i * 3 + k] as f64 - b[i * 3 + k] as f64;
            se += d * d;
        }
        n += 3.0;
    }
    10.0 * (255.0f64 * 255.0 / (se / n).max(1e-9)).log10()
}

fn box2(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (nw, nh) = (w / 2, h / 2);
    let mut out = vec![0u8; nw * nh * 3];
    for y in 0..nh {
        for x in 0..nw {
            for k in 0..3 {
                let s: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|&(dx, dy)| rgb[((2 * y + dy) * w + 2 * x + dx) * 3 + k] as u32).sum();
                out[(y * nw + x) * 3 + k] = ((s + 2) / 4) as u8;
            }
        }
    }
    out
}

fn nearest(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * 2 * h * 2 * 3];
    for y in 0..h * 2 {
        for x in 0..w * 2 {
            out[(y * w * 2 + x) * 3..][..3].copy_from_slice(&rgb[((y / 2) * w + x / 2) * 3..][..3]);
        }
    }
    out
}

fn bilinear(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * 2 * h * 2 * 3];
    for y in 0..h * 2 {
        for x in 0..w * 2 {
            let (fx, fy) = ((x as f32 + 0.5) / 2.0 - 0.5, (y as f32 + 0.5) / 2.0 - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - x0, fy - y0);
            let at = |xx: f32, yy: f32, k: usize| rgb[((yy.clamp(0.0, h as f32 - 1.0) as usize) * w + xx.clamp(0.0, w as f32 - 1.0) as usize) * 3 + k] as f32;
            for k in 0..3 {
                let v = (at(x0, y0, k) * (1.0 - tx) + at(x0 + 1.0, y0, k) * tx) * (1.0 - ty)
                    + (at(x0, y0 + 1.0, k) * (1.0 - tx) + at(x0 + 1.0, y0 + 1.0, k) * tx) * ty;
                out[(y * w * 2 + x) * 3 + k] = v.round() as u8;
            }
        }
    }
    out
}

fn report(title: &str, orig: &[u8], w: usize, h: usize, skip: &dyn Fn(usize) -> bool) {
    let small = box2(orig, w, h);
    let (sw, sh) = (w / 2, h / 2);
    println!("{title}");
    for (name, img) in [
        ("nearest (what zoom does today)", nearest(&small, sw, sh)),
        ("bilinear", bilinear(&small, sw, sh)),
        ("Lanczos3", upscale::lanczos(&small, sw, sh, 2, 0.0)),
        ("Lanczos3 + unsharp 0.5 (ground filter)", upscale::lanczos(&small, sw, sh, 2, 0.5)),
    ] {
        println!("  {name:44} {:6.2} dB", psnr(orig, &img, skip));
    }
}

fn main() {
    let game = GameDir::locate().expect("game not found");
    // ground: central 1024x1024 of M01
    let jpg = srf::to_jpeg(&std::fs::read(game.resolve("Data\\Levels\\M01.srf").unwrap()).unwrap()).unwrap();
    let mut dec = jpeg_decoder::Decoder::new(&jpg[..]);
    let px = dec.decode().unwrap();
    let bw = dec.info().unwrap().width as usize;
    let mut crop = Vec::new();
    for y in 512..1536 {
        crop.extend_from_slice(&px[(y * bw + 512) * 3..(y * bw + 1536) * 3]);
    }
    report("Level ground (M01 centre 1024x1024, shrunk 2x then enlarged back)", &crop, 1024, 1024, &|_| false);

    // sprites: building frames, only pixels whose 2x2 block is fully opaque
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    for name in ["builds\\blue\\_base.ps6", "builds\\blue\\_heavy.ps6", "builds\\blue\\_wind.ps6", "units\\1A_blue.ps6"] {
        let Some(e) = mega.find(name) else { continue };
        let frames = sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap();
        let f = &frames[0];
        let (w, h) = ((f.width as usize) & !1, (f.height as usize) & !1);
        let at = |x: usize, y: usize| (f.mask[y * f.width as usize + x], sprite::rgb565_to_rgb8(f.pixels[y * f.width as usize + x]));
        let mut rgb = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                rgb[(y * w + x) * 3..][..3].copy_from_slice(&at(x, y).1);
            }
        }
        let full = |i: usize| {
            let (x, y) = (i % w, i / w);
            let (bx, by) = (x & !1, y & !1);
            ![(0, 0), (1, 0), (0, 1), (1, 1)].iter().all(|&(dx, dy)| at(bx + dx, by + dy).0)
        };
        report(&format!("Sprite {name} frame 0 ({w}x{h}, opaque 2x2 blocks only)"), &rgb, w, h, &full);
    }
}
