//! Sound effects: the plain PCM wavs of `Data\Sounds\SFX` by the exe's effect id (`formats::sfx::SFX`), several voices at once,
//! with the exe's random pitch jitter of up to +-10 % where it asks for it.
//!
//! Two pools (an improvement over the exe, whose player `0x4075b0` mixes up to eight voices and drops a new sound when all are busy):
//! - world sounds (`play_at`: shots, explosions, aircraft) get `WORLD_VOICES` voices and are still dropped when all play;
//! - player action sounds (`play`: clicks, orders, air strike launch, rally point, turret placed, menus) have their own
//!   `PRIORITY_VOICES`, ignore the distance from the screen centre and are never dropped: when that pool is full its oldest voice stops.

use opensow_formats::{sfx, wav, GameDir};
use rodio::Source;
use std::collections::HashMap;
use std::num::NonZero;
use std::sync::Arc;

/// World voices playing at once (the exe: eight for everything).
pub const WORLD_VOICES: usize = 32;
/// Player action voices playing at once, apart from the world ones.
pub const PRIORITY_VOICES: usize = 16;

/// Sound effects and speech: decoded sounds, the players they run on, and the options' switches and volumes.
pub struct Sfx {
    /// The game's output (`audio::mixer`): its mixer, rate and channels, and the generation the voices and `route` were made on.
    out: Option<(rodio::mixer::Mixer, rodio::SampleRate, rodio::ChannelCount, u64)>,
    /// The voices' own mixer when the sound enhancement (`hifi`, a soft limiter on the sum) or the debug recording
    /// (`OPENSOW_AUDIO_DUMP=file`) is on; its player feeds the device. `None`: voices play straight into the device's mixer.
    route: Option<(rodio::mixer::Mixer, rodio::Player)>,
    /// The `hifi::enabled()` the route was built for: the Options switch can change it while the device is open.
    route_hifi: bool,
    /// Logging the started sounds for the debug recording, with its start time.
    log: Option<std::time::Instant>,
    /// The enhanced (faded) samples per effect id, for the sinc resampler (`hifi`).
    hq: HashMap<u8, Arc<[f32]>>,
    cache: HashMap<u8, Option<wav::Wav>>,
    voices: Vec<rodio::Player>,
    /// The player action pool, oldest first.
    priority: Vec<rodio::Player>,
    /// The speech stream: one voice at a time, a new line replaces the one still talking.
    speech: Option<rodio::Player>,
    /// Never open a device (`--mute`, tests, headless).
    pub enabled: bool,
    /// The options menu's FX / VOICE rows: switch and volume 0..=100 (`[audio+0x58]` / `[audio+0x54]`).
    pub fx_on: bool,
    /// Effect volume, 0..=100.
    pub fx_volume: i32,
    /// Speech switch.
    pub voice_on: bool,
    /// Speech volume, 0..=100.
    pub voice_volume: i32,
    /// Effects asked for so far (ids), for tests.
    pub played: Vec<u8>,
    /// Speech files asked for so far, for tests.
    pub spoken: Vec<String>,
}

impl Sfx {
    /// Nothing loaded and no device opened yet.
    pub fn new() -> Sfx {
        Sfx {
            out: None,
            route: None,
            route_hifi: false,
            log: None,
            hq: HashMap::new(),
            cache: HashMap::new(),
            voices: Vec::new(),
            priority: Vec::new(),
            speech: None,
            enabled: false,
            fx_on: true,
            fx_volume: 100,
            voice_on: true,
            voice_volume: 100,
            played: Vec::new(),
            spoken: Vec::new(),
        }
    }

    /// The game's output (opened with the first sound, `audio::mixer`) and the voices' mixer (`route`); false when there is none.
    /// When the output was replaced (it died) or the sound enhancement switch changed, the route is built again and the voices on
    /// the old one, which are silent or would bypass the new route, are dropped.
    fn open(&mut self) -> bool {
        let Some((mixer, rate, channels)) = crate::audio::mixer() else { return false };
        let generation = crate::audio::generation();
        let fresh = self.out.as_ref().is_none_or(|o| o.3 != generation);
        if fresh || self.route_hifi != crate::hifi::enabled() {
            self.voices.clear();
            self.priority.clear();
            self.speech = None;
            self.route = route(&mixer, rate, channels);
            self.route_hifi = crate::hifi::enabled();
            if std::env::var_os("OPENSOW_AUDIO_DUMP").is_some() && self.log.is_none() {
                self.log = Some(std::time::Instant::now());
            }
        }
        self.out = Some((mixer, rate, channels, generation));
        true
    }

    /// A new player on the voices' mixer.
    fn player(&self) -> Option<rodio::Player> {
        let o = self.out.as_ref()?;
        Some(rodio::Player::connect_new(self.route.as_ref().map_or(&o.0, |r| &r.0)))
    }

    fn out_rate(&self) -> Option<rodio::SampleRate> {
        self.out.as_ref().map(|o| o.1)
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
        let Some(w) = game.resolve(&format!("Data\\Sounds\\{dir}\\{name}")).and_then(|p| std::fs::read(p).ok()).and_then(|d| wav::parse(&d).ok()) else {
            return;
        };
        let (Some(ch), Some(rate)) = (NonZero::new(w.channels), NonZero::new(w.sample_rate)) else { return };
        if !self.open() {
            return;
        }
        let (Some(player), Some(out)) = (self.player(), self.out_rate()) else { return };
        player.set_volume(sfx::volume(self.voice_volume, None).map_or(0.0, |v| sfx::gains(v, 0).0));
        if crate::hifi::enabled() {
            let data = crate::hifi::prepare(w.pcm.iter().map(|&s| s as f32 / 32768.0), w.channels, w.sample_rate, true);
            player.append(crate::hifi::Resampled::new(data, w.channels, w.sample_rate, 1.0, out, false));
        } else {
            player.append(rodio::buffer::SamplesBuffer::new(ch, rate, w.pcm.iter().map(|&s| s as f32 / 32768.0).collect::<Vec<f32>>()));
        }
        self.speech = Some(player);
    }

    /// Play the player action / interface effect `id` unpositioned (centred, full effect volume) in the priority pool; `speed`
    /// 1.0 is the recorded pitch. Unknown ids and missing files are ignored.
    pub fn play(&mut self, game: &GameDir, id: u8, speed: f32) {
        let gains = sfx::volume(self.fx_volume, None).map_or((0.0, 0.0), |v| sfx::gains(v, 0));
        self.play_gains(game, id, speed, gains, true);
    }

    /// Play effect `id` at `(dx, dy)` pixels from the screen centre with the exe's pan and distance volume (`0x4075b0`: `sfx::pan`,
    /// `sfx::volume`); too far away to be heard (about 667 px) it is dropped like the exe does. `pan_dx` is the sound's x from the
    /// **fixed** 320 px mark of the view (`0x406510` subtracts 0x140, not half the screen width: a sound in the middle of a wider
    /// screen leans right, as in the original).
    pub fn play_at(&mut self, game: &GameDir, id: u8, speed: f32, dx: i32, dy: i32, pan_dx: i32) {
        // DELIBERATE DEVIATION: the exe stops hearing sounds about 667 px from the view centre; here the distance counts 4/9
        let Some(vol) = sfx::volume(self.fx_volume, Some((dx * 4 / 9, dy * 4 / 9))) else { return };
        // DELIBERATE DEVIATION: every sound is centred (the exe pans by position, `sfx::pan(pan_dx)`); distance volume is kept
        let _ = pan_dx;
        self.play_gains(game, id, speed, sfx::gains(vol, 0), false);
    }

    fn play_gains(&mut self, game: &GameDir, id: u8, speed: f32, gains: (f32, f32), priority: bool) {
        if self.played.len() < 256 {
            self.played.push(id);
        }
        if !self.enabled || !self.fx_on || gains == (0.0, 0.0) {
            return;
        }
        if !self.open() {
            return;
        }
        let wav = self.cache.entry(id).or_insert_with(|| {
            let rel = sfx::path(id)?;
            let file = game.resolve(&format!("Data\\Sounds\\{}", rel.replace('/', "\\")))?;
            wav::parse(&std::fs::read(file).ok()?).ok()
        });
        let Some(w) = wav.as_ref() else { return };
        if let Some(t) = self.log {
            eprintln!("sfx {:.3}s id {id} speed {speed:.3} gains {gains:?} priority {priority}", t.elapsed().as_secs_f32());
        }
        if priority {
            self.priority.retain(|v| !v.empty());
            if self.priority.len() >= PRIORITY_VOICES {
                self.priority.remove(0).stop();
            }
        } else {
            self.voices.retain(|v| !v.empty());
            if self.voices.len() >= WORLD_VOICES {
                return;
            }
        }
        let (Some(ch), Some(rate)) = (NonZero::new(w.channels), NonZero::new(w.sample_rate)) else { return };
        // fields, not `self.player()`: `w` still borrows the cache
        let Some(o) = self.out.as_ref() else { return };
        let out = o.1;
        let player = rodio::Player::connect_new(self.route.as_ref().map_or(&o.0, |r| &r.0));
        if crate::hifi::enabled() {
            let data =
                self.hq.entry(id).or_insert_with(|| crate::hifi::prepare(w.pcm.iter().map(|&s| s as f32 / 32768.0), w.channels, w.sample_rate, true)).clone();
            if gains.0 == gains.1 || w.channels > 2 {
                // centred (all sounds are, `play_at`): the one volume on the player, the samples stay mono
                player.set_volume(gains.0);
                player.append(crate::hifi::Resampled::new(data, w.channels, w.sample_rate, speed, out, false));
            } else {
                let mut st: Vec<f32> = if w.channels == 1 { data.iter().flat_map(|&v| [v, v]).collect() } else { data.to_vec() };
                for f in st.chunks_exact_mut(2) {
                    f[0] *= gains.0;
                    f[1] *= gains.1;
                }
                player.append(crate::hifi::Resampled::new(st.into(), 2, w.sample_rate, speed, out, false));
            }
            if priority {
                self.priority.push(player)
            } else {
                self.voices.push(player)
            }
            return;
        }
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
        if priority {
            self.priority.push(player)
        } else {
            self.voices.push(player)
        }
    }
}

/// The voices' mixer and the player that feeds it to `device`: with the sound enhancement a soft limiter (-1 dB threshold, 5 ms attack,
/// 100 ms release) keeps the sum of many full-scale shots from clipping; with `OPENSOW_AUDIO_DUMP` the result is also appended to that
/// file (raw 16-bit little-endian, device rate and channels) and every started effect is logged. Neither: `None`, the original path.
/// The mixer always holds one silent voice: rodio's mixer ends when it has no voice at all, and its player then drops it for good,
/// so the first effect (whose wav is read after the route is built) or any later one could find a mixer nobody plays.
fn route(device: &rodio::mixer::Mixer, rate: rodio::SampleRate, channels: rodio::ChannelCount) -> Option<(rodio::mixer::Mixer, rodio::Player)> {
    use std::io::Write;
    struct Tap<S> {
        inner: S,
        out: std::io::BufWriter<std::fs::File>,
        n: usize,
    }
    impl<S: Source<Item = f32>> Iterator for Tap<S> {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            let v = self.inner.next()?;
            let _ = self.out.write_all(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            self.n += 1;
            if self.n.is_multiple_of(48000) {
                let _ = self.out.flush();
            }
            Some(v)
        }
    }
    impl<S: Source<Item = f32>> Source for Tap<S> {
        fn current_span_len(&self) -> Option<usize> {
            self.inner.current_span_len()
        }
        fn channels(&self) -> rodio::ChannelCount {
            self.inner.channels()
        }
        fn sample_rate(&self) -> rodio::SampleRate {
            self.inner.sample_rate()
        }
        fn total_duration(&self) -> Option<std::time::Duration> {
            None
        }
    }
    let dump = std::env::var("OPENSOW_AUDIO_DUMP").ok();
    if !crate::hifi::enabled() && dump.is_none() {
        return None;
    }
    let (mixer, source) = rodio::mixer::mixer(channels, rate);
    mixer.add(rodio::source::Zero::new(channels, rate));
    let mut source: Box<dyn Source + Send> = Box::new(source);
    if crate::hifi::enabled() {
        source = Box::new(source.limit(rodio::source::LimitSettings::default()));
    }
    if let Some(path) = dump {
        if let Ok(f) = std::fs::File::create(&path) {
            eprintln!("audio: recording the effects mix to {path} (raw 16-bit little-endian, {rate} Hz, {channels} channels)");
            source = Box::new(Tap { inner: source, out: std::io::BufWriter::new(f), n: 0 });
        }
    }
    let master = rodio::Player::connect_new(device);
    master.append(source);
    Some((mixer, master))
}

/// The linear gain of an options row: its switch and its 0..=100 volume through the exe's curve (`30 * (volume - 100)`
/// hundredths of a dB, `sfx::volume`); off or volume 0 is silence.
pub fn gain(on: bool, volume: i32) -> f32 {
    if !on {
        return 0.0;
    }
    sfx::volume(volume, None).map_or(0.0, |v| sfx::gains(v, 0).0)
}

/// +-10 % pitch from a cheap clock-based pick (the exe uses its visual generator, so the sequence need not match).
pub fn jitter() -> f32 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_micros()) % 201;
    0.9 + n as f32 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_effects_route_survives_a_pull_before_its_first_voice() {
        let (ch, rate) = (NonZero::new(2).unwrap(), NonZero::new(48000).unwrap());
        let (device, mut out) = rodio::mixer::mixer(ch, rate);
        let Some((voices, _master)) = route(&device, rate, ch) else { return }; // enhancement off: no route
                                                                                // the output pulls 0.1 s before any effect is there (the first effect's wav is still being read)
        for _ in 0..9600 {
            out.next();
        }
        voices.add(rodio::source::SineWave::new(440.0).take_duration(std::time::Duration::from_millis(200)));
        let peak = (0..19200).filter_map(|_| out.next()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.1, "the effect reaches the output (peak {peak})");
        // and after a quiet moment with no voice at all, the next one still does
        for _ in 0..48000 {
            out.next();
        }
        voices.add(rodio::source::SineWave::new(440.0).take_duration(std::time::Duration::from_millis(200)));
        let peak = (0..19200).filter_map(|_| out.next()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.1, "a later effect too (peak {peak})");
    }
}

#[cfg(test)]
mod gain_tests {
    #[test]
    fn an_options_row_gain_uses_the_exes_curve() {
        assert_eq!(super::gain(true, 100), 1.0);
        assert_eq!(super::gain(false, 100), 0.0);
        assert_eq!(super::gain(true, 0), 0.0);
        assert!((super::gain(true, 15) - 10f32.powf(-2550.0 / 2000.0)).abs() < 1e-6);
    }
}
