//! `.srf` / `.srfa` images: a baseline JPEG (decoded in-game by Intel IJL)
//! whose SOI/APP0 header has been replaced by ~0x37 obfuscated bytes.
//! Restoring a JPEG means writing `FF D8` and copying from the first DQT marker.

use crate::{Error, Result};

const DQT: [u8; 2] = [0xFF, 0xDB];

/// Returns a standard JPEG byte stream for an SRF file.
pub fn to_jpeg(srf: &[u8]) -> Result<Vec<u8>> {
    let start = srf.windows(2).position(|w| w == DQT).ok_or_else(|| Error::Invalid("no JPEG DQT marker in SRF".into()))?;
    let mut out = Vec::with_capacity(srf.len() - start + 2);
    out.extend([0xFF, 0xD8]);
    out.extend(&srf[start..]);
    Ok(out)
}

/// Inverse of [`to_jpeg`]: keeps the obfuscated header of `template` (any existing `.srf`; every file
/// carries the same 55 bytes) and appends the JPEG's segments in the installed files' order: DQT tables, then SOF,
/// then the rest (DHT, DRI...) and the scan. SOI and APPn/COM segments are dropped (the header replaces them).
/// Encoders differ in segment order (some write SOF first), which is why they are re-sequenced.
/// Baseline and progressive JPEGs both occur in the install (IJL decodes either).
pub fn from_jpeg(template: &[u8], jpeg: &[u8]) -> Result<Vec<u8>> {
    let head = template.windows(2).position(|w| w == DQT).ok_or_else(|| Error::Invalid("no JPEG DQT marker in template SRF".into()))?;
    if jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
        return Err(Error::Invalid("not a JPEG (no SOI)".into()));
    }
    let (mut dqt, mut sof, mut rest) = (Vec::new(), Vec::new(), Vec::new());
    let mut i = 2;
    loop {
        if i + 4 > jpeg.len() || jpeg[i] != 0xFF {
            return Err(Error::Invalid("JPEG ends before its scan".into()));
        }
        let marker = jpeg[i + 1];
        if marker == 0xDA {
            break;
        }
        let end = i + 2 + u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        let seg = jpeg.get(i..end).ok_or_else(|| Error::Invalid("truncated JPEG segment".into()))?;
        match marker {
            0xE0..=0xEF | 0xFE => {}
            0xDB => dqt.extend(seg),
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => sof.extend(seg),
            _ => rest.extend(seg),
        }
        i = end;
    }
    if dqt.is_empty() || sof.is_empty() {
        return Err(Error::Invalid("JPEG lacks DQT or SOF".into()));
    }
    let mut out = template[..head].to_vec();
    for part in [&dqt[..], &sof, &rest, &jpeg[i..]] {
        out.extend(part);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn restores_soi() {
        let srf = [0x41, 0x03, 0x00, 0xFF, 0xDB, 0x00, 0x84];
        assert_eq!(super::to_jpeg(&srf).unwrap(), [0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x84]);
    }

    #[test]
    fn from_jpeg_round_trips() {
        // SOI, DQT, SOF0, DHT, SOS + data, EOI (the order of the installed files)
        let srf = [0x41, 0x03, 0x00, 0xFF, 0xDB, 0, 3, 1, 0xFF, 0xC0, 0, 3, 2, 0xFF, 0xC4, 0, 3, 3, 0xFF, 0xDA, 0, 2, 0xFF, 0xD9];
        let jpeg = super::to_jpeg(&srf).unwrap();
        assert_eq!(super::from_jpeg(&srf, &jpeg).unwrap(), srf);
        // a foreign JPEG: APP0 first, SOF before DQT (jpeg-encoder's order) -> same layout as the installed files
        let other = [0xFF, 0xD8, 0xFF, 0xE0, 0, 2, 0xFF, 0xC0, 0, 3, 2, 0xFF, 0xDB, 0, 3, 1, 0xFF, 0xC4, 0, 3, 3, 0xFF, 0xDA, 0, 2, 0xFF, 0xD9];
        assert_eq!(super::from_jpeg(&srf, &other).unwrap(), srf);
        assert!(super::from_jpeg(&srf, &[0xFF, 0xD8]).is_err());
        assert!(super::from_jpeg(&srf, &[0xFF, 0xD8, 0xFF, 0xDB, 0, 3, 1, 0xFF, 0xDA, 0, 2]).is_err());
    }
}
