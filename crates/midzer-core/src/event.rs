use crate::chord::{Chord, Quality};
use crate::conductor::Section;

/// Number of `u32` words in an encoded event.
pub const WORDS_PER_EVENT: usize = 4;

/// Upper bound of events produced in one render block; extra events are dropped.
pub const MAX_EVENTS_PER_BLOCK: usize = 512;

pub const DRUM_CHANNEL: u8 = 9;

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
    /// A song bar starts; sent on every downbeat while a song plays.
    SongPosition { bar: u32 },
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
}

const NO_CHORD: u32 = 0xff;

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
            EngineEvent::SongPosition { bar } => [op::SONG_POSITION, bar, 0, frame],
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
