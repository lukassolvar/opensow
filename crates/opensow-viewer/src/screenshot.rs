//! Saving a frame buffer as a PNG, for the headless `--screenshot` runs of the viewer and the game.

/// Writes `buf` (`w` x `h` pixels, each `0x00RRGGBB`) to `path` as an 8-bit RGB PNG.
///
/// Panics when the file cannot be written: a screenshot run has nothing else to do.
pub fn write_png(path: &str, buf: &[u32], w: usize, h: usize) {
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).expect("create png")), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&rgb)).expect("write png");
}
