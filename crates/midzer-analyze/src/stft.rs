//! Short-time Fourier transform: magnitude spectra of Hann-windowed frames.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

pub struct Stft {
    pub size: usize,
    pub hop: usize,
    window: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
}

impl Stft {
    pub fn new(size: usize, hop: usize) -> Self {
        let window = (0..size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
            .collect();
        Self { size, hop, window, fft: RealFftPlanner::new().plan_fft_forward(size) }
    }

    /// Frames of `signal`, the frame `t` centred on sample `t * hop`.
    pub fn frames(&self, signal: &[f32]) -> usize {
        signal.len() / self.hop + 1
    }

    /// Calls `each(t, magnitudes)` for every frame; `magnitudes` has `size / 2 + 1` bins.
    pub fn run(&self, signal: &[f32], mut each: impl FnMut(usize, &[f32])) {
        let mut input = self.fft.make_input_vec();
        let mut output = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        let mut mags = vec![0.0f32; self.size / 2 + 1];
        let half = self.size as isize / 2;
        for t in 0..self.frames(signal) {
            let start = (t * self.hop) as isize - half;
            for (i, slot) in input.iter_mut().enumerate() {
                let j = start + i as isize;
                *slot = if j >= 0 && (j as usize) < signal.len() { signal[j as usize] * self.window[i] } else { 0.0 };
            }
            self.fft.process_with_scratch(&mut input, &mut output, &mut scratch).expect("buffer sizes match the plan");
            for (m, c) in mags.iter_mut().zip(&output) {
                *m = c.norm();
            }
            each(t, &mags);
        }
    }
}

/// A spectral peak: frequency in Hz and linear magnitude.
#[derive(Clone, Copy, Debug)]
pub struct Peak {
    pub hz: f32,
    pub mag: f32,
}

/// Local maxima of `mags` above `floor` between `lo` and `hi` Hz, refined by
/// parabolic interpolation of the log magnitudes.
pub fn peaks(mags: &[f32], bin_hz: f32, lo: f32, hi: f32, floor: f32, out: &mut Vec<Peak>) {
    out.clear();
    let first = ((lo / bin_hz) as usize).max(1);
    let last = ((hi / bin_hz) as usize).min(mags.len() - 2);
    for k in first..=last {
        let b = mags[k];
        if b <= floor || b <= mags[k - 1] || b < mags[k + 1] {
            continue;
        }
        let (la, lb, lc) = (mags[k - 1].max(1e-12).ln(), b.ln(), mags[k + 1].max(1e-12).ln());
        let denom = la - 2.0 * lb + lc;
        let delta = if denom.abs() > 1e-12 { (0.5 * (la - lc) / denom).clamp(-0.5, 0.5) } else { 0.0 };
        let mag = (lb - 0.25 * (la - lc) * delta).exp();
        out.push(Peak { hz: (k as f32 + delta) * bin_hz, mag });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_frequency_of_a_tone_between_bins() {
        let rate = 22_050.0;
        let hz = 443.7;
        let signal: Vec<f32> = (0..8192).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate).sin()).collect();
        let stft = Stft::new(4096, 4096);
        let mut found = Vec::new();
        stft.run(&signal, |t, mags| {
            if t == 1 {
                peaks(mags, rate / 4096.0, 50.0, 5000.0, 1.0, &mut found);
            }
        });
        let top = found.iter().max_by(|a, b| a.mag.total_cmp(&b.mag)).unwrap();
        assert!((top.hz - hz).abs() < 0.5, "found {}", top.hz);
    }
}
