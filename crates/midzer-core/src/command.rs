/// Number of `u32` words in an encoded command.
pub const WORDS_PER_COMMAND: usize = 4;

/// A command from the UI to the engine.
///
/// Commands are encoded as fixed-size `u32` records so they can travel through
/// a lock-free ring buffer without allocation. The TypeScript mirror lives in
/// `packages/audio-web/src/protocol.ts` and must be kept in sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    SetGain { gain: f32 },
    AllNotesOff,
    Play,
    Stop,
    SetTempo { bpm: f32 },
    /// Notes below `note` form chords; 0 uses the whole keyboard.
    SetSplit { note: u8 },
    /// Silences or restores the band part on `channel`.
    SetMute { channel: u8, muted: bool },
}

mod op {
    pub const NOTE_ON: u32 = 1;
    pub const NOTE_OFF: u32 = 2;
    pub const SET_GAIN: u32 = 3;
    pub const ALL_NOTES_OFF: u32 = 4;
    pub const PLAY: u32 = 5;
    pub const STOP: u32 = 6;
    pub const SET_TEMPO: u32 = 7;
    pub const SET_SPLIT: u32 = 8;
    pub const SET_MUTE: u32 = 9;
}

impl Command {
    pub fn encode(self) -> [u32; WORDS_PER_COMMAND] {
        match self {
            Command::NoteOn { note, velocity } => [op::NOTE_ON, note as u32, velocity as u32, 0],
            Command::NoteOff { note } => [op::NOTE_OFF, note as u32, 0, 0],
            Command::SetGain { gain } => [op::SET_GAIN, gain.to_bits(), 0, 0],
            Command::AllNotesOff => [op::ALL_NOTES_OFF, 0, 0, 0],
            Command::Play => [op::PLAY, 0, 0, 0],
            Command::Stop => [op::STOP, 0, 0, 0],
            Command::SetTempo { bpm } => [op::SET_TEMPO, bpm.to_bits(), 0, 0],
            Command::SetSplit { note } => [op::SET_SPLIT, note as u32, 0, 0],
            Command::SetMute { channel, muted } => [op::SET_MUTE, channel as u32, muted as u32, 0],
        }
    }

    /// Decodes a record, returning `None` for unknown opcodes or out-of-range values.
    pub fn decode(words: [u32; WORDS_PER_COMMAND]) -> Option<Self> {
        let [opcode, a, b, _] = words;
        let midi = |v: u32| u8::try_from(v).ok().filter(|v| *v < 128);
        let float = |v: u32| Some(f32::from_bits(v)).filter(|f| f.is_finite());
        match opcode {
            op::NOTE_ON => Some(Command::NoteOn { note: midi(a)?, velocity: midi(b)? }),
            op::NOTE_OFF => Some(Command::NoteOff { note: midi(a)? }),
            op::SET_GAIN => Some(Command::SetGain { gain: float(a)? }),
            op::ALL_NOTES_OFF => Some(Command::AllNotesOff),
            op::PLAY => Some(Command::Play),
            op::STOP => Some(Command::Stop),
            op::SET_TEMPO => Some(Command::SetTempo { bpm: float(a)? }),
            op::SET_SPLIT => Some(Command::SetSplit { note: midi(a)? }),
            op::SET_MUTE => Some(Command::SetMute { channel: u8::try_from(a).ok().filter(|c| *c < 16)?, muted: b != 0 }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for cmd in [
            Command::NoteOn { note: 60, velocity: 100 },
            Command::NoteOff { note: 127 },
            Command::SetGain { gain: 0.25 },
            Command::AllNotesOff,
            Command::Play,
            Command::Stop,
            Command::SetTempo { bpm: 96.5 },
            Command::SetSplit { note: 54 },
            Command::SetMute { channel: 9, muted: true },
        ] {
            assert_eq!(Command::decode(cmd.encode()), Some(cmd));
        }
    }

    #[test]
    fn rejects_invalid_records() {
        assert_eq!(Command::decode([0, 0, 0, 0]), None);
        assert_eq!(Command::decode([op::NOTE_ON, 128, 100, 0]), None);
        assert_eq!(Command::decode([op::SET_GAIN, f32::NAN.to_bits(), 0, 0]), None);
        assert_eq!(Command::decode([op::SET_TEMPO, f32::INFINITY.to_bits(), 0, 0]), None);
    }
}
