//! MPEG-1 Layer II audio (ISO 11172-3) decoder for the `.sgv` movies.
//!
//! All `.sgv` soundtracks are 44.1 kHz layer II, 160 or 192 kbit/s stereo; the
//! allocation tables used by those rates (B.2a/B.2b) are implemented, other
//! bit-rate classes are rejected. Output is interleaved 16-bit PCM.

use crate::{Error, Result};
use std::sync::OnceLock;

/// Decoded soundtrack.
#[derive(Debug, Clone, Default)]
pub struct Audio {
    /// Samples per second.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: usize,
    /// Interleaved samples.
    pub pcm: Vec<i16>,
}

impl Audio {
    /// Length of the soundtrack in seconds.
    pub fn seconds(&self) -> f64 {
        self.pcm.len() as f64 / (self.channels.max(1) as f64 * self.sample_rate.max(1) as f64)
    }

    /// A canonical 16-bit PCM WAV file.
    pub fn to_wav(&self) -> Vec<u8> {
        let data = self.pcm.len() * 2;
        let mut w = Vec::with_capacity(44 + data);
        w.extend(b"RIFF");
        w.extend((36 + data as u32).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend((self.channels as u16).to_le_bytes());
        w.extend(self.sample_rate.to_le_bytes());
        w.extend((self.sample_rate * self.channels as u32 * 2).to_le_bytes());
        w.extend((self.channels as u16 * 2).to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend((data as u32).to_le_bytes());
        for s in &self.pcm {
            w.extend(s.to_le_bytes());
        }
        w
    }
}

const BITRATES: [u32; 15] = [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384];
const SAMPLE_RATES: [u32; 3] = [44100, 48000, 32000];

/// Quantiser class steps; classes 0, 1 and 3 (3, 5 and 9 steps) pack three samples into one code.
const STEPS: [u32; 17] = [3, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535];
const BITS: [u32; 17] = [5, 7, 3, 10, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// Allocation table B.2a/B.2b: (bits of the allocation field, class per code); code 0 is "no samples".
const ALLOC: [(u32, &[u8]); 4] = [
    (4, &[0, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (4, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16]),
    (3, &[0, 1, 2, 3, 4, 5, 16]),
    (2, &[0, 1, 16]),
];

fn alloc_row(sb: usize) -> (u32, &'static [u8]) {
    ALLOC[match sb {
        0..=2 => 0,
        3..=10 => 1,
        11..=22 => 2,
        _ => 3,
    }]
}

// The synthesis window D[i] * 65536 for i = 0..=256 (the rest follows by symmetry).
const WINDOW: [i32; 257] = [
    0, -1, -1, -1, -1, -1, -1, -2, -2, -2, -2, -3, -3, -4, -4, -5, -5, -6, -7, -7, -8, -9, -10, -11, -13, -14, -16, -17, -19, -21, -24, -26, -29, -31, -35,
    -38, -41, -45, -49, -53, -58, -63, -68, -73, -79, -85, -91, -97, -104, -111, -117, -125, -132, -139, -147, -154, -161, -169, -176, -183, -190, -196, -202,
    -208, 213, 218, 222, 225, 227, 228, 228, 227, 224, 221, 215, 208, 200, 189, 177, 163, 146, 127, 106, 83, 57, 29, -2, -36, -72, -111, -153, -197, -244,
    -294, -347, -401, -459, -519, -581, -645, -711, -779, -848, -919, -991, -1064, -1137, -1210, -1283, -1356, -1428, -1498, -1567, -1634, -1698, -1759, -1817,
    -1870, -1919, -1962, -2001, -2032, -2057, -2075, -2085, -2087, -2080, -2063, 2037, 2000, 1952, 1893, 1822, 1739, 1644, 1535, 1414, 1280, 1131, 970, 794,
    605, 402, 185, -45, -288, -545, -814, -1095, -1388, -1692, -2006, -2330, -2663, -3004, -3351, -3705, -4063, -4425, -4788, -5153, -5517, -5879, -6237,
    -6589, -6935, -7271, -7597, -7910, -8209, -8491, -8755, -8998, -9219, -9416, -9585, -9727, -9838, -9916, -9959, -9966, -9935, -9863, -9750, -9592, -9389,
    -9139, -8840, -8492, -8092, -7640, -7134, 6574, 5959, 5288, 4561, 3776, 2935, 2037, 1082, 70, -998, -2122, -3300, -4533, -5818, -7154, -8540, -9975,
    -11455, -12980, -14548, -16155, -17799, -19478, -21189, -22929, -24694, -26482, -28289, -30112, -31947, -33791, -35640, -37489, -39336, -41176, -43006,
    -44821, -46617, -48390, -50137, -51853, -53534, -55178, -56778, -58333, -59838, -61289, -62684, -64019, -65290, -66494, -67629, -68692, -69679, -70590,
    -71420, -72169, -72835, -73415, -73908, -74313, -74630, -74856, -74992, 75038,
];

struct Tables {
    /// N[i][k] = cos((16 + i)(2k + 1) pi / 64)
    matrix: [[f32; 32]; 64],
    window: [f32; 512],
    scale: [f32; 64],
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut matrix = [[0f32; 32]; 64];
        for (i, row) in matrix.iter_mut().enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                *v = (((16 + i) * (2 * k + 1)) as f64 * std::f64::consts::PI / 64.0).cos() as f32;
            }
        }
        let mut window = [0f32; 512];
        for i in 0..512 {
            let v = if i <= 256 {
                WINDOW[i] as f32
            } else {
                // the prototype filter is symmetric and the window flips sign every 64 taps, so the
                // mirrored half is the negated first half except at multiples of 64
                let m = 512 - i;
                if m % 64 == 0 {
                    WINDOW[m] as f32
                } else {
                    -(WINDOW[m] as f32)
                }
            };
            window[i] = v / 65536.0;
        }
        let mut scale = [0f32; 64];
        for (i, s) in scale.iter_mut().enumerate() {
            *s = 2f64.powf(1.0 - i as f64 / 3.0) as f32;
        }
        Tables { matrix, window, scale }
    })
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data.get(self.pos >> 3).copied().unwrap_or(0);
            v = v << 1 | (byte >> (7 - (self.pos & 7)) & 1) as u32;
            self.pos += 1;
        }
        v
    }

    fn overrun(&self) -> bool {
        self.pos > self.data.len() * 8
    }
}

/// Polyphase synthesis state of one channel.
struct Synth {
    v: [f32; 1024],
}

impl Synth {
    fn new() -> Synth {
        Synth { v: [0.0; 1024] }
    }

    /// One slot of 32 subband samples to 32 PCM samples.
    fn slot(&mut self, s: &[f32; 32], out: &mut [f32; 32]) {
        let t = tables();
        self.v.copy_within(0..960, 64);
        for i in 0..64 {
            self.v[i] = t.matrix[i].iter().zip(s).map(|(a, b)| a * b).sum();
        }
        for (j, o) in out.iter_mut().enumerate() {
            let mut acc = 0f32;
            for i in 0..8 {
                acc += self.v[i * 128 + j] * t.window[i * 64 + j];
                acc += self.v[i * 128 + 96 + j] * t.window[i * 64 + 32 + j];
            }
            *o = acc;
        }
    }
}

/// Decode a layer II elementary stream (concatenated frames).
pub fn decode(es: &[u8]) -> Result<Audio> {
    let t = tables();
    let mut audio = Audio::default();
    let mut synth = [Synth::new(), Synth::new()];
    let mut pos = 0;
    while pos + 4 <= es.len() {
        if es[pos] != 0xff || es[pos + 1] & 0xf6 != 0xf4 {
            pos += 1; // resync (layer II, MPEG-1)
            continue;
        }
        let (b2, b3) = (es[pos + 2], es[pos + 3]);
        let bitrate_index = (b2 >> 4) as usize;
        let rate_index = (b2 >> 2 & 3) as usize;
        if bitrate_index == 0 || bitrate_index > 14 || rate_index == 3 {
            pos += 1;
            continue;
        }
        let (bitrate, rate) = (BITRATES[bitrate_index], SAMPLE_RATES[rate_index]);
        let size = (144_000 * bitrate / rate) as usize + (b2 >> 1 & 1) as usize;
        if pos + size > es.len() {
            break;
        }
        let mode = b3 >> 6;
        let channels = if mode == 3 { 1 } else { 2 };
        if audio.channels == 0 {
            audio.sample_rate = rate;
            audio.channels = channels;
        } else if audio.channels != channels || audio.sample_rate != rate {
            return Err(Error::Invalid("audio format changes inside the stream".into()));
        }
        let per_channel = bitrate / channels as u32;
        let sblimit = match (rate, per_channel) {
            (48000, 56..) | (_, 56..=80) => 27,
            (44100 | 32000, 96..) => 30,
            _ => return Err(Error::Invalid(format!("unsupported layer II table at {bitrate} kbit/s, {rate} Hz"))),
        };
        let bound = if mode == 1 { (4 + 4 * (b3 >> 4 & 3) as usize).min(sblimit) } else { sblimit };
        let mut b = Bits { data: &es[pos..pos + size], pos: 32 };
        if es[pos + 1] & 1 == 0 {
            b.pos += 16; // CRC
        }
        let mut alloc = [[0u8; 32]; 2];
        for sb in 0..sblimit {
            let (nbal, _) = alloc_row(sb);
            for ch in 0..channels {
                if sb < bound || ch == 0 {
                    alloc[ch][sb] = b.read(nbal) as u8;
                } else {
                    alloc[ch][sb] = alloc[0][sb];
                }
            }
        }
        let mut scfsi = [[0u8; 32]; 2];
        for sb in 0..sblimit {
            for ch in 0..channels {
                if alloc[ch][sb] != 0 {
                    scfsi[ch][sb] = b.read(2) as u8;
                }
            }
        }
        let mut sf = [[[0u8; 3]; 32]; 2];
        for sb in 0..sblimit {
            for ch in 0..channels {
                if alloc[ch][sb] == 0 {
                    continue;
                }
                let s = &mut sf[ch][sb];
                match scfsi[ch][sb] {
                    0 => *s = [b.read(6) as u8, b.read(6) as u8, b.read(6) as u8],
                    1 => {
                        let (a, c) = (b.read(6) as u8, b.read(6) as u8);
                        *s = [a, a, c];
                    }
                    2 => {
                        let a = b.read(6) as u8;
                        *s = [a, a, a];
                    }
                    _ => {
                        let (a, c) = (b.read(6) as u8, b.read(6) as u8);
                        *s = [a, c, c];
                    }
                }
            }
        }
        // 12 groups of three samples per subband
        let mut sub = [[[0f32; 32]; 36]; 2];
        for gr in 0..12 {
            for sb in 0..sblimit {
                let (_, classes) = alloc_row(sb);
                let mut shared = [0f32; 3];
                for ch in 0..channels {
                    let code = alloc[ch][sb] as usize;
                    if code == 0 {
                        continue;
                    }
                    let class = *classes.get(code - 1).ok_or_else(|| Error::Invalid("bad allocation code".into()))? as usize;
                    let n = STEPS[class];
                    let mut samples = [0u32; 3];
                    if sb >= bound && ch == 1 {
                        // joint stereo: the quantised samples are shared, scale factors are not
                        for (j, v) in shared.iter().enumerate() {
                            samples[j] = *v as u32;
                        }
                    } else if matches!(class, 0 | 1 | 3) {
                        let mut c = b.read(BITS[class]);
                        for s in &mut samples {
                            *s = c % n;
                            c /= n;
                        }
                    } else {
                        for s in &mut samples {
                            *s = b.read(BITS[class]);
                        }
                    }
                    if sb >= bound && ch == 0 {
                        for (j, s) in samples.iter().enumerate() {
                            shared[j] = *s as f32;
                        }
                    }
                    let factor = t.scale[sf[ch][sb][gr / 4] as usize % 64];
                    for (j, &k) in samples.iter().enumerate() {
                        let v = (2.0 * k as f32 - (n - 1) as f32) / n as f32;
                        sub[ch][gr * 3 + j][sb] = v * factor;
                    }
                }
            }
        }
        if b.overrun() {
            return Err(Error::Invalid(format!("layer II frame at {pos:#x} runs past its end")));
        }
        let base = audio.pcm.len();
        audio.pcm.resize(base + 1152 * channels, 0);
        for ch in 0..channels {
            for slot in 0..36 {
                let mut out = [0f32; 32];
                synth[ch].slot(&sub[ch][slot], &mut out);
                for (j, v) in out.iter().enumerate() {
                    audio.pcm[base + (slot * 32 + j) * channels + ch] = (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
                }
            }
        }
        pos += size;
    }
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_the_filter_prototype() {
        let t = tables();
        assert_eq!(t.window[0], 0.0);
        assert!((t.window[256] - 1.144_989).abs() < 1e-8);
        assert!((t.window[1] + 0.000015259).abs() < 1e-8);
        assert!((t.window[64] - 0.003250122).abs() < 1e-8);
        // second half: negated mirror, except at multiples of 64
        assert!((t.window[511] - 0.000015259).abs() < 1e-8);
        assert_eq!(t.window[448], t.window[64]);
        assert_eq!(t.window[257], -t.window[255]);
    }

    #[test]
    fn scale_factors_fall_by_the_cube_root_of_two() {
        let t = tables();
        assert_eq!(t.scale[0], 2.0);
        assert!((t.scale[3] - 1.0).abs() < 1e-6);
        assert!((t.scale[1] / t.scale[0] - 2f32.powf(-1.0 / 3.0)).abs() < 1e-6);
    }

    /// 192 kbit/s, 44.1 kHz, stereo layer II frame whose every subband is unallocated: silence.
    fn silent_frame() -> Vec<u8> {
        let mut f = vec![0xff, 0xfd, 0xa0, 0x00]; // no CRC, bitrate index 10, 44.1 kHz, stereo
        f.resize(144_000 * 192 / 44100, 0);
        f
    }

    #[test]
    fn decodes_silence_and_skips_garbage() {
        let mut es = vec![1, 2, 3];
        es.extend(silent_frame());
        es.extend(silent_frame());
        let a = decode(&es).unwrap();
        assert_eq!((a.sample_rate, a.channels, a.pcm.len()), (44100, 2, 2 * 1152 * 2));
        assert!(a.pcm.iter().all(|&s| s == 0));
        assert!((a.seconds() - 2304.0 / 44100.0).abs() < 1e-9);
        let wav = a.to_wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + a.pcm.len() * 2);
    }

    #[test]
    fn rejects_unsupported_rates() {
        // 32 kbit/s mono needs the low-rate allocation tables
        let mut f = vec![0xff, 0xfd, 0x14, 0xc0];
        f.resize(144_000 * 32 / 44100 + 1, 0);
        assert!(decode(&f).is_err());
    }
}
