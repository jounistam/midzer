use crate::band::Band;
use crate::chord::recognize;
use crate::command::Command;
use crate::drums::{Instrument, Kit, Piece};
use crate::event::{EngineEvent, EventBuffer, MIC_CHANNEL, PLAYER_DRUM_CHANNEL, Timed};
use crate::listen::{Heard, HeardList, ListenMode, Listener};

/// MIDI channel for the player's own notes.
pub const PLAYER_CHANNEL: u8 = 0;
/// How long a drum hit sounds before its release, in seconds. Kits send their
/// own releases right after the hit, which would cut cymbals short.
const DRUM_GATE_S: f32 = 0.1;

/// The engine for one session: player input, chord detection and the band.
///
/// Per render block the host calls [`Engine::handle`] for each queued command,
/// then [`Engine::process`] once and plays the returned events, then
/// [`Engine::end_block`]. Nothing here allocates after construction.
pub struct Engine {
    band: Band,
    held: [bool; 128],
    /// Only notes below this key form chords; 0 means the whole keyboard.
    split: u8,
    events: EventBuffer,
    listener: Listener,
    /// Keys the microphone reports as sounding, on [`MIC_CHANNEL`].
    mic_held: [bool; 128],
    instrument: Instrument,
    kit: Kit,
    /// Frames until each piece's last hit is released; 0 once it is.
    drum_gate: [u32; Piece::COUNT],
    gate_frames: u32,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            band: Band::new(sample_rate),
            held: [false; 128],
            split: 0,
            events: EventBuffer::default(),
            listener: Listener::new(sample_rate),
            mic_held: [false; 128],
            instrument: Instrument::Keys,
            kit: Kit::default(),
            drum_gate: [0; Piece::COUNT],
            gate_frames: (sample_rate * DRUM_GATE_S) as u32,
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        let drums = self.instrument == Instrument::Drums;
        match cmd {
            Command::NoteOn { note, velocity } if drums => self.drum_note(note, velocity),
            Command::NoteOff { .. } if drums => {}
            Command::NoteOn { note, velocity: 0 } | Command::NoteOff { note } => {
                self.held[note as usize] = false;
                self.events.push(0, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key: note });
                self.update_keys_held();
            }
            Command::NoteOn { note, velocity } => {
                self.held[note as usize] = true;
                self.events.push(0, EngineEvent::NoteOn { channel: PLAYER_CHANNEL, key: note, velocity });
                self.band.note_played(velocity);
                self.band.onset(0.0);
                self.band.set_keys_held(true);
                self.detect_chord();
            }
            Command::AllNotesOff => {
                for note in 0..128u8 {
                    if std::mem::take(&mut self.held[note as usize]) {
                        self.events.push(0, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key: note });
                    }
                }
                self.update_keys_held();
            }
            Command::Play => self.band.play(&mut self.events),
            Command::Stop => self.band.stop(&mut self.events),
            Command::SetTempo { bpm } => self.band.set_tempo(bpm),
            Command::SetSplit { note } => self.split = note,
            Command::SetMute { channel, muted } => self.band.set_muted(channel, muted, &mut self.events),
            Command::SetStyle { index } => self.band.set_style(index, &mut self.events),
            Command::Section { request } => self.band.request(request, &mut self.events),
            Command::SetFollow { on } => self.band.set_follow(on, &mut self.events),
            Command::SetEnergy { energy } => self.band.set_energy(energy, &mut self.events),
            Command::SetIdleMode { mode } => self.band.set_idle_mode(mode),
            Command::SetLatency { ms } => self.band.set_latency(ms),
            Command::SetClick { on } => self.band.set_click(on, &mut self.events),
            Command::SongClear { bars } => self.band.clear_song(bars as u32),
            Command::SongChord { beat, slot } => self.band.set_song_slot(beat as u32, slot),
            Command::SongMark { bar, mark } => self.band.set_song_mark(bar as u32, mark),
            Command::SongLoop { start, end } => self.band.set_song_loop(start as u32, end as u32),
            Command::SetSong { on } => self.band.set_song(on),
            Command::SongSeek { bar } => self.band.seek_song(bar as u32),
            Command::SongWait { tick } => self.band.set_song_wait(tick),
            Command::SetListen { mode, gate_db } => {
                let mut heard = HeardList::default();
                self.listener.set_gate(gate_db);
                self.listener.set_mode(mode, &mut heard);
                self.on_heard(&heard);
            }
            Command::SetTempoFollow { on } => self.band.set_tempo_follow(on, &mut self.events),
            Command::DrumNote { note, velocity } => self.drum_note(note, velocity),
            Command::DrumHit { piece, velocity } => self.drum_hit(piece, velocity),
            Command::KitNote { note, piece } => self.kit.set(note, piece),
            Command::SetInstrument { instrument } => {
                if instrument != self.instrument {
                    self.handle(Command::AllNotesOff);
                    self.instrument = instrument;
                }
            }
            Command::DrumChoke { note } => {
                if let Some(piece) = self.kit.piece(note) {
                    self.release_drum(piece, 0);
                }
            }
            // Kits send their own open and closed hi-hat notes; the pedal is for the UI.
            Command::HiHatPedal { .. } => {}
            // Mixing is the synth's job; the host routes these.
            Command::SetGain { .. } | Command::SetKitSound { .. } => {}
        }
    }

    /// Feeds this block's microphone input, before [`Engine::process`].
    pub fn listen(&mut self, input: &[f32]) {
        if self.listener.mode() == ListenMode::Off {
            return;
        }
        let mut heard = HeardList::default();
        self.listener.push(input, &mut heard);
        self.on_heard(&heard);
    }

    fn on_heard(&mut self, heard: &HeardList) {
        let late_ms = self.listener.mode().delay_ms();
        for item in heard.as_slice() {
            match *item {
                Heard::NoteOn { key, velocity } => {
                    self.mic_note(key, Some(velocity));
                    self.band.note_played(velocity);
                    self.band.onset(late_ms);
                }
                Heard::NoteOff { key } => self.mic_note(key, None),
                Heard::Onset { velocity } => {
                    self.band.note_played(velocity);
                    self.band.onset(late_ms);
                }
                Heard::Chord { chord, velocity } => {
                    self.release_mic();
                    // Reported in root position around middle C, for the keyboard and Learn.
                    for pc in chord.pitch_classes() {
                        self.mic_note(60 + pc, Some(velocity));
                    }
                    self.band.set_chord(chord, late_ms, &mut self.events);
                }
                Heard::Silence => self.release_mic(),
                Heard::Level { level, key } => self.events.push(0, EngineEvent::Input { level, key }),
            }
        }
        self.update_keys_held();
    }

    fn mic_note(&mut self, key: u8, velocity: Option<u8>) {
        let key = key.min(127);
        self.mic_held[key as usize] = velocity.is_some();
        let event = match velocity {
            Some(velocity) => EngineEvent::NoteOn { channel: MIC_CHANNEL, key, velocity },
            None => EngineEvent::NoteOff { channel: MIC_CHANNEL, key },
        };
        self.events.push(0, event);
    }

    fn release_mic(&mut self) {
        for key in 0..128u8 {
            if self.mic_held[key as usize] {
                self.mic_note(key, None);
            }
        }
    }

    /// Held keys and a ringing mic note both count as playing.
    fn update_keys_held(&mut self) {
        self.band.set_keys_held(self.held.contains(&true) || self.listener.sounding());
    }

    fn drum_note(&mut self, note: u8, velocity: u8) {
        if let Some(piece) = self.kit.piece(note) {
            self.drum_hit(piece, velocity);
        }
    }

    /// Sounds a hit on [`PLAYER_DRUM_CHANNEL`] and counts it as playing.
    fn drum_hit(&mut self, piece: Piece, velocity: u8) {
        if velocity == 0 {
            return;
        }
        self.release_drum(piece, 0);
        self.drum_gate[piece.index()] = self.gate_frames;
        self.events.push(0, EngineEvent::NoteOn { channel: PLAYER_DRUM_CHANNEL, key: piece.note(), velocity });
        self.band.note_played(velocity);
        self.band.onset(0.0);
    }

    fn release_drum(&mut self, piece: Piece, frame: u32) {
        if std::mem::take(&mut self.drum_gate[piece.index()]) > 0 {
            self.events.push(frame, EngineEvent::NoteOff { channel: PLAYER_DRUM_CHANNEL, key: piece.note() });
        }
    }

    /// Releases the hits whose gate ends in this block.
    fn release_drums(&mut self, frames: u32) {
        for piece in Piece::ALL {
            match self.drum_gate[piece.index()] {
                0 => {}
                left if left < frames => self.release_drum(piece, left),
                left => self.drum_gate[piece.index()] = left - frames,
            }
        }
    }

    /// Schedules this block and returns all of its events, sorted by frame.
    pub fn process(&mut self, frames: u32) -> &[Timed] {
        self.release_drums(frames);
        self.band.process(frames, &mut self.events);
        self.events.sort();
        self.events.as_slice()
    }

    pub fn end_block(&mut self) {
        self.events.clear();
    }

    pub fn is_playing(&self) -> bool {
        self.band.is_playing()
    }

    pub fn chord(&self) -> Option<crate::chord::Chord> {
        self.band.chord()
    }

    /// Recognises the chord on a new key press. Releasing keys never changes
    /// the chord: the band keeps the last one, like an arranger keyboard.
    fn detect_chord(&mut self) {
        let limit = if self.split == 0 { 128 } else { self.split as usize };
        let mut notes = [0u8; 128];
        let mut len = 0;
        for note in (0..limit).filter(|&n| self.held[n]) {
            notes[len] = note as u8;
            len += 1;
        }
        if let Some(chord) = recognize(&notes[..len]) {
            self.band.set_chord(chord, 0.0, &mut self.events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::{Chord, Quality};

    fn chord_events(events: &[Timed]) -> Vec<Option<Chord>> {
        events
            .iter()
            .filter_map(|t| match t.event {
                EngineEvent::ChordChanged(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn player_notes_pass_through_and_form_chords() {
        let mut engine = Engine::new(48_000.0);
        for note in [57, 60, 64] {
            engine.handle(Command::NoteOn { note, velocity: 90 });
        }
        let events = engine.process(128);
        assert_eq!(events.iter().filter(|t| matches!(t.event, EngineEvent::NoteOn { channel: 0, .. })).count(), 3);
        assert_eq!(chord_events(events).last(), Some(&Some(Chord::new(9, Quality::Minor))));
    }

    #[test]
    fn releasing_keys_keeps_the_chord() {
        let mut engine = Engine::new(48_000.0);
        for note in [60, 64, 67] {
            engine.handle(Command::NoteOn { note, velocity: 90 });
        }
        engine.process(128);
        engine.end_block();
        for note in [60, 64, 67] {
            engine.handle(Command::NoteOff { note });
        }
        assert!(chord_events(engine.process(128)).is_empty());
    }

    #[test]
    fn split_ignores_melody_notes() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::SetSplit { note: 60 });
        for note in [48, 52, 55, 71, 74] {
            engine.handle(Command::NoteOn { note, velocity: 90 });
        }
        assert_eq!(chord_events(engine.process(128)).last(), Some(&Some(Chord::new(0, Quality::Major))));
    }

    fn drum_events(events: &[Timed]) -> Vec<(u32, bool, u8)> {
        events
            .iter()
            .filter_map(|t| match t.event {
                EngineEvent::NoteOn { channel: PLAYER_DRUM_CHANNEL, key, .. } => Some((t.frame, true, key)),
                EngineEvent::NoteOff { channel: PLAYER_DRUM_CHANNEL, key } => Some((t.frame, false, key)),
                _ => None,
            })
            .collect()
    }

    /// Runs blocks until `frames` have passed, collecting the player's drum events.
    fn run_drums(engine: &mut Engine, frames: u32) -> Vec<(u32, bool, u8)> {
        let mut out = Vec::new();
        for block in 0..frames / 128 {
            out.extend(drum_events(engine.process(128)).into_iter().map(|(f, on, key)| (block * 128 + f, on, key)));
            engine.end_block();
        }
        out
    }

    #[test]
    fn drum_channel_notes_are_hits_released_after_the_gate() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::DrumNote { note: 38, velocity: 100 });
        // The kit's own release is ignored; the gate releases 100 ms later.
        engine.handle(Command::NoteOff { note: 38 });
        let events = run_drums(&mut engine, 9_600);
        assert_eq!(events, vec![(0, true, 38), (4_800, false, 38)]);
        assert!(chord_events(engine.process(128)).is_empty());
    }

    #[test]
    fn drums_mode_plays_every_note_through_the_kit() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::SetInstrument { instrument: Instrument::Drums });
        engine.handle(Command::KitNote { note: 26, piece: Some(Piece::HiHatOpen) });
        engine.handle(Command::KitNote { note: 36, piece: None });
        for note in [26, 36, 60, 64, 67] {
            engine.handle(Command::NoteOn { note, velocity: 90 });
        }
        let events = engine.process(128);
        assert!(chord_events(events).is_empty(), "drums form no chords");
        assert!(!events.iter().any(|t| matches!(t.event, EngineEvent::NoteOn { channel: PLAYER_CHANNEL, .. })));
        assert_eq!(drum_events(events), vec![(0, true, Piece::HiHatOpen.note())]);
    }

    #[test]
    fn retriggers_release_first_and_chokes_cut_the_hit() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::DrumHit { piece: Piece::Crash, velocity: 110 });
        assert_eq!(run_drums(&mut engine, 128), vec![(0, true, 49)]);
        engine.handle(Command::DrumHit { piece: Piece::Crash, velocity: 80 });
        assert_eq!(run_drums(&mut engine, 128), vec![(0, false, 49), (0, true, 49)]);
        engine.handle(Command::DrumChoke { note: 49 });
        assert_eq!(run_drums(&mut engine, 9_600), vec![(0, false, 49)], "choked, so no gate release later");
        engine.handle(Command::DrumHit { piece: Piece::Kick, velocity: 0 });
        assert!(run_drums(&mut engine, 128).is_empty());
    }

    #[test]
    fn end_block_clears_events() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::Play);
        assert!(!engine.process(128).is_empty());
        engine.end_block();
        engine.handle(Command::Stop);
        engine.end_block();
        assert!(engine.process(128).is_empty());
    }

    #[test]
    fn microphone_chords_set_the_harmony_and_show_as_notes() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::SetListen { mode: ListenMode::Chords, gate_db: -50.0 });
        let hz = |key: f32| 440.0 * 2f32.powf((key - 69.0) / 12.0);
        let mut chords = Vec::new();
        let mut mic_keys = Vec::new();
        for block in 0..300 {
            let input: Vec<f32> = (0..128)
                .map(|i| {
                    let t = (block * 128 + i) as f32 / 48_000.0;
                    [48.0, 55.0, 60.0, 64.0, 67.0]
                        .iter()
                        .map(|&k| (2.0 * std::f32::consts::PI * hz(k) * t).sin() * 0.1)
                        .sum()
                })
                .collect();
            engine.listen(&input);
            for timed in engine.process(128) {
                match timed.event {
                    EngineEvent::ChordChanged(c) => chords.push(c),
                    EngineEvent::NoteOn { channel: MIC_CHANNEL, key, .. } => mic_keys.push(key),
                    _ => {}
                }
            }
            engine.end_block();
        }
        assert_eq!(chords, [Some(Chord::new(0, Quality::Major))]);
        assert_eq!(mic_keys, [60, 64, 67]);
        engine.handle(Command::SetListen { mode: ListenMode::Off, gate_db: -50.0 });
        let offs = engine.process(128).iter().filter(|t| matches!(t.event, EngineEvent::NoteOff { channel: MIC_CHANNEL, .. })).count();
        assert_eq!(offs, 3, "turning the mic off releases its notes");
    }
}
