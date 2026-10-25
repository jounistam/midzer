use crate::band::BandDrums;
use crate::chord::{Chord, Quality};
use crate::conductor::{IdleMode, Request};
use crate::drums::{Instrument, KitAction, Piece, Trigger};
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
    /// How many beats song bar `bar` has (1–7); after `SongClear` every bar has four.
    SongBar { bar: u16, beats: u8 },
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
    /// A note from a drum source (the MIDI drum channel), played through the kit map.
    DrumNote { note: u8, velocity: u8 },
    /// A hit on a piece, from the on-screen or computer-keyboard pads.
    DrumHit { piece: Piece, velocity: u8 },
    /// Which piece `note` plays; `None` ignores it.
    KitNote { note: u8, piece: Option<Piece> },
    SetInstrument { instrument: Instrument },
    /// The hi-hat pedal position (MIDI CC 4) as the kit sends it.
    HiHatPedal { value: u8 },
    /// A cymbal grabbed to stop it ringing (polyphonic aftertouch on its note).
    DrumChoke { note: u8 },
    /// Whether the built-in synth sounds the player's drums; an e-kit's own module usually does.
    SetKitSound { on: bool },
    /// Drummer mode: the pieces whose four even hits count the band in, as a
    /// bit set of [`Piece::bit`]; 0 turns the count-in off.
    SetCountIn { pieces: u32 },
    /// What the band's own drums play when the player plays drums.
    SetBandDrums { mode: BandDrums },
    /// What a pad note or a controller on the kit does; `None` clears it.
    KitTrigger { trigger: Trigger, action: Option<KitAction> },
    /// A MIDI controller change, for kit triggers such as a foot switch.
    ControlChange { cc: u8, value: u8 },
    /// Whether songs play their recording (see [`crate::Engine::set_track`]) instead of the band.
    SetTrack { on: bool },
    /// Empties the recording's tempo map and sets how many beat times follow.
    TrackClear { beats: u16 },
    /// When song beat `index` is heard, in seconds into the recording.
    TrackBeat { index: u16, seconds: f32 },
    /// Whether the recording's separate drum stem is heard.
    SetTrackDrums { on: bool },
    /// Whether live audio (see [`crate::Engine::listen_live`]) is followed and
    /// the player's hits judged against its beats, instead of the band.
    SetLive { on: bool },
    /// How much later, in ms, the player hears the live source than the engine gets it.
    SetLiveOffset { ms: f32 },
    /// The live beat heard nearest to now is a 1.
    LiveDownbeat,
    /// The live beat is twice (or half) as fast as the one found.
    LiveTempo { double: bool },
    /// Where the live beats fall, as a listener that heard the last seconds
    /// says (`midzer-hear`): a beat at engine `frame` (its low 32 bits), the
    /// beat's length in frames, which beat of the bar it is if known (0 is
    /// the 1), how sure it is (0–255), and whether it is a stored song's
    /// known grid, to keep to rather than track.
    LiveGuide { frame: u32, period: f32, one: Option<u8>, confidence: u8, known: bool },
    /// Forgets everything heard live and starts listening afresh, as after silence.
    LiveReset,
    /// A bar of `beats` beats (1–7) starts at the live beat at engine `frame`
    /// (its low 32 bits), as a stored song followed live says; sent ahead.
    LiveBar { frame: u32, beats: u8 },
    /// Empties the song's tune.
    MelodyClear,
    /// A note of the song's tune: from song tick `tick`, `len` ticks long.
    MelodyNote { tick: u32, len: u32, key: u8 },
    /// How loud the tune plays under the band, 0 (not at all) to 1.
    SetMelody { volume: f32 },
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
    pub const DRUM_NOTE: u32 = 26;
    pub const DRUM_HIT: u32 = 27;
    pub const KIT_NOTE: u32 = 28;
    pub const SET_INSTRUMENT: u32 = 29;
    pub const HI_HAT_PEDAL: u32 = 30;
    pub const DRUM_CHOKE: u32 = 31;
    pub const SET_KIT_SOUND: u32 = 32;
    pub const SET_COUNT_IN: u32 = 33;
    pub const SET_BAND_DRUMS: u32 = 34;
    pub const KIT_TRIGGER: u32 = 35;
    pub const CONTROL_CHANGE: u32 = 36;
    pub const SET_TRACK: u32 = 37;
    pub const TRACK_CLEAR: u32 = 38;
    pub const TRACK_BEAT: u32 = 39;
    pub const SET_TRACK_DRUMS: u32 = 40;
    pub const SET_LIVE: u32 = 41;
    pub const SET_LIVE_OFFSET: u32 = 42;
    pub const LIVE_DOWNBEAT: u32 = 43;
    pub const LIVE_TEMPO: u32 = 44;
    pub const LIVE_GUIDE: u32 = 45;
    pub const LIVE_RESET: u32 = 46;
    pub const SONG_BAR: u32 = 47;
    pub const LIVE_BAR: u32 = 48;
    pub const MELODY_CLEAR: u32 = 49;
    pub const MELODY_NOTE: u32 = 50;
    pub const SET_MELODY: u32 = 51;
}

/// MIDI channel 10, where e-kits and drum pads send.
const MIDI_DRUM_CHANNEL: u8 = 9;
/// The MIDI controller for the hi-hat pedal (foot controller).
const HI_HAT_CC: u8 = 4;

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
const INSTRUMENTS: [Instrument; 2] = [Instrument::Keys, Instrument::Drums];

/// Triggers travel as `[kind, number]`, kind 0 a note and 1 a controller.
fn trigger_words(trigger: Trigger) -> (u32, u32) {
    match trigger {
        Trigger::Note(n) => (0, n as u32),
        Trigger::Control(c) => (1, c as u32),
    }
}

fn index_of<T: PartialEq>(all: &[T], item: T) -> u32 {
    all.iter().position(|x| *x == item).unwrap_or(0) as u32
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
            Command::SongBar { bar, beats } => [op::SONG_BAR, bar as u32, beats as u32, 0],
            Command::SongLoop { start, end } => [op::SONG_LOOP, start as u32, end as u32, 0],
            Command::SetSong { on } => [op::SET_SONG, on as u32, 0, 0],
            Command::SongSeek { bar } => [op::SONG_SEEK, bar as u32, 0, 0],
            Command::SongWait { tick } => [op::SONG_WAIT, tick.is_some() as u32, tick.unwrap_or(0), 0],
            Command::SetListen { mode, gate_db } => {
                let mode = ListenMode::ALL.iter().position(|m| *m == mode).unwrap_or(0) as u32;
                [op::SET_LISTEN, mode, gate_db.to_bits(), 0]
            }
            Command::SetTempoFollow { on } => [op::SET_TEMPO_FOLLOW, on as u32, 0, 0],
            Command::DrumNote { note, velocity } => [op::DRUM_NOTE, note as u32, velocity as u32, 0],
            Command::DrumHit { piece, velocity } => [op::DRUM_HIT, piece.index() as u32, velocity as u32, 0],
            // Pieces travel as index + 1, with 0 for none.
            Command::KitNote { note, piece } => [op::KIT_NOTE, note as u32, piece.map_or(0, |p| p.index() as u32 + 1), 0],
            Command::SetInstrument { instrument } => {
                [op::SET_INSTRUMENT, INSTRUMENTS.iter().position(|i| *i == instrument).unwrap_or(0) as u32, 0, 0]
            }
            Command::HiHatPedal { value } => [op::HI_HAT_PEDAL, value as u32, 0, 0],
            Command::DrumChoke { note } => [op::DRUM_CHOKE, note as u32, 0, 0],
            Command::SetKitSound { on } => [op::SET_KIT_SOUND, on as u32, 0, 0],
            Command::SetCountIn { pieces } => [op::SET_COUNT_IN, pieces, 0, 0],
            Command::SetBandDrums { mode } => [op::SET_BAND_DRUMS, index_of(&BandDrums::ALL, mode), 0, 0],
            // Actions travel as index + 1, with 0 for none.
            Command::KitTrigger { trigger, action } => {
                let (kind, number) = trigger_words(trigger);
                [op::KIT_TRIGGER, kind, number, action.map_or(0, |a| index_of(&KitAction::ALL, a) + 1)]
            }
            Command::ControlChange { cc, value } => [op::CONTROL_CHANGE, cc as u32, value as u32, 0],
            Command::SetTrack { on } => [op::SET_TRACK, on as u32, 0, 0],
            Command::TrackClear { beats } => [op::TRACK_CLEAR, beats as u32, 0, 0],
            Command::TrackBeat { index, seconds } => [op::TRACK_BEAT, index as u32, seconds.to_bits(), 0],
            Command::SetTrackDrums { on } => [op::SET_TRACK_DRUMS, on as u32, 0, 0],
            Command::SetLive { on } => [op::SET_LIVE, on as u32, 0, 0],
            Command::SetLiveOffset { ms } => [op::SET_LIVE_OFFSET, ms.to_bits(), 0, 0],
            Command::LiveDownbeat => [op::LIVE_DOWNBEAT, 0, 0, 0],
            Command::LiveTempo { double } => [op::LIVE_TEMPO, double as u32, 0, 0],
            Command::LiveReset => [op::LIVE_RESET, 0, 0, 0],
            Command::LiveBar { frame, beats } => [op::LIVE_BAR, frame, beats as u32, 0],
            Command::MelodyClear => [op::MELODY_CLEAR, 0, 0, 0],
            Command::MelodyNote { tick, len, key } => [op::MELODY_NOTE, tick, len, key as u32],
            Command::SetMelody { volume } => [op::SET_MELODY, volume.to_bits(), 0, 0],
            Command::LiveGuide { frame, period, one, confidence, known } => [op::LIVE_GUIDE, frame, period.to_bits(), one.map_or(0, |o| o as u32 + 1) | (confidence as u32) << 8 | (known as u32) << 16],
        }
    }

    /// Translates a raw MIDI message from a controller; mirrors `midiToCommand` in
    /// `packages/audio-web/src/midi.ts`.
    pub fn from_midi(data: &[u8]) -> Option<Self> {
        let (status, d1, d2) = (*data.first()?, data.get(1).copied().unwrap_or(0), data.get(2).copied().unwrap_or(0));
        let drums = status & 0x0f == MIDI_DRUM_CHANNEL;
        match status & 0xf0 {
            0x90 if d2 > 0 && d1 < 128 && drums => Some(Command::DrumNote { note: d1, velocity: d2.min(127) }),
            0x90 if d2 > 0 && d1 < 128 => Some(Command::NoteOn { note: d1, velocity: d2.min(127) }),
            // Drum hits don't hold: their releases mean nothing.
            0x80 | 0x90 if drums => None,
            0x80 | 0x90 if d1 < 128 => Some(Command::NoteOff { note: d1 }),
            0xa0 if d2 > 0 && d1 < 128 => Some(Command::DrumChoke { note: d1 }),
            0xb0 if d1 == HI_HAT_CC => Some(Command::HiHatPedal { value: d2.min(127) }),
            // All Sound Off / All Notes Off
            0xb0 if d1 == 120 || d1 == 123 => Some(Command::AllNotesOff),
            0xb0 if d1 < 120 => Some(Command::ControlChange { cc: d1, value: d2.min(127) }),
            _ => None,
        }
    }

    /// Decodes a record, returning `None` for unknown opcodes or out-of-range values.
    pub fn decode(words: [u32; WORDS_PER_COMMAND]) -> Option<Self> {
        let [opcode, a, b, c] = words;
        let short = |v: u32| u16::try_from(v).ok();
        let midi = |v: u32| u8::try_from(v).ok().filter(|v| *v < 128);
        let float = |v: u32| Some(f32::from_bits(v)).filter(|f| f.is_finite());
        let bar_beats = |v: u32| u8::try_from(v).ok().filter(|b| (1..=crate::song::MAX_BAR_BEATS as u8).contains(b));
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
            op::SONG_BAR => Some(Command::SongBar { bar: short(a)?, beats: bar_beats(b)? }),
            op::SONG_LOOP => Some(Command::SongLoop { start: short(a)?, end: short(b)? }),
            op::SET_SONG => Some(Command::SetSong { on: a != 0 }),
            op::SONG_SEEK => Some(Command::SongSeek { bar: short(a)? }),
            op::SONG_WAIT => Some(Command::SongWait { tick: (a != 0).then_some(b) }),
            op::SET_LISTEN => Some(Command::SetListen { mode: *ListenMode::ALL.get(a as usize)?, gate_db: float(b)? }),
            op::SET_TEMPO_FOLLOW => Some(Command::SetTempoFollow { on: a != 0 }),
            op::DRUM_NOTE => Some(Command::DrumNote { note: midi(a)?, velocity: midi(b)? }),
            op::DRUM_HIT => Some(Command::DrumHit { piece: Piece::from_index(a as usize)?, velocity: midi(b)? }),
            op::KIT_NOTE => Some(Command::KitNote {
                note: midi(a)?,
                piece: if b == 0 { None } else { Some(Piece::from_index(b as usize - 1)?) },
            }),
            op::SET_INSTRUMENT => Some(Command::SetInstrument { instrument: *INSTRUMENTS.get(a as usize)? }),
            op::HI_HAT_PEDAL => Some(Command::HiHatPedal { value: midi(a)? }),
            op::DRUM_CHOKE => Some(Command::DrumChoke { note: midi(a)? }),
            op::SET_KIT_SOUND => Some(Command::SetKitSound { on: a != 0 }),
            op::SET_COUNT_IN => Some(Command::SetCountIn { pieces: a & ((1 << Piece::COUNT) - 1) }),
            op::SET_BAND_DRUMS => Some(Command::SetBandDrums { mode: *BandDrums::ALL.get(a as usize)? }),
            op::KIT_TRIGGER => Some(Command::KitTrigger {
                trigger: match a {
                    0 => Trigger::Note(midi(b)?),
                    1 => Trigger::Control(midi(b)?),
                    _ => return None,
                },
                action: if c == 0 { None } else { Some(*KitAction::ALL.get(c as usize - 1)?) },
            }),
            op::CONTROL_CHANGE => Some(Command::ControlChange { cc: midi(a)?, value: midi(b)? }),
            op::SET_TRACK => Some(Command::SetTrack { on: a != 0 }),
            op::TRACK_CLEAR => Some(Command::TrackClear { beats: short(a)? }),
            op::TRACK_BEAT => Some(Command::TrackBeat { index: short(a)?, seconds: float(b).filter(|s| *s >= 0.0)? }),
            op::SET_TRACK_DRUMS => Some(Command::SetTrackDrums { on: a != 0 }),
            op::SET_LIVE => Some(Command::SetLive { on: a != 0 }),
            op::SET_LIVE_OFFSET => Some(Command::SetLiveOffset { ms: float(a).filter(|ms| ms.abs() <= 1000.0)? }),
            op::LIVE_DOWNBEAT => Some(Command::LiveDownbeat),
            op::LIVE_TEMPO => Some(Command::LiveTempo { double: a != 0 }),
            op::LIVE_RESET => Some(Command::LiveReset),
            op::LIVE_BAR => Some(Command::LiveBar { frame: a, beats: bar_beats(b)? }),
            op::MELODY_CLEAR => Some(Command::MelodyClear),
            op::MELODY_NOTE => Some(Command::MelodyNote { tick: a, len: b, key: midi(c)? }),
            op::SET_MELODY => Some(Command::SetMelody { volume: float(a).filter(|v| (0.0..=1.0).contains(v))? }),
            op::LIVE_GUIDE => Some(Command::LiveGuide {
                frame: a,
                period: float(b).filter(|p| *p > 0.0)?,
                one: match c & 0xff {
                    0 => None,
                    o @ 1..=4 => Some(o as u8 - 1),
                    _ => return None,
                },
                confidence: (c >> 8) as u8,
                known: c >> 16 & 1 != 0,
            }),
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
            Command::DrumNote { note: 38, velocity: 127 },
            Command::DrumHit { piece: Piece::China, velocity: 1 },
            Command::DrumHit { piece: Piece::Kick, velocity: 90 },
            Command::KitNote { note: 26, piece: Some(Piece::HiHatOpen) },
            Command::KitNote { note: 0, piece: Some(Piece::Kick) },
            Command::KitNote { note: 127, piece: None },
            Command::SetInstrument { instrument: Instrument::Drums },
            Command::SetInstrument { instrument: Instrument::Keys },
            Command::HiHatPedal { value: 64 },
            Command::DrumChoke { note: 49 },
            Command::SetKitSound { on: true },
            Command::SetCountIn { pieces: Piece::HiHatClosed.bit() | Piece::SideStick.bit() },
            Command::SetCountIn { pieces: 0 },
            Command::SetBandDrums { mode: BandDrums::Percussion },
            Command::SetBandDrums { mode: BandDrums::Full },
            Command::KitTrigger { trigger: Trigger::Note(60), action: Some(KitAction::NextSection) },
            Command::KitTrigger { trigger: Trigger::Control(64), action: Some(KitAction::StartStop) },
            Command::KitTrigger { trigger: Trigger::Control(127), action: None },
            Command::ControlChange { cc: 64, value: 127 },
            Command::SetTrack { on: true },
            Command::TrackClear { beats: 2049 },
            Command::TrackBeat { index: 2048, seconds: 312.625 },
            Command::SetTrackDrums { on: false },
            Command::SetLive { on: true },
            Command::SetLiveOffset { ms: -35.5 },
            Command::LiveDownbeat,
            Command::LiveTempo { double: true },
            Command::LiveReset,
            Command::SongBar { bar: 511, beats: 7 },
            Command::SongBar { bar: 0, beats: 1 },
            Command::LiveBar { frame: 4_000_000_000, beats: 3 },
            Command::LiveGuide { frame: 4_000_000_000, period: 22_050.5, one: Some(3), confidence: 200, known: true },
            Command::LiveGuide { frame: 7, period: 24_000.0, one: None, confidence: 0, known: false },
            Command::MelodyClear,
            Command::MelodyNote { tick: 1_000_000, len: 240, key: 72 },
            Command::SetMelody { volume: 0.4 },
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
        assert_eq!(Command::from_midi(&[0xb0, 64, 127]), Some(Command::ControlChange { cc: 64, value: 127 }));
        assert_eq!(Command::from_midi(&[0xb0, 121, 0]), None);
        // Channel 10 is drums: hits only, releases ignored.
        assert_eq!(Command::from_midi(&[0x99, 38, 120]), Some(Command::DrumNote { note: 38, velocity: 120 }));
        assert_eq!(Command::from_midi(&[0x89, 38, 0]), None);
        assert_eq!(Command::from_midi(&[0x99, 38, 0]), None);
        assert_eq!(Command::from_midi(&[0xb9, 4, 90]), Some(Command::HiHatPedal { value: 90 }));
        assert_eq!(Command::from_midi(&[0xa9, 49, 127]), Some(Command::DrumChoke { note: 49 }));
        assert_eq!(Command::from_midi(&[0xa9, 49, 0]), None);
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
        assert_eq!(Command::decode([op::DRUM_HIT, Piece::COUNT as u32, 100, 0]), None);
        assert_eq!(Command::decode([op::KIT_NOTE, 36, Piece::COUNT as u32 + 1, 0]), None);
        assert_eq!(Command::decode([op::SET_INSTRUMENT, 2, 0, 0]), None);
        assert_eq!(Command::decode([op::SET_BAND_DRUMS, 4, 0, 0]), None);
        assert_eq!(Command::decode([op::KIT_TRIGGER, 2, 60, 1]), None);
        assert_eq!(Command::decode([op::KIT_TRIGGER, 0, 128, 1]), None);
        assert_eq!(Command::decode([op::KIT_TRIGGER, 0, 60, 5]), None);
        assert_eq!(Command::decode([op::CONTROL_CHANGE, 128, 0, 0]), None);
    }
}
