//! Streaming band-limited resampling to the networks' 22.05 kHz, with a
//! windowed-sinc kernel read from a table (like `midzer-analyze`'s, which
//! works on whole files).

const ZERO_CROSSINGS: usize = 12;
const TABLE_STEPS: usize = 256;

pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    half: usize,
    table: Vec<f32>,
    /// Input kept for the kernel, and the input index of its first sample.
    input: Vec<f32>,
    input_start: u64,
    /// The next output sample, in input samples from the start.
    next: f64,
}

impl Resampler {
    pub fn new(from: f32, to: f32) -> Self {
        let ratio = to as f64 / from as f64;
        let cutoff = ratio.min(1.0) * 0.94;
        let half = (ZERO_CROSSINGS as f64 / cutoff).ceil() as usize;
        let table = (0..=half * TABLE_STEPS + 1)
            .map(|i| {
                let d = i as f64 / TABLE_STEPS as f64;
                let x = std::f64::consts::PI * cutoff * d;
                let sinc = if d == 0.0 { 1.0 } else { x.sin() / x };
                let w = d / (half as f64 + 1.0);
                let blackman = 0.42 + 0.5 * (std::f64::consts::PI * w).cos() + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                (cutoff * sinc * blackman.max(0.0)) as f32
            })
            .collect();
        Self { step: 1.0 / ratio, half, table, input: Vec::new(), input_start: 0, next: 0.0 }
    }

    fn kernel(&self, d: f64) -> f32 {
        let pos = d.abs() * TABLE_STEPS as f64;
        let i = pos as usize;
        if i + 1 >= self.table.len() {
            return 0.0;
        }
        let frac = (pos - i as f64) as f32;
        self.table[i] + (self.table[i + 1] - self.table[i]) * frac
    }

    /// Takes the next input samples and appends what can be resampled to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.input.extend_from_slice(input);
        let end = self.input_start + self.input.len() as u64;
        loop {
            let center = self.next.floor() as i64;
            if center + self.half as i64 >= end as i64 {
                break;
            }
            let lo = (center - self.half as i64 + 1).max(self.input_start as i64) as u64;
            let hi = (center + self.half as i64) as u64;
            let mut sum = 0.0f32;
            for k in lo..=hi {
                sum += self.input[(k - self.input_start) as usize] * self.kernel(self.next - k as f64);
            }
            out.push(sum);
            self.next += self.step;
        }
        // Keep only what the kernel still needs.
        let keep_from = (self.next.floor() as i64 - self.half as i64).max(self.input_start as i64) as u64;
        let drop = (keep_from - self.input_start) as usize;
        if drop > 4096 {
            self.input.drain(..drop);
            self.input_start = keep_from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_in_pieces_comes_out_whole() {
        let tone = |hz: f32, rate: f32, n: usize| (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate).sin()).collect::<Vec<_>>();
        let input = tone(440.0, 48_000.0, 48_000);
        let mut r = Resampler::new(48_000.0, 22_050.0);
        let mut out = Vec::new();
        for chunk in input.chunks(128) {
            r.push(chunk, &mut out);
        }
        let want = tone(440.0, 22_050.0, out.len());
        // Past the start-up, within a small error.
        let err = out[100..].iter().zip(&want[100..]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.01, "error {err}");
        assert!(out.len() > 21_900);
    }
}
