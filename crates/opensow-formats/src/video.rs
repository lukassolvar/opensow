//! `.sgv` videos: briefing/intro movies in `Data/Levels`.
//!
//! The exe has no decoder of its own: it builds a DirectShow multimedia
//! stream (`IAMMultiMediaStream`, object at 0x6b409c) and opens the path, so
//! the operating system's codecs do the work. The files are therefore ordinary
//! media: most are MPEG-1 system streams, `mA`/`mB` are AVI (Indeo 5 video).
//! This module only walks the containers (see docs/video.md).

use crate::{Error, Reader, Result};

/// MPEG-1 picture-rate codes (ISO 11172-2 table), frames per second.
pub const FRAME_RATES: [f64; 9] = [0.0, 23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94, 60.0];

/// Picture coding types by the 3-bit field.
pub const PICTURE_TYPES: [&str; 5] = ["?", "I", "P", "B", "D"];

/// What the first audio frame header of an MPEG stream says.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MpegAudio {
    /// MPEG audio layer (1, 2 or 3).
    pub layer: u8,
    /// Bit rate in kbit/s.
    pub bitrate_kbps: u32,
    /// Samples per second.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u8,
}

/// Summary of an MPEG-1 system stream (the `.sgv` movies M01-M22, mI, mO), gathered by walking its packs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mpeg {
    /// Number of packs.
    pub packs: u32,
    /// Number of video packets.
    pub video_packets: u32,
    /// Number of audio packets.
    pub audio_packets: u32,
    /// Number of padding packets.
    pub padding_packets: u32,
    /// Number of packets of any other stream.
    pub other_packets: u32,
    /// Picture width in pixels.
    pub width: u16,
    /// Picture height in pixels.
    pub height: u16,
    /// Index into [`FRAME_RATES`].
    pub frame_rate_code: u8,
    /// Video bit rate in bits per second (0x3ffff = variable).
    pub bit_rate: Option<u32>,
    /// Mux rate in bytes per second (50 per unit).
    pub mux_rate: u32,
    /// Number of groups of pictures.
    pub gops: u32,
    /// Pictures by type I, P, B, D.
    pub pictures: [u32; 4],
    /// The audio stream, when the movie has one.
    pub audio: Option<MpegAudio>,
    /// System clock reference of the last pack, seconds (90 kHz clock).
    pub duration: f64,
}

impl Mpeg {
    /// Frames per second, 0 for an unknown code.
    pub fn frame_rate(&self) -> f64 {
        FRAME_RATES.get(self.frame_rate_code as usize).copied().unwrap_or(0.0)
    }
}

/// One stream of an AVI file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AviStream {
    /// `vids` or `auds`.
    pub kind: [u8; 4],
    /// Codec fourcc (video) or format tag as text (audio).
    pub handler: [u8; 4],
    /// Time scale: `rate / scale` is frames (or samples) per second.
    pub scale: u32,
    /// Rate, see `scale`.
    pub rate: u32,
    /// Length of the stream in `scale / rate` units.
    pub length: u32,
    /// Video: width, height, bit count; audio: channels, sample rate, bits.
    pub format: [u32; 3],
    /// Number of data chunks of this stream.
    pub chunks: u32,
    /// The stream format chunk as stored (BITMAPINFO with its palette, or WAVEFORMAT).
    pub strf: Vec<u8>,
}

/// Summary of an AVI file (the `.sgv` movies mA and mB).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Avi {
    /// Microseconds per frame, from the main header.
    pub micros_per_frame: u32,
    /// Total number of frames.
    pub frames: u32,
    /// Picture width in pixels.
    pub width: u32,
    /// Picture height in pixels.
    pub height: u32,
    /// The streams, in file order.
    pub streams: Vec<AviStream>,
}

/// A parsed `.sgv` movie: MPEG-1 or AVI, told apart by the first four bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum Video {
    /// An MPEG-1 system stream.
    Mpeg(Mpeg),
    /// An AVI file.
    Avi(Avi),
}

/// Walks a `.sgv` file and summarises it.
pub fn parse(data: &[u8]) -> Result<Video> {
    match data.get(..4) {
        Some([0, 0, 1, 0xba]) => parse_mpeg(data).map(Video::Mpeg),
        Some(b"RIFF") => parse_avi(data).map(Video::Avi),
        _ => Err(Error::Invalid("not an MPEG-1 system stream or AVI".into())),
    }
}

fn scr(b: &[u8]) -> Option<f64> {
    // 0010 xxx1 xxxxxxxx xxxxxxx1 xxxxxxxx xxxxxxx1 (33 bits at 90 kHz)
    let v = (((b[0] >> 1) & 7) as u64) << 30 | (b[1] as u64) << 22 | ((b[2] >> 1) as u64) << 15 | (b[3] as u64) << 7 | (b[4] >> 1) as u64;
    (b[0] & 0xf0 == 0x20).then_some(v as f64 / 90_000.0)
}

/// Offset of the elementary payload inside an MPEG-1 PES packet body.
fn payload_start(body: &[u8]) -> usize {
    let mut i = 0;
    while body.get(i) == Some(&0xff) {
        i += 1;
    }
    if matches!(body.get(i), Some(b) if b >> 6 == 1) {
        i += 2; // STD buffer
    }
    match body.get(i) {
        Some(b) if b >> 4 == 2 => i + 5,
        Some(b) if b >> 4 == 3 => i + 10,
        Some(_) => i + 1,
        None => i,
    }
}

fn audio_header(p: &[u8]) -> Option<MpegAudio> {
    let at = p.windows(2).position(|w| w[0] == 0xff && w[1] & 0xe0 == 0xe0)?;
    let h = p.get(at..at + 4)?;
    if h[1] >> 3 & 3 != 3 {
        return None; // not MPEG-1 audio
    }
    let layer = 4 - (h[1] >> 1 & 3);
    const L2: [u32; 16] = [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0];
    const L3: [u32; 16] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
    const L1: [u32; 16] = [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0];
    let table = match layer {
        1 => &L1,
        2 => &L2,
        _ => &L3,
    };
    let rate = [44100, 48000, 32000, 0][(h[2] >> 2 & 3) as usize];
    (layer <= 3 && rate != 0).then(|| MpegAudio {
        layer,
        bitrate_kbps: table[(h[2] >> 4) as usize],
        sample_rate: rate,
        channels: if h[3] >> 6 == 3 { 1 } else { 2 },
    })
}

fn parse_mpeg(data: &[u8]) -> Result<Mpeg> {
    let mut m = Mpeg::default();
    let mut r = Reader::new(data);
    let mut last_scr = 0.0;
    // Elementary-stream scanner state: the last bytes of the previous video packet
    // can hold the start of a start code, so keep a three-byte carry.
    let mut carry: Vec<u8> = Vec::new();
    while r.remaining() >= 4 {
        // some files pad to a 2048-byte sector with zeros before the next pack
        while data.get(r.pos()..r.pos() + 4) == Some(&[0, 0, 0, 0]) || data.get(r.pos()..r.pos() + 3) == Some(&[0, 0, 0]) {
            r.bytes(1, "zero fill")?;
        }
        let code = r.u32("start code")?.swap_bytes();
        match code {
            0x1ba => {
                let b = r.bytes(8, "pack header")?;
                last_scr = scr(b).ok_or_else(|| Error::Invalid("not an MPEG-1 pack header".into()))?;
                m.mux_rate = ((b[5] as u32 & 0x7f) << 15 | (b[6] as u32) << 7 | (b[7] as u32 >> 1)) * 50;
                m.packs += 1;
            }
            0x1b9 => {} // the files hold several programs back to back: the audio runs on past the first end code
            0x1bb..=0x1ff => {
                let len = r.u16("packet length")?.swap_bytes() as usize;
                let body = r.bytes(len, "packet")?;
                match code & 0xff {
                    0xbb => {}
                    0xbe => m.padding_packets += 1,
                    0xe0..=0xef => {
                        m.video_packets += 1;
                        let es = &body[payload_start(body).min(body.len())..];
                        scan_video(&mut m, &mut carry, es);
                    }
                    0xc0..=0xdf => {
                        m.audio_packets += 1;
                        if m.audio.is_none() {
                            m.audio = audio_header(&body[payload_start(body).min(body.len())..]);
                        }
                    }
                    _ => m.other_packets += 1,
                }
            }
            _ => return Err(Error::Invalid(format!("unexpected start code {code:#x} at {:#x}", r.pos() - 4))),
        }
    }
    m.duration = last_scr;
    Ok(m)
}

fn scan_video(m: &mut Mpeg, carry: &mut Vec<u8>, es: &[u8]) {
    let mut buf = std::mem::take(carry);
    buf.extend_from_slice(es);
    let mut i = 0;
    while i + 4 <= buf.len() {
        if buf[i..i + 3] == [0, 0, 1] {
            // a code is decided once the bytes its header needs have arrived
            let need = match buf[i + 3] {
                0xb3 => 12,
                0x00 => 6,
                _ => 4,
            };
            if i + need > buf.len() {
                break;
            }
            match buf[i + 3] {
                0xb3 if m.width == 0 => {
                    let h = &buf[i + 4..i + 12];
                    m.width = (h[0] as u16) << 4 | (h[1] >> 4) as u16;
                    m.height = ((h[1] & 15) as u16) << 8 | h[2] as u16;
                    m.frame_rate_code = h[3] & 15;
                    let br = (h[4] as u32) << 10 | (h[5] as u32) << 2 | (h[6] >> 6) as u32;
                    m.bit_rate = (br != 0x3ffff).then_some(br * 400);
                }
                0xb8 => m.gops += 1,
                0x00 => {
                    let t = (buf[i + 5] >> 3 & 7) as usize;
                    if (1..=4).contains(&t) {
                        m.pictures[t - 1] += 1;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    // bytes before `i` are decided; keep the rest so a header split across packets is seen whole
    *carry = buf[i..].to_vec();
}

/// The concatenated payload of every video packet: the MPEG-1 elementary stream.
pub fn video_stream(data: &[u8]) -> Result<Vec<u8>> {
    elementary_stream(data, 0x1e0..=0x1ef)
}

/// The concatenated payload of every audio packet: the layer II elementary stream.
pub fn audio_stream(data: &[u8]) -> Result<Vec<u8>> {
    elementary_stream(data, 0x1c0..=0x1df)
}

fn elementary_stream(data: &[u8], ids: std::ops::RangeInclusive<u32>) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    while r.remaining() >= 4 {
        while data.get(r.pos()..r.pos() + 3) == Some(&[0, 0, 0]) {
            r.bytes(1, "zero fill")?;
        }
        let code = r.u32("start code")?.swap_bytes();
        match code {
            0x1ba => {
                r.bytes(8, "pack header")?;
            }
            0x1b9 => {} // the files hold several programs back to back: the audio runs on past the first end code
            0x1bb..=0x1ff => {
                let len = r.u16("packet length")?.swap_bytes() as usize;
                let body = r.bytes(len, "packet")?;
                if ids.contains(&code) {
                    out.extend_from_slice(&body[payload_start(body).min(body.len())..]);
                }
            }
            _ => return Err(Error::Invalid(format!("unexpected start code {code:#x} at {:#x}", r.pos() - 4))),
        }
    }
    Ok(out)
}

/// Payloads of the `movi` chunks that belong to one AVI stream, in file order.
pub fn avi_chunks(data: &[u8], stream: usize) -> Result<Vec<&[u8]>> {
    fn walk<'a>(data: &'a [u8], mut pos: usize, end: usize, tag: [u8; 2], out: &mut Vec<&'a [u8]>) -> Result<()> {
        while pos + 8 <= end {
            let id = &data[pos..pos + 4];
            let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = pos + 8;
            let body_end = (body + size).min(end);
            if id == b"LIST" && body + 4 <= end {
                walk(data, body + 4, body_end, tag, out)?;
            } else if id[..2] == tag && (id[2..] == *b"dc" || id[2..] == *b"db" || id[2..] == *b"wb") {
                out.push(&data[body..body_end]);
            }
            pos = body + size + (size & 1);
        }
        Ok(())
    }
    if data.get(..4) != Some(b"RIFF") {
        return Err(Error::Invalid("not an AVI".into()));
    }
    let tag = [b'0' + (stream / 10) as u8, b'0' + (stream % 10) as u8];
    let mut out = Vec::new();
    walk(data, 12, data.len(), tag, &mut out)?;
    Ok(out)
}

fn parse_avi(data: &[u8]) -> Result<Avi> {
    let mut r = Reader::new(data);
    if r.bytes(4, "RIFF")? != b"RIFF" {
        return Err(Error::Invalid("no RIFF".into()));
    }
    r.u32("size")?;
    if r.bytes(4, "form")? != b"AVI " {
        return Err(Error::Invalid("RIFF is not AVI".into()));
    }
    let mut a = Avi::default();
    walk_riff(&mut r, data.len(), &mut a)?;
    Ok(a)
}

fn data_at(r: &Reader, size: usize) -> Vec<u8> {
    Reader::at(r.data(), r.pos()).bytes(size, "chunk").map(<[u8]>::to_vec).unwrap_or_default()
}

fn walk_riff(r: &mut Reader, end: usize, a: &mut Avi) -> Result<()> {
    while r.pos() + 8 <= end {
        let id: [u8; 4] = r.bytes(4, "chunk id")?.try_into().unwrap();
        let size = r.u32("chunk size")? as usize;
        let body_end = (r.pos() + size).min(end);
        match &id {
            b"LIST" => {
                let kind = r.bytes(4, "list type")?;
                if kind == b"movi" {
                    count_movi(r, body_end, a)?;
                } else {
                    walk_riff(r, body_end, a)?;
                }
            }
            b"avih" => {
                a.micros_per_frame = r.u32("avih")?;
                r.bytes(12, "avih")?;
                a.frames = r.u32("frames")?;
                r.bytes(12, "avih")?;
                a.width = r.u32("width")?;
                a.height = r.u32("height")?;
            }
            b"strh" => {
                let kind: [u8; 4] = r.bytes(4, "type")?.try_into().unwrap();
                let handler: [u8; 4] = r.bytes(4, "handler")?.try_into().unwrap();
                r.bytes(12, "strh")?;
                let (scale, rate) = (r.u32("scale")?, r.u32("rate")?);
                r.u32("start")?;
                let length = r.u32("length")?;
                a.streams.push(AviStream { kind, handler, scale, rate, length, ..Default::default() });
            }
            b"strf" => {
                if let Some(s) = a.streams.last_mut() {
                    s.strf = data_at(r, size);
                    if &s.kind == b"vids" {
                        r.u32("size")?;
                        let (w, h) = (r.u32("w")?, r.u32("h")?);
                        r.u16("planes")?;
                        let bits = r.u16("bits")? as u32;
                        let comp: [u8; 4] = r.bytes(4, "compression")?.try_into().unwrap();
                        s.handler = comp;
                        s.format = [w, h, bits];
                    } else if &s.kind == b"auds" {
                        let tag = r.u16("tag")? as u32;
                        let ch = r.u16("channels")? as u32;
                        let rate = r.u32("rate")?;
                        r.bytes(6, "align")?;
                        let bits = r.u16("bits")? as u32;
                        s.handler = tag.to_le_bytes();
                        s.format = [ch, rate, bits];
                    }
                }
            }
            _ => {}
        }
        // chunks are word-aligned
        let next = body_end + (size & 1);
        if next < r.pos() {
            return Err(Error::Invalid("chunk overlaps".into()));
        }
        let skip = next - r.pos();
        r.bytes(skip.min(r.remaining()), "chunk body")?;
    }
    Ok(())
}

fn count_movi(r: &mut Reader, end: usize, a: &mut Avi) -> Result<()> {
    while r.pos() + 8 <= end {
        let id: [u8; 4] = r.bytes(4, "chunk id")?.try_into().unwrap();
        let size = r.u32("chunk size")? as usize;
        if &id == b"LIST" || &id == b"RIFF" {
            r.bytes(4, "list type")?; // `rec ` lists: their members follow inline
            continue;
        }
        if let (Some(d0), Some(d1)) = ((id[0] as char).to_digit(10), (id[1] as char).to_digit(10)) {
            if let Some(s) = a.streams.get_mut((d0 * 10 + d1) as usize) {
                s.chunks += 1;
            }
        }
        let skip = (size + (size & 1)).min(r.remaining());
        r.bytes(skip, "movi chunk")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(scr_ticks: u64) -> Vec<u8> {
        let mut v = vec![0, 0, 1, 0xba];
        v.push(0x21 | ((scr_ticks >> 29) & 0xe) as u8);
        v.push((scr_ticks >> 22) as u8);
        v.push(((scr_ticks >> 14) & 0xfe) as u8 | 1);
        v.push((scr_ticks >> 7) as u8);
        v.push(((scr_ticks << 1) & 0xfe) as u8 | 1);
        v.extend([0x80, 0x43, 0x79]); // mux rate 8000 * 50 / 50 ... any value with marker bits
        v
    }

    fn packet(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut body = vec![0xff, 0x0f]; // one stuffing byte, then "no PTS" marker
        body.extend(payload);
        let mut v = vec![0, 0, 1, id];
        v.extend((body.len() as u16).to_be_bytes());
        v.extend(body);
        v
    }

    #[test]
    fn walks_an_mpeg1_system_stream() {
        // sequence header 320x208, rate code 3, bit rate 8000*400, one GOP, an I and a B picture
        let seq = [0, 0, 1, 0xb3, 0x14, 0x00, 0xd0, 0x33, 0x07, 0xd0, 0x20, 0xa0];
        let mut es = seq.to_vec();
        es.extend([0, 0, 1, 0xb8, 0, 0, 0, 0]);
        es.extend([0, 0, 1, 0x00, 0x00, 0x0f, 0xff]); // type I (001)
        es.extend([0, 0, 1, 0x00, 0x00, 0x1f, 0xff]); // type B (011)
        let audio = [0xff, 0xfd, 0x80, 0x00, 1, 2, 3]; // MPEG-1 layer 2, 128 kbps, 44.1 kHz, stereo
        let mut s = pack(90_000 * 3);
        s.extend(packet(0xe0, &es[..9])); // split inside the sequence header
        s.extend(packet(0xe0, &es[9..]));
        s.extend(packet(0xc0, &audio));
        s.extend(packet(0xbe, &[0xff; 4]));
        s.extend(pack(90_000 * 5));
        s.extend([0, 0, 1, 0xb9]);
        let Video::Mpeg(m) = parse(&s).unwrap() else { panic!() };
        assert_eq!((m.packs, m.video_packets, m.audio_packets, m.padding_packets), (2, 2, 1, 1));
        assert_eq!((m.width, m.height, m.frame_rate_code, m.bit_rate), (320, 208, 3, Some(8000 * 400)));
        assert_eq!(m.frame_rate(), 25.0);
        assert_eq!((m.gops, m.pictures), (1, [1, 0, 1, 0]));
        assert_eq!(m.audio, Some(MpegAudio { layer: 2, bitrate_kbps: 128, sample_rate: 44100, channels: 2 }));
        assert_eq!(m.duration, 5.0);
    }

    #[test]
    fn walks_an_avi() {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut v = id.to_vec();
            v.extend((body.len() as u32).to_le_bytes());
            v.extend(body);
            if body.len() % 2 == 1 {
                v.push(0);
            }
            v
        }
        fn list(kind: &[u8; 4], body: Vec<u8>) -> Vec<u8> {
            let mut b = kind.to_vec();
            b.extend(body);
            chunk(b"LIST", &b)
        }
        let mut avih = vec![0u8; 56];
        avih[..4].copy_from_slice(&80_000u32.to_le_bytes());
        avih[16..20].copy_from_slice(&2u32.to_le_bytes());
        avih[32..36].copy_from_slice(&640u32.to_le_bytes());
        avih[36..40].copy_from_slice(&480u32.to_le_bytes());
        let mut strh = [b"vids".as_slice(), b"iv50"].concat();
        strh.extend([0u8; 12]);
        strh.extend(1u32.to_le_bytes());
        strh.extend(25u32.to_le_bytes());
        strh.extend(0u32.to_le_bytes());
        strh.extend(2u32.to_le_bytes());
        let mut strf = 40u32.to_le_bytes().to_vec();
        strf.extend(640u32.to_le_bytes());
        strf.extend(480u32.to_le_bytes());
        strf.extend([1, 0, 24, 0]);
        strf.extend(b"IV50");
        let hdrl = [chunk(b"avih", &avih), list(b"strl", [chunk(b"strh", &strh), chunk(b"strf", &strf)].concat())].concat();
        let movi = [chunk(b"00dc", &[1, 2, 3]), chunk(b"00dc", &[4])].concat();
        let body = [b"AVI ".to_vec(), list(b"hdrl", hdrl), list(b"movi", movi)].concat();
        let mut file = b"RIFF".to_vec();
        file.extend((body.len() as u32).to_le_bytes());
        file.extend(body);
        let Video::Avi(a) = parse(&file).unwrap() else { panic!() };
        assert_eq!((a.micros_per_frame, a.frames, a.width, a.height), (80_000, 2, 640, 480));
        assert_eq!(a.streams.len(), 1);
        assert_eq!((&a.streams[0].handler, a.streams[0].format, a.streams[0].chunks), (b"IV50", [640, 480, 24], 2));
    }

    #[test]
    fn rejects_other_data() {
        assert!(parse(b"nope").is_err());
        assert!(parse(&[0, 0, 1, 0xba, 0x21]).is_err());
    }
}
