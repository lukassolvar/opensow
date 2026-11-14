//! The game's one audio output (not in the exe, which plays through DirectSound buffers): opened with the first sound and kept
//! for the whole run. Music, movie soundtracks, effects and speech are players on its mixer (docs/audio.md, "One output").
//!
//! The port used to open a new output stream for every music track, boot page and movie, plus one for the effects. Opening a
//! stream is slow on some systems (on PulseAudio a new stream plays only after about a second), so every
//! track change began with a gap and the boot pages, which restart their track every two seconds, were mostly silent.
//!
//! When the output dies (an error that means the device is gone, or [`Stall`]: a silent heartbeat voice stops moving) it is
//! dropped and [`generation`] goes up; the owners notice their players are stale and ask again, and the next request opens a
//! new output. An open that fails (no device, or the sound server is not up yet) is tried again on a later request, 3 s after
//! the first failure, then 6, 12... up to 60 s apart, so a machine without audio costs little and a late server is still found.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The wait before opening the output again after `failures` failed attempts in a row: 3 s, doubling, at most 60 s.
fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs((3u64 << failures.saturating_sub(1).min(5)).min(60))
}

/// More stream errors than this within a second mean the stream is dead even when none of them is fatal on its own (a backend
/// that fails on every call once its server is gone); a device that only underruns now and then stays far below.
const ERROR_FLOOD: u32 = 100;

/// Notices an output that died without a fatal error: something is playing (not paused, sound queued), yet the position has not
/// moved for [`Stall::LIMIT`]. Seen when the sound server goes away under some backends.
pub struct Stall {
    last: Duration,
    since: Instant,
}

impl Stall {
    /// Longer than a new stream takes to start (about 1 s on PulseAudio, up to 2 s with some buffer sizes).
    pub const LIMIT: Duration = Duration::from_secs(3);

    pub fn new() -> Stall {
        Stall { last: Duration::MAX, since: Instant::now() }
    }

    /// Feeds the position at `now`; true once it has stood still for `LIMIT` while `active`. Movement or an inactive moment
    /// starts the count again.
    pub fn stalled(&mut self, pos: Duration, active: bool, now: Instant) -> bool {
        if !active || pos != self.last {
            self.last = pos;
            self.since = now;
            return false;
        }
        now.saturating_duration_since(self.since) >= Self::LIMIT
    }
}

/// An open output with its heartbeat.
struct Output {
    device: rodio::MixerDeviceSink,
    /// Set by the stream's error callback for errors that mean the device is gone.
    dead: Arc<AtomicBool>,
    /// A silent voice that always plays; its position stops when the stream stops pulling sound (adding 0.0 keeps the mix exact).
    heartbeat: rodio::Player,
    stall: Stall,
}

#[derive(Default)]
struct State {
    out: Option<Output>,
    /// Bumped whenever the output is replaced: players made on an older one are silent.
    generation: u64,
    /// When the next open may be tried (after a failed one); `None` = now.
    retry_at: Option<Instant>,
    /// Failed opens in a row.
    failures: u32,
    /// Last `OPENSOW_AUDIO_DEBUG` line.
    debug_at: Option<Instant>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The output's mixer, rate and channel count, opening the output on first use (or again after it died); `None` without one.
pub fn mixer() -> Option<(rodio::mixer::Mixer, rodio::SampleRate, rodio::ChannelCount)> {
    STATE.with_borrow_mut(|s| {
        if s.out.is_none() {
            if s.retry_at.is_some_and(|t| Instant::now() < t) {
                return None;
            }
            match open(s.failures == 0) {
                Some(o) => {
                    if s.failures > 0 {
                        eprintln!("audio: output open again");
                    }
                    s.out = Some(o);
                    s.retry_at = None;
                    s.failures = 0;
                }
                None => {
                    s.failures += 1;
                    s.retry_at = Some(Instant::now() + retry_delay(s.failures));
                }
            }
        }
        let o = s.out.as_ref()?;
        let cfg = o.device.config();
        Some((o.device.mixer().clone(), cfg.sample_rate(), cfg.channel_count()))
    })
}

/// Which output the current players belong to; it changes when the output is replaced.
pub fn generation() -> u64 {
    STATE.with_borrow(|s| s.generation)
}

/// Called every frame: drops a dead output (fatal error or stalled heartbeat) so that the next request opens a new one.
pub fn tick() {
    STATE.with_borrow_mut(|s| {
        let Some(o) = s.out.as_mut() else { return };
        let b = &o.heartbeat;
        // OPENSOW_AUDIO_DEBUG=1: the output's state once a second (for reports of missing sound; not in the exe)
        if std::env::var_os("OPENSOW_AUDIO_DEBUG").is_some() && s.debug_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            s.debug_at = Some(Instant::now());
            let cfg = o.device.config();
            eprintln!(
                "audio debug: output {} ({} Hz, {} ch), heartbeat at {:.2} s, error flagged {}",
                s.generation,
                cfg.sample_rate(),
                cfg.channel_count(),
                b.get_pos().as_secs_f32(),
                o.dead.load(Ordering::Relaxed)
            );
        }
        let stalled = o.stall.stalled(b.get_pos(), !b.empty() && !b.is_paused(), Instant::now());
        if o.dead.load(Ordering::Relaxed) || stalled {
            eprintln!("audio: the output stopped ({}), opening it again", if stalled { "no progress" } else { "device lost" });
            lose(s);
        }
    });
}

/// Forget the open output (it died): the generation goes up and the next request opens a new one.
fn lose(s: &mut State) {
    s.out = None;
    s.generation += 1;
    s.retry_at = None;
    s.failures = 0;
}

/// Treat the open output as dead, as `tick` does when it notices (tests).
#[cfg(test)]
pub fn kill() {
    STATE.with_borrow_mut(|s| {
        if s.out.is_some() {
            lose(s);
        }
    });
}

/// cpal errors after which the stream will not play again; others (an underrun, a rerouted default device) are only reported.
fn fatal(kind: rodio::cpal::ErrorKind) -> bool {
    use rodio::cpal::ErrorKind as K;
    matches!(kind, K::DeviceNotAvailable | K::HostUnavailable | K::StreamInvalidated)
}

/// Opens an output; `report` prints why when it fails (the first failure in a row only, retries stay quiet).
fn open(report: bool) -> Option<Output> {
    let dead = Arc::new(AtomicBool::new(false));
    let flag = dead.clone();
    let device = open_output(move |kind, burst| {
        if fatal(kind) || burst > ERROR_FLOOD {
            flag.store(true, Ordering::Relaxed);
        }
    })
    .map_err(|e| {
        if report {
            eprintln!("audio: no output ({e}); trying again later");
        }
    })
    .ok()?;
    let heartbeat = rodio::Player::connect_new(device.mixer());
    heartbeat.append(rodio::source::Zero::new(device.config().channel_count(), device.config().sample_rate()));
    Some(Output { device, dead, heartbeat, stall: Stall::new() })
}

/// Open the system's default output. rodio asks for a ~2048-frame buffer, which some devices (Bluetooth headphones such as
/// AirPods accept only 15..=960) reject, leaving the game silent; so smaller buffers are tried until the device accepts one.
/// `on_error` gets the kind of every error the stream reports later and how many came within the last second; only the first
/// three of such a burst are printed.
fn open_output(on_error: impl Fn(rodio::cpal::ErrorKind, u32) + Clone + Send + 'static) -> Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
    use rodio::cpal::BufferSize;
    let mut last = None;
    let mut sizes = vec![BufferSize::Fixed(2048), BufferSize::Fixed(960), BufferSize::Fixed(512), BufferSize::Fixed(256), BufferSize::Default];
    // OPENSOW_AUDIO_BUFFER=N tries N frames first (to compare devices / buffer sizes by ear); OPENSOW_AUDIO_BUFFER=0 the device default
    if let Some(n) = std::env::var("OPENSOW_AUDIO_BUFFER").ok().and_then(|v| v.parse::<u32>().ok()) {
        sizes.insert(0, if n == 0 { BufferSize::Default } else { BufferSize::Fixed(n) });
    }
    for size in sizes {
        let cb = on_error.clone();
        let mut burst = (Instant::now(), 0u32);
        let r = rodio::DeviceSinkBuilder::from_default_device().and_then(|b| {
            b.with_buffer_size(size)
                .with_error_callback(move |e| {
                    let now = Instant::now();
                    if now.duration_since(burst.0) > Duration::from_secs(1) {
                        burst = (now, 0);
                    }
                    burst.1 += 1;
                    match burst.1 {
                        1..=3 => eprintln!("audio stream error: {e}"),
                        4 => eprintln!("audio stream error: more within a second, not shown"),
                        _ => {}
                    }
                    cb(e.kind(), burst.1);
                })
                .open_stream()
        });
        match r {
            Ok(mut d) => {
                d.log_on_drop(false);
                return Ok(d);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("at least one attempt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_standing_still_while_playing_counts_as_dead() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let ms = Duration::from_millis;
        let mut s = Stall::new();
        assert!(!s.stalled(ms(0), true, at(0)));
        assert!(!s.stalled(ms(500), true, at(500)), "moving");
        assert!(!s.stalled(ms(500), true, at(3400)), "2.9 s still: a new stream may take this long to start");
        assert!(s.stalled(ms(500), true, at(3500)), "3 s still: dead");
        assert!(!s.stalled(ms(510), true, at(3600)), "moving again");
        // paused or nothing queued never counts, and restarts the count
        assert!(!s.stalled(ms(510), false, at(9000)));
        assert!(!s.stalled(ms(510), true, at(10_000)));
        assert!(s.stalled(ms(510), true, at(12_000)));
        // a looping track wraps to 0: that is movement
        let mut s = Stall::new();
        s.stalled(ms(5000), true, at(0));
        assert!(!s.stalled(ms(0), true, at(3500)));
    }

    #[test]
    fn a_failed_open_is_tried_again_later_and_later() {
        let secs: Vec<u64> = (1..=8).map(|n| retry_delay(n).as_secs()).collect();
        assert_eq!(secs, [3, 6, 12, 24, 48, 60, 60, 60]);
    }

    #[test]
    fn only_errors_that_end_the_stream_are_fatal() {
        use rodio::cpal::ErrorKind as K;
        assert!(fatal(K::DeviceNotAvailable) && fatal(K::HostUnavailable) && fatal(K::StreamInvalidated));
        assert!(!fatal(K::Xrun) && !fatal(K::DeviceChanged) && !fatal(K::BackendError));
    }

    #[test]
    fn a_lost_output_is_replaced_and_the_generation_moves_on() {
        let Some(_) = mixer() else { return }; // no audio device here
        let g = generation();
        tick();
        assert_eq!(generation(), g, "a healthy output stays");
        kill();
        assert_eq!(generation(), g + 1);
        assert!(mixer().is_some(), "the next request opens a new output");
        assert_eq!(generation(), g + 1);
    }
}
