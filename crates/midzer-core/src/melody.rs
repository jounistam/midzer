//! The song's tune, played quietly under the band for a player learning it.
//!
//! Notes are loaded through commands into a fixed array, in song ticks
//! ([`crate::style::PPQ`] per beat), so loading and playing never allocate.
//! The band plays them on [`MELODY_CHANNEL`] as the song goes, so they stop
//! and wait with it.

use std::ops::Range;

/// The MIDI channel the tune plays on.
pub const MELODY_CHANNEL: u8 = 4;
/// General MIDI flute: clear, and unlike the player's piano.
pub const MELODY_PROGRAM: u8 = 73;
pub const MAX_MELODY_NOTES: usize = 2048;
/// The velocity at full volume.
const LOUDEST: f32 = 110.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MelodyNote {
    /// Song tick it starts on.
    pub tick: u32,
    pub len: u32,
    pub key: u8,
}

pub struct Melody {
    notes: [MelodyNote; MAX_MELODY_NOTES],
    count: usize,
    volume: f32,
}

impl Default for Melody {
    fn default() -> Self {
        Self { notes: [MelodyNote::default(); MAX_MELODY_NOTES], count: 0, volume: 0.0 }
    }
}

impl Melody {
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// Adds a note, in order of its tick; past [`MAX_MELODY_NOTES`] it is dropped.
    pub fn add(&mut self, note: MelodyNote) {
        if self.count == MAX_MELODY_NOTES {
            return;
        }
        let at = self.notes[..self.count].partition_point(|n| n.tick <= note.tick);
        self.notes.copy_within(at..self.count, at + 1);
        self.notes[at] = note;
        self.count += 1;
    }

    /// How loud it plays, 0 (not at all) to 1.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// The notes starting in song ticks `from..to` (see [`Self::note`]), none when silent.
    pub fn starting(&self, from: f64, to: f64) -> Range<usize> {
        if self.volume <= 0.0 || to <= from {
            return 0..0;
        }
        let notes = &self.notes[..self.count];
        notes.partition_point(|n| (n.tick as f64) < from)..notes.partition_point(|n| (n.tick as f64) < to)
    }

    pub fn note(&self, i: usize) -> MelodyNote {
        self.notes[i]
    }

    /// Their velocity: the synth's loudness goes with its square, so the volume is linear.
    pub fn velocity(&self) -> u8 {
        (LOUDEST * self.volume.sqrt()).round().clamp(1.0, 127.0) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_notes_in_order_and_finds_them_by_tick() {
        let mut m = Melody::default();
        for (tick, key) in [(960, 64), (0, 60), (480, 62)] {
            m.add(MelodyNote { tick, len: 480, key });
        }
        assert!(m.starting(0.0, 2000.0).is_empty(), "silent until it has a volume");
        m.set_volume(0.25);
        assert_eq!(m.starting(0.0, 2000.0).map(|i| m.note(i).key).collect::<Vec<_>>(), [60, 62, 64]);
        assert_eq!(m.starting(480.0, 960.0).map(|i| m.note(i).key).collect::<Vec<_>>(), [62]);
        assert_eq!(m.velocity(), 55);
        m.clear();
        assert!(m.starting(0.0, 2000.0).is_empty());
    }
}
