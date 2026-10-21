//! Band-limited resampling with a windowed-sinc kernel read from a table.

const ZERO_CROSSINGS: usize = 12;
const TABLE_STEPS: usize = 256;

/// Resamples `input` from `from` Hz to `to` Hz.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    // Low-pass just below the lower Nyquist frequency, in input samples.
    let cutoff = ratio.min(1.0) * 0.94;
    let half = (ZERO_CROSSINGS as f64 / cutoff).ceil() as usize;
    let table: Vec<f32> = (0..=half * TABLE_STEPS + 1)
        .map(|i| {
            let d = i as f64 / TABLE_STEPS as f64;
            let x = std::f64::consts::PI * cutoff * d;
            let sinc = if d == 0.0 { 1.0 } else { x.sin() / x };
            let w = d / (half as f64 + 1.0);
            // Blackman window over |d| < half + 1.
            let blackman = 0.42 + 0.5 * (std::f64::consts::PI * w).cos() + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
            (cutoff * sinc * blackman.max(0.0)) as f32
        })
        .collect();
    let kernel = |d: f64| {
        let pos = d.abs() * TABLE_STEPS as f64;
        let i = pos as usize;
        if i + 1 >= table.len() {
            return 0.0;
        }
        let frac = (pos - i as f64) as f32;
        table[i] + (table[i + 1] - table[i]) * frac
    };
    let out_len = (input.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let t = n as f64 / ratio;
        let center = t.floor() as isize;
        let lo = (center - half as isize + 1).max(0) as usize;
        let hi = ((center + half as isize) as usize).min(input.len() - 1);
        let mut sum = 0.0f32;
        for (k, x) in input.iter().enumerate().take(hi + 1).skip(lo) {
            sum += x * kernel(t - k as f64);
        }
        out.push(sum);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect()
    }

    #[test]
    fn keeps_a_tone_below_nyquist() {
        let out = resample(&sine(440.0, 44_100, 1.0), 44_100, 22_050);
        assert_eq!(out.len(), 22_050);
        let expected = sine(440.0, 22_050, 1.0);
        let err = out[1000..21_000].iter().zip(&expected[1000..21_000]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.01, "max error {err}");
    }

    #[test]
    fn removes_a_tone_above_the_new_nyquist() {
        let out = resample(&sine(15_000.0, 44_100, 0.5), 44_100, 22_050);
        let peak = out[500..10_000].iter().map(|x| x.abs()).fold(0.0, f32::max);
        assert!(peak < 0.01, "aliased peak {peak}");
    }

    #[test]
    fn upsamples() {
        let out = resample(&sine(1000.0, 22_050, 0.5), 22_050, 44_100);
        let expected = sine(1000.0, 44_100, 0.5);
        let err = out[2000..20_000].iter().zip(&expected[2000..20_000]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.01, "max error {err}");
    }
}
