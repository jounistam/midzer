use crate::chord::{Chord, Quality};
use crate::conductor::Section;

/// Number of `u32` words in an encoded event.
pub const WORDS_PER_EVENT: usize = 4;

/// Upper bound of events produced in one render block; extra events are dropped.
pub const MAX_EVENTS_PER_BLOCK: usize = 512;

pub const DRUM_CHANNEL: u8 = 9;
/// MIDI channel of what the microphone heard. These notes are reported but
/// not played: the player's own sound is already in the room.
pub const MIC_CHANNEL: u8 = 15;

/// Something the engine did, for the synth, MIDI out and the UI.
///
/// Encoded as `[opcode, a, b, frame]`; the TypeScript mirror lives in
/// `packages/audio-web/src/protocol.ts` and must be kept in sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineEvent {
    NoteOn { channel: u8, key: u8, velocity: u8 },
    NoteOff { channel: u8, key: u8 },
    ProgramChange { channel: u8, program: u8 },
    /// The band's harmony changed; `None` before the first chord is played.
    ChordChanged(Option<Chord>),
    Beat { bar: u32, beat: u8 },
    Transport { playing: bool },
    /// Style, section and energy changed; sent at the beat where it is heard.
    Band(BandState),
    /// A song beat starts; sent on every beat while a song plays.
    SongPosition { beat: u32 },
    /// The band stopped at song tick `tick` to wait for the player, or went on from there.
    Hold { holding: bool, tick: u32 },
    /// Microphone loudness (0..=255 over -80..0 dBFS) and the note it hears, for meters.
    Input { level: u8, key: Option<u8> },
    /// Tempo follow moved the tempo, or turning it off restored it.
    Tempo { bpm: f32 },
}

/// What the band is doing, for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandState {
    pub style: u8,
    /// A style change queued for the next bar line.
    pub next_style: Option<u8>,
    pub playing: bool,
    pub section: Section,
    pub variation: u8,
    /// The variation after the current intro, fill or rest.
    pub next_variation: u8,
    /// Queued for the next beat (fill) or bar line (intro, ending).
    pub fill_pending: bool,
    pub intro_pending: bool,
    pub ending_pending: bool,
    pub follow: bool,
    /// 0..=255.
    pub energy: u8,
}

mod op {
    pub const NOTE_ON: u32 = 1;
    pub const NOTE_OFF: u32 = 2;
    pub const PROGRAM_CHANGE: u32 = 3;
    pub const CHORD_CHANGED: u32 = 4;
    pub const BEAT: u32 = 5;
    pub const TRANSPORT: u32 = 6;
    pub const BAND: u32 = 7;
    pub const SONG_POSITION: u32 = 8;
    pub const HOLD: u32 = 9;
    pub const INPUT: u32 = 10;
    pub const TEMPO: u32 = 11;
}

const NO_CHORD: u32 = 0xff;
const NO_KEY: u32 = 0xff;

impl EngineEvent {
    pub fn encode(self, frame: u32) -> [u32; WORDS_PER_EVENT] {
        match self {
            EngineEvent::NoteOn { channel, key, velocity } => {
                [op::NOTE_ON, (channel as u32) << 8 | key as u32, velocity as u32, frame]
            }
            EngineEvent::NoteOff { channel, key } => {
                [op::NOTE_OFF, (channel as u32) << 8 | key as u32, 0, frame]
            }
            EngineEvent::ProgramChange { channel, program } => {
                [op::PROGRAM_CHANGE, channel as u32, program as u32, frame]
            }
            EngineEvent::ChordChanged(None) => [op::CHORD_CHANGED, NO_CHORD, 0, frame],
            EngineEvent::ChordChanged(Some(c)) => [
                op::CHORD_CHANGED,
                c.root as u32,
                (c.bass as u32) << 8 | quality_index(c.quality),
                frame,
            ],
            EngineEvent::Beat { bar, beat } => [op::BEAT, bar, beat as u32, frame],
            EngineEvent::Transport { playing } => [op::TRANSPORT, playing as u32, 0, frame],
            EngineEvent::SongPosition { beat } => [op::SONG_POSITION, beat, 0, frame],
            EngineEvent::Hold { holding, tick } => [op::HOLD, holding as u32, tick, frame],
            EngineEvent::Input { level, key } => [op::INPUT, level as u32, key.map_or(NO_KEY, u32::from), frame],
            EngineEvent::Tempo { bpm } => [op::TEMPO, bpm.to_bits(), 0, frame],
            EngineEvent::Band(s) => {
                let section = Section::ALL.iter().position(|x| *x == s.section).unwrap_or(0) as u32;
                let flags = s.playing as u32
                    | (s.fill_pending as u32) << 1
                    | (s.ending_pending as u32) << 2
                    | (s.follow as u32) << 3
                    | (s.intro_pending as u32) << 4;
                [
                    op::BAND,
                    s.style as u32 | (s.next_style.unwrap_or(0xff) as u32) << 8 | section << 16 | (s.variation as u32) << 24,
                    s.next_variation as u32 | flags << 8 | (s.energy as u32) << 16,
                    frame,
                ]
            }
        }
    }

    /// Decodes a record from [`EngineEvent::encode`] into the event and its frame.
    pub fn decode(words: [u32; WORDS_PER_EVENT]) -> Option<(Self, u32)> {
        let [opcode, a, b, frame] = words;
        let byte = |v: u32| (v & 0xff) as u8;
        let event = match opcode {
            op::NOTE_ON => EngineEvent::NoteOn { channel: byte(a >> 8), key: byte(a), velocity: byte(b) },
            op::NOTE_OFF => EngineEvent::NoteOff { channel: byte(a >> 8), key: byte(a) },
            op::PROGRAM_CHANGE => EngineEvent::ProgramChange { channel: byte(a), program: byte(b) },
            op::CHORD_CHANGED if a == NO_CHORD => EngineEvent::ChordChanged(None),
            op::CHORD_CHANGED => EngineEvent::ChordChanged(Some(Chord {
                root: byte(a),
                quality: *Quality::ALL.get((b & 0xff) as usize)?,
                bass: byte(b >> 8),
            })),
            op::BEAT => EngineEvent::Beat { bar: a, beat: byte(b) },
            op::TRANSPORT => EngineEvent::Transport { playing: a != 0 },
            op::SONG_POSITION => EngineEvent::SongPosition { beat: a },
            op::HOLD => EngineEvent::Hold { holding: a != 0, tick: b },
            op::INPUT => EngineEvent::Input { level: byte(a), key: (b != NO_KEY).then_some(byte(b)) },
            op::TEMPO => EngineEvent::Tempo { bpm: f32::from_bits(a) },
            op::BAND => {
                let flags = byte(b >> 8);
                let next_style = byte(a >> 8);
                EngineEvent::Band(BandState {
                    style: byte(a),
                    next_style: (next_style != 0xff).then_some(next_style),
                    section: *Section::ALL.get(byte(a >> 16) as usize)?,
                    variation: byte(a >> 24),
                    next_variation: byte(b),
                    playing: flags & 1 != 0,
                    fill_pending: flags & 2 != 0,
                    ending_pending: flags & 4 != 0,
                    follow: flags & 8 != 0,
                    intro_pending: flags & 16 != 0,
                    energy: byte(b >> 16),
                })
            }
            _ => return None,
        };
        Some((event, frame))
    }

    /// The MIDI message for external gear: the band's notes and programs.
    /// The player's own notes and the microphone's are not sent.
    pub fn to_midi(&self) -> Option<[u8; 3]> {
        match *self {
            EngineEvent::NoteOn { channel, .. } | EngineEvent::NoteOff { channel, .. }
                if channel == crate::engine::PLAYER_CHANNEL || channel == MIC_CHANNEL =>
            {
                None
            }
            EngineEvent::NoteOn { channel, key, velocity } => Some([0x90 | channel, key, velocity]),
            EngineEvent::NoteOff { channel, key } => Some([0x80 | channel, key, 0]),
            EngineEvent::ProgramChange { channel, program } => Some([0xc0 | channel, program, 0]),
            _ => None,
        }
    }

    /// Sort key for events on the same frame: releases before new notes, so a
    /// retriggered key is not cut off by its own note-off.
    fn order(&self) -> u8 {
        match self {
            EngineEvent::NoteOff { .. } => 0,
            EngineEvent::ProgramChange { .. } | EngineEvent::Transport { .. } => 1,
            _ => 2,
        }
    }
}

fn quality_index(quality: Quality) -> u32 {
    Quality::ALL.iter().position(|q| *q == quality).unwrap_or(0) as u32
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timed {
    /// Offset in frames from the start of the current block.
    pub frame: u32,
    pub event: EngineEvent,
}

/// Fixed-capacity event list for one render block.
pub struct EventBuffer {
    events: [Timed; MAX_EVENTS_PER_BLOCK],
    len: usize,
    dropped: u32,
}

impl Default for EventBuffer {
    fn default() -> Self {
        let empty = Timed { frame: 0, event: EngineEvent::Transport { playing: false } };
        Self { events: [empty; MAX_EVENTS_PER_BLOCK], len: 0, dropped: 0 }
    }
}

impl EventBuffer {
    pub fn push(&mut self, frame: u32, event: EngineEvent) {
        if self.len < MAX_EVENTS_PER_BLOCK {
            self.events[self.len] = Timed { frame, event };
            self.len += 1;
        } else {
            self.dropped += 1;
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Sorts by frame without allocating.
    pub fn sort(&mut self) {
        self.events[..self.len].sort_unstable_by_key(|t| (t.frame, t.event.order()));
    }

    pub fn as_slice(&self) -> &[Timed] {
        &self.events[..self.len]
    }

    /// Total events dropped because a block overflowed; useful as a health metric.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_offs_sort_before_note_ons_on_the_same_frame() {
        let mut buf = EventBuffer::default();
        buf.push(10, EngineEvent::NoteOn { channel: 1, key: 60, velocity: 90 });
        buf.push(10, EngineEvent::NoteOff { channel: 1, key: 60 });
        buf.push(5, EngineEvent::Beat { bar: 0, beat: 1 });
        buf.sort();
        let frames: Vec<_> = buf.as_slice().iter().map(|t| (t.frame, t.event.order())).collect();
        assert_eq!(frames, [(5, 2), (10, 0), (10, 2)]);
    }

    #[test]
    fn overflow_is_counted_not_fatal() {
        let mut buf = EventBuffer::default();
        for _ in 0..MAX_EVENTS_PER_BLOCK + 3 {
            buf.push(0, EngineEvent::Transport { playing: true });
        }
        assert_eq!(buf.as_slice().len(), MAX_EVENTS_PER_BLOCK);
        assert_eq!(buf.dropped(), 3);
    }

    #[test]
    fn encodes_chords() {
        let chord = Chord { root: 9, quality: Quality::Minor7, bass: 0 };
        let words = EngineEvent::ChordChanged(Some(chord)).encode(7);
        assert_eq!(words, [op::CHORD_CHANGED, 9, 11, 7]);
        assert_eq!(EngineEvent::ChordChanged(None).encode(0)[1], NO_CHORD);
    }

    #[test]
    fn decodes_what_it_encodes() {
        let band = BandState {
            style: 3,
            next_style: Some(1),
            playing: true,
            section: Section::Rest,
            variation: 2,
            next_variation: 0,
            fill_pending: true,
            intro_pending: false,
            ending_pending: true,
            follow: false,
            energy: 77,
        };
        for event in [
            EngineEvent::NoteOn { channel: 15, key: 64, velocity: 99 },
            EngineEvent::NoteOff { channel: 9, key: 38 },
            EngineEvent::ProgramChange { channel: 2, program: 33 },
            EngineEvent::ChordChanged(None),
            EngineEvent::ChordChanged(Some(Chord { root: 4, quality: Quality::Sus4, bass: 11 })),
            EngineEvent::Beat { bar: 70_000, beat: 3 },
            EngineEvent::Transport { playing: true },
            EngineEvent::Band(band),
            EngineEvent::Band(BandState { next_style: None, ..band }),
            EngineEvent::SongPosition { beat: 99 },
            EngineEvent::Hold { holding: true, tick: 1234 },
            EngineEvent::Input { level: 9, key: None },
            EngineEvent::Input { level: 9, key: Some(0) },
            EngineEvent::Tempo { bpm: 88.25 },
        ] {
            assert_eq!(EngineEvent::decode(event.encode(42)), Some((event, 42)));
        }
        assert_eq!(EngineEvent::decode([99, 0, 0, 0]), None);
    }

    #[test]
    fn only_band_notes_and_programs_go_to_midi_out() {
        assert_eq!(EngineEvent::NoteOn { channel: 2, key: 40, velocity: 90 }.to_midi(), Some([0x92, 40, 90]));
        assert_eq!(EngineEvent::NoteOff { channel: 9, key: 36 }.to_midi(), Some([0x89, 36, 0]));
        assert_eq!(EngineEvent::NoteOn { channel: 0, key: 60, velocity: 90 }.to_midi(), None);
        assert_eq!(EngineEvent::NoteOn { channel: MIC_CHANNEL, key: 60, velocity: 90 }.to_midi(), None);
        assert_eq!(EngineEvent::Beat { bar: 0, beat: 0 }.to_midi(), None);
    }

    #[test]
    fn encodes_input_and_tempo() {
        assert_eq!(EngineEvent::Input { level: 200, key: Some(60) }.encode(3), [op::INPUT, 200, 60, 3]);
        assert_eq!(EngineEvent::Input { level: 0, key: None }.encode(0)[2], NO_KEY);
        assert_eq!(EngineEvent::Tempo { bpm: 101.5 }.encode(0), [op::TEMPO, 101.5f32.to_bits(), 0, 0]);
    }

    #[test]
    fn encodes_band_state() {
        let state = BandState {
            style: 2,
            next_style: None,
            playing: true,
            section: Section::Fill,
            variation: 1,
            next_variation: 2,
            fill_pending: false,
            intro_pending: true,
            ending_pending: true,
            follow: true,
            energy: 200,
        };
        let [opcode, a, b, frame] = EngineEvent::Band(state).encode(5);
        assert_eq!((opcode, frame), (op::BAND, 5));
        assert_eq!(a, 2 | 0xff << 8 | 2 << 16 | 1 << 24);
        assert_eq!(b, 2 | 0b11101 << 8 | 200 << 16);
    }
}
