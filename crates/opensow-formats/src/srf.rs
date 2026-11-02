//! `.srf` / `.srfa` images: a baseline JPEG (decoded in-game by Intel IJL)
//! whose SOI/APP0 header has been replaced by ~0x37 obfuscated bytes.
//! Restoring a JPEG means writing `FF D8` and copying from the first DQT marker.

use crate::{Error, Result};

const DQT: [u8; 2] = [0xFF, 0xDB];

/// Returns a standard JPEG byte stream for an SRF file.
pub fn to_jpeg(srf: &[u8]) -> Result<Vec<u8>> {
    let start = srf
        .windows(2)
        .position(|w| w == DQT)
        .ok_or_else(|| Error::Invalid("no JPEG DQT marker in SRF".into()))?;
    let mut out = Vec::with_capacity(srf.len() - start + 2);
    out.extend([0xFF, 0xD8]);
    out.extend(&srf[start..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn restores_soi() {
        let srf = [0x41, 0x03, 0x00, 0xFF, 0xDB, 0x00, 0x84];
        assert_eq!(super::to_jpeg(&srf).unwrap(), [0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x84]);
    }
}
