//! Sound effects: the plain PCM wavs of `Data\Sounds\SFX` by the exe's effect id (`formats::sfx::SFX`), several voices at once
//! (the exe's player `0x4075b0` mixes up to eight), with the exe's random pitch jitter of up to +-10 % where it asks for it.

use opensow_formats::{sfx, wav, GameDir};
use rodio::Source;
use std::collections::HashMap;
use std::num::NonZero;

/// Voices playing at once (the exe: eight).
const VOICES: usize = 8;

pub struct Sfx {
    device: Option<rodio::MixerDeviceSink>,
    cache: HashMap<u8, Option<wav::Wav>>,
    voices: Vec<rodio::Player>,
    /// The speech stream: one voice at a time, a new line replaces the one still talking.
    speech: Option<rodio::Player>,
    /// Never open a device (`--mute`, tests, headless).
    pub enabled: bool,
    /// The options menu's FX / VOICE rows: switch and volume 0..=100 (`[audio+0x58]` / `[audio+0x54]`).
    pub fx_on: bool,
    pub fx_volume: i32,
    pub voice_on: bool,
    pub voice_volume: i32,
    /// Effects asked for so far (ids), for tests.
    pub played: Vec<u8>,
    /// Speech files asked for so far, for tests.
    pub spoken: Vec<String>,
}

impl Sfx {
    pub fn new() -> Sfx {
        Sfx { device: None, cache: HashMap::new(), voices: Vec::new(), speech: None, enabled: false, fx_on: true, fx_volume: 100, voice_on: true, voice_volume: 100, played: Vec::new(), spoken: Vec::new() }
    }

    /// Say the speech file `name` (`Data\\Sounds\\Speech`), cutting off whatever is still being said.
    pub fn say(&mut self, game: &GameDir, name: &str) {
        self.say_in(game, "Speech", name);
    }

    /// Plays a voice file of `Data\Sounds\<dir>` (the tutorial's `Tutorial\NN.wav` lives next to `Speech`).
    pub fn say_in(&mut self, game: &GameDir, dir: &str, name: &str) {
        if self.spoken.len() < 256 {
            self.spoken.push(name.to_owned());
        }
        if !self.enabled || !self.voice_on {
            return;
        }
        if let Some(old) = self.speech.take() {
            old.stop();
        }
        let Some(w) = game.resolve(&format!("Data\\Sounds\\{dir}\\{name}")).and_then(|p| std::fs::read(p).ok()).and_then(|d| wav::parse(&d).ok()) else { return };
        let (Some(ch), Some(rate)) = (NonZero::new(w.channels), NonZero::new(w.sample_rate)) else { return };
        if self.device.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut d) => {
                    d.log_on_drop(false);
                    self.device = Some(d);
                }
                Err(_) => {
                    self.enabled = false;
                    return;
                }
            }
        }
        let Some(device) = self.device.as_ref() else { return };
        let player = rodio::Player::connect_new(device.mixer());
        player.set_volume(sfx::volume(self.voice_volume, None).map_or(0.0, |v| sfx::gains(v, 0).0));
        player.append(rodio::buffer::SamplesBuffer::new(ch, rate, w.pcm.iter().map(|&s| s as f32 / 32768.0).collect::<Vec<f32>>()));
        self.speech = Some(player);
    }

    /// Play effect `id`; `speed` 1.0 is the recorded pitch. Unknown ids and missing files are ignored.
    pub fn play(&mut self, game: &GameDir, id: u8, speed: f32) {
        let gains = sfx::volume(self.fx_volume, None).map_or((0.0, 0.0), |v| sfx::gains(v, 0));
        self.play_gains(game, id, speed, gains);
    }

    /// Play effect `id` at `(dx, dy)` pixels from the screen centre with the exe's pan and distance volume (`0x4075b0`: `sfx::pan`,
    /// `sfx::volume`); too far away to be heard (about 667 px) it is dropped like the exe does. `pan_dx` is the sound's x from the
    /// **fixed** 320 px mark of the view (`0x406510` subtracts 0x140, not half the screen width: a sound in the middle of a wider
    /// screen leans right, as in the original).
    pub fn play_at(&mut self, game: &GameDir, id: u8, speed: f32, dx: i32, dy: i32, pan_dx: i32) {
        let Some(vol) = sfx::volume(self.fx_volume, Some((dx, dy))) else { return };
        self.play_gains(game, id, speed, sfx::gains(vol, sfx::pan(pan_dx)));
    }

    fn play_gains(&mut self, game: &GameDir, id: u8, speed: f32, gains: (f32, f32)) {
        if self.played.len() < 256 {
            self.played.push(id);
        }
        if !self.enabled || !self.fx_on || gains == (0.0, 0.0) {
            return;
        }
        let wav = self.cache.entry(id).or_insert_with(|| {
            let rel = sfx::path(id)?;
            let file = game.resolve(&format!("Data\\Sounds\\{}", rel.replace('/', "\\")))?;
            wav::parse(&std::fs::read(file).ok()?).ok()
        });
        let Some(w) = wav.as_ref() else { return };
        if self.device.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut d) => {
                    d.log_on_drop(false);
                    self.device = Some(d);
                }
                Err(_) => {
                    self.enabled = false;
                    return;
                }
            }
        }
        let Some(device) = self.device.as_ref() else { return };
        self.voices.retain(|v| !v.empty());
        if self.voices.len() >= VOICES {
            return;
        }
        let (Some(ch), Some(rate)) = (NonZero::new(w.channels), NonZero::new(w.sample_rate)) else { return };
        let player = rodio::Player::connect_new(device.mixer());
        let mut samples: Vec<f32> = w.pcm.iter().map(|&s| s as f32 / 32768.0).collect();
        let mut ch = ch;
        if gains != (1.0, 1.0) {
            // DirectSound pans by attenuating one side: make it stereo and scale each channel
            if w.channels == 1 {
                samples = samples.iter().flat_map(|&v| [v, v]).collect();
                ch = NonZero::new(2).unwrap();
            }
            if w.channels <= 2 {
                for f in samples.chunks_exact_mut(2) {
                    f[0] *= gains.0;
                    f[1] *= gains.1;
                }
            }
        }
        player.append(rodio::buffer::SamplesBuffer::new(ch, rate, samples).speed(speed));
        self.voices.push(player);
    }
}

/// +-10 % pitch from a cheap clock-based pick (the exe uses its visual generator, so the sequence need not match).
pub fn jitter() -> f32 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() / 1000) % 201;
    0.9 + n as f32 / 1000.0
}
