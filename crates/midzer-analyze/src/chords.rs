//! Chords per beat: chroma matched against harmonic chord templates, decoded
//! with a hidden Markov model so chords hold until the music clearly changes,
//! preferring changes on the bar line once the bars are known.

use midzer_core::{Chord, Quality};

use crate::chroma::{BeatChroma, Chroma, normalized};

/// Chord qualities worth telling apart in a recording; rarer ones read as noise.
const QUALITIES: [(Quality, f32); 5] = [
    (Quality::Major, 0.0),
    (Quality::Minor, 0.0),
    (Quality::Dominant7, 0.05),
    (Quality::Minor7, 0.05),
    (Quality::Major7, 0.07),
];
/// Log-likelihood per unit of template similarity.
const SHARPNESS: f32 = 18.0;
const BASS_WEIGHT: f32 = 0.25;
/// Bonus for chords that belong to the key.
const DIATONIC: f32 = 0.04;
/// What "no chord" scores, in template-similarity units, on a beat with music.
const NO_CHORD: f32 = 0.45;
/// Beats quieter than this share of the median beat count as silence.
const SILENT: f32 = 0.08;
/// Cost of changing chord between two beats.
const CHANGE: f32 = 2.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub tonic: u8,
    pub minor: bool,
}

/// Krumhansl–Kessler key profiles.
const MAJOR_PROFILE: [f32; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE: [f32; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/// The key whose profile correlates best with the song's chroma.
pub fn key(beats: &[BeatChroma]) -> Key {
    let mut total = [0.0f32; 12];
    for b in beats {
        let t = normalized(&b.treble);
        let bass = normalized(&b.bass);
        for k in 0..12 {
            total[k] += t[k] + 0.5 * bass[k];
        }
    }
    let corr = |profile: &[f32; 12], tonic: usize| {
        let mean_p = profile.iter().sum::<f32>() / 12.0;
        let mean_t = total.iter().sum::<f32>() / 12.0;
        let (mut num, mut dp, mut dt) = (0.0, 0.0, 0.0);
        for k in 0..12 {
            let p = profile[(k + 12 - tonic) % 12] - mean_p;
            let t = total[k] - mean_t;
            num += p * t;
            dp += p * p;
            dt += t * t;
        }
        num / (dp * dt).sqrt().max(1e-9)
    };
    let mut best = (Key { tonic: 0, minor: false }, f32::MIN);
    for tonic in 0..12 {
        for (minor, profile) in [(false, &MAJOR_PROFILE), (true, &MINOR_PROFILE)] {
            let c = corr(profile, tonic);
            if c > best.1 {
                best = (Key { tonic: tonic as u8, minor }, c);
            }
        }
    }
    best.0
}

/// Whether `chord` is built on the key's scale (a minor key's relative major's, plus its V and V7).
fn diatonic(chord: Chord, key: Key) -> bool {
    use Quality::*;
    let major_tonic = if key.minor { (key.tonic + 3) % 12 } else { key.tonic };
    let q = chord.quality;
    let on_scale = match (chord.root + 12 - major_tonic) % 12 {
        0 | 5 => matches!(q, Major | Major7),
        7 => matches!(q, Major | Dominant7),
        2 | 4 | 9 => matches!(q, Minor | Minor7),
        _ => false,
    };
    on_scale || key.minor && (chord.root + 12 - key.tonic) % 12 == 7 && matches!(q, Major | Dominant7)
}

/// Templates with the first harmonics of each tone, so a tone's fifth partial is expected.
fn template(chord: Chord) -> Chroma {
    let mut t = [0.0f32; 12];
    for &i in chord.quality.intervals() {
        let pc = (chord.root + i) % 12;
        for (h, w) in [(0u8, 1.0f32), (0, 0.6), (7, 0.36), (0, 0.22)] {
            t[((pc + h) % 12) as usize] += w;
        }
    }
    normalized(&t)
}

/// All chord states; the last state is "no chord".
fn states() -> Vec<Chord> {
    let mut out = Vec::with_capacity(12 * QUALITIES.len());
    for (q, _) in QUALITIES {
        for root in 0..12 {
            out.push(Chord::new(root, q));
        }
    }
    out
}

/// Where each beat falls in the bar, when known: changes cost less on the downbeat.
#[derive(Clone, Copy)]
pub enum Meter {
    Unknown,
    /// Beat `i` is the downbeat when `i % 4 == phase`.
    Bars { phase: usize },
}

fn change_cost(beat: usize, meter: Meter) -> f32 {
    match meter {
        Meter::Unknown => CHANGE,
        Meter::Bars { phase } => match (beat + 4 - phase % 4) % 4 {
            0 => CHANGE * 0.45,
            2 => CHANGE * 0.9,
            _ => CHANGE * 1.4,
        },
    }
}

/// The chord on each beat; `None` is no chord (silence or only drums).
pub fn decode(beats: &[BeatChroma], key: Key, meter: Meter) -> Vec<Option<Chord>> {
    let n = beats.len();
    if n == 0 {
        return Vec::new();
    }
    let chords = states();
    let templates: Vec<Chroma> = chords.iter().map(|&c| template(c)).collect();
    let penalty: Vec<f32> = chords
        .iter()
        .map(|c| {
            let rarity = QUALITIES.iter().find(|(q, _)| *q == c.quality).map_or(0.1, |(_, p)| *p);
            rarity - if diatonic(*c, key) { DIATONIC } else { 0.0 }
        })
        .collect();
    let mut energies: Vec<f32> = beats.iter().map(|b| b.energy).collect();
    energies.sort_by(f32::total_cmp);
    let median = energies[n / 2].max(1e-9);
    let s = chords.len() + 1;
    let none = chords.len();

    let emission = |b: &BeatChroma, out: &mut Vec<f32>| {
        out.clear();
        let treble = normalized(&b.treble);
        let bass_max = b.bass.iter().fold(0.0f32, |m, &v| m.max(v)).max(1e-12);
        let silent = b.energy < SILENT * median;
        for (c, t) in chords.iter().zip(&templates) {
            let sim: f32 = treble.iter().zip(t).map(|(a, b)| a * b).sum();
            let bass = b.bass[c.root as usize] / bass_max;
            out.push(SHARPNESS * (sim + BASS_WEIGHT * bass - penalty[out.len()]));
        }
        out.push(SHARPNESS * if silent { 2.0 } else { NO_CHORD });
    };

    let mut score = vec![0.0f32; s];
    let mut next = vec![0.0f32; s];
    let mut back = vec![0u16; n * s];
    let mut e = Vec::with_capacity(s);
    emission(&beats[0], &mut e);
    score.copy_from_slice(&e);
    for t in 1..n {
        emission(&beats[t], &mut e);
        let cost = change_cost(t, meter);
        let (best_prev, best_score) = score.iter().enumerate().fold((0, f32::MIN), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
        for j in 0..s {
            let (from, v) = if score[j] >= best_score - cost { (j, score[j]) } else { (best_prev, best_score - cost) };
            next[j] = v + e[j];
            back[t * s + j] = from as u16;
        }
        std::mem::swap(&mut score, &mut next);
    }
    let mut state = score.iter().enumerate().fold((0, f32::MIN), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc }).0;
    let mut path = vec![0usize; n];
    for t in (0..n).rev() {
        path[t] = state;
        if t > 0 {
            state = back[t * s + state] as usize;
        }
    }
    path.into_iter().map(|i| (i != none).then(|| chords[i])).collect()
}

/// Holds the chord before through runs of "no chord" shorter than `beats` between
/// two chords: inside the music those are quiet moments, not breaks for the band.
pub fn hold_short_gaps(chords: &mut [Option<Chord>], beats: usize) {
    let mut i = 0;
    while i < chords.len() {
        if chords[i].is_some() {
            i += 1;
            continue;
        }
        let end = (i..chords.len()).find(|&j| chords[j].is_some()).unwrap_or(chords.len());
        if i > 0 && end < chords.len() && end - i < beats {
            let held = chords[i - 1];
            chords[i..end].iter_mut().for_each(|c| *c = held);
        }
        i = end;
    }
}

/// Turns a chord into an inversion where the bass clearly plays another chord tone.
pub fn with_bass(chords: &mut [Option<Chord>], beats: &[BeatChroma]) {
    let mut start = 0;
    while start < chords.len() {
        let mut end = start + 1;
        while end < chords.len() && chords[end] == chords[start] {
            end += 1;
        }
        if let Some(chord) = chords[start] {
            let mut bass = [0.0f32; 12];
            for b in &beats[start..end] {
                for k in 0..12 {
                    bass[k] += b.bass[k];
                }
            }
            let low = (0..12).max_by(|&a, &b| bass[a].total_cmp(&bass[b])).unwrap_or(0);
            let tones: Vec<u8> = chord.quality.intervals().iter().map(|i| (chord.root + i) % 12).collect();
            if low as u8 != chord.root && tones.contains(&(low as u8)) && bass[low] > 2.0 * bass[chord.root as usize] {
                let inverted = Chord { bass: low as u8, ..chord };
                chords[start..end].iter_mut().for_each(|c| *c = Some(inverted));
            }
        }
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(tones: &[u8], bass: u8) -> BeatChroma {
        let mut b = BeatChroma { energy: 1.0, ..Default::default() };
        for &t in tones {
            b.treble[t as usize] += 1.0;
            b.treble[((t + 7) % 12) as usize] += 0.3;
        }
        b.treble.iter_mut().for_each(|v| *v += 0.08);
        b.bass[bass as usize] = 1.0;
        b
    }

    #[test]
    fn reads_a_progression_and_holds_through_a_stray_beat() {
        let c = beat(&[0, 4, 7], 0);
        let am = beat(&[9, 0, 4], 9);
        let f = beat(&[5, 9, 0], 5);
        let g7 = beat(&[7, 11, 2, 5], 7);
        // A passing E over the F.
        let stray = beat(&[5, 9, 0, 4], 5);
        let mut song = vec![c; 4];
        song.extend([am; 4]);
        song.extend([f, f, stray, f]);
        song.extend([g7; 4]);
        let k = key(&song);
        assert_eq!(k, Key { tonic: 0, minor: false });
        let chords = decode(&song, k, Meter::Bars { phase: 0 });
        let names: Vec<String> = chords.iter().step_by(4).map(|c| c.map_or("N".into(), |c| c.to_string())).collect();
        assert_eq!(names, ["C", "Am", "F", "G7"]);
        assert!(chords[8..12].iter().all(|c| c.unwrap().to_string() == "F"));
    }

    #[test]
    fn silence_is_no_chord() {
        let mut song = vec![beat(&[0, 4, 7], 0); 4];
        song.extend(vec![BeatChroma { energy: 0.001, ..Default::default() }; 4]);
        let chords = decode(&song, Key { tonic: 0, minor: false }, Meter::Unknown);
        assert!(chords[..4].iter().all(Option::is_some));
        assert!(chords[5..].iter().all(Option::is_none));
    }

    #[test]
    fn short_gaps_hold_the_chord_but_breaks_stay() {
        let c = Some(Chord::new(0, Quality::Major));
        let g = Some(Chord::new(7, Quality::Major));
        let mut chords = vec![None, c, None, None, g];
        chords.extend(vec![None; 8]);
        chords.push(c);
        chords.push(None);
        hold_short_gaps(&mut chords, 8);
        assert_eq!(&chords[..5], &[None, c, c, c, g]);
        assert!(chords[5..13].iter().all(Option::is_none));
        assert_eq!(chords[14], None);
    }

    #[test]
    fn a_strong_third_in_the_bass_makes_an_inversion() {
        let mut chords = vec![Some(Chord::new(0, Quality::Major)); 4];
        let beats = vec![beat(&[0, 4, 7], 4); 4];
        with_bass(&mut chords, &beats);
        assert_eq!(chords[0].unwrap().to_string(), "C/E");
    }
}
