//! The log-mel spectrogram Beat This! reads, as torchaudio makes it: 22.05 kHz,
//! a 1024-point FFT with a periodic Hann window every 441 samples (50 frames a
//! second), magnitudes over √1024 (torchaudio's "frame_length" normalization),
//! 128 Slaney mel bands from 30 Hz to 11 kHz, and ln(1 + 1000·x).

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};
use realfft::num_complex::Complex;

use crate::beat_this::MELS;

pub const RATE: u32 = 22_050;
pub const HOP: usize = 441;
pub const SIZE: usize = 1024;
const BINS: usize = SIZE / 2 + 1;

pub struct Mel {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// [513, 128].
    filters: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mags: Vec<f32>,
}

impl Mel {
    pub fn new(filters: Vec<f32>) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(SIZE);
        let norm = (SIZE as f32).sqrt();
        let window = (0..SIZE).map(|i| (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos()) / norm).collect();
        Self {
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
            filters,
            input: vec![0.0; SIZE],
            mags: vec![0.0; BINS],
        }
    }

    /// One frame from the `SIZE` samples around its centre, into `out` [128].
    pub fn frame(&mut self, samples: &[f32], out: &mut [f32]) {
        for i in 0..SIZE {
            self.input[i] = samples[i] * self.window[i];
        }
        self.fft.process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch).expect("fft sizes");
        for (m, c) in self.mags.iter_mut().zip(&self.spectrum) {
            *m = c.norm();
        }
        out[..MELS].fill(0.0);
        for (k, &m) in self.mags.iter().enumerate() {
            if m == 0.0 {
                continue;
            }
            let row = &self.filters[k * MELS..(k + 1) * MELS];
            for j in 0..MELS {
                out[j] += m * row[j];
            }
        }
        for v in out[..MELS].iter_mut() {
            *v = (1.0 + 1000.0 * *v).ln();
        }
    }

    /// The whole spectrogram of `signal`, padded by reflection at both ends
    /// like torchaudio: [frames, 128].
    pub fn whole(&mut self, signal: &[f32]) -> Vec<f32> {
        let half = SIZE / 2;
        let n = signal.len();
        let reflect = |i: isize| -> f32 {
            let mut i = i;
            if i < 0 {
                i = -i;
            }
            if i >= n as isize {
                i = 2 * (n as isize - 1) - i;
            }
            signal[i.clamp(0, n as isize - 1) as usize]
        };
        let frames = n / HOP + 1;
        let mut out = vec![0.0; frames * MELS];
        let mut buf = vec![0.0; SIZE];
        for f in 0..frames {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = reflect((f * HOP + i) as isize - half as isize);
            }
            self.frame(&buf, &mut out[f * MELS..(f + 1) * MELS]);
        }
        out
    }
}
