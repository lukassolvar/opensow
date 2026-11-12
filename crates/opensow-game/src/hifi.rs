//! Run-time sound enhancement (docs/audio.md, "Sound enhancement"), the audio sibling of the texture enhancement: nothing is stored,
//! the original wavs are only played better.
//!
//! - **Resampling**: the game's 22050 Hz sounds are converted to the device rate by a windowed-sinc filter (`Resampled`) instead
//!   of rodio's linear interpolation, whose mirror images above 11 kHz sound gritty on headphones. The pitch jitter (`speed`) is
//!   folded into the same filter.
//! - **Fades**: 2 ms in / 3 ms out (`prepare`), so files that start or end mid-waveform do not click.
//! - The effects mix also gets a soft limiter (`sfx.rs`), so many overlapping full-scale shots do not clip.
//!
//! The Options page 2 switch (`Extra::sound`), or `--no-sound-enhance` / `OPENSOW_SOUND_ENHANCE=0` for a run, turns all of it off and
//! plays exactly as before.

use rodio::Source;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Switches the sound enhancement on or off.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether the sound enhancement is on.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Zero crossings on each side of the filter kernel.
const HALF: usize = 16;
/// Kernel table entries per zero crossing.
const RES: usize = 256;
/// Fraction of the lower Nyquist frequency kept: a little below 1 leaves room for the window's transition band.
const CUTOFF: f64 = 0.95;
const FADE_IN_MS: u32 = 2;
const FADE_OUT_MS: u32 = 3;

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)) * (x / (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

/// Kaiser-windowed sinc, sampled at `RES` steps per zero crossing over `0..=HALF` (plus one guard entry).
fn kernel() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let beta = 8.6;
        let norm = bessel_i0(beta);
        (0..=HALF * RES + 1)
            .map(|i| {
                let x = i as f64 / RES as f64;
                if x >= HALF as f64 {
                    return 0.0;
                }
                let sinc = if x == 0.0 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
                let r = x / HALF as f64;
                (sinc * bessel_i0(beta * (1.0 - r * r).sqrt()) / norm) as f32
            })
            .collect()
    })
}

/// 16-bit samples to floats; with `fades` the first 2 ms ramp up from silence and the last 3 ms down to it.
pub fn prepare(pcm: impl IntoIterator<Item = f32>, channels: u16, rate: u32, fades: bool) -> Arc<[f32]> {
    let mut s: Vec<f32> = pcm.into_iter().collect();
    if fades {
        let ch = channels.max(1) as usize;
        let frames = s.len() / ch;
        let fin = ((rate * FADE_IN_MS / 1000) as usize).clamp(1, frames.max(1));
        let fout = ((rate * FADE_OUT_MS / 1000) as usize).clamp(1, frames.max(1));
        for i in 0..fin.min(frames) {
            let g = i as f32 / fin as f32;
            s[i * ch..(i + 1) * ch].iter_mut().for_each(|v| *v *= g);
        }
        for i in 0..fout.min(frames) {
            let g = i as f32 / fout as f32;
            let f = frames - 1 - i;
            s[f * ch..(f + 1) * ch].iter_mut().for_each(|v| *v *= g);
        }
    }
    s.into()
}

/// Interleaved samples played at `out_rate` through a windowed-sinc filter; `speed` > 1 plays faster and higher (the pitch jitter).
pub struct Resampled {
    data: Arc<[f32]>,
    ch: usize,
    frames: usize,
    /// Input frames per output frame.
    step: f64,
    /// Input position of the next output frame.
    pos: f64,
    /// Filter cutoff relative to the input Nyquist (below 1 when the input is played faster than the output can hold).
    fc: f64,
    /// Taps on each side of the centre.
    reach: i64,
    looping: bool,
    out_rate: rodio::SampleRate,
    frame: Vec<f32>,
    next: usize,
}

impl Resampled {
    /// A sound resampled to the output rate while it plays; `speed` changes the pitch.
    pub fn new(data: Arc<[f32]>, channels: u16, in_rate: u32, speed: f32, out_rate: rodio::SampleRate, looping: bool) -> Resampled {
        let ch = channels.max(1) as usize;
        let step = in_rate as f64 * speed.max(0.01) as f64 / out_rate.get() as f64;
        let fc = CUTOFF * (1.0 / step).min(1.0);
        let reach = (HALF as f64 / fc).ceil() as i64;
        let frames = data.len() / ch;
        Resampled { data, ch, frames, step, pos: 0.0, fc, reach, looping, out_rate, frame: vec![0.0; ch], next: ch }
    }

    fn sample(&self, frame: i64, c: usize) -> f32 {
        let n = self.frames as i64;
        let f = if self.looping && n > 0 { frame.rem_euclid(n) } else { frame };
        if f < 0 || f >= n {
            return 0.0;
        }
        self.data[f as usize * self.ch + c]
    }

    /// Filter one output frame at `self.pos` into `self.frame`.
    fn render(&mut self) {
        let table = kernel();
        let base = self.pos.floor() as i64;
        self.frame.iter_mut().for_each(|v| *v = 0.0);
        let scale = self.fc as f32;
        for k in base - self.reach + 1..=base + self.reach {
            let u = (self.pos - k as f64).abs() * self.fc * RES as f64;
            let i = u as usize;
            if i + 1 >= table.len() {
                continue;
            }
            let t = (u - i as f64) as f32;
            let w = (table[i] + (table[i + 1] - table[i]) * t) * scale;
            if w == 0.0 {
                continue;
            }
            for c in 0..self.ch {
                self.frame[c] += self.sample(k, c) * w;
            }
        }
    }
}

impl Iterator for Resampled {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.next == self.ch {
            if self.frames == 0 || (!self.looping && self.pos >= self.frames as f64) {
                return None;
            }
            self.render();
            self.pos += self.step;
            if self.looping && self.pos >= self.frames as f64 {
                self.pos -= self.frames as f64;
            }
            self.next = 0;
        }
        let v = self.frame[self.next];
        self.next += 1;
        Some(v)
    }
}

impl Source for Resampled {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        rodio::ChannelCount::new(self.ch as u16).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.out_rate
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        if self.looping {
            return None;
        }
        Some(std::time::Duration::from_secs_f64(self.frames as f64 / self.step / self.out_rate.get() as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZero;

    fn sine(freq: f64, rate: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate).sin() as f32 * 0.5).collect()
    }

    /// Worst error against the exact sine, away from the start and end.
    fn worst(out: &[f32], freq: f64, rate: f64) -> f32 {
        out[200..out.len() - 200]
            .iter()
            .enumerate()
            .map(|(i, &v)| (v - (2.0 * std::f64::consts::PI * freq * (i + 200) as f64 / rate).sin() as f32 * 0.5).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn a_sine_converts_to_48k_almost_exactly_and_far_better_than_linear() {
        let input: Arc<[f32]> = sine(5000.0, 22050.0, 22050).into();
        let out: Vec<f32> = Resampled::new(input.clone(), 1, 22050, 1.0, NonZero::new(48000).unwrap(), false).collect();
        assert!((out.len() as i64 - 48000).abs() <= 2, "length {}", out.len());
        let sinc = worst(&out, 5000.0, 48000.0);
        // rodio-style linear interpolation of the same input
        let linear: Vec<f32> = (0..47000)
            .map(|j| {
                let p = j as f64 * 22050.0 / 48000.0;
                let (i, t) = (p.floor() as usize, (p - p.floor()) as f32);
                input[i] + (input[i + 1] - input[i]) * t
            })
            .collect();
        let lin = worst(&linear, 5000.0, 48000.0);
        assert!(sinc < 0.002, "sinc error {sinc}");
        assert!(lin > 20.0 * sinc, "linear {lin} vs sinc {sinc}");
    }

    #[test]
    fn speed_plays_faster_and_higher_and_stereo_stays_apart() {
        let st: Vec<f32> = (0..2000).flat_map(|_| [0.5f32, -0.25]).collect();
        let out: Vec<f32> = Resampled::new(st.into(), 2, 22050, 1.1, NonZero::new(44100).unwrap(), false).collect();
        let frames = out.len() / 2;
        assert!((frames as f64 - 2000.0 * 2.0 / 1.1).abs() < 3.0, "{frames}");
        // the middle of a constant signal keeps its level per channel
        assert!((out[2 * (frames / 2)] - 0.5).abs() < 1e-3 && (out[2 * (frames / 2) + 1] + 0.25).abs() < 1e-3);
    }

    #[test]
    fn a_loop_wraps_without_a_seam_and_never_ends() {
        let input: Arc<[f32]> = sine(441.0, 22050.0, 2200).into(); // exactly 44 periods of 50 samples
        let out: Vec<f32> = Resampled::new(input, 1, 22050, 1.0, NonZero::new(48000).unwrap(), true).take(48000 * 3).collect();
        assert_eq!(out.len(), 48000 * 3);
        assert!(worst(&out, 441.0, 48000.0) < 0.002);
    }

    #[test]
    fn fades_ramp_from_and_to_silence() {
        let s = prepare(vec![1.0f32; 2000], 1, 22050, true);
        assert_eq!(s[0], 0.0);
        assert!(s[20] > 0.0 && s[20] < 1.0);
        assert_eq!(s[1000], 1.0);
        assert_eq!(s[1999], 0.0);
        let plain = prepare(vec![1.0f32; 10], 1, 22050, false);
        assert!(plain.iter().all(|&v| v == 1.0));
    }
}
