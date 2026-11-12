//! Plain RIFF/WAVE PCM files: all of `Data\Sounds` (music, effects, speech) is uncompressed 16-bit, 22050 Hz, mono.

use crate::{Error, Reader, Result};

/// A decoded `.wav` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    /// Number of channels.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Interleaved 16-bit samples.
    pub pcm: Vec<i16>,
}

impl Wav {
    /// Length of the sound in seconds.
    pub fn seconds(&self) -> f64 {
        self.pcm.len() as f64 / (self.channels.max(1) as f64 * self.sample_rate.max(1) as f64)
    }
}

/// Parses a 16-bit PCM `.wav` file.
pub fn parse(data: &[u8]) -> Result<Wav> {
    let mut r = Reader::new(data);
    if r.bytes(4, "RIFF")? != b"RIFF" {
        return Err(Error::Invalid("not a RIFF file".into()));
    }
    r.u32("riff size")?;
    if r.bytes(4, "WAVE")? != b"WAVE" {
        return Err(Error::Invalid("not a WAVE file".into()));
    }
    let mut fmt = None;
    while r.remaining() >= 8 {
        let id = r.bytes(4, "chunk id")?.to_vec();
        let size = r.u32("chunk size")? as usize;
        // a damaged size on the last chunk still plays what is there
        let body = r.bytes(size.min(r.remaining()), "chunk")?;
        if size & 1 == 1 && r.remaining() > 0 {
            r.u8("pad")?;
        }
        match &id[..] {
            b"fmt " => {
                let mut f = Reader::new(body);
                let (tag, channels, rate) = (f.u16("tag")?, f.u16("channels")?, f.u32("rate")?);
                f.u32("byte rate")?;
                f.u16("align")?;
                let bits = f.u16("bits")?;
                if tag != 1 || bits != 16 || channels == 0 {
                    return Err(Error::Invalid(format!("unsupported wav format tag {tag}, {bits} bits, {channels} channels")));
                }
                fmt = Some((channels, rate));
            }
            b"data" => {
                let (channels, sample_rate) = fmt.ok_or_else(|| Error::Invalid("wav data before fmt".into()))?;
                let pcm = body.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                return Ok(Wav { channels, sample_rate, pcm });
            }
            _ => {}
        }
    }
    Err(Error::Invalid("wav has no data chunk".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(samples: &[i16]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(b"RIFF");
        d.extend((36 + samples.len() as u32 * 2).to_le_bytes());
        d.extend(b"WAVEfmt ");
        d.extend(16u32.to_le_bytes());
        d.extend([1, 0, 1, 0]);
        d.extend(22050u32.to_le_bytes());
        d.extend(44100u32.to_le_bytes());
        d.extend([2, 0, 16, 0]);
        d.extend(b"LIST");
        d.extend(3u32.to_le_bytes());
        d.extend([1, 2, 3, 0]); // odd chunk + pad byte
        d.extend(b"data");
        d.extend((samples.len() as u32 * 2).to_le_bytes());
        d.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        d
    }

    #[test]
    fn reads_pcm_and_skips_other_chunks() {
        let w = parse(&make(&[1, -2, 300])).unwrap();
        assert_eq!((w.channels, w.sample_rate, w.pcm.clone()), (1, 22050, vec![1, -2, 300]));
    }

    #[test]
    fn rejects_other_data() {
        assert!(parse(b"RIFX0000WAVE").is_err());
        assert!(parse(&make(&[1])[..30]).is_err());
    }
}
