//! Hearing a whole song at once, for imported recordings: the same networks
//! as [`crate::Hearing`], but each frame sees the song around it on both
//! sides, as in the networks' own full-song inference.
//!
//! - Beat This! runs over the log-mel spectrogram in chunks of 1500 frames
//!   (30 s), each with 6 frames of border thrown away, and where chunks
//!   overlap the earlier one is kept (`split_predict_aggregate(…, 1500, 6,
//!   "keep_first")`). The peaks are its minimal postprocessor's, refined to a
//!   fraction of a frame; each downbeat moves to the nearest beat. No grid is
//!   fitted, so a song that drifts in tempo keeps its drift, but the beats
//!   keep to one level: the network sometimes hears a stretch at double or
//!   half the song's tempo, and those stretches are thinned out or filled in.
//! - BTC runs over the constant-Q spectrum in windows of 108 frames that
//!   overlap by half; each frame takes the label from the window it is most
//!   central in. Flickers of one frame are smoothed away.
//!
//! Times are in seconds from the start of the signal (22.05 kHz).

use midzer_core::Chord;

use crate::beat_this::{BeatThis, FPS, MELS};
use crate::btc::{BINS, Btc, CLASSES, FRAMES as CHORD_FRAMES, chord_of};
use crate::cqt::{Cqt, HOP as CQT_HOP};
use crate::hearing::{RATE, peaks, smooth};
use crate::mel::Mel;

/// Beat This!'s chunks and the border cut from each.
const CHUNK: usize = 1500;
const BORDER: usize = 6;
/// BTC's windows start this many frames apart.
const CHORD_STEP: usize = CHORD_FRAMES / 2;

/// Beats closer than this (of the beat's length) to the last one kept are
/// dropped; gaps of up to `MAX_FILL` beats are filled in. The beat's length
/// follows the beats kept, a little each beat.
const LEVEL_SKIP: f64 = 0.75;
const MAX_FILL: f64 = 8.0;
const LEVEL_FOLLOW: f64 = 0.1;

/// A chord from one time to another (seconds); `None` is no chord.
pub type TimedChord = (f64, f64, Option<Chord>);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SongHeard {
    pub beats: Vec<f64>,
    /// Each one of `beats`.
    pub downbeats: Vec<f64>,
    pub chords: Vec<TimedChord>,
}

/// Beats, downbeats and chords of `mix`; `progress` gets the share done (0..1).
pub fn hear_song(mix: &[f32], progress: &mut dyn FnMut(f32)) -> Result<SongHeard, String> {
    let (beats, downbeats) = hear_beats(mix, &mut |f| progress(0.5 * f))?;
    let chords = hear_chords(mix, &mut |f| progress(0.5 + 0.5 * f))?;
    Ok(SongHeard { beats, downbeats, chords })
}

/// Beats and downbeats (seconds) of `signal`.
pub fn hear_beats(signal: &[f32], progress: &mut dyn FnMut(f32)) -> Result<(Vec<f64>, Vec<f64>), String> {
    if signal.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut model = BeatThis::load(crate::BEAT_THIS)?;
    let mels = Mel::new(model.mel_filters.clone()).whole(signal);
    let (beat, downbeat) = beat_logits(&mut model, &mels, progress);
    let at = |j: f64| j / FPS as f64;
    let beats = one_level(&peaks(&beat).into_iter().map(|p| at(refine(&beat, p.0))).collect::<Vec<_>>());
    let mut downbeats: Vec<f64> = Vec::new();
    for (j, _) in peaks(&downbeat) {
        let t = at(j);
        let Some(&nearest) = beats.iter().min_by(|a, b| (*a - t).abs().total_cmp(&(*b - t).abs())) else { break };
        if downbeats.last() != Some(&nearest) {
            downbeats.push(nearest);
        }
    }
    Ok((beats, downbeats))
}

/// `beats` at the song's own tempo throughout (the most common beat length,
/// drifting with the song): beats in a stretch heard twice as fast are
/// thinned out, and in one heard half as fast filled in, on the grid of
/// the beats before.
fn one_level(beats: &[f64]) -> Vec<f64> {
    if beats.len() < 4 {
        return beats.to_vec();
    }
    let mut intervals: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_by(f64::total_cmp);
    let mut period = intervals[intervals.len() / 2];
    let mut out = vec![beats[0]];
    for &b in &beats[1..] {
        let last = out[out.len() - 1];
        let gap = b - last;
        if gap < LEVEL_SKIP * period {
            continue;
        }
        let n = (gap / period).round().max(1.0);
        if n <= MAX_FILL {
            for k in 1..n as usize {
                out.push(last + k as f64 * gap / n);
            }
            let step = gap / n;
            if (step / period - 1.0).abs() < 0.25 {
                period += LEVEL_FOLLOW * (step - period);
            }
        }
        out.push(b);
    }
    out
}

/// Beat and downbeat logits for every mel frame, chunk by chunk.
fn beat_logits(model: &mut BeatThis, mels: &[f32], progress: &mut dyn FnMut(f32)) -> (Vec<f32>, Vec<f32>) {
    let n = mels.len() / MELS;
    let stride = CHUNK - 2 * BORDER;
    // Chunks start a border before the frames they keep; the last ends a border past the end.
    let mut starts: Vec<isize> = (0..).map(|i| i as isize * stride as isize - BORDER as isize).take_while(|&s| s < n as isize - BORDER as isize).collect();
    if n > stride
        && let Some(last) = starts.last_mut()
    {
        *last = n as isize - (CHUNK - BORDER) as isize;
    }
    let (mut beat, mut downbeat) = (vec![-1000.0f32; n], vec![-1000.0f32; n]);
    let mut filled = vec![false; n];
    let mut chunk = Vec::with_capacity(CHUNK * MELS);
    for (i, &start) in starts.iter().enumerate() {
        // Silence (zeros in log-mel) past either end.
        let (from, to) = (start.max(0) as usize, ((start + CHUNK as isize) as usize).min(n));
        let left = (-start).max(0) as usize;
        let right = (start + CHUNK as isize - n as isize).clamp(0, BORDER as isize) as usize;
        chunk.clear();
        chunk.resize(left * MELS, 0.0);
        chunk.extend_from_slice(&mels[from * MELS..to * MELS]);
        chunk.resize(chunk.len() + right * MELS, 0.0);
        let frames = chunk.len() / MELS;
        let logits = model.run(&chunk, frames);
        for k in BORDER..frames.saturating_sub(BORDER) {
            let j = start + k as isize;
            if j < 0 || j >= n as isize || filled[j as usize] {
                continue;
            }
            let j = j as usize;
            filled[j] = true;
            beat[j] = logits.beat[k];
            downbeat[j] = logits.downbeat[k];
        }
        progress((i + 1) as f32 / starts.len() as f32);
    }
    (beat, downbeat)
}

/// A peak's frame to a fraction of a frame: the top of a parabola through
/// the logits around it. Peaks already between two frames stay.
pub(crate) fn refine(logits: &[f32], at: f64) -> f64 {
    let j = at as usize;
    if at.fract() != 0.0 || j == 0 || j + 1 >= logits.len() {
        return at;
    }
    let (a, b, c) = (logits[j - 1] as f64, logits[j] as f64, logits[j + 1] as f64);
    let curve = a - 2.0 * b + c;
    if curve >= 0.0 {
        return at;
    }
    at + (0.5 * (a - c) / curve).clamp(-0.5, 0.5)
}

/// The chords of `signal`, oldest first, merged into spans.
pub fn hear_chords(signal: &[f32], progress: &mut dyn FnMut(f32)) -> Result<Vec<TimedChord>, String> {
    if signal.is_empty() {
        return Ok(Vec::new());
    }
    let mut model = Btc::load(crate::BTC)?;
    let cqt = Cqt::new().whole(signal);
    let n = cqt.len() / BINS;
    let mut starts: Vec<usize> = (0..).map(|i| i * CHORD_STEP).take_while(|&s| s + CHORD_FRAMES < n).collect();
    starts.push(n.saturating_sub(CHORD_FRAMES));
    starts.dedup();
    let mut classes = vec![0usize; n];
    for (i, &start) in starts.iter().enumerate() {
        let len = CHORD_FRAMES.min(n - start);
        let logits = model.run(&cqt[start * BINS..(start + len) * BINS], len);
        // A window keeps its middle half; the first and last keep their ends too.
        let keep_from = if i == 0 { 0 } else { (CHORD_FRAMES - CHORD_STEP) / 2 };
        let keep_to = if i + 1 == starts.len() { len } else { (CHORD_FRAMES + CHORD_STEP) / 2 };
        for (k, row) in logits.as_chunks::<CLASSES>().0.iter().enumerate().take(keep_to).skip(keep_from) {
            classes[start + k] = (0..CLASSES).max_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap();
        }
        progress((i + 1) as f32 / starts.len() as f32);
    }
    smooth(&mut classes);
    let hop = CQT_HOP as f64 / RATE;
    let end = signal.len() as f64 / RATE;
    let mut out: Vec<TimedChord> = Vec::new();
    for (t, &c) in classes.iter().enumerate() {
        // Frame t is centred on sample t · 2048.
        let (a, b) = (((t as f64 - 0.5) * hop).max(0.0), ((t as f64 + 0.5) * hop).min(end));
        let chord = chord_of(c);
        match out.last_mut() {
            Some(last) if last.2 == chord => last.1 = b,
            _ => out.push((a, b, chord)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_moves_toward_its_higher_neighbour() {
        let logits = [0.0, 1.0, 3.0, 2.0, 0.0];
        let t = refine(&logits, 2.0);
        assert!(t > 2.0 && t < 2.5, "{t}");
        assert_eq!(refine(&logits, 2.5), 2.5);
        assert_eq!(refine(&[1.0, 2.0, 1.0], 1.0), 1.0);
    }

    /// A 120 BPM groove (kick on 1 and 3, snare on 2 and 4, hi-hats on the
    /// eighths) over C for two bars, then G for two.
    fn song(seconds: f64) -> Vec<f32> {
        let len = (seconds * RATE) as usize;
        let mut seed = 5u32;
        (0..len)
            .map(|i| {
                let t = i as f64 / RATE;
                let bar = (t / 2.0) as usize;
                let roots: &[f64] = if bar % 4 < 2 { &[130.8, 164.8, 196.0] } else { &[98.0, 123.5, 146.8] };
                let tones: f64 = roots.iter().flat_map(|&hz| (1..=3).map(move |h| 0.04 / h as f64 * (std::f64::consts::TAU * hz * h as f64 * t).sin())).sum();
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f64 / (1u32 << 23) as f64 * 2.0 - 1.0;
                let (beat, into) = ((t / 0.5) as usize, t % 0.5);
                let drum = if beat % 2 == 0 {
                    (-into / 0.06).exp() * (std::f64::consts::TAU * (50.0 * into + 3.0 * (1.0 - (-into / 0.03).exp()))).sin() * 0.8
                } else {
                    noise * (-into / 0.05).exp() * 0.5
                };
                let hat = noise * (-(t % 0.25) / 0.008).exp() * 0.15;
                (tones + drum + hat) as f32
            })
            .collect()
    }

    #[test]
    fn beats_keep_to_one_level_through_doubled_and_halved_stretches() {
        // 0.5 s beats; 20–30 s heard twice as fast, 40–50 s half as fast, and the tempo drifts 10 % by the end.
        let mut beats = Vec::new();
        let mut t = 0.0f64;
        while t < 70.0 {
            let p = 0.5 * (1.0 - 0.1 * t / 70.0);
            if (20.0..30.0).contains(&t) {
                beats.push(t + 0.5 * p);
            }
            if !(40.0..50.0).contains(&t) || ((t - 40.0) / 0.5).round() as i64 % 2 == 0 {
                beats.push(t);
            }
            t += p;
        }
        beats.sort_by(f64::total_cmp);
        let level = one_level(&beats);
        let gaps: Vec<f64> = level.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|&g| (0.4..0.55).contains(&g)), "{gaps:?}");
        // The beats kept are the song's own, not the ones in between.
        assert!(level.iter().filter(|t| (20.0..30.0).contains(*t)).all(|t| beats.contains(t)));
    }

    #[test]
    fn hears_a_long_song_s_beats_and_chords_across_chunks() {
        // Over 30 s, so Beat This! runs in more than one chunk and BTC in many windows.
        let mix = song(40.0);
        let mut last = 0.0;
        let heard = hear_song(&mix, &mut |f| {
            assert!(f >= last && f <= 1.0);
            last = f;
        })
        .unwrap();
        assert_eq!(last, 1.0);
        let inner: Vec<f64> = heard.beats.iter().copied().filter(|&t| t > 1.0 && t < 39.0).collect();
        assert!(inner.len() >= 72, "{} beats", inner.len());
        let off = inner.iter().map(|t| (t / 0.5 - (t / 0.5).round()).abs() * 0.5).fold(0.0, f64::max);
        assert!(off < 0.03, "a beat {off} s off the clicks");
        let ones = heard.downbeats.iter().filter(|&&t| (t / 2.0 - (t / 2.0).round()).abs() < 0.05).count();
        assert!(ones * 4 >= heard.downbeats.len() * 3, "{:?}", heard.downbeats);
        let roots: Vec<u8> = heard.chords.iter().filter(|c| c.1 - c.0 > 1.0).filter_map(|c| c.2.map(|c| c.root)).collect();
        assert!(roots.contains(&0) && roots.contains(&7), "{:?}", heard.chords);
    }
}
