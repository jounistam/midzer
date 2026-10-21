//! Pitch content per beat: spectral peaks folded onto semitones (after
//! estimating the recording's tuning), split into a treble and a bass chroma.

use crate::stft::{Peak, Stft, peaks};
use crate::RATE;

pub const HOP: usize = 1024;
const SIZE: usize = 8192;
const LOW_HZ: f32 = 38.0;
const HIGH_HZ: f32 = 4200.0;
/// MIDI keys counted for the treble chroma and for the bass.
const TREBLE: (f32, f32) = (45.0, 86.0);
const BASS: (f32, f32) = (28.0, 54.0);

pub type Chroma = [f32; 12];

/// Pitch-class content of one beat.
#[derive(Clone, Copy, Default, Debug)]
pub struct BeatChroma {
    pub treble: Chroma,
    pub bass: Chroma,
    /// Sum of peak magnitudes, for telling silence from music.
    pub energy: f32,
}

fn midi_of(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Spectral peaks of every frame.
pub fn frame_peaks(signal: &[f32]) -> Vec<Vec<Peak>> {
    let stft = Stft::new(SIZE, HOP);
    let bin_hz = RATE as f32 / SIZE as f32;
    let mut frames = Vec::with_capacity(stft.frames(signal));
    let mut found = Vec::new();
    stft.run(signal, |_, mags| {
        let max = mags.iter().fold(0.0f32, |m, &v| m.max(v));
        peaks(mags, bin_hz, LOW_HZ, HIGH_HZ, max * 0.003, &mut found);
        // Keep the strongest peaks; the rest is mostly noise and leakage.
        found.sort_by(|a, b| b.mag.total_cmp(&a.mag));
        found.truncate(60);
        frames.push(found.clone());
    });
    frames
}

/// How far the recording sits from A = 440 Hz, in semitones (−0.5..0.5).
pub fn tuning(frames: &[Vec<Peak>]) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for p in frames.iter().flatten() {
        let phase = std::f64::consts::TAU * midi_of(p.hz) as f64;
        let w = p.mag.sqrt() as f64;
        re += w * phase.cos();
        im += w * phase.sin();
    }
    if re == 0.0 && im == 0.0 {
        return 0.0;
    }
    (im.atan2(re) / std::f64::consts::TAU) as f32
}

/// The share of a peak at fractional key `m` given to the key's own pitch class
/// within `range`, faded out over the last semitone at each end.
fn weight(m: f32, range: (f32, f32)) -> f32 {
    if m < range.0 - 1.0 || m > range.1 + 1.0 {
        return 0.0;
    }
    ((m - range.0 + 1.0).min(range.1 + 1.0 - m)).clamp(0.0, 1.0)
}

/// Averages the frames between consecutive beat times (seconds) into one chroma per beat.
pub fn per_beat(frames: &[Vec<Peak>], tuning: f32, beats: &[f32]) -> Vec<BeatChroma> {
    let fps = RATE as f32 / HOP as f32;
    let frame_chroma = |peaks: &[Peak]| {
        let mut c = BeatChroma::default();
        for p in peaks {
            let m = midi_of(p.hz) - tuning;
            let key = m.round();
            // A peak far from any semitone is likely noise or a drum.
            let off = (m - key).abs();
            let sharp = 1.0 - (off / 0.5).powi(2) * 0.8;
            let pc = (key as i32).rem_euclid(12) as usize;
            let mag = p.mag.sqrt() * sharp;
            c.treble[pc] += mag * weight(key, TREBLE);
            // Lower bass notes count for more: the bass is the lowest line.
            c.bass[pc] += mag * weight(key, BASS) * (1.0 + (BASS.1 - key).max(0.0) / 12.0);
            c.energy += p.mag;
        }
        c
    };
    let mut out = Vec::with_capacity(beats.len());
    for (i, &start) in beats.iter().enumerate() {
        let end = beats.get(i + 1).copied().unwrap_or(start + if i > 0 { start - beats[i - 1] } else { 0.5 });
        // Skip the first and last few frames of the beat, where the chord may still be changing.
        let span = end - start;
        let (a, b) = (((start + 0.1 * span) * fps).round() as usize, ((end - 0.05 * span) * fps).round() as usize);
        let mut sum = BeatChroma::default();
        let mut count = 0;
        for f in a..b.max(a + 1).min(frames.len()) {
            let c = frame_chroma(&frames[f]);
            for k in 0..12 {
                sum.treble[k] += c.treble[k];
                sum.bass[k] += c.bass[k];
            }
            sum.energy += c.energy;
            count += 1;
        }
        if count > 0 {
            let n = count as f32;
            sum.treble.iter_mut().chain(sum.bass.iter_mut()).for_each(|v| *v /= n);
            sum.energy /= n;
        }
        out.push(sum);
    }
    out
}

pub fn normalized(c: &Chroma) -> Chroma {
    let norm = c.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm < 1e-12 {
        return [0.0; 12];
    }
    c.map(|v| v / norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(keys: &[f32], cents: f32, seconds: f32) -> Vec<f32> {
        let n = (RATE as f32 * seconds) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                keys.iter()
                    .map(|&k| {
                        let hz = 440.0 * 2f32.powf((k + cents / 100.0 - 69.0) / 12.0);
                        (1..=4).map(|h| 0.6f32.powi(h - 1) * (std::f32::consts::TAU * hz * h as f32 * t).sin()).sum::<f32>()
                    })
                    .sum::<f32>()
                    * 0.1
            })
            .collect()
    }

    #[test]
    fn reads_the_tuning_and_the_pitch_classes() {
        // A minor (A2 bass, A3 C4 E4), tuned 30 cents sharp.
        let x = chord(&[45.0, 57.0, 60.0, 64.0], 30.0, 3.0);
        let frames = frame_peaks(&x);
        let t = tuning(&frames);
        assert!((t - 0.3).abs() < 0.05, "tuning {t}");
        let beats = per_beat(&frames, t, &[0.5, 1.0, 1.5, 2.0]);
        let c = normalized(&beats[1].treble);
        let mut order: Vec<usize> = (0..12).collect();
        order.sort_by(|&a, &b| c[b].total_cmp(&c[a]));
        let mut top: Vec<usize> = order[..3].to_vec();
        top.sort();
        assert_eq!(top, vec![0, 4, 9]);
        let bass = beats[1].bass;
        let low = (0..12).max_by(|&a, &b| bass[a].total_cmp(&bass[b])).unwrap();
        assert_eq!(low, 9);
    }
}
