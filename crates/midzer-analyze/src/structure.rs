//! Bars and sections: where the downbeat falls, and which stretches of bars
//! repeat (verse, chorus) or stand out (intro, ending).

use midzer_core::Chord;

use crate::chroma::{BeatChroma, normalized};

/// Which of the four beat positions is the downbeat: chords change there and the bass hits there.
pub fn downbeat_phase(chords: &[Option<Chord>], low: &[f32], all: &[f32]) -> usize {
    let z = |x: &[f32]| {
        let mean = x.iter().sum::<f32>() / x.len().max(1) as f32;
        let sd = (x.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / x.len().max(1) as f32).sqrt().max(1e-9);
        x.iter().map(|v| (v - mean) / sd).collect::<Vec<f32>>()
    };
    let (low, all) = (z(low), z(all));
    let mut score = [0.0f32; 4];
    for i in 0..chords.len() {
        let change = i > 0 && chords[i] != chords[i - 1] && chords[i].is_some();
        score[i % 4] += if change { 1.0 } else { 0.0 } + 0.35 * low.get(i).copied().unwrap_or(0.0) + 0.15 * all.get(i).copied().unwrap_or(0.0);
    }
    (0..4).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Intro,
    A,
    B,
    C,
    Ending,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Intro => "intro",
            Part::A => "A",
            Part::B => "B",
            Part::C => "C",
            Part::Ending => "ending",
        }
    }
}

/// Segments shorter than this many bars are not sections of their own.
const MIN_BARS: usize = 4;
const KERNEL: usize = 4;
/// Segments at least this similar get the same label.
const SAME: f32 = 0.82;

/// Section starts (bar, part). `bars[b]` is the four beats of bar `b`; `loudness[b]` its RMS.
pub fn sections(bars: &[[BeatChroma; 4]], loudness: &[f32]) -> Vec<(usize, Part)> {
    let n = bars.len();
    if n < 2 * MIN_BARS {
        return vec![(0, Part::A)];
    }
    let features: Vec<[f32; 48]> = bars
        .iter()
        .map(|bar| {
            let mut f = [0.0f32; 48];
            for (j, beat) in bar.iter().enumerate() {
                let t = normalized(&beat.treble);
                let b = normalized(&beat.bass);
                for k in 0..12 {
                    f[j * 12 + k] = t[k] + 0.5 * b[k];
                }
            }
            let norm = f.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
            f.map(|v| v / norm)
        })
        .collect();
    let sim = |i: usize, j: usize| features[i].iter().zip(&features[j]).map(|(a, b)| a * b).sum::<f32>();
    let db: Vec<f32> = loudness.iter().map(|&r| 20.0 * r.max(1e-6).log10()).collect();

    // Novelty: a checkerboard kernel along the diagonal of the self-similarity, plus loudness jumps.
    let mut novelty = vec![0.0f32; n];
    for (b, slot) in novelty.iter_mut().enumerate().take(n - 2).skip(2) {
        let k = KERNEL.min(b).min(n - b);
        let (mut within, mut across) = (0.0, 0.0);
        let (mut wn, mut an) = (0, 0);
        for i in b - k..b + k {
            for j in b - k..b + k {
                if i == j {
                    continue;
                }
                if (i < b) == (j < b) {
                    within += sim(i, j);
                    wn += 1;
                } else {
                    across += sim(i, j);
                    an += 1;
                }
            }
        }
        let before = db[b - k..b].iter().sum::<f32>() / k as f32;
        let after = db[b..b + k].iter().sum::<f32>() / k as f32;
        *slot = (within / wn.max(1) as f32 - across / an.max(1) as f32) + 0.04 * (after - before).abs();
        // Sections tend to start every 4 or 8 bars.
        *slot *= if b % 8 == 0 { 1.5 } else if b % 4 == 0 { 1.35 } else if b % 2 == 0 { 1.0 } else { 0.6 };
    }
    let mean = novelty.iter().sum::<f32>() / n as f32;
    let sd = (novelty.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n as f32).sqrt();
    let threshold = mean + 0.6 * sd;
    let mut order: Vec<usize> = (MIN_BARS..n.saturating_sub(MIN_BARS / 2)).filter(|&b| novelty[b] > threshold).collect();
    order.sort_by(|&a, &b| novelty[b].total_cmp(&novelty[a]));
    let mut starts = vec![0usize];
    for b in order {
        if starts.iter().all(|&s| s.abs_diff(b) >= MIN_BARS) {
            starts.push(b);
        }
    }
    starts.sort();

    // Label segments: a segment like an earlier one gets its label.
    let segments: Vec<(usize, usize)> = starts.iter().enumerate().map(|(i, &s)| (s, starts.get(i + 1).copied().unwrap_or(n))).collect();
    let seg_sim = |a: (usize, usize), b: (usize, usize)| {
        let len = (a.1 - a.0).min(b.1 - b.0);
        (0..len).map(|k| sim(a.0 + k, b.0 + k)).sum::<f32>() / len.max(1) as f32
    };
    let mut labels: Vec<usize> = Vec::with_capacity(segments.len());
    let mut reps: Vec<usize> = Vec::new();
    for (i, &seg) in segments.iter().enumerate() {
        let found = reps.iter().enumerate().map(|(l, &r)| (l, seg_sim(segments[r], seg))).max_by(|a, b| a.1.total_cmp(&b.1));
        match found {
            Some((l, s)) if s >= SAME => labels.push(l),
            _ => {
                reps.push(i);
                labels.push(reps.len() - 1);
            }
        }
    }
    let seg_db = |s: (usize, usize)| db[s.0..s.1].iter().sum::<f32>() / (s.1 - s.0) as f32;
    let mut song_db: Vec<f32> = db.clone();
    song_db.sort_by(f32::total_cmp);
    let median_db = song_db[n / 2];

    // Intro and ending: a short, unrepeated or quiet first/last segment.
    let count = |l: usize| labels.iter().filter(|&&x| x == l).count();
    let mut parts: Vec<Option<Part>> = vec![None; segments.len()];
    let special = |i: usize| {
        let s = segments[i];
        segments.len() > 1 && s.1 - s.0 <= 8 && (count(labels[i]) == 1 || seg_db(s) < median_db - 12.0)
    };
    if special(0) {
        parts[0] = Some(Part::Intro);
    }
    let last = segments.len() - 1;
    if last > 0 && special(last) {
        parts[last] = Some(Part::Ending);
    }

    // The other labels become A, B and C by loudness: the band plays louder variations later in the alphabet.
    let mut label_db: Vec<(usize, f32)> = (0..reps.len())
        .filter(|&l| (0..segments.len()).any(|i| labels[i] == l && parts[i].is_none()))
        .map(|l| {
            let segs: Vec<f32> = (0..segments.len()).filter(|&i| labels[i] == l && parts[i].is_none()).map(|i| seg_db(segments[i])).collect();
            (l, segs.iter().sum::<f32>() / segs.len() as f32)
        })
        .collect();
    label_db.sort_by(|a, b| a.1.total_cmp(&b.1));
    let ranks = label_db.len();
    let part_of = |l: usize| {
        let rank = label_db.iter().position(|&(x, _)| x == l).unwrap_or(0);
        match ranks {
            0 | 1 => Part::A,
            2 => [Part::A, Part::B][rank],
            _ => [Part::A, Part::B, Part::C][rank * 3 / ranks],
        }
    };
    let mut out = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let part = parts[i].unwrap_or_else(|| part_of(labels[i]));
        if out.last().is_none_or(|&(_, p)| p != part) {
            out.push((seg.0, part));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use midzer_core::Quality;

    fn bar(tones: &[u8]) -> [BeatChroma; 4] {
        let mut b = BeatChroma { energy: 1.0, ..Default::default() };
        for &t in tones {
            b.treble[t as usize] = 1.0;
        }
        b.treble.iter_mut().for_each(|v| *v += 0.05);
        b.bass[tones[0] as usize] = 1.0;
        [b; 4]
    }

    #[test]
    fn the_downbeat_is_where_chords_change() {
        let c = Some(Chord::new(0, Quality::Major));
        let g = Some(Chord::new(7, Quality::Major));
        // A pickup beat, then bars of C and G.
        let mut chords = vec![None];
        for _ in 0..6 {
            chords.extend([c; 4]);
            chords.extend([g; 4]);
        }
        let flat = vec![0.0; chords.len()];
        assert_eq!(downbeat_phase(&chords, &flat, &flat), 1);
    }

    #[test]
    fn finds_verse_and_chorus_by_repetition_and_loudness() {
        let verse = [bar(&[0, 4, 7]), bar(&[9, 0, 4]), bar(&[5, 9, 0]), bar(&[7, 11, 2])];
        let chorus = [bar(&[5, 9, 0]), bar(&[7, 11, 2]), bar(&[0, 4, 7]), bar(&[0, 4, 7])];
        let mut bars = Vec::new();
        let mut loud = Vec::new();
        for (part, db) in [(&verse, 0.2), (&verse, 0.2), (&chorus, 0.5), (&chorus, 0.5), (&verse, 0.2), (&verse, 0.2), (&chorus, 0.5), (&chorus, 0.5)] {
            bars.extend_from_slice(part);
            loud.extend([db; 4]);
        }
        let found = sections(&bars, &loud);
        assert_eq!(found, vec![(0, Part::A), (8, Part::B), (16, Part::A), (24, Part::B)]);
    }
}
