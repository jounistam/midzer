//! Turns chords into concrete notes for the band parts.

use crate::chord::Chord;
use crate::style::Tone;

pub const MAX_VOICING_NOTES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Voicing {
    notes: [u8; MAX_VOICING_NOTES],
    len: usize,
}

impl Voicing {
    pub fn notes(&self) -> &[u8] {
        &self.notes[..self.len]
    }

    pub fn center(&self) -> Option<f32> {
        (self.len > 0).then(|| self.notes().iter().map(|&n| n as f32).sum::<f32>() / self.len as f32)
    }
}

/// Picks the close-position inversion of `chord` within `low..=high` whose
/// average pitch is nearest to `center`. Passing the previous voicing's center
/// gives smooth voice leading between chords.
pub fn voice_near(chord: &Chord, center: f32, low: u8, high: u8) -> Voicing {
    let mut pcs = [0u8; MAX_VOICING_NOTES];
    let mut count = 0;
    for pc in chord.pitch_classes().take(MAX_VOICING_NOTES) {
        pcs[count] = pc;
        count += 1;
    }
    pcs[..count].sort_unstable();

    let mut best = Voicing::default();
    let mut best_distance = f32::INFINITY;
    for inversion in 0..count {
        let first_pc = pcs[inversion];
        let mut start = low + (first_pc + 12 - low % 12) % 12;
        while start <= high {
            let mut v = Voicing { len: count, ..Voicing::default() };
            let mut prev = start;
            for i in 0..count {
                let pc = pcs[(inversion + i) % count];
                let note = if i == 0 { start } else { prev + (pc + 12 - prev % 12) % 12 };
                v.notes[i] = note;
                prev = note;
            }
            if v.notes[count - 1] <= high {
                let distance = (v.center().unwrap_or(center) - center).abs();
                if distance < best_distance {
                    best = v;
                    best_distance = distance;
                }
            }
            start += 12;
        }
    }
    best
}

/// The bass note for `tone`, with the root kept in the E1..D#2 range.
pub fn bass_note(chord: &Chord, tone: Tone) -> u8 {
    let root = 28 + (chord.root + 12 - 4) % 12;
    let intervals = chord.quality.intervals();
    let find = |candidates: &[u8], fallback: u8| {
        candidates.iter().copied().find(|c| intervals.contains(c)).unwrap_or(fallback)
    };
    root + match tone {
        Tone::Root => 0,
        Tone::Third => find(&[4, 3, 5, 2], 7),
        Tone::Fifth => find(&[7, 6, 8], 7),
        Tone::Octave => 12,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Quality;

    #[test]
    fn voicings_stay_in_range_and_contain_chord_tones() {
        for root in 0..12 {
            for quality in [Quality::Major, Quality::Minor7, Quality::Dominant7] {
                let chord = Chord::new(root, quality);
                let v = voice_near(&chord, 64.0, 55, 79);
                assert_eq!(v.notes().len(), quality.intervals().len());
                assert!(v.notes().iter().all(|n| (55..=79).contains(n)));
                assert!(v.notes().windows(2).all(|w| w[0] < w[1] && w[1] - w[0] < 12));
                let mut pcs: Vec<_> = v.notes().iter().map(|n| n % 12).collect();
                pcs.sort();
                let mut expected: Vec<_> = chord.pitch_classes().collect();
                expected.sort();
                assert_eq!(pcs, expected);
            }
        }
    }

    #[test]
    fn voice_leading_moves_little() {
        // C → F → G should stay near the same register instead of jumping.
        let mut center = 64.0;
        for root in [0, 5, 7, 0] {
            let v = voice_near(&Chord::new(root, Quality::Major), center, 55, 79);
            let next = v.center().unwrap();
            assert!((next - center).abs() <= 4.0, "jumped from {center} to {next}");
            center = next;
        }
    }

    #[test]
    fn bass_range() {
        for root in 0..12 {
            let n = bass_note(&Chord::new(root, Quality::Major), Tone::Root);
            assert!((28..40).contains(&n));
            assert_eq!(n % 12, root);
        }
        let am = Chord::new(9, Quality::Minor);
        assert_eq!(bass_note(&am, Tone::Third) - bass_note(&am, Tone::Root), 3);
        assert_eq!(bass_note(&am, Tone::Fifth) - bass_note(&am, Tone::Root), 7);
    }
}
