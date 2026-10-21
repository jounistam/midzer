//! Beat tracking: an onset-strength envelope, the tempo from its
//! autocorrelation, and beat times by dynamic programming (Ellis 2007).

use crate::stft::Stft;
use crate::RATE;

pub const HOP: usize = 256;
const SIZE: usize = 1024;
const BANDS: usize = 40;
const LOW_BAND_HZ: f32 = 200.0;
/// The tempo prior: a log-normal around this many BPM.
const PRIOR_BPM: f32 = 115.0;
const PRIOR_OCTAVES: f32 = 0.7;
const MIN_BPM: f32 = 50.0;
const MAX_BPM: f32 = 220.0;
/// How strongly the beat tracker keeps the period (librosa's default).
const TIGHTNESS: f32 = 100.0;

pub fn frames_per_second() -> f32 {
    RATE as f32 / HOP as f32
}

/// Onset strength per frame: all bands, and only the bass bands.
pub struct Onsets {
    pub all: Vec<f32>,
    pub low: Vec<f32>,
}

/// Spectral flux over log-spaced bands, compared with the frame two hops back.
pub fn onsets(signal: &[f32]) -> Onsets {
    let stft = Stft::new(SIZE, HOP);
    let bin_hz = RATE as f32 / SIZE as f32;
    // Triangular bands spaced evenly in log frequency from 40 Hz to 10 kHz.
    let edges: Vec<f32> = (0..BANDS + 2).map(|i| 40.0 * (10_000.0f32 / 40.0).powf(i as f32 / (BANDS + 1) as f32)).collect();
    let bands: Vec<Vec<(usize, f32)>> = (0..BANDS)
        .map(|b| {
            let (lo, mid, hi) = (edges[b], edges[b + 1], edges[b + 2]);
            let mut w: Vec<(usize, f32)> = ((lo / bin_hz).floor() as usize..=(hi / bin_hz).ceil() as usize)
                .filter_map(|k| {
                    let f = k as f32 * bin_hz;
                    let v = if f < mid { (f - lo) / (mid - lo) } else { (hi - f) / (hi - mid) };
                    (v > 0.0).then_some((k, v))
                })
                .collect();
            if w.is_empty() {
                w.push(((mid / bin_hz).round() as usize, 1.0));
            }
            w
        })
        .collect();
    let low_bands = edges.iter().skip(1).take(BANDS).filter(|&&f| f < LOW_BAND_HZ).count();

    let peak = signal.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
    let floor = (1e-4 * peak * SIZE as f32).ln();
    let n = stft.frames(signal);
    let mut log_bands = vec![[0.0f32; BANDS]; n];
    stft.run(signal, |t, mags| {
        for (b, weights) in bands.iter().enumerate() {
            let e: f32 = weights.iter().map(|&(k, w)| mags.get(k).copied().unwrap_or(0.0) * w).sum();
            log_bands[t][b] = e.max(1e-12).ln().max(floor);
        }
    });
    let mut all = vec![0.0f32; n];
    let mut low = vec![0.0f32; n];
    for t in 2..n {
        for b in 0..BANDS {
            // Compare with the loudest neighbouring band, so vibrato doesn't count.
            let prev = (b.saturating_sub(1)..=(b + 1).min(BANDS - 1)).map(|j| log_bands[t - 2][j]).fold(f32::MIN, f32::max);
            let rise = (log_bands[t][b] - prev).max(0.0);
            all[t] += rise;
            if b < low_bands {
                low[t] += rise;
            }
        }
    }
    Onsets { all: normalize(detrend(&all)), low: normalize(detrend(&low)) }
}

/// Subtracts a moving average of about half a second and keeps what rises above it.
fn detrend(x: &[f32]) -> Vec<f32> {
    let half = (0.25 * frames_per_second()) as usize;
    let mut prefix = vec![0.0f64; x.len() + 1];
    for (i, v) in x.iter().enumerate() {
        prefix[i + 1] = prefix[i] + *v as f64;
    }
    (0..x.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(x.len()));
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            (x[i] - mean as f32).max(0.0)
        })
        .collect()
}

fn normalize(x: Vec<f32>) -> Vec<f32> {
    let mean = x.iter().sum::<f32>() / x.len().max(1) as f32;
    let sd = (x.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / x.len().max(1) as f32).sqrt();
    if sd < 1e-9 {
        return x;
    }
    x.into_iter().map(|v| v / sd).collect()
}

/// The beat period in frames, from the autocorrelation of the onset envelope
/// weighted by a tempo prior.
pub fn tempo_period(env: &[f32]) -> f32 {
    let fps = frames_per_second();
    let min_lag = (60.0 * fps / MAX_BPM).floor() as usize;
    let max_lag = (60.0 * fps / MIN_BPM).ceil() as usize;
    let ac: Vec<f32> = (0..=max_lag + 1)
        .map(|lag| {
            if lag >= env.len() {
                return 0.0;
            }
            let s: f32 = env.iter().zip(&env[lag..]).map(|(a, b)| a * b).sum();
            s / (env.len() - lag) as f32
        })
        .collect();
    let weighted = |lag: usize| {
        let bpm = 60.0 * fps / lag as f32;
        let z = (bpm / PRIOR_BPM).log2() / PRIOR_OCTAVES;
        ac[lag] * (-0.5 * z * z).exp()
    };
    let best = (min_lag..=max_lag).max_by(|&a, &b| weighted(a).total_cmp(&weighted(b))).unwrap_or(min_lag);
    // Refine between lags with a parabola through the neighbours.
    let (a, b, c) = (ac[best - 1], ac[best], ac[best + 1]);
    let denom = a - 2.0 * b + c;
    let delta = if denom.abs() > 1e-9 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
    best as f32 + delta
}

/// Beat frames that follow strong onsets while keeping close to `period`.
pub fn track(env: &[f32], period: f32) -> Vec<usize> {
    let n = env.len();
    if n < 4 {
        return Vec::new();
    }
    // Smooth the envelope with a Gaussian a fraction of a beat wide.
    let width = period.round() as isize;
    let kernel: Vec<f32> = (-width..=width).map(|i| (-0.5 * (i as f32 * 32.0 / period).powi(2)).exp()).collect();
    let local: Vec<f32> = (0..n as isize)
        .map(|t| {
            kernel
                .iter()
                .enumerate()
                .map(|(k, w)| {
                    let j = t + k as isize - width;
                    if j >= 0 && (j as usize) < n { env[j as usize] * w } else { 0.0 }
                })
                .sum()
        })
        .collect();

    let lo = (period * 2.0).round() as usize;
    let hi = (period / 2.0).round().max(1.0) as usize;
    let penalty: Vec<f32> = (hi..=lo).map(|d| -TIGHTNESS * (d as f32 / period).ln().powi(2)).collect();
    let mut score = vec![0.0f32; n];
    let mut back = vec![usize::MAX; n];
    let max_local = local.iter().fold(0.0f32, |m, &v| m.max(v));
    let mut started = false;
    for t in 0..n {
        let mut best = f32::MIN;
        let mut from = usize::MAX;
        if t >= hi {
            for d in hi..=lo.min(t) {
                let s = score[t - d] + penalty[d - hi];
                if s > best {
                    best = s;
                    from = t - d;
                }
            }
        }
        // Until the first clear onset, beats may start anywhere.
        if !started || from == usize::MAX {
            best = 0.0;
            from = usize::MAX;
        }
        score[t] = local[t] + best;
        back[t] = from;
        if local[t] > 0.01 * max_local {
            started = true;
        }
    }

    // The last beat: the last local maximum of the score above half the median of those maxima.
    let maxima: Vec<usize> = (1..n - 1).filter(|&t| score[t] > score[t - 1] && score[t] >= score[t + 1]).collect();
    let Some(mut t) = (if maxima.is_empty() {
        None
    } else {
        let mut values: Vec<f32> = maxima.iter().map(|&t| score[t]).collect();
        values.sort_by(f32::total_cmp);
        let median = values[values.len() / 2];
        maxima.iter().rev().find(|&&t| score[t] > 0.5 * median).copied()
    }) else {
        return Vec::new();
    };
    let mut beats = vec![t];
    while back[t] != usize::MAX {
        t = back[t];
        beats.push(t);
    }
    beats.reverse();

    // Drop weak beats at either end (silence before and after the music).
    let rms = (local.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
    let strong = |b: &usize| local[*b] > 0.5 * rms;
    let first = beats.iter().position(strong).unwrap_or(0);
    let last = beats.iter().rposition(strong).unwrap_or(beats.len() - 1);
    beats[first..=last].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Short noise bursts at `bpm`, louder on every fourth.
    fn clicks(bpm: f32, seconds: f32) -> Vec<f32> {
        let mut x = vec![0.0f32; (RATE as f32 * seconds) as usize];
        let period = 60.0 / bpm * RATE as f32;
        let mut seed = 1u32;
        let mut k = 0;
        let mut at = 0.5 * RATE as f32;
        while (at as usize) + 2000 < x.len() {
            let amp = if k % 4 == 0 { 0.9 } else { 0.5 };
            for i in 0..1500 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1 << 23) as f32 * 2.0 - 1.0;
                x[at as usize + i] += amp * noise * (-(i as f32) / 300.0).exp();
            }
            at += period;
            k += 1;
        }
        x
    }

    #[test]
    fn finds_the_tempo_and_the_beats_of_clicks() {
        for bpm in [72.0, 100.0, 128.0, 150.0] {
            let x = clicks(bpm, 20.0);
            let env = onsets(&x).all;
            let period = tempo_period(&env);
            let found = 60.0 * frames_per_second() / period;
            assert!((found - bpm).abs() < 1.5, "{bpm} BPM read as {found}");
            let beats = track(&env, period);
            let expected = ((20.0 - 0.5) * bpm / 60.0) as usize;
            assert!(beats.len().abs_diff(expected) <= 2, "{} beats, expected {expected}", beats.len());
            let first = beats[0] as f32 / frames_per_second();
            let phase = ((first - 0.5) / (60.0 / bpm)).fract();
            assert!(phase < 0.05 || phase > 0.95, "beats off the clicks by {phase} of a beat");
        }
    }
}
