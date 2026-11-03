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
    /// Never open a device (`--mute`, tests, headless).
    pub enabled: bool,
    /// Effects asked for so far (ids), for tests.
    pub played: Vec<u8>,
}

impl Sfx {
    pub fn new() -> Sfx {
        Sfx { device: None, cache: HashMap::new(), voices: Vec::new(), enabled: false, played: Vec::new() }
    }

    /// Play effect `id`; `speed` 1.0 is the recorded pitch. Unknown ids and missing files are ignored.
    pub fn play(&mut self, game: &GameDir, id: u8, speed: f32) {
        if self.played.len() < 256 {
            self.played.push(id);
        }
        if !self.enabled {
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
        let samples: Vec<f32> = w.pcm.iter().map(|&s| s as f32 / 32768.0).collect();
        player.append(rodio::buffer::SamplesBuffer::new(ch, rate, samples).speed(speed));
        self.voices.push(player);
    }
}

/// +-10 % pitch from a cheap clock-based pick (the exe uses its visual generator, so the sequence need not match).
pub fn jitter() -> f32 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() / 1000) % 201;
    0.9 + n as f32 / 1000.0
}
