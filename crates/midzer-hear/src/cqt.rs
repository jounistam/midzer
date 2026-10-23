//! The constant-Q spectrum BTC reads, as librosa's `cqt` makes it: 144 bins,
//! 24 an octave from C1, a frame every 2048 samples at 22.05 kHz, scaled by
//! the square root of each filter's length; BTC takes ln(|C| + 1e-6).
//!
//! librosa filters octave by octave on a halved signal; this does it directly
//! at the full rate, with the same Hann-windowed complex filters, which
//! matches it to a median of 0.01 in the log (checked against librosa on real
//! songs). The lowest filter is about a second long, so a frame needs about
//! half a second of audio on each side.

use crate::btc::BINS;

pub const HOP: usize = 2048;
const RATE: f32 = 22_050.0;
const PER_OCTAVE: f32 = 24.0;
/// C1 in Hz.
const FMIN: f64 = 32.703_195_662_574_83;

struct Filter {
    /// Offset of the first tap from the frame's centre.
    start: isize,
    /// The window over its sum, times √length, times cos and −sin of the carrier.
    re: Vec<f32>,
    im: Vec<f32>,
}

pub struct Cqt {
    filters: Vec<Filter>,
    /// Samples each side of a frame's centre that the longest filter reaches.
    pub reach: usize,
}

impl Default for Cqt {
    fn default() -> Self {
        Self::new()
    }
}

impl Cqt {
    pub fn new() -> Self {
        let r = 2f64.powf(2.0 / PER_OCTAVE as f64);
        let q = (r + 1.0) / (r - 1.0);
        let filters: Vec<Filter> = (0..BINS)
            .map(|k| {
                let f = FMIN * 2f64.powf(k as f64 / PER_OCTAVE as f64);
                let length = q * RATE as f64 / f;
                let (lo, hi) = ((-length / 2.0).floor() as isize, (length / 2.0).floor() as isize);
                let n = (hi - lo) as usize;
                let window: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()).collect();
                let gain = length.sqrt() / window.iter().sum::<f64>();
                let w = 2.0 * std::f64::consts::PI * f / RATE as f64;
                let re = (0..n).map(|i| (window[i] * gain * (w * (lo + i as isize) as f64).cos()) as f32).collect();
                let im = (0..n).map(|i| (-window[i] * gain * (w * (lo + i as isize) as f64).sin()) as f32).collect();
                Filter { start: lo, re, im }
            })
            .collect();
        let reach = filters.iter().map(|f| (-f.start) as usize).max().unwrap_or(0) + 1;
        Self { filters, reach }
    }

    /// ln(|C| + 1e-6) of the frame centred on sample `centre` of `signal`,
    /// which counts as silent outside `from..to`.
    pub fn frame(&self, signal: &[f32], centre: isize, from: usize, to: usize, out: &mut [f32]) {
        let to = to.min(signal.len());
        for (k, f) in self.filters.iter().enumerate() {
            let first = centre + f.start;
            let a = (from as isize - first).max(0) as usize;
            let b = ((to as isize - first).max(0) as usize).min(f.re.len());
            let (mut re, mut im) = (0.0f32, 0.0f32);
            if a < b {
                let x = &signal[(first + a as isize) as usize..(first + b as isize) as usize];
                let (kr, ki) = (&f.re[a..b], &f.im[a..b]);
                for i in 0..x.len() {
                    re += x[i] * kr[i];
                    im += x[i] * ki[i];
                }
            }
            out[k] = ((re * re + im * im).sqrt() + 1e-6).ln();
        }
    }

    /// The frames librosa gives for the whole of `signal`: [frames, 144].
    pub fn whole(&self, signal: &[f32]) -> Vec<f32> {
        let frames = signal.len() / HOP + 1;
        let mut out = vec![0.0; frames * BINS];
        for t in 0..frames {
            self.frame(signal, (t * HOP) as isize, 0, signal.len(), &mut out[t * BINS..(t + 1) * BINS]);
        }
        out
    }
}
