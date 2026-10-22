//! How the harmony of live audio changes from beat to beat, to find the 1:
//! chords mostly change on it. The input is low-passed and decimated, and
//! about twelve times a second a 4096-point spectrum is folded into bass and
//! treble chroma. Those frames are kept with their times, so the chroma of
//! any recent stretch, such as a beat, can be read back.
//!
//! Nothing here allocates after [`Harmony::new`].

use crate::fft::Fft;
use crate::listen::Biquad;

const SIZE: usize = 4096;
/// The decimated rate aimed for; the factor is the nearest whole number.
const RATE_HZ: f32 = 12_000.0;
/// A new frame every this many decimated samples.
const EVERY: usize = 512;
/// Frames kept: about 10 s.
const FRAMES: usize = 256;
const BASS_HZ: (f32, f32) = (40.0, 250.0);
const TREBLE_HZ: (f32, f32) = (250.0, 2_000.0);
/// The treble's weight when two stretches are compared; the bass leads.
const TREBLE_WEIGHT: f32 = 0.5;
/// Peak-to-average ratios of a chroma: up to the first it's noise, and it
/// counts fully as pitched from the first plus the second.
const TONAL_FLAT: f32 = 1.5;
const TONAL_RANGE: f32 = 1.0;

/// Bass then treble chroma.
pub(crate) type Chroma = [f32; 24];

pub(crate) struct Harmony {
    filters: [Biquad; 2],
    factor: usize,
    phase: usize,
    ring: Box<[f32]>,
    write: usize,
    since: usize,
    fft: Fft,
    window: Box<[f32]>,
    re: Box<[f32]>,
    im: Box<[f32]>,
    bin_hz: f32,
    /// The bins from the bass's lowest to the treble's highest.
    lo: usize,
    hi: usize,
    mags: Box<[f32]>,
    /// Each frame's centre as an engine frame, and its chroma.
    times: Box<[u64]>,
    frames: Box<[Chroma]>,
    count: u64,
    /// The engine frame of the next input sample.
    clock: u64,
}

impl Harmony {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let factor = (sample_rate / RATE_HZ).round().max(1.0) as usize;
        let rate = sample_rate / factor as f32;
        let bin_hz = rate / SIZE as f32;
        let cutoff = 0.4 * rate;
        Self {
            filters: [Biquad::low_pass(sample_rate, cutoff); 2],
            factor,
            phase: 0,
            ring: vec![0.0; SIZE].into(),
            write: 0,
            since: 0,
            fft: Fft::new(SIZE),
            window: (0..SIZE).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos()).collect(),
            re: vec![0.0; SIZE].into(),
            im: vec![0.0; SIZE].into(),
            bin_hz,
            lo: ((BASS_HZ.0 / bin_hz) as usize).max(2),
            hi: ((TREBLE_HZ.1 / bin_hz) as usize).min(SIZE / 2 - 2),
            mags: vec![0.0; SIZE / 2].into(),
            times: vec![0; FRAMES].into(),
            frames: vec![[0.0; 24]; FRAMES].into(),
            count: 0,
            clock: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.ring.fill(0.0);
        self.since = 0;
        self.count = 0;
    }

    /// Takes `input`, whose first sample is at engine frame `start`.
    pub(crate) fn push(&mut self, input: &[f32], start: u64) {
        self.clock = start;
        for &x in input {
            let y = self.filters[0].run(x);
            let y = self.filters[1].run(y);
            self.clock += 1;
            self.phase += 1;
            if self.phase < self.factor {
                continue;
            }
            self.phase = 0;
            self.ring[self.write] = y;
            self.write = (self.write + 1) % SIZE;
            self.since += 1;
            if self.since >= EVERY {
                self.since = 0;
                self.frame();
            }
        }
    }

    fn frame(&mut self) {
        for i in 0..SIZE {
            self.re[i] = self.ring[(self.write + i) % SIZE] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);
        for k in self.lo - 1..=self.hi + 1 {
            self.mags[k] = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt();
        }
        let loudest = self.mags[self.lo..=self.hi].iter().fold(0.0f32, |m, &v| m.max(v));
        let mut chroma = [0.0f32; 24];
        for k in self.lo..=self.hi {
            let (a, b, c) = (self.mags[k - 1], self.mags[k], self.mags[k + 1]);
            // Spectral peaks only: the leakage around them would smear the pitch.
            if b <= a || b < c || b < loudest * 0.01 {
                continue;
            }
            let denom = a - 2.0 * b + c;
            let shift = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
            let hz = (k as f32 + shift) * self.bin_hz;
            let midi = 69.0 + 12.0 * (hz / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            let weight = 1.0 - (midi - midi.round()).abs();
            if hz >= BASS_HZ.0 && hz < BASS_HZ.1 {
                chroma[pc] += b.sqrt() * weight;
            } else if hz >= TREBLE_HZ.0 && hz < TREBLE_HZ.1 {
                chroma[12 + pc] += b.sqrt() * weight;
            }
        }
        let at = (self.count as usize) % FRAMES;
        // The window's centre, in engine frames.
        self.times[at] = self.clock.saturating_sub((SIZE / 2 * self.factor) as u64);
        self.frames[at] = chroma;
        self.count += 1;
    }

    /// The part of a stretch whose frames hear only that stretch, in engine
    /// frames: half a window in from each end, so neither the next chord nor
    /// the drums' attacks on the beats blur it. A short stretch keeps its
    /// middle frame.
    pub(crate) fn inside(&self, from: f64, to: f64) -> (f64, f64) {
        let half = (SIZE / 2 * self.factor) as f64;
        let (a, b) = (from + 0.5 * half, to - half);
        if a < b {
            return (a, b);
        }
        let mid = 0.5 * (from + to);
        let step = (EVERY * self.factor) as f64;
        (mid - 0.5 * step, mid + 0.5 * step)
    }

    /// The average chroma of the frames centred between engine frames `from`
    /// and `to`; `None` when there are none.
    pub(crate) fn between(&self, from: f64, to: f64) -> Option<Chroma> {
        let mut sum = [0.0f32; 24];
        let mut n = 0;
        let kept = self.count.min(FRAMES as u64);
        for j in 0..kept {
            let at = ((self.count - 1 - j) as usize) % FRAMES;
            let t = self.times[at] as f64;
            if t < from {
                break;
            }
            if t < to {
                for (s, v) in sum.iter_mut().zip(&self.frames[at]) {
                    *s += v;
                }
                n += 1;
            }
        }
        (n > 0).then_some(sum)
    }
}

/// How different two stretches' harmony is, 0 (the same) to 1. Each part
/// counts only as far as both stretches sound like pitches rather than noise.
pub(crate) fn change(a: &Chroma, b: &Chroma) -> f32 {
    let distance = |x: &[f32], y: &[f32]| {
        let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
        let norm = (x.iter().map(|p| p * p).sum::<f32>() * y.iter().map(|q| q * q).sum::<f32>()).sqrt();
        let d = if norm > 1e-12 { 1.0 - dot / norm } else { 0.0 };
        d * tonal(x).min(tonal(y))
    };
    (distance(&a[..12], &b[..12]) + TREBLE_WEIGHT * distance(&a[12..], &b[12..])) / (1.0 + TREBLE_WEIGHT)
}

/// 0 for a flat chroma (noise, drums) up to 1 for one with clear pitches:
/// the loudest pitch class against the average.
fn tonal(c: &[f32]) -> f32 {
    let mean = c.iter().sum::<f32>() / c.len() as f32;
    let peak = c.iter().fold(0.0f32, |m, &v| m.max(v));
    if mean <= 1e-12 {
        return 0.0;
    }
    ((peak / mean - TONAL_FLAT) / TONAL_RANGE).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// A chord of sines (MIDI keys) from `from` to `to` seconds.
    fn add_chord(out: &mut [f32], keys: &[f32], from: f32, to: f32) {
        for key in keys {
            let hz = 440.0 * 2f32.powf((key - 69.0) / 12.0);
            for i in (from * SR) as usize..((to * SR) as usize).min(out.len()) {
                out[i] += 0.1 * (2.0 * std::f32::consts::PI * hz * i as f32 / SR).sin();
            }
        }
    }

    #[test]
    fn a_new_chord_is_a_change_and_the_same_one_is_not() {
        let mut signal = vec![0.0f32; (6.0 * SR) as usize];
        // C major with a C bass for 3 s, then F major with an F bass.
        add_chord(&mut signal, &[36.0, 60.0, 64.0, 67.0], 0.0, 3.0);
        add_chord(&mut signal, &[41.0, 60.0, 65.0, 69.0], 3.0, 6.0);
        let mut h = Harmony::new(SR);
        for (i, block) in signal.chunks(128).enumerate() {
            h.push(block, (i * 128) as u64);
        }
        let s = |t: f32| t as f64 * SR as f64;
        let c1 = h.between(s(0.8), s(1.8)).unwrap();
        let c2 = h.between(s(1.8), s(2.7)).unwrap();
        let f = h.between(s(3.3), s(4.5)).unwrap();
        assert!(change(&c1, &c2) < 0.05, "{}", change(&c1, &c2));
        assert!(change(&c2, &f) > 0.3, "{}", change(&c2, &f));
        // The C bass is the loudest bass pitch class.
        let bass = c1[..12].iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(bass, 0);
        assert!(h.between(s(10.0), s(11.0)).is_none());
    }
}
