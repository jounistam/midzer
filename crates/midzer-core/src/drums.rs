//! A drummer's kit: the pieces a hit can land on, and the map from the notes
//! an e-kit sends to those pieces. Kits disagree about notes beyond the
//! General MIDI basics, so the UI sends each kit's map with
//! [`crate::Command::KitNote`]; the map starts out as General MIDI.

/// What the player plays. With drums, every note is a drum hit through the
/// kit map; with keys, only notes on the MIDI drum channel are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Instrument {
    #[default]
    Keys,
    Drums,
}

/// Something on the kit a drummer can hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Kick,
    Snare,
    /// A rimshot or the rim zone of the snare.
    SnareRim,
    SideStick,
    HiHatClosed,
    HiHatOpen,
    /// The hi-hat played with the pedal ("chick").
    HiHatPedal,
    /// Rack toms from high to low, then the floor toms.
    Tom1,
    Tom2,
    Tom3,
    Tom4,
    Crash,
    Crash2,
    Ride,
    RideBell,
    Splash,
    China,
}

impl Piece {
    pub const COUNT: usize = 17;
    pub const ALL: [Piece; Self::COUNT] = [
        Piece::Kick,
        Piece::Snare,
        Piece::SnareRim,
        Piece::SideStick,
        Piece::HiHatClosed,
        Piece::HiHatOpen,
        Piece::HiHatPedal,
        Piece::Tom1,
        Piece::Tom2,
        Piece::Tom3,
        Piece::Tom4,
        Piece::Crash,
        Piece::Crash2,
        Piece::Ride,
        Piece::RideBell,
        Piece::Splash,
        Piece::China,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// The General MIDI drum key the piece sounds and is reported as.
    pub fn note(self) -> u8 {
        match self {
            Piece::Kick => 36,
            Piece::Snare => 38,
            Piece::SnareRim => 40,
            Piece::SideStick => 37,
            Piece::HiHatClosed => 42,
            Piece::HiHatOpen => 46,
            Piece::HiHatPedal => 44,
            Piece::Tom1 => 48,
            Piece::Tom2 => 45,
            Piece::Tom3 => 43,
            Piece::Tom4 => 41,
            Piece::Crash => 49,
            Piece::Crash2 => 57,
            Piece::Ride => 51,
            Piece::RideBell => 53,
            Piece::Splash => 55,
            Piece::China => 52,
        }
    }
}

/// Which piece each incoming note plays; unmapped notes are ignored.
#[derive(Debug, Clone)]
pub struct Kit {
    pieces: [Option<Piece>; 128],
}

impl Default for Kit {
    /// The General MIDI drum map, with both of each pair of GM toms on one piece.
    fn default() -> Self {
        let mut kit = Self { pieces: [None; 128] };
        for piece in Piece::ALL {
            kit.set(piece.note(), Some(piece));
        }
        for (note, piece) in [(35, Piece::Kick), (50, Piece::Tom1), (47, Piece::Tom2), (59, Piece::Ride)] {
            kit.set(note, Some(piece));
        }
        kit
    }
}

impl Kit {
    pub fn piece(&self, note: u8) -> Option<Piece> {
        self.pieces.get(note as usize).copied().flatten()
    }

    pub fn set(&mut self, note: u8, piece: Option<Piece>) {
        if let Some(slot) = self.pieces.get_mut(note as usize) {
            *slot = piece;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_have_distinct_notes_and_indices() {
        for (i, piece) in Piece::ALL.iter().enumerate() {
            assert_eq!(piece.index(), i);
            assert_eq!(Piece::from_index(i), Some(*piece));
            assert!(Piece::ALL.iter().filter(|p| p.note() == piece.note()).count() == 1, "{piece:?}");
        }
        assert_eq!(Piece::from_index(Piece::COUNT), None);
    }

    #[test]
    fn default_kit_is_general_midi() {
        let kit = Kit::default();
        for piece in Piece::ALL {
            assert_eq!(kit.piece(piece.note()), Some(piece));
        }
        assert_eq!(kit.piece(35), Some(Piece::Kick));
        assert_eq!(kit.piece(60), None);
        assert_eq!(kit.piece(200), None);
    }
}
