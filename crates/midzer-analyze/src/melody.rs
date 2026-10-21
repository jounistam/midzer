//! A rough melody from a full mix: the most salient pitch per frame by
//! harmonic summation, tracked with a Viterbi pass that favours small steps,
//! then cut into notes.

use crate::stft::{Peak, Stft, peaks};
use crate::RATE;

const SIZE: usize = 2048;
const HOP: usize = 256;
/// Candidate pitches, in quarter semitones from `LOW` to `HIGH` (MIDI keys).
const LOW: f32 = 55.0;
const HIGH: f32 = 84.0;
const STEPS_PER_SEMITONE: f32 = 4.0;
const HARMONICS: usize = 8;
/// Viterbi costs: per quarter semitone moved between frames, and for starting or ending a note.
const STEP_COST: f32 = 0.12;
const VOICING_COST: f32 = 1.2;
const UNVOICED: f32 = 0.35;
const MIN_NOTE_SECONDS: f32 = 0.1;

/// A note in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedNote {
    pub start: f32,
    pub end: f32,
    pub key: u8,
    /// 0..1.
    pub level: f32,
}

pub fn track(signal: &[f32], tuning: f32) -> Vec<TimedNote> {
    let stft = Stft::new(SIZE, HOP);
    let bin_hz = RATE as f32 / SIZE as f32;
    let count = ((HIGH - LOW) * STEPS_PER_SEMITONE) as usize + 1;
    let candidate_hz: Vec<f32> = (0..count)
        .map(|c| 440.0 * 2f32.powf((LOW + tuning + c as f32 / STEPS_PER_SEMITONE - 69.0) / 12.0))
        .collect();
    let frames = stft.frames(signal);
    let mut salience = vec![0.0f32; frames * count];
    let mut found: Vec<Peak> = Vec::new();
    stft.run(signal, |t, mags| {
        let max = mags.iter().fold(0.0f32, |m, &v| m.max(v));
        peaks(mags, bin_hz, 150.0, 5000.0, max * 0.01, &mut found);
        found.sort_by(|a, b| b.mag.total_cmp(&a.mag));
        found.truncate(40);
        let row = &mut salience[t * count..(t + 1) * count];
        for p in &found {
            let amp = p.mag.sqrt();
            for h in 1..=HARMONICS {
                // Candidates this peak could be harmonic `h` of, within half a semitone.
                let f0 = p.hz / h as f32;
                let pos = (12.0 * (f0 / candidate_hz[0]).log2()) * STEPS_PER_SEMITONE;
                let lo = (pos - 2.0).ceil().max(0.0) as usize;
                let hi = ((pos + 2.0).floor() as isize).min(count as isize - 1);
                if hi < 0 {
                    continue;
                }
                for (c, slot) in row.iter_mut().enumerate().take(hi as usize + 1).skip(lo) {
                    let dev = (c as f32 - pos) / (STEPS_PER_SEMITONE / 2.0);
                    let w = (std::f32::consts::FRAC_PI_2 * dev).cos().powi(2);
                    *slot += amp * w * 0.8f32.powi(h as i32 - 1);
                }
            }
        }
    });

    // Normalise by a loud frame, so the unvoiced level means the same in quiet and loud songs.
    let mut maxima: Vec<f32> = (0..frames).map(|t| salience[t * count..(t + 1) * count].iter().fold(0.0f32, |m, &v| m.max(v))).collect();
    let frame_max = maxima.clone();
    maxima.sort_by(f32::total_cmp);
    let reference = maxima[(frames * 9 / 10).min(frames - 1)].max(1e-9);

    // Viterbi over candidates plus an unvoiced state (index `count`).
    let s = count + 1;
    let mut score = vec![0.0f32; s];
    let mut best = vec![0.0f32; count];
    let mut arg = vec![0u16; count];
    let mut back = vec![0u16; frames * s];
    let mut next = vec![0.0f32; s];
    let emission = |t: usize, c: usize| (salience[t * count + c] / reference).max(1e-3).ln();
    for t in 0..frames {
        // Best predecessor for each candidate with an L1 step cost, in two passes.
        for c in 0..count {
            best[c] = score[c];
            arg[c] = c as u16;
        }
        for c in 1..count {
            if best[c - 1] - STEP_COST > best[c] {
                best[c] = best[c - 1] - STEP_COST;
                arg[c] = arg[c - 1];
            }
        }
        for c in (0..count - 1).rev() {
            if best[c + 1] - STEP_COST > best[c] {
                best[c] = best[c + 1] - STEP_COST;
                arg[c] = arg[c + 1];
            }
        }
        let unvoiced = score[count];
        let (top, top_score) = score[..count].iter().enumerate().fold((0, f32::MIN), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
        let row = &mut back[t * s..(t + 1) * s];
        for c in 0..count {
            let (from, v) = if unvoiced - VOICING_COST > best[c] { (count, unvoiced - VOICING_COST) } else { (arg[c] as usize, best[c]) };
            next[c] = v + emission(t, c);
            row[c] = from as u16;
        }
        let (from, v) = if top_score - VOICING_COST > unvoiced { (top, top_score - VOICING_COST) } else { (count, unvoiced) };
        next[count] = v + UNVOICED.ln();
        row[count] = from as u16;
        std::mem::swap(&mut score, &mut next);
    }
    let mut state = score.iter().enumerate().fold((0, f32::MIN), |a, (i, &v)| if v > a.1 { (i, v) } else { a }).0;
    let mut path = vec![count; frames];
    for t in (0..frames).rev() {
        path[t] = state;
        state = back[t * s + state] as usize;
    }

    // Cut the path into notes: a note continues while it stays within half a semitone of where it started.
    let seconds = |t: usize| (t * HOP) as f32 / RATE as f32;
    let key_of = |c: usize| LOW + c as f32 / STEPS_PER_SEMITONE;
    let mut notes = Vec::new();
    let mut t = 0;
    while t < frames {
        if path[t] == count {
            t += 1;
            continue;
        }
        let start = t;
        let mut keys = vec![key_of(path[t])];
        t += 1;
        while t < frames && path[t] != count {
            let median = {
                let mut k = keys.clone();
                k.sort_by(f32::total_cmp);
                k[k.len() / 2]
            };
            if (key_of(path[t]) - median).abs() > 0.6 {
                break;
            }
            keys.push(key_of(path[t]));
            t += 1;
        }
        if seconds(t) - seconds(start) < MIN_NOTE_SECONDS {
            continue;
        }
        keys.sort_by(f32::total_cmp);
        let key = keys[keys.len() / 2].round().clamp(0.0, 127.0) as u8;
        let level = (start..t).map(|i| frame_max[i] / reference).sum::<f32>() / (t - start) as f32;
        notes.push(TimedNote { start: seconds(start), end: seconds(t), key, level: level.min(1.0) });
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_a_sung_line_over_a_quiet_chord() {
        let keys = [64.0, 62.0, 60.0, 62.0, 64.0, 64.0, 67.0];
        let note_s = 0.4;
        let n = (RATE as f32 * note_s * keys.len() as f32) as usize;
        let signal: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let k = keys[((t / note_s) as usize).min(keys.len() - 1)];
                let hz = 440.0 * 2f32.powf((k - 69.0) / 12.0);
                let in_note = (t % note_s) / note_s;
                let env = if in_note < 0.8 { 1.0 } else { 0.0 };
                let voice: f32 = (1..=6).map(|h| 0.7f32.powi(h - 1) * (std::f32::consts::TAU * hz * h as f32 * t).sin()).sum();
                // A quiet C major chord under it, an octave lower.
                let pad: f32 = [48.0f32, 52.0, 55.0].iter().map(|&k| (std::f32::consts::TAU * 440.0 * 2f32.powf((k - 69.0) / 12.0) * t).sin()).sum();
                0.2 * env * voice + 0.03 * pad
            })
            .collect();
        let notes = track(&signal, 0.0);
        let found: Vec<u8> = notes.iter().map(|n| n.key).collect();
        assert_eq!(found, vec![64, 62, 60, 62, 64, 64, 67]);
        assert!((notes[2].start - 0.8).abs() < 0.05, "{:?}", notes[2]);
    }
}
