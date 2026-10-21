use crate::band::Band;
use crate::chord::recognize;
use crate::command::Command;
use crate::event::{EngineEvent, EventBuffer, MIC_CHANNEL, Timed};
use crate::listen::{Heard, HeardList, ListenMode, Listener};

/// MIDI channel for the player's own notes.
pub const PLAYER_CHANNEL: u8 = 0;

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
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        match cmd {
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
            // Mixing is the synth's job; the host routes this one.
            Command::SetGain { .. } => {}
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

    /// Schedules this block and returns all of its events, sorted by frame.
    pub fn process(&mut self, frames: u32) -> &[Timed] {
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
