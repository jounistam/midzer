//! An audio fingerprint for recognising a song again: landmark hashes, as
//! Shazam made them (Wang, ISMIR 2003). The audio is taken to 11.025 kHz and
//! a spectrogram's strongest local peaks are found; each peak is paired with
//! a few after it, and a pair's two frequencies and the time between them
//! make a hash. The same music gives the same hashes, at the same distance
//! apart in time, wherever it starts and over some noise, so matching hashes
//! that agree on one time offset name the song and the place in it.
//!
//! Imports and live-learned songs store their hashes; the listener hashes
//! what it hears with the same code.

use realfft::RealFftPlanner;

/// The input rate, and the rate the hashes are made at.
pub const RATE: f64 = 22_050.0;
const SIZE: usize = 1024;
const HOP: usize = 256;
/// Hash frames a second (at 11.025 kHz).
pub const FPS: f64 = RATE / 2.0 / HOP as f64;
/// Bins up to about 5 kHz.
const BINS: usize = 464;
/// A peak is the highest in this many frames and bins either side, and above `FLOOR_DB`.
const NEAR_FRAMES: usize = 8;
const NEAR_BINS: usize = 16;
const FLOOR_DB: f32 = -60.0;
/// The strongest peaks kept a frame.
const PEAKS_PER_FRAME: usize = 3;
/// Each peak pairs with this many after it, at most `MAX_DT` frames on and `MAX_DF` bins away.
const FAN_OUT: usize = 3;
const MAX_DT: usize = 63;
const MAX_DF: i32 = 63;

/// A landmark: the hash, and the frame of its first peak.
pub type Landmark = (u32, u32);

/// The landmarks of mono audio at 22.05 kHz, in frame order.
pub fn fingerprint(samples: &[f32]) -> Vec<Landmark> {
    let audio = halve(samples);
    let spec = spectrogram(&audio);
    let frames = spec.len() / BINS;
    let peaks = peaks(&spec, frames);
    let mut out = Vec::new();
    for (i, &(t1, f1)) in peaks.iter().enumerate() {
        let mut paired = 0;
        for &(t2, f2) in &peaks[i + 1..] {
            let dt = t2 - t1;
            if dt > MAX_DT {
                break;
            }
            let df = f2 as i32 - f1 as i32;
            if dt == 0 || df.abs() > MAX_DF {
                continue;
            }
            out.push((hash(f1, df, dt), t1 as u32));
            paired += 1;
            if paired == FAN_OUT {
                break;
            }
        }
    }
    out
}

fn hash(f1: usize, df: i32, dt: usize) -> u32 {
    ((f1 as u32) << 13) | (((df + 64) as u32) << 6) | dt as u32
}

/// Down to 11.025 kHz, through a small low-pass.
fn halve(samples: &[f32]) -> Vec<f32> {
    let at = |i: usize| samples.get(i).copied().unwrap_or(0.0);
    (0..samples.len() / 2).map(|k| 0.25 * at((2 * k).wrapping_sub(1)) + 0.5 * at(2 * k) + 0.25 * at(2 * k + 1)).collect()
}

/// Levels in dB, `BINS` a frame; a full-scale sine is about 0 dB.
fn spectrogram(audio: &[f32]) -> Vec<f32> {
    if audio.len() < SIZE {
        return Vec::new();
    }
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let window: Vec<f32> = (0..SIZE).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos()).collect();
    let norm = 2.0 / window.iter().sum::<f32>();
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let frames = (audio.len() - SIZE) / HOP + 1;
    let mut out = Vec::with_capacity(frames * BINS);
    for f in 0..frames {
        let from = f * HOP;
        for i in 0..SIZE {
            input[i] = audio[from + i] * window[i];
        }
        fft.process_with_scratch(&mut input, &mut spectrum, &mut scratch).expect("fft sizes");
        out.extend(spectrum[..BINS].iter().map(|c| 20.0 * (c.norm() * norm).max(1e-6).log10()));
    }
    out
}

/// The peaks as (frame, bin), in frame order.
fn peaks(spec: &[f32], frames: usize) -> Vec<(usize, usize)> {
    // The highest level near each cell: across bins, then across frames.
    let mut across = vec![0.0f32; spec.len()];
    for f in 0..frames {
        let row = &spec[f * BINS..(f + 1) * BINS];
        for b in 0..BINS {
            let (lo, hi) = (b.saturating_sub(NEAR_BINS), (b + NEAR_BINS + 1).min(BINS));
            across[f * BINS + b] = row[lo..hi].iter().fold(f32::MIN, |m, &v| m.max(v));
        }
    }
    let mut out = Vec::new();
    let mut found: Vec<(f32, usize)> = Vec::new();
    for f in 0..frames {
        let (lo, hi) = (f.saturating_sub(NEAR_FRAMES), (f + NEAR_FRAMES + 1).min(frames));
        found.clear();
        for b in 0..BINS {
            let v = spec[f * BINS + b];
            if v < FLOOR_DB {
                continue;
            }
            if (lo..hi).all(|g| across[g * BINS + b] <= v) {
                found.push((v, b));
            }
        }
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut bins: Vec<usize> = found.iter().take(PEAKS_PER_FRAME).map(|p| p.1).collect();
        bins.sort_unstable();
        out.extend(bins.into_iter().map(|b| (f, b)));
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Random notes with a few harmonics, a new one every quarter second.
    pub(crate) fn tune(seconds: f64, seed: u32) -> Vec<f32> {
        let mut seed = seed;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        let notes: Vec<f64> = (0..(seconds * 4.0) as usize + 1).map(|_| 110.0 * 2f64.powf((next() % 36) as f64 / 12.0)).collect();
        (0..(seconds * RATE) as usize)
            .map(|i| {
                let t = i as f64 / RATE;
                let hz = notes[(t * 4.0) as usize];
                let decay = (-(t % 0.25) * 6.0).exp();
                (1..4).map(|h| 0.2 / h as f64 * (std::f64::consts::TAU * hz * h as f64 * t).sin()).sum::<f64>() as f32 * decay as f32
            })
            .collect()
    }

    /// How many landmarks of `heard` match `song` at the offset most of them agree on, and that offset.
    fn best_offset(song: &[Landmark], heard: &[Landmark]) -> (usize, i64) {
        let mut index: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(h, t) in song {
            index.entry(h).or_default().push(t);
        }
        let mut votes: HashMap<i64, usize> = HashMap::new();
        for &(h, t) in heard {
            for &s in index.get(&h).into_iter().flatten() {
                *votes.entry(s as i64 - t as i64).or_default() += 1;
            }
        }
        votes.into_iter().map(|(o, n)| (n, o)).max().unwrap_or((0, 0))
    }

    #[test]
    fn the_same_music_matches_at_one_offset_and_other_music_does_not() {
        let song = tune(30.0, 1);
        let marks = fingerprint(&song);
        let per_second = marks.len() as f64 / 30.0;
        assert!((20.0..200.0).contains(&per_second), "{per_second} landmarks a second");

        // Ten seconds from 12.3 s in, a little quieter and with some hiss.
        let from = (12.3 * RATE) as usize;
        let mut seed = 9u32;
        let heard: Vec<f32> = song[from..from + (10.0 * RATE) as usize]
            .iter()
            .map(|&x| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                0.7 * x + 0.01 * ((seed >> 16) as f32 / 32768.0 - 1.0)
            })
            .collect();
        let (n, offset) = best_offset(&marks, &fingerprint(&heard));
        assert!((offset as f64 - 12.3 * FPS).abs() <= 1.0, "offset {offset}");
        assert!(n >= 50, "{n} matches");

        let (other, _) = best_offset(&marks, &fingerprint(&tune(10.0, 2)));
        assert!(other * 5 < n, "{other} matches with other music");
        assert!(fingerprint(&vec![0.0; RATE as usize * 5]).is_empty(), "silence has none");
    }
}
