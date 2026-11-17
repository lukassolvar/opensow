// The application icon at build time, shared by the build scripts of opensow-launcher and opensow-game (`include!`d; it
// is not a module of either crate). Source: `crates/opensow-launcher/art/icon.png`, square RGBA (docs/release.md "Icons").
//
// Writes into OUT_DIR:
// - `icon64.rgba`: 64x64 RGBA for the window icon (`include_bytes!` in the program; Windows title bar and taskbar,
//   X11; macOS ignores window icons and shows the bundle's `.icns`);
// - on Windows targets `OpenSOW.ico` (PNG entries 16..256) linked into the `.exe` as resource 1, so Explorer, the
//   taskbar and shortcuts show it.

use std::path::{Path, PathBuf};

/// Decodes a square PNG into (side, straight RGBA bytes); RGB and grey PNGs get full alpha.
fn icon_decode(bytes: &[u8]) -> (usize, Vec<u8>) {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().expect("icon.png");
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).expect("icon.png frame");
    assert_eq!(info.width, info.height, "icon.png must be square");
    let n = info.width as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..n * n * 4].to_vec(),
        png::ColorType::Rgb => buf.chunks_exact(3).take(n * n).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).take(n * n).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().take(n * n).flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => unreachable!("EXPAND turns palettes into RGB(A)"),
    };
    (n, rgba)
}

/// Box filter from `n` x `n` down (or up) to `size` x `size`, averaging with premultiplied alpha so transparent edges
/// do not darken.
fn icon_scaled(rgba: &[u8], n: usize, size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size * size * 4);
    for oy in 0..size {
        let (y0, y1) = (oy * n / size, ((oy + 1) * n).div_ceil(size).max(oy * n / size + 1));
        for ox in 0..size {
            let (x0, x1) = (ox * n / size, ((ox + 1) * n).div_ceil(size).max(ox * n / size + 1));
            let mut acc = [0f64; 4];
            for y in y0..y1.min(n) {
                for x in x0..x1.min(n) {
                    let p = &rgba[(y * n + x) * 4..][..4];
                    let a = p[3] as f64 / 255.0;
                    acc[0] += p[0] as f64 * a;
                    acc[1] += p[1] as f64 * a;
                    acc[2] += p[2] as f64 * a;
                    acc[3] += a;
                }
            }
            let count = ((y1.min(n) - y0) * (x1.min(n) - x0)) as f64;
            if acc[3] <= 0.0 {
                out.extend([0, 0, 0, 0]);
            } else {
                let ch = |i: usize| (acc[i] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out.extend([ch(0), ch(1), ch(2), (acc[3] / count * 255.0).round().clamp(0.0, 255.0) as u8]);
            }
        }
    }
    out
}

fn icon_png(rgba: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, size as u32, size as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Best);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
    out
}

/// An `.ico` file whose entries are PNG images (Windows Vista and later read them at every size).
fn icon_ico(n: usize, rgba: &[u8], sizes: &[usize]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| icon_png(&icon_scaled(rgba, n, s), s)).collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes()); // reserved
    out.extend(1u16.to_le_bytes()); // type: icon
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&s, img) in sizes.iter().zip(&images) {
        let side = if s >= 256 { 0 } else { s as u8 }; // 0 means 256
        out.extend([side, side, 0, 0]); // width, height, palette, reserved
        out.extend(1u16.to_le_bytes()); // planes
        out.extend(32u16.to_le_bytes()); // bits per pixel
        out.extend((img.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in &images {
        out.extend(img);
    }
    out
}

/// Runs the icon part of a build script; `icon_png_path` is `crates/opensow-launcher/art/icon.png`.
fn build_icon(icon_png_path: &Path) {
    println!("cargo:rerun-if-changed={}", icon_png_path.display());
    let bytes = std::fs::read(icon_png_path).expect("read icon.png");
    let (n, rgba) = icon_decode(&bytes);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("icon64.rgba"), icon_scaled(&rgba, n, 64)).expect("write icon64.rgba");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let ico = out.join("OpenSOW.ico");
        std::fs::write(&ico, icon_ico(n, &rgba, &[16, 20, 24, 32, 40, 48, 64, 128, 256])).expect("write OpenSOW.ico");
        let rc = out.join("icon.rc");
        // rc strings take backslash escapes: forward slashes work in every resource compiler
        let path = ico.display().to_string().replace('\\', "/");
        std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).expect("write icon.rc");
        embed_resource::compile(&rc, embed_resource::NONE).manifest_required().expect("Windows icon resource");
    }
}
