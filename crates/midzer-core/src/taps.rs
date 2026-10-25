//! How a player taps: how often and how hard, smoothed. The solo and the
//! accompaniment both play busier the faster the taps come.

/// Taps per beat counted at most.
const MAX_RATE: f64 = 4.0;
/// How fast the energy follows the taps, up and down, in beats.
const RISE_BEATS: f64 = 0.25;
const FALL_BEATS: f64 = 0.75;
/// Notes this close together are one chord, not faster playing.
pub const CHORD_BEATS: f64 = 0.1;

#[derive(Debug, Clone, Copy, Default)]
pub struct Taps {
    /// Taps per beat, smoothed, and the frame of the last tap.
    pub rate: f64,
    pub last_tap: Option<u64>,
    /// The taps' velocity, smoothed.
    pub touch: f32,
}

impl Taps {
    /// Counts a tap at engine frame `now`, beats `period` frames apart;
    /// with `chords`, notes closer than [`CHORD_BEATS`] count as one.
    pub fn tap(&mut self, velocity: u8, now: u64, period: f64, chords: bool) {
        // The first tap, or the first after a while: eighths to start with.
        self.count(velocity, now, period, chords, self.gap(), 1.0);
    }

    /// Like [`Taps::tap`], with taps up to `gap` beats apart still counting
    /// towards the rate, and a first tap counting as `first` taps per beat.
    pub fn count(&mut self, velocity: u8, now: u64, period: f64, chords: bool, gap: f64, first: f64) {
        let since = self.last_tap.map(|last| (now - last) as f64 / period);
        self.rate = match since {
            // A chord's notes are one tap.
            Some(dt) if dt < CHORD_BEATS && chords => self.rate,
            Some(dt) if dt < gap => 0.5 * self.rate + 0.5 * (1.0 / dt.max(1.0 / MAX_RATE)),
            _ => first,
        };
        self.last_tap = Some(now);
        self.touch = if since.is_some() { 0.6 * self.touch + 0.4 * velocity as f32 } else { velocity as f32 };
    }

    /// How many beats may pass between taps before they stop counting.
    pub fn gap(&self) -> f64 {
        1.25 / self.rate.max(0.25) + 0.5
    }

    /// Whether the last tap is recent enough at engine frame `at` to still count.
    pub fn recent(&self, at: u64, period: f64) -> bool {
        self.last_tap.is_some_and(|last| at.saturating_sub(last) as f64 / period <= self.gap())
    }

    /// Forgets the taps; the touch stays for the next first tap to replace.
    pub fn clear(&mut self) {
        self.rate = 0.0;
        self.last_tap = None;
    }
}

/// 0.5 taps a beat is calm (0.25), one is eighths (0.5), two sixteenths
/// (0.75), four and more everything (1).
pub fn energy_of(rate: f64) -> f32 {
    if rate <= 0.0 {
        return 0.0;
    }
    (0.25 + 0.25 * (rate / 0.5).log2()).clamp(0.0, 1.0) as f32
}

/// `energy` moved towards `target` over `beats`: quickly up, slower down.
pub fn follow(energy: f32, target: f32, beats: f64) -> f32 {
    let k = (beats / if target > energy { RISE_BEATS } else { FALL_BEATS }).min(1.0) as f32;
    energy + (target - energy) * k
}
