//! Compares texture enhancement methods on the user's own install: `cargo run --release -p opensow-viewer --example upscale_preview -- out_dir`
//! Writes sprites.png (rows: original / Scale2x / smooth, all shown at the same size over a ground colour) and bg.png (crop of a level background).
use opensow_formats::{megafile::MegaFile, sprite, srf, GameDir};
use opensow_viewer::upscale;

fn write(path: &std::path::Path, w: usize, h: usize, rgba: &[u8]) {
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).unwrap()), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// nearest-neighbour enlargement
fn nearest(rgba: &[u8], w: usize, h: usize, k: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * k * h * k * 4];
    for y in 0..h * k {
        for x in 0..w * k {
            out[(y * w * k + x) * 4..][..4].copy_from_slice(&rgba[((y / k) * w + x / k) * 4..][..4]);
        }
    }
    out
}

fn over(rgba: &mut [u8], ground: [u8; 3]) {
    for p in rgba.chunks_mut(4) {
        if p[3] == 0 {
            p[..3].copy_from_slice(&ground);
        }
        p[3] = 255;
    }
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let game = GameDir::locate().expect("game not found");
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let picks = [("units\\1A_blue.ps6", 8), ("units\\1C_blue.ps6", 0), ("builds\\blue\\_base.ps6", 0), ("builds\\blue\\_heavy.ps6", 0), ("builds\\blue\\sentry01.ps6", 0), ("extras\\money.ps6", 0), ("units\\2A_blue.ps6", 16)];
    let ground = [96, 110, 70];
    let k = 4; // display factor of the ORIGINAL; 2x data is shown at k/2
    let mut cells: Vec<Vec<Vec<u8>>> = Vec::new(); // per pick: 3 images of the same size
    let mut sizes = Vec::new();
    for (name, idx) in picks {
        let Some(e) = mega.find(name) else { eprintln!("missing {name}"); continue };
        let frames = sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap();
        let f = &frames[idx.min(frames.len() - 1)];
        let (w, h) = (f.width as usize, f.height as usize);
        let rgba = f.to_rgba(None);
        let mut a = nearest(&rgba, w, h, k);
        let mut b = nearest(&upscale::scale2x(&rgba, w, h), w * 2, h * 2, k / 2);
        let mut c = nearest(&upscale::smooth2x(&rgba, w, h), w * 2, h * 2, k / 2);
        let mut d = nearest(&upscale::smooth(&rgba, w, h, 2, 0.6), w * 2, h * 2, k / 2);
        for img in [&mut a, &mut b, &mut c, &mut d] {
            over(img, ground);
        }
        cells.push(vec![a, b, c, d]);
        sizes.push((w * k, h * k));
    }
    let (cw, ch) = (sizes.iter().map(|s| s.0).max().unwrap() + 8, sizes.iter().map(|s| s.1).max().unwrap() + 8);
    let (tw, th) = (cw * cells.len(), ch * 4);
    let mut sheet = vec![255u8; tw * th * 4];
    for (i, row) in cells.iter().enumerate() {
        for (r, img) in row.iter().enumerate() {
            let (w, h) = sizes[i];
            for y in 0..h {
                let d = ((r * ch + y + 4) * tw + i * cw + 4) * 4;
                sheet[d..d + w * 4].copy_from_slice(&img[y * w * 4..(y + 1) * w * 4]);
            }
        }
    }
    write(&out.join("sprites.png"), tw, th, &sheet);

    // background crop: original / Lanczos / Lanczos + sharpen
    let srf_bytes = std::fs::read(game.resolve("Data\\Levels\\M01.srf").unwrap()).unwrap();
    let jpg = srf::to_jpeg(&srf_bytes).unwrap();
    let mut dec = jpeg_decoder::Decoder::new(&jpg[..]);
    let px = dec.decode().unwrap();
    let info = dec.info().unwrap();
    let (bw, bh) = (info.width as usize, info.height as usize);
    let (x0, y0, cw2, ch2) = (700usize, 700usize, 160usize, 120usize);
    let mut crop = Vec::new();
    for y in y0..y0 + ch2 {
        crop.extend_from_slice(&px[(y * bw + x0) * 3..(y * bw + x0 + cw2) * 3]);
    }
    let rgba = |rgb: &[u8]| rgb.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect::<Vec<u8>>();
    let a = nearest(&rgba(&crop), cw2, ch2, 4);
    let b = nearest(&rgba(&upscale::lanczos(&crop, cw2, ch2, 2, 0.0)), cw2 * 2, ch2 * 2, 2);
    let c = nearest(&rgba(&upscale::lanczos(&crop, cw2, ch2, 2, 0.7)), cw2 * 2, ch2 * 2, 2);
    let (w, h) = (cw2 * 4, ch2 * 4);
    let mut sheet = vec![255u8; w * 3 * h * 4];
    for (i, img) in [a, b, c].iter().enumerate() {
        for y in 0..h {
            sheet[(y * w * 3 + i * w) * 4..][..w * 4].copy_from_slice(&img[y * w * 4..(y + 1) * w * 4]);
        }
    }
    write(&out.join("bg.png"), w * 3, h, &sheet);
    println!("wrote {}/sprites.png and bg.png (background {bw}x{bh})", out.display());
}
