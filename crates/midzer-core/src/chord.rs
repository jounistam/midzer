//! Chord recognition from a set of held MIDI notes.

use std::fmt;

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/// A set of pitch classes as a 12-bit mask (bit 0 = C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PitchSet(pub u16);

impl PitchSet {
    pub fn from_notes(notes: impl IntoIterator<Item = u8>) -> Self {
        Self(notes.into_iter().fold(0, |mask, n| mask | 1 << (n % 12)))
    }

    pub fn contains(self, pc: u8) -> bool {
        self.0 & (1 << pc) != 0
    }

    pub fn len(self) -> u32 {
        self.0.count_ones()
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Re-expresses the set relative to `root`, so `root` becomes bit 0.
    fn relative_to(self, root: u8) -> u16 {
        let r = root as u32 % 12;
        ((self.0 as u32 >> r | (self.0 as u32) << (12 - r)) & 0xfff) as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Power,
    Major6,
    Minor6,
    Dominant7,
    Major7,
    Minor7,
    HalfDiminished7,
    Diminished7,
    Add9,
}

impl Quality {
    /// Chord tones as intervals above the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Quality::Major => &[0, 4, 7],
            Quality::Minor => &[0, 3, 7],
            Quality::Diminished => &[0, 3, 6],
            Quality::Augmented => &[0, 4, 8],
            Quality::Sus2 => &[0, 2, 7],
            Quality::Sus4 => &[0, 5, 7],
            Quality::Power => &[0, 7],
            Quality::Major6 => &[0, 4, 7, 9],
            Quality::Minor6 => &[0, 3, 7, 9],
            Quality::Dominant7 => &[0, 4, 7, 10],
            Quality::Major7 => &[0, 4, 7, 11],
            Quality::Minor7 => &[0, 3, 7, 10],
            Quality::HalfDiminished7 => &[0, 3, 6, 10],
            Quality::Diminished7 => &[0, 3, 6, 9],
            Quality::Add9 => &[0, 2, 4, 7],
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Quality::Major => "",
            Quality::Minor => "m",
            Quality::Diminished => "dim",
            Quality::Augmented => "aug",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Power => "5",
            Quality::Major6 => "6",
            Quality::Minor6 => "m6",
            Quality::Dominant7 => "7",
            Quality::Major7 => "maj7",
            Quality::Minor7 => "m7",
            Quality::HalfDiminished7 => "m7b5",
            Quality::Diminished7 => "dim7",
            Quality::Add9 => "add9",
        }
    }

    /// Bias against rarer readings so ambiguous partial chords resolve to the
    /// everyday interpretation.
    fn rarity(self) -> i32 {
        match self {
            Quality::Major | Quality::Minor => 0,
            Quality::Augmented => 4,
            _ => 1,
        }
    }

    fn mask(self) -> u16 {
        self.intervals().iter().fold(0, |m, i| m | 1 << i)
    }

    pub const ALL: [Quality; 15] = [
        Quality::Major,
        Quality::Minor,
        Quality::Diminished,
        Quality::Augmented,
        Quality::Sus2,
        Quality::Sus4,
        Quality::Power,
        Quality::Major6,
        Quality::Minor6,
        Quality::Dominant7,
        Quality::Major7,
        Quality::Minor7,
        Quality::HalfDiminished7,
        Quality::Diminished7,
        Quality::Add9,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// Root pitch class, 0 = C.
    pub root: u8,
    pub quality: Quality,
    /// Lowest sounding pitch class; differs from `root` for inversions.
    pub bass: u8,
}

impl Chord {
    pub const fn new(root: u8, quality: Quality) -> Self {
        Self { root, quality, bass: root }
    }

    /// Pitch classes of the chord tones.
    pub fn pitch_classes(&self) -> impl Iterator<Item = u8> + '_ {
        self.quality.intervals().iter().map(|i| (self.root + i) % 12)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", NOTE_NAMES[self.root as usize], self.quality.suffix())?;
        if self.bass != self.root {
            write!(f, "/{}", NOTE_NAMES[self.bass as usize])?;
        }
        Ok(())
    }
}

/// Names the chord formed by `notes` (MIDI note numbers, any order).
///
/// Every present pitch class is tried as a root against every chord template;
/// the score rewards matched tones and the bass being the root, and penalises
/// extra and missing tones. Fewer than two pitch classes give `None`.
pub fn recognize(notes: &[u8]) -> Option<Chord> {
    let bass = *notes.iter().min()? % 12;
    let set = PitchSet::from_notes(notes.iter().copied());
    if set.len() < 2 {
        return None;
    }

    let mut best: Option<(i32, Chord)> = None;
    for root in (0..12).filter(|&pc| set.contains(pc)) {
        let rel = set.relative_to(root);
        for quality in Quality::ALL {
            let template = quality.mask();
            let matched = (rel & template).count_ones() as i32;
            let extra = (rel & !template).count_ones() as i32;
            let missing = (template & !rel).count_ones() as i32;
            let score = 10 * matched - 12 * extra - 4 * missing - quality.rarity()
                + if root == bass { 3 } else { 0 };
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, Chord { root, quality, bass }));
            }
        }
    }
    best.map(|(_, chord)| chord)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(notes: &[u8]) -> String {
        recognize(notes).map(|c| c.to_string()).unwrap_or_default()
    }

    #[test]
    fn triads() {
        assert_eq!(name(&[60, 64, 67]), "C");
        assert_eq!(name(&[57, 60, 64]), "Am");
        assert_eq!(name(&[59, 62, 65]), "Bdim");
        assert_eq!(name(&[60, 64, 68]), "Caug");
        assert_eq!(name(&[60, 65, 67]), "Csus4");
        assert_eq!(name(&[60, 62, 67]), "Csus2");
    }

    #[test]
    fn inversions_and_octave_doubling() {
        assert_eq!(name(&[64, 67, 72]), "C/E");
        assert_eq!(name(&[55, 60, 64]), "C/G");
        assert_eq!(name(&[48, 60, 64, 67, 72]), "C");
    }

    #[test]
    fn sevenths_and_sixths() {
        assert_eq!(name(&[60, 64, 67, 70]), "C7");
        assert_eq!(name(&[60, 64, 67, 71]), "Cmaj7");
        assert_eq!(name(&[62, 65, 69, 72]), "Dm7");
        assert_eq!(name(&[59, 62, 65, 69]), "Bm7b5");
        assert_eq!(name(&[60, 63, 66, 69]), "Cdim7");
        assert_eq!(name(&[60, 64, 67, 62 + 12]), "Cadd9");
    }

    #[test]
    fn bass_resolves_ambiguous_sets() {
        // Same pitch classes, different bass.
        assert_eq!(name(&[57, 60, 64, 67]), "Am7");
        assert_eq!(name(&[60, 64, 67, 69]), "C6");
        assert_eq!(name(&[60, 65, 67]), "Csus4");
        assert_eq!(name(&[53, 55, 60]), "Fsus2");
    }

    #[test]
    fn partial_chords() {
        assert_eq!(name(&[60, 64]), "C");
        assert_eq!(name(&[64, 72]), "C/E");
        assert_eq!(name(&[60, 63]), "Cm");
        assert_eq!(name(&[60, 67]), "C5");
        assert_eq!(name(&[60, 64, 70]), "C7");
    }

    #[test]
    fn too_few_notes() {
        assert_eq!(recognize(&[]), None);
        assert_eq!(recognize(&[60]), None);
        assert_eq!(recognize(&[60, 72]), None);
    }

    #[test]
    fn relative_rotation() {
        let set = PitchSet::from_notes([57, 60, 64]); // A C E
        assert_eq!(set.relative_to(9), 0b1000_1001); // 0, 3, 7
    }
}
