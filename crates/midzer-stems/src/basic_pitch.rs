//! Note transcription with basic-pitch: the model's note and onset
//! activations over 2-second windows, then its note-creation step ported
//! from `basic_pitch/note_creation.py` (onsets, inferred onsets and the
//! "melodia trick" for notes without a clear onset).

use std::collections::BinaryHeap;

use midzer_analyze::TimedNote;
use ort::session::Session;
use ort::value::Tensor;

pub const RATE: u32 = 22_050;
const FFT_HOP: usize = 256;
const WINDOW: usize = RATE as usize * 2 - FFT_HOP; // 43 844
const FRAMES_PER_WINDOW: usize = 172;
const OVERLAP_FRAMES: usize = 30;
const OVERLAP: usize = OVERLAP_FRAMES * FFT_HOP;
const HOP: usize = WINDOW - OVERLAP;
const KEYS: usize = 88;
const LOWEST_KEY: u8 = 21;
const ONSET_THRESHOLD: f32 = 0.5;
const FRAME_THRESHOLD: f32 = 0.3;
/// 127.7 ms in frames.
const MIN_NOTE_FRAMES: usize = 11;
const ENERGY_TOLERANCE: usize = 11;

/// The range of keys to keep, e.g. a bass's or a voice's.
#[derive(Clone, Copy)]
pub struct Range {
    pub low: u8,
    pub high: u8,
}

/// Note and onset activations per frame, and the time of each frame.
struct Activations {
    notes: Vec<[f32; KEYS]>,
    onsets: Vec<[f32; KEYS]>,
    times: Vec<f32>,
}

fn activations(session: &mut Session, audio: &[f32], progress: &mut dyn FnMut(f32) -> bool) -> Result<Activations, String> {
    let mut padded = vec![0.0f32; OVERLAP / 2];
    padded.extend_from_slice(audio);
    let windows = padded.len().div_ceil(HOP).max(1);
    let keep = FRAMES_PER_WINDOW - OVERLAP_FRAMES;
    let mut out = Activations { notes: Vec::new(), onsets: Vec::new(), times: Vec::new() };
    let in_name = session.inputs()[0].name().to_string();
    for w in 0..windows {
        let start = w * HOP;
        let mut input = vec![0.0f32; WINDOW];
        let end = (start + WINDOW).min(padded.len());
        if start < end {
            input[..end - start].copy_from_slice(&padded[start..end]);
        }
        let tensor = Tensor::from_array(([1usize, WINDOW, 1], input)).map_err(|e| e.to_string())?;
        let outputs = session.run(ort::inputs![in_name.as_str() => tensor]).map_err(|e| e.to_string())?;
        let (_, note) = outputs["StatefulPartitionedCall:1"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let (_, onset) = outputs["StatefulPartitionedCall:2"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        for k in OVERLAP_FRAMES / 2..FRAMES_PER_WINDOW - OVERLAP_FRAMES / 2 {
            let mut n = [0.0f32; KEYS];
            let mut o = [0.0f32; KEYS];
            n.copy_from_slice(&note[k * KEYS..(k + 1) * KEYS]);
            o.copy_from_slice(&onset[k * KEYS..(k + 1) * KEYS]);
            out.notes.push(n);
            out.onsets.push(o);
            out.times.push(((start + k * FFT_HOP) as f32 - (OVERLAP / 2) as f32) / RATE as f32);
        }
        if !progress((w + 1) as f32 / windows as f32) {
            return Err(crate::CANCELLED.into());
        }
    }
    // Trim the frames past the end of the audio.
    let frames = ((audio.len() as f64 / HOP as f64) * keep as f64) as usize;
    out.notes.truncate(frames);
    out.onsets.truncate(frames);
    out.times.truncate(frames);
    Ok(out)
}

pub fn transcribe(session: &mut Session, audio: &[f32], range: Range, progress: &mut dyn FnMut(f32) -> bool) -> Result<Vec<TimedNote>, String> {
    let act = activations(session, audio, progress)?;
    Ok(notes(act, range))
}

fn notes(act: Activations, range: Range) -> Vec<TimedNote> {
    let n = act.notes.len();
    if n < 3 {
        return Vec::new();
    }
    let lo = range.low.saturating_sub(LOWEST_KEY) as usize;
    let hi = ((range.high.saturating_sub(LOWEST_KEY)) as usize + 1).min(KEYS);
    let frames = act.notes;
    let mut onsets = act.onsets;
    for t in 0..n {
        for f in (0..lo).chain(hi..KEYS) {
            onsets[t][f] = 0.0;
        }
    }
    let in_range = |f: usize| f >= lo && f < hi;

    // Inferred onsets: big rises in the note activation, scaled to the onsets' maximum.
    let mut diff = vec![[0.0f32; KEYS]; n];
    let mut max_diff = 0.0f32;
    for t in 2..n {
        for f in lo..hi {
            let d = (frames[t][f] - frames[t - 1][f]).min(frames[t][f] - frames[t - 2][f]).max(0.0);
            diff[t][f] = d;
            max_diff = max_diff.max(d);
        }
    }
    let max_onset = onsets.iter().flat_map(|r| r.iter()).fold(0.0f32, |m, &v| m.max(v));
    if max_diff > 0.0 {
        for t in 0..n {
            for f in lo..hi {
                onsets[t][f] = onsets[t][f].max(max_onset * diff[t][f] / max_diff);
            }
        }
    }

    // Onset peaks over time, latest first.
    let mut peaks = Vec::new();
    for t in 1..n - 1 {
        for f in lo..hi {
            let v = onsets[t][f];
            if v >= ONSET_THRESHOLD && v > onsets[t - 1][f] && v > onsets[t + 1][f] {
                peaks.push((t, f));
            }
        }
    }
    peaks.sort_by(|a, b| b.cmp(a));

    let mut remaining: Vec<[f32; KEYS]> = frames.clone();
    let mut out = Vec::new();
    let mean = |a: usize, b: usize, f: usize| (a..b).map(|t| frames[t][f]).sum::<f32>() / (b - a).max(1) as f32;
    let clear = |remaining: &mut Vec<[f32; KEYS]>, t: usize, f: usize| {
        remaining[t][f] = 0.0;
        if f + 1 < KEYS {
            remaining[t][f + 1] = 0.0;
        }
        if f > 0 {
            remaining[t][f - 1] = 0.0;
        }
    };
    for (start, f) in peaks {
        if start >= n - 1 {
            continue;
        }
        let mut i = start + 1;
        let mut k = 0;
        while i < n - 1 && k < ENERGY_TOLERANCE {
            if remaining[i][f] < FRAME_THRESHOLD { k += 1 } else { k = 0 }
            i += 1;
        }
        i -= k;
        if i - start <= MIN_NOTE_FRAMES {
            continue;
        }
        for t in start..i {
            clear(&mut remaining, t, f);
        }
        out.push((start, i, f, mean(start, i, f)));
    }

    // The melodia trick: grow notes from the strongest leftover activations.
    let mut heap: BinaryHeap<(u32, usize, usize)> = BinaryHeap::new();
    for (t, row) in remaining.iter().enumerate() {
        for f in lo..hi {
            if row[f] > FRAME_THRESHOLD {
                heap.push((row[f].to_bits(), t, f));
            }
        }
    }
    while let Some((bits, mid, f)) = heap.pop() {
        if remaining[mid][f].to_bits() != bits || !in_range(f) {
            continue; // already used by another note
        }
        remaining[mid][f] = 0.0;
        let mut i = mid + 1;
        let mut k = 0;
        while i < n - 1 && k < ENERGY_TOLERANCE {
            if remaining[i][f] < FRAME_THRESHOLD { k += 1 } else { k = 0 }
            clear(&mut remaining, i, f);
            i += 1;
        }
        let end = i - 1 - k;
        let mut i = mid as isize - 1;
        let mut k = 0;
        while i > 0 && k < ENERGY_TOLERANCE {
            if remaining[i as usize][f] < FRAME_THRESHOLD { k += 1 } else { k = 0 }
            clear(&mut remaining, i as usize, f);
            i -= 1;
        }
        let start = (i + 1 + k as isize).max(0) as usize;
        if end <= start || end - start <= MIN_NOTE_FRAMES {
            continue;
        }
        out.push((start, end, f, mean(start, end, f)));
    }

    let mut notes: Vec<TimedNote> = out
        .into_iter()
        .map(|(a, b, f, amp)| TimedNote { start: act.times[a].max(0.0), end: act.times[b.min(n - 1)], key: LOWEST_KEY + f as u8, level: amp })
        .collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.key.cmp(&b.key)));
    notes
}

/// One note at a time: where notes overlap, the stronger one wins.
pub fn top_line(notes: &[TimedNote]) -> Vec<TimedNote> {
    let mut out: Vec<TimedNote> = Vec::new();
    for &n in notes {
        let weight = |x: &TimedNote| x.level * (x.end - x.start).min(1.0);
        match out.last_mut() {
            Some(prev) if n.start < prev.end - 0.03 => {
                if weight(&n) > weight(prev) {
                    prev.end = n.start;
                    if prev.end - prev.start < 0.08 {
                        out.pop();
                    }
                    out.push(n);
                }
            }
            _ => out.push(n),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(frames: usize, notes: &[(usize, usize, usize)]) -> Activations {
        let mut a = Activations { notes: vec![[0.0; KEYS]; frames], onsets: vec![[0.0; KEYS]; frames], times: (0..frames).map(|t| t as f32 * 0.01).collect() };
        for &(start, end, f) in notes {
            a.onsets[start][f] = 0.9;
            for t in start..end {
                a.notes[t][f] = 0.8;
            }
        }
        a
    }

    #[test]
    fn makes_notes_from_onsets_and_activations() {
        let found = notes(act(200, &[(10, 60, 39), (70, 120, 43)]), Range { low: 21, high: 108 });
        let keys: Vec<(u8, usize, usize)> = found.iter().map(|n| (n.key, (n.start * 100.0).round() as usize, (n.end * 100.0).round() as usize)).collect();
        assert_eq!(keys, vec![(60, 10, 60), (64, 70, 120)]);
    }

    #[test]
    fn keeps_to_the_range_and_drops_short_notes() {
        let found = notes(act(200, &[(10, 60, 10), (70, 75, 43), (80, 150, 43)]), Range { low: 40, high: 90 });
        assert_eq!(found.iter().map(|n| n.key).collect::<Vec<_>>(), vec![64]);
    }

    #[test]
    fn the_top_line_keeps_the_stronger_of_overlapping_notes() {
        let n = |start: f32, end: f32, key: u8, level: f32| TimedNote { start, end, key, level };
        let line = top_line(&[n(0.0, 1.0, 60, 0.8), n(0.5, 0.9, 72, 0.3), n(1.0, 2.0, 62, 0.7)]);
        assert_eq!(line.iter().map(|n| n.key).collect::<Vec<_>>(), vec![60, 62]);
    }
}
