//! Drum hits from the separated drum stem: onsets in three bands, read as
//! kick, snare and hi-hat. Rough, but enough for a drum track to edit.

use midzer_analyze::stft::Stft;
use midzer_analyze::{RATE, TimedNote};

const SIZE: usize = 1024;
const HOP: usize = 128;
pub const KICK: u8 = 36;
pub const SNARE: u8 = 38;
pub const HIHAT: u8 = 42;
/// (lowest Hz, highest Hz, GM drum key)
const BANDS: [(f32, f32, u8); 3] = [(30.0, 140.0, KICK), (160.0, 2500.0, SNARE), (6000.0, 11_000.0, HIHAT)];
const MIN_GAP_SECONDS: f32 = 0.06;

pub fn hits(drums: &[f32]) -> Vec<TimedNote> {
    let stft = Stft::new(SIZE, HOP);
    let bin_hz = RATE as f32 / SIZE as f32;
    let frames = stft.frames(drums);
    let mut energy = vec![[0.0f32; 3]; frames];
    stft.run(drums, |t, mags| {
        for (b, &(lo, hi, _)) in BANDS.iter().enumerate() {
            let (a, z) = ((lo / bin_hz) as usize, ((hi / bin_hz) as usize).min(mags.len() - 1));
            energy[t][b] = mags[a..=z].iter().map(|m| m * m).sum::<f32>().sqrt();
        }
    });
    let fps = RATE as f32 / HOP as f32;
    let mut out = Vec::new();
    for (b, &(_, _, key)) in BANDS.iter().enumerate() {
        let log: Vec<f32> = energy.iter().map(|e| (e[b] + 1e-6).ln()).collect();
        let flux: Vec<f32> = (0..frames).map(|t| if t < 2 { 0.0 } else { (log[t] - log[t - 2]).max(0.0) }).collect();
        let peak = energy.iter().map(|e| e[b]).fold(0.0f32, f32::max).max(1e-9);
        // A hit: a flux peak above a local median plus a margin, and loud enough.
        let half = (0.1 * fps) as usize;
        let mut last = f32::MIN;
        for t in 1..frames.saturating_sub(1) {
            if flux[t] < flux[t - 1] || flux[t] < flux[t + 1] {
                continue;
            }
            let mut around: Vec<f32> = flux[t.saturating_sub(half)..(t + half).min(frames)].to_vec();
            around.sort_by(f32::total_cmp);
            let median = around[around.len() / 2];
            let level = energy[(t + 2).min(frames - 1)][b] / peak;
            let at = t as f32 / fps;
            if flux[t] > median + 1.0 && level > 0.08 && at - last >= MIN_GAP_SECONDS {
                out.push(TimedNote { start: at, end: at + 0.1, key, level: level.sqrt().min(1.0) });
                last = at;
            }
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hears_kicks_and_hats() {
        let mut x = vec![0.0f32; RATE as usize * 5 / 2];
        let mut seed = 7u32;
        for k in 0..4 {
            let at = (0.25 + 0.5 * k as f32) * RATE as f32;
            for i in 0..3000 {
                let t = i as f32 / RATE as f32;
                x[at as usize + i] += 0.8 * (std::f32::consts::TAU * 55.0 * t).sin() * (-t * 30.0).exp();
            }
            let hat = at as usize + RATE as usize / 4;
            for i in 0..600 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1 << 23) as f32 * 2.0 - 1.0;
                // Crude high-pass: differences of white noise.
                x[hat + i] += 0.3 * noise * (-(i as f32) / 120.0).exp();
            }
        }
        let found = hits(&x);
        let kicks = found.iter().filter(|n| n.key == KICK).count();
        let hats = found.iter().filter(|n| n.key == HIHAT).count();
        assert_eq!((kicks, hats), (4, 4), "{found:?}");
    }
}
