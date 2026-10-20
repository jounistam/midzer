use crate::chord::{Chord, Quality};

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
}

mod op {
    pub const NOTE_ON: u32 = 1;
    pub const NOTE_OFF: u32 = 2;
    pub const PROGRAM_CHANGE: u32 = 3;
    pub const CHORD_CHANGED: u32 = 4;
    pub const BEAT: u32 = 5;
    pub const TRANSPORT: u32 = 6;
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
        assert_eq!(words, [op::CHORD_CHANGED, 9, 0 << 8 | 11, 7]);
        assert_eq!(EngineEvent::ChordChanged(None).encode(0)[1], NO_CHORD);
    }
}
