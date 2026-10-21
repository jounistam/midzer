use crate::chord::{Chord, Quality};
use crate::conductor::{IdleMode, Request};
use crate::listen::ListenMode;
use crate::song::Slot;

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
    /// Empties the song chart and sets its length in bars.
    SongClear { bars: u16 },
    /// The chart's harmony from song beat `beat` on.
    SongChord { beat: u16, slot: Slot },
    /// A section the song asks for at bar `bar`: intro, variation, fill or ending.
    SongMark { bar: u16, mark: Option<Request> },
    /// Repeats bars `start..end`; an empty range turns the loop off.
    SongLoop { start: u16, end: u16 },
    /// Whether the song chart, rather than the player, sets the harmony.
    SetSong { on: bool },
    /// Stopped: where the song starts. Playing: a jump on the next bar line.
    SongSeek { bar: u16 },
    /// The band stops at song tick `tick` (in [`crate::style::PPQ`] per beat)
    /// and waits there until the wait moves or is cleared with `None`.
    SongWait { tick: Option<u32> },
    /// What the microphone listens for, and how loud a sound must be (dBFS) to count.
    SetListen { mode: ListenMode, gate_db: f32 },
    /// Whether the band follows the player's tempo.
    SetTempoFollow { on: bool },
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
    pub const SONG_CLEAR: u32 = 17;
    pub const SONG_CHORD: u32 = 18;
    pub const SONG_MARK: u32 = 19;
    pub const SONG_LOOP: u32 = 20;
    pub const SET_SONG: u32 = 21;
    pub const SONG_SEEK: u32 = 22;
    pub const SONG_WAIT: u32 = 23;
    pub const SET_LISTEN: u32 = 24;
    pub const SET_TEMPO_FOLLOW: u32 = 25;
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

/// Slots travel as `[kind, root | quality << 8 | bass << 16]` with kind
/// 0 hold, 1 no chord, 2 chord.
fn slot_words(slot: Slot) -> (u32, u32) {
    match slot {
        Slot::Hold => (0, 0),
        Slot::NoChord => (1, 0),
        Slot::Chord(c) => {
            let quality = Quality::ALL.iter().position(|q| *q == c.quality).unwrap_or(0) as u32;
            (2, c.root as u32 | quality << 8 | (c.bass as u32) << 16)
        }
    }
}

fn slot_from(kind: u32, packed: u32) -> Option<Slot> {
    match kind {
        0 => Some(Slot::Hold),
        1 => Some(Slot::NoChord),
        2 => {
            let (root, quality, bass) = (packed & 0xff, packed >> 8 & 0xff, packed >> 16);
            if root >= 12 || bass >= 12 {
                return None;
            }
            let quality = *Quality::ALL.get(quality as usize)?;
            Some(Slot::Chord(Chord { root: root as u8, quality, bass: bass as u8 }))
        }
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
            Command::SongClear { bars } => [op::SONG_CLEAR, bars as u32, 0, 0],
            Command::SongChord { beat, slot } => {
                let (kind, packed) = slot_words(slot);
                [op::SONG_CHORD, beat as u32, kind, packed]
            }
            Command::SongMark { bar, mark } => [op::SONG_MARK, bar as u32, mark.map_or(0, |m| request_code(m) + 1), 0],
            Command::SongLoop { start, end } => [op::SONG_LOOP, start as u32, end as u32, 0],
            Command::SetSong { on } => [op::SET_SONG, on as u32, 0, 0],
            Command::SongSeek { bar } => [op::SONG_SEEK, bar as u32, 0, 0],
            Command::SongWait { tick } => [op::SONG_WAIT, tick.is_some() as u32, tick.unwrap_or(0), 0],
            Command::SetListen { mode, gate_db } => {
                let mode = ListenMode::ALL.iter().position(|m| *m == mode).unwrap_or(0) as u32;
                [op::SET_LISTEN, mode, gate_db.to_bits(), 0]
            }
            Command::SetTempoFollow { on } => [op::SET_TEMPO_FOLLOW, on as u32, 0, 0],
        }
    }

    /// Translates a raw MIDI message from a controller; mirrors `midiToCommand` in
    /// `packages/audio-web/src/midi.ts`.
    pub fn from_midi(data: &[u8]) -> Option<Self> {
        let (status, d1, d2) = (*data.first()?, data.get(1).copied().unwrap_or(0), data.get(2).copied().unwrap_or(0));
        match status & 0xf0 {
            0x90 if d2 > 0 && d1 < 128 => Some(Command::NoteOn { note: d1, velocity: d2.min(127) }),
            0x80 | 0x90 if d1 < 128 => Some(Command::NoteOff { note: d1 }),
            // All Sound Off / All Notes Off
            0xb0 if d1 == 120 || d1 == 123 => Some(Command::AllNotesOff),
            _ => None,
        }
    }

    /// Decodes a record, returning `None` for unknown opcodes or out-of-range values.
    pub fn decode(words: [u32; WORDS_PER_COMMAND]) -> Option<Self> {
        let [opcode, a, b, c] = words;
        let short = |v: u32| u16::try_from(v).ok();
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
            op::SONG_CLEAR => Some(Command::SongClear { bars: short(a)? }),
            op::SONG_CHORD => Some(Command::SongChord { beat: short(a)?, slot: slot_from(b, c)? }),
            op::SONG_MARK => Some(Command::SongMark {
                bar: short(a)?,
                mark: if b == 0 { None } else { Some(request_from(b - 1)?) },
            }),
            op::SONG_LOOP => Some(Command::SongLoop { start: short(a)?, end: short(b)? }),
            op::SET_SONG => Some(Command::SetSong { on: a != 0 }),
            op::SONG_SEEK => Some(Command::SongSeek { bar: short(a)? }),
            op::SONG_WAIT => Some(Command::SongWait { tick: (a != 0).then_some(b) }),
            op::SET_LISTEN => Some(Command::SetListen { mode: *ListenMode::ALL.get(a as usize)?, gate_db: float(b)? }),
            op::SET_TEMPO_FOLLOW => Some(Command::SetTempoFollow { on: a != 0 }),
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
            Command::SongClear { bars: 32 },
            Command::SongChord { beat: 7, slot: Slot::Hold },
            Command::SongChord { beat: 8, slot: Slot::NoChord },
            Command::SongChord { beat: 9, slot: Slot::Chord(Chord { root: 9, quality: Quality::Minor7, bass: 0 }) },
            Command::SongMark { bar: 3, mark: None },
            Command::SongMark { bar: 4, mark: Some(Request::Variation(1)) },
            Command::SongMark { bar: 5, mark: Some(Request::Intro) },
            Command::SongLoop { start: 2, end: 6 },
            Command::SetSong { on: true },
            Command::SongSeek { bar: 12 },
            Command::SongWait { tick: Some(5 * 480 + 240) },
            Command::SongWait { tick: None },
            Command::SetListen { mode: ListenMode::Chords, gate_db: -45.0 },
            Command::SetTempoFollow { on: true },
        ] {
            assert_eq!(Command::decode(cmd.encode()), Some(cmd));
        }
    }

    #[test]
    fn translates_midi() {
        assert_eq!(Command::from_midi(&[0x93, 60, 100]), Some(Command::NoteOn { note: 60, velocity: 100 }));
        assert_eq!(Command::from_midi(&[0x90, 60, 0]), Some(Command::NoteOff { note: 60 }));
        assert_eq!(Command::from_midi(&[0x80, 61, 30]), Some(Command::NoteOff { note: 61 }));
        assert_eq!(Command::from_midi(&[0xb0, 123, 0]), Some(Command::AllNotesOff));
        assert_eq!(Command::from_midi(&[0xb0, 64, 127]), None);
        assert_eq!(Command::from_midi(&[0xf8]), None);
        assert_eq!(Command::from_midi(&[]), None);
    }

    #[test]
    fn rejects_invalid_records() {
        assert_eq!(Command::decode([0, 0, 0, 0]), None);
        assert_eq!(Command::decode([op::NOTE_ON, 128, 100, 0]), None);
        assert_eq!(Command::decode([op::SET_GAIN, f32::NAN.to_bits(), 0, 0]), None);
        assert_eq!(Command::decode([op::SET_TEMPO, f32::INFINITY.to_bits(), 0, 0]), None);
        assert_eq!(Command::decode([op::SECTION, 6, 0, 0]), None);
        assert_eq!(Command::decode([op::SET_IDLE_MODE, 3, 0, 0]), None);
        assert_eq!(Command::decode([op::SONG_CHORD, 0, 2, 12]), None);
        assert_eq!(Command::decode([op::SONG_CHORD, 0, 2, 15 << 8]), None);
        assert_eq!(Command::decode([op::SONG_CHORD, 0, 3, 0]), None);
        assert_eq!(Command::decode([op::SONG_MARK, 0, 7, 0]), None);
        assert_eq!(Command::decode([op::SONG_CLEAR, 70_000, 0, 0]), None);
        assert_eq!(Command::decode([op::SET_LISTEN, 3, 0, 0]), None);
    }
}
