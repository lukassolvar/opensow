//! The intro movie (`mI.sgv`), decoded by `opensow-formats` and played full screen before the menu.
//!
//! Video is decoded one frame at a time and the frame shown follows the clock; if the machine is
//! too slow frames are dropped, never the sound. The sound is the whole layer II track decoded up front.

use opensow_formats::{indeo5, mp2, mpeg1, msvideo1, video, GameDir};
use std::num::NonZero;

/// Language directories of the exe's path table (0x48ff48..): index 0 is the English `Data\Levels`.
pub const LANGUAGES: [&str; 7] = ["", "SK", "CZ", "PL", "HU", "RU", "CHINNESSE"];

/// A looping AVI animation: `mA.sgv` (the green BRIEFING backdrop, Indeo 5) or `mB.sgv` (the globe, MS Video 1).
pub struct Anim {
    data: Vec<u8>,
    indeo: bool,
    dec: Option<msvideo1::Decoder>,
    chunks: usize,
    fps: f64,
    clock: f64,
    /// Index of the frame in `rgb`, if any.
    index: Option<usize>,
    pub width: usize,
    pub height: usize,
    rgb: Vec<u32>,
    /// Play once and stop at the last frame instead of looping.
    pub once: bool,
    ended: bool,
    /// The PCM soundtrack (channels, rate, samples) of `mA`, not yet playing.
    audio: Option<(u16, u32, Vec<f32>)>,
    sound: Option<Sound>,
}

impl Anim {
    pub fn open(game: &GameDir, name: &str) -> Result<Anim, String> {
        let file = game.resolve(&format!("data\\levels\\{name}.sgv")).ok_or_else(|| format!("{name}.sgv not found"))?;
        Anim::from_bytes(std::fs::read(file).map_err(|e| e.to_string())?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Anim, String> {
        let video::Video::Avi(avi) = video::parse(&data).map_err(|e| e.to_string())? else { return Err("not an AVI".into()) };
        let s = avi.streams.first().ok_or("no video stream")?;
        let indeo = s.handler.eq_ignore_ascii_case(b"IV50");
        let chunks = video::avi_chunks(&data, 0).map_err(|e| e.to_string())?.len();
        if chunks == 0 || avi.micros_per_frame == 0 {
            return Err("empty animation".into());
        }
        let dec = (!indeo).then(|| msvideo1::Decoder::new(s.format[0] as usize, s.format[1] as usize, msvideo1::Decoder::palette_from_strf(&s.strf)));
        let (width, height) = (s.format[0] as usize, s.format[1] as usize);
        // 16-bit PCM audio stream, if there is one (mA has it, mB none)
        let audio = avi.streams.get(1).filter(|a| a.kind == *b"auds" && a.format[2] == 16).and_then(|a| {
            let pcm: Vec<f32> = video::avi_chunks(&data, 1).ok()?.concat().chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
            Some((a.format[0] as u16, a.format[1], pcm))
        });
        let mut a = Anim { data, indeo, dec, chunks, fps: 1e6 / avi.micros_per_frame as f64, clock: 0.0, index: None, width, height, rgb: vec![0; width * height], once: false, ended: false, audio, sound: None };
        a.show(0)?;
        Ok(a)
    }

    /// Open the audio device and start the soundtrack (once); returns whether sound is playing.
    pub fn start_sound(&mut self) -> bool {
        if self.sound.is_none() {
            self.sound = self.audio.take().and_then(|(c, r, s)| Sound::start_pcm(c, r, s, false));
        }
        self.sound.is_some()
    }

    /// A one-shot animation has played its last frame.
    pub fn finished(&self) -> bool {
        self.ended
    }

    pub fn update(&mut self, dt: f64) {
        self.clock += dt;
        let mut want = (self.clock * self.fps) as usize;
        if self.once && want >= self.chunks {
            want = self.chunks - 1;
            self.ended = true;
        } else {
            want %= self.chunks;
        }
        if self.index != Some(want) {
            let _ = self.show(want);
        }
    }

    /// Make frame `n` the current one. Indeo 5 frames are all intra; MS Video 1 ones are deltas, so
    /// they are decoded in order (from the start again after the loop wraps).
    fn show(&mut self, n: usize) -> Result<(), String> {
        let chunks = video::avi_chunks(&self.data, 0).map_err(|e| e.to_string())?;
        let rgb: Vec<u8> = if self.indeo {
            indeo5::decode(chunks.get(n).ok_or("no such frame")?).map_err(|e| e.to_string())?.to_rgb()
        } else {
            let dec = self.dec.as_mut().ok_or("no decoder")?;
            let from = match self.index {
                Some(i) if i < n => i + 1,
                Some(i) if i == n => return Ok(()),
                _ => 0,
            };
            for c in &chunks[from..=n] {
                dec.decode(c).map_err(|e| e.to_string())?;
            }
            dec.to_rgb()
        };
        for (p, c) in self.rgb.iter_mut().zip(rgb.chunks_exact(3)) {
            *p = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        }
        self.index = Some(n);
        Ok(())
    }

    /// Draw the frame scaled into the rectangle (x, y, w, h) of the buffer (nearest neighbour).
    pub fn draw(&self, buf: &mut [u32], bw: usize, bh: usize, (x, y, w, h): (usize, usize, usize, usize)) {
        for dy in 0..h.min(bh.saturating_sub(y)) {
            let sy = (dy * self.height / h).min(self.height - 1);
            for dx in 0..w.min(bw.saturating_sub(x)) {
                buf[(y + dy) * bw + x + dx] = self.rgb[sy * self.width + (dx * self.width / w).min(self.width - 1)];
            }
        }
    }
}

pub struct Sound {
    _device: rodio::MixerDeviceSink,
    pub player: rodio::Player,
}

pub struct Intro {
    /// File stem, e.g. `mI`, `mO` or `M01`.
    pub name: String,
    stream: mpeg1::Stream<'static>,
    fps: f64,
    frame: Option<mpeg1::Frame>,
    /// Index of `frame` in the movie.
    index: usize,
    /// Seconds since the movie started.
    clock: f64,
    ended: bool,
    /// The soundtrack, decoded but not yet playing.
    audio: Option<mp2::Audio>,
    sound: Option<Sound>,
}

impl Intro {
    /// Open `Data\Levels\<name>.sgv` (or the language copy) without starting the sound.
    pub fn open(game: &GameDir, name: &str, language: usize) -> Result<Intro, String> {
        let dir = LANGUAGES.get(language).ok_or("unknown language")?;
        let default = format!("data\\levels\\{name}.sgv");
        // only the intro and outro have language copies; everything else falls back to the main folder
        let file = (!dir.is_empty())
            .then(|| game.resolve(&format!("data\\diffdata\\{dir}\\levels\\{name}.sgv")))
            .flatten()
            .or_else(|| game.resolve(&default))
            .ok_or_else(|| format!("{default} not found"))?;
        let mut intro = Intro::from_bytes(&std::fs::read(file).map_err(|e| e.to_string())?)?;
        intro.name = name.to_owned();
        Ok(intro)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Intro, String> {
        let es = video::video_stream(data).map_err(|e| e.to_string())?;
        let mut stream = mpeg1::Stream::owned(es);
        let frame = stream.next_frame().map_err(|e| e.to_string())?.ok_or("empty video")?;
        let fps = stream.frame_rate();
        if fps <= 0.0 {
            return Err("video has no frame rate".into());
        }
        // a missing or unsupported soundtrack only means silence
        let audio = video::audio_stream(data).ok().and_then(|a| mp2::decode(&a).ok());
        Ok(Intro { name: String::new(), stream, fps, frame: Some(frame), index: 0, clock: 0.0, ended: false, audio, sound: None })
    }

    /// Open the audio device and start the soundtrack; returns whether sound is playing.
    pub fn start_sound(&mut self) -> bool {
        if self.sound.is_none() {
            self.sound = self.audio.take().and_then(Sound::start);
        }
        self.sound.is_some()
    }

    pub fn finished(&self) -> bool {
        self.ended
    }

    /// Advance the clock and decode up to the frame that is due.
    pub fn update(&mut self, dt: f64) {
        self.clock += dt;
        self.seek((self.clock * self.fps) as usize);
    }

    /// Headless: play from the start up to frame `n` (animations included).
    pub fn jump(&mut self, n: usize) {
        self.update(n as f64 / self.fps);
    }

    /// Decode forward until frame `target` is the current one (or the movie ends).
    pub fn seek(&mut self, target: usize) {
        while self.index < target && !self.ended {
            match self.stream.next_frame() {
                Ok(Some(f)) => {
                    self.frame = Some(f);
                    self.index += 1;
                }
                _ => {
                    self.ended = true;
                    if let Some(s) = &self.sound {
                        s.player.stop();
                    }
                }
            }
        }
    }

    /// Fill the buffer: the frame scaled to fit with black bars.
    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize) {
        buf.fill(0);
        let Some(f) = &self.frame else { return };
        let scale = (w as f64 / f.width as f64).min(h as f64 / f.height as f64);
        let (dw, dh) = (((f.width as f64 * scale) as usize).clamp(1, w), ((f.height as f64 * scale) as usize).clamp(1, h));
        let (x0, y0) = ((w - dw) / 2, (h - dh) / 2);
        for y in 0..dh {
            let sy = (y * f.height / dh).min(f.height - 1);
            let row = &mut buf[(y0 + y) * w + x0..(y0 + y) * w + x0 + dw];
            for (x, p) in row.iter_mut().enumerate() {
                *p = f.rgb_at((x * f.width / dw).min(f.width - 1), sy);
            }
        }
    }
}

impl Sound {
    fn start(audio: mp2::Audio) -> Option<Sound> {
        Sound::start_pcm(audio.channels as u16, audio.sample_rate, audio.pcm.iter().map(|&s| s as f32 / 32768.0).collect(), false)
    }

    /// Open the default audio device and play interleaved samples.
    pub fn start_pcm(channels: u16, rate: u32, samples: Vec<f32>, looping: bool) -> Option<Sound> {
        let mut device = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        device.log_on_drop(false);
        let player = rodio::Player::connect_new(device.mixer());
        let buffer = rodio::buffer::SamplesBuffer::new(NonZero::new(channels)?, NonZero::new(rate)?, samples);
        if looping {
            use rodio::Source;
            player.append(buffer.repeat_infinite());
        } else {
            player.append(buffer);
        }
        Some(Sound { _device: device, player })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_a_mission_briefing_movie_headless() {
        let Some(game) = GameDir::locate() else { return };
        let mut m = Intro::open(&game, "M01", 0).unwrap();
        let mut buf = vec![0u32; 640 * 480];
        m.draw(&mut buf, 640, 480);
        // 320x208 letterboxed into 4:3: black bars above and below, picture in the middle
        assert_eq!(buf[0], 0);
        assert!(buf[240 * 640 + 320] != 0 || buf.iter().any(|&p| p != 0));
        m.update(0.2); // 5 frames at 25 fps
        assert_eq!(m.index, 5);
        m.seek(100_000);
        assert!(m.finished());
        assert_eq!(m.index, 322);
        assert!(Intro::open(&game, "M01", 99).is_err());
    }

    #[test]
    fn briefing_screen_animations_loop() {
        let Some(game) = GameDir::locate() else { return };
        let mut a = Anim::open(&game, "mA").unwrap();
        assert_eq!((a.width, a.height), (640, 480));
        let first = a.rgb.clone();
        a.update(1.0);
        assert_eq!(a.index, Some(12));
        assert_ne!(a.rgb, first);
        a.update(8.0); // 9 s = frame 112, wraps to 8 of 104
        assert_eq!(a.index, Some(8));
        let mut b = Anim::open(&game, "mB").unwrap();
        b.update(7.0); // 87
        let late = b.rgb.clone();
        b.update(2.0); // 9 s -> wraps to 12; delta frames restart from frame 0
        assert_eq!(b.index, Some(12));
        assert_ne!(b.rgb, late);
    }
}
