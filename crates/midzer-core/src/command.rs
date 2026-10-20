use crate::conductor::{IdleMode, Request};

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
    /// Selects a built-in style by index; while playing it changes on the next bar.
    SetStyle { index: u8 },
    /// Intro, a variation, a fill or the ending. Intro while stopped starts the band.
    Section { request: Request },
    /// Whether energy and variation follow the player's touch.
    SetFollow { on: bool },
    /// Energy 0..=1 used while not following.
    SetEnergy { energy: f32 },
    SetIdleMode { mode: IdleMode },
    /// How late, in ms, the player's notes arrive after the beat they meant.
    SetLatency { ms: f32 },
    /// A click on every beat instead of the band, for latency calibration.
    SetClick { on: bool },
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
    pub const SET_STYLE: u32 = 10;
    pub const SECTION: u32 = 11;
    pub const SET_FOLLOW: u32 = 12;
    pub const SET_ENERGY: u32 = 13;
    pub const SET_IDLE_MODE: u32 = 14;
    pub const SET_LATENCY: u32 = 15;
    pub const SET_CLICK: u32 = 16;
}

fn request_code(request: Request) -> u32 {
    match request {
        Request::Intro => 0,
        Request::Variation(v) => 1 + v as u32,
        Request::Fill => 4,
        Request::Ending => 5,
    }
}

fn request_from(code: u32) -> Option<Request> {
    match code {
        0 => Some(Request::Intro),
        1..=3 => Some(Request::Variation(code as u8 - 1)),
        4 => Some(Request::Fill),
        5 => Some(Request::Ending),
        _ => None,
    }
}

const IDLE_MODES: [IdleMode; 3] = [IdleMode::KeepPlaying, IdleMode::Rest, IdleMode::End];

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
            Command::SetStyle { index } => [op::SET_STYLE, index as u32, 0, 0],
            Command::Section { request } => [op::SECTION, request_code(request), 0, 0],
            Command::SetFollow { on } => [op::SET_FOLLOW, on as u32, 0, 0],
            Command::SetEnergy { energy } => [op::SET_ENERGY, energy.to_bits(), 0, 0],
            Command::SetIdleMode { mode } => [op::SET_IDLE_MODE, IDLE_MODES.iter().position(|m| *m == mode).unwrap_or(0) as u32, 0, 0],
            Command::SetLatency { ms } => [op::SET_LATENCY, ms.to_bits(), 0, 0],
            Command::SetClick { on } => [op::SET_CLICK, on as u32, 0, 0],
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
            op::SET_STYLE => Some(Command::SetStyle { index: u8::try_from(a).ok()? }),
            op::SECTION => Some(Command::Section { request: request_from(a)? }),
            op::SET_FOLLOW => Some(Command::SetFollow { on: a != 0 }),
            op::SET_ENERGY => Some(Command::SetEnergy { energy: float(a)? }),
            op::SET_IDLE_MODE => Some(Command::SetIdleMode { mode: *IDLE_MODES.get(a as usize)? }),
            op::SET_LATENCY => Some(Command::SetLatency { ms: float(a)? }),
            op::SET_CLICK => Some(Command::SetClick { on: a != 0 }),
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
            Command::SetStyle { index: 3 },
            Command::Section { request: Request::Intro },
            Command::Section { request: Request::Variation(2) },
            Command::Section { request: Request::Fill },
            Command::Section { request: Request::Ending },
            Command::SetFollow { on: true },
            Command::SetEnergy { energy: 0.75 },
            Command::SetIdleMode { mode: IdleMode::End },
            Command::SetLatency { ms: 42.5 },
            Command::SetClick { on: true },
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
        assert_eq!(Command::decode([op::SECTION, 6, 0, 0]), None);
        assert_eq!(Command::decode([op::SET_IDLE_MODE, 3, 0, 0]), None);
    }
}
