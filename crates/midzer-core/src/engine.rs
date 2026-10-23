use crate::backing::{Backing, Track};
use crate::band::{Band, BandDrums};
use crate::chord::recognize;
use crate::command::Command;
use crate::conductor::Request;
use crate::drummer::{CountIn, CountInListener};
use crate::drums::{Instrument, Kit, KitAction, Piece, Triggers};
use crate::event::{EngineEvent, EventBuffer, MIC_CHANNEL, PLAYER_DRUM_CHANNEL, Timed};
use crate::listen::{Heard, HeardList, ListenMode, Listener};
use crate::live::Live;

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
/// Keys notes within this many ms of a strike's first note belong to it (a chord).
const STRIKE_MS: f32 = 35.0;

pub struct Engine {
    band: Band,
    held: [bool; 128],
    /// A keys strike being gathered while a recording or live source plays:
    /// the frame of its first note and the pitch classes so far.
    strike: Option<(u64, u16)>,
    strike_frames: u64,
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
    sample_rate: f32,
    /// Frames since the engine started, for timing hits while the band is stopped.
    now: u64,
    triggers: Triggers,
    count_in: CountInListener,
    /// A counted-in start: frames from the next block's start, and the tempo.
    start_in: Option<(u64, f32)>,
    /// What the band's drums play for a drummer.
    band_drums: BandDrums,
    /// A recording that plays songs instead of the band.
    backing: Backing,
    /// Live audio the player plays along with, instead of the band.
    live: Live,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            band: Band::new(sample_rate),
            held: [false; 128],
            strike: None,
            strike_frames: (STRIKE_MS / 1000.0 * sample_rate) as u64,
            split: 0,
            events: EventBuffer::default(),
            listener: Listener::new(sample_rate),
            mic_held: [false; 128],
            instrument: Instrument::Keys,
            kit: Kit::default(),
            drum_gate: [0; Piece::COUNT],
            gate_frames: (sample_rate * DRUM_GATE_S) as u32,
            sample_rate,
            now: 0,
            triggers: Triggers::default(),
            count_in: CountInListener::default(),
            start_in: None,
            band_drums: BandDrums::Off,
            backing: Backing::new(sample_rate),
            live: Live::new(sample_rate),
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        let drums = self.instrument == Instrument::Drums;
        let track = self.backing.active();
        let live = self.live.active();
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
                if track || live {
                    self.strike_note(note);
                }
            }
            Command::AllNotesOff => {
                for note in 0..128u8 {
                    if std::mem::take(&mut self.held[note as usize]) {
                        self.events.push(0, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key: note });
                    }
                }
                self.update_keys_held();
            }
            Command::Play if live => self.live.play(self.now, &mut self.events),
            Command::Play if track => self.backing.play(self.band.bpm(), &mut self.events),
            Command::Play => {
                self.start_in = None;
                self.band.play(&mut self.events);
            }
            Command::Stop => {
                self.start_in = None;
                self.band.stop(&mut self.events);
                if self.backing.is_playing() {
                    self.backing.stop(0, &mut self.events);
                    self.events.push(0, EngineEvent::Tempo { bpm: self.band.bpm() });
                }
                self.live.stop(0, &mut self.events);
            }
            Command::SetTempo { bpm } => self.band.set_tempo(bpm),
            Command::SetSplit { note } => self.split = note,
            Command::SetMute { channel, muted } => self.band.set_muted(channel, muted, &mut self.events),
            Command::SetStyle { index } => self.band.set_style(index, &mut self.events),
            // The recording or the live source has its own arrangement.
            Command::Section { .. } if track || live => {}
            Command::Section { request } => self.band.request(request, &mut self.events),
            Command::SetFollow { on } => self.band.set_follow(on, &mut self.events),
            Command::SetEnergy { energy } => self.band.set_energy(energy, &mut self.events),
            Command::SetIdleMode { mode } => self.band.set_idle_mode(mode),
            Command::SetLatency { ms } => self.band.set_latency(ms),
            Command::SetClick { on } => self.band.set_click(on, &mut self.events),
            Command::SongClear { bars } => self.band.clear_song(bars as u32),
            Command::SongChord { beat, slot } => self.band.set_song_slot(beat as u32, slot),
            Command::SongMark { bar, mark } => self.band.set_song_mark(bar as u32, mark),
            Command::SongLoop { start, end } => {
                self.band.set_song_loop(start as u32, end as u32);
                self.backing.set_loop(start as u32, end as u32);
            }
            Command::SetSong { on } => self.band.set_song(on),
            Command::SongSeek { bar } => {
                self.band.seek_song(bar as u32);
                self.backing.seek(bar as u32);
            }
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
                    self.update_drummer();
                    // The band may have reported its own tempo; the live source's stands.
                    self.live.retell_tempo();
                }
            }
            Command::DrumChoke { note } => {
                if let Some(piece) = self.kit.piece(note) {
                    self.release_drum(piece, 0);
                    // Grabbing a cymbal stops the band dead.
                    if drums && piece.is_cymbal() {
                        self.handle(Command::Stop);
                    }
                }
            }
            Command::SetCountIn { pieces } => {
                self.count_in.set_pieces(pieces);
                self.events.push(0, EngineEvent::CountIn { hits: 0 });
            }
            Command::SetBandDrums { mode } => {
                self.band_drums = mode;
                self.update_drummer();
            }
            Command::KitTrigger { trigger, action } => self.triggers.set(trigger, action),
            Command::ControlChange { cc, value } => {
                if let Some(action) = self.triggers.control(cc, value) {
                    self.kit_action(action);
                }
            }
            Command::SetTrack { on } => {
                if on != track {
                    self.handle(Command::Stop);
                    if on && live {
                        self.handle(Command::SetLive { on: false });
                    }
                    self.backing.set_active(on, &mut self.events);
                }
            }
            Command::SetLive { on } => {
                if on != live {
                    self.handle(Command::Stop);
                    if on && track {
                        self.handle(Command::SetTrack { on: false });
                    }
                    self.live.set_active(on, &mut self.events);
                    if !on {
                        self.events.push(0, EngineEvent::Tempo { bpm: self.band.bpm() });
                    }
                }
            }
            Command::SetLiveOffset { ms } => self.live.set_offset(ms),
            Command::LiveDownbeat => self.live.set_downbeat(self.now),
            Command::LiveTempo { double } => self.live.scale_tempo(double),
            Command::LiveGuide { frame, period, one, confidence } => self.live.guide(frame, period, one, confidence, self.now),
            Command::TrackClear { beats } => self.backing.clear_beats(beats as u32),
            Command::TrackBeat { index, seconds } => self.backing.set_beat(index as u32, seconds),
            Command::SetTrackDrums { on } => self.backing.set_drums(on),
            // Kits send their own open and closed hi-hat notes; the pedal is for the UI.
            Command::HiHatPedal { .. } => {}
            // The synth's gain is the host's to set; the recording follows it.
            Command::SetGain { gain } => self.backing.set_gain(gain),
            Command::SetKitSound { .. } => {}
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

    /// The engine frame of the next block's first sample.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Feeds this block of the live source (see [`Command::SetLive`]), before [`Engine::process`].
    pub fn listen_live(&mut self, input: &[f32]) {
        self.live.listen(input, self.now, &mut self.events);
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

    /// Drummer mode follows the instrument; the band's drum layer only applies to it.
    fn update_drummer(&mut self) {
        let drums = self.instrument == Instrument::Drums;
        self.band.set_drummer(drums, &mut self.events);
        self.band.set_drum_layer(if drums { self.band_drums } else { BandDrums::Full }, &mut self.events);
        self.count_in.reset();
        self.start_in = None;
    }

    fn drum_note(&mut self, note: u8, velocity: u8) {
        if let Some(action) = self.triggers.note(note) {
            self.kit_action(action);
        } else if let Some(piece) = self.kit.piece(note) {
            self.drum_hit(piece, velocity);
        }
    }

    fn kit_action(&mut self, action: KitAction) {
        let playing = self.is_playing();
        if (self.backing.active() || self.live.active()) && action != KitAction::StartStop {
            return;
        }
        match action {
            KitAction::StartStop => self.handle(if playing { Command::Stop } else { Command::Play }),
            KitAction::NextSection => {
                let request = Request::Variation(self.band.next_section());
                self.band.request(request, &mut self.events);
            }
            KitAction::Fill => self.band.request(Request::Fill, &mut self.events),
            KitAction::Ending => self.band.request(Request::Ending, &mut self.events),
        }
    }

    /// Sounds a hit on [`PLAYER_DRUM_CHANNEL`] and counts it as playing. A
    /// drummer's hits also count the band in and lead it.
    fn drum_hit(&mut self, piece: Piece, velocity: u8) {
        if velocity == 0 {
            return;
        }
        self.release_drum(piece, 0);
        self.drum_gate[piece.index()] = self.gate_frames;
        self.events.push(0, EngineEvent::NoteOn { channel: PLAYER_DRUM_CHANNEL, key: piece.note(), velocity });
        self.band.note_played(velocity);
        if self.live.active() {
            // Judged against the live source a beat later.
            self.live.hit(piece, velocity, self.now, &mut self.events);
        } else if self.backing.active() {
            // Judged against the recording; nothing to lead or count in.
            self.backing.drum_hit(piece, velocity, self.band.latency_ms(), &mut self.events);
        } else if self.instrument != Instrument::Drums {
            self.band.onset(0.0);
        } else if self.band.is_playing() {
            self.band.drum_hit(piece, velocity, &mut self.events);
        } else if self.start_in.is_none() {
            self.count_in_hit(piece);
        }
    }

    /// A hit while stopped: four even ones start the band on the next 1,
    /// timed so the player hears it where they expect it.
    fn count_in_hit(&mut self, piece: Piece) {
        match self.count_in.hit(piece, self.now, self.sample_rate) {
            CountIn::Ignored => {}
            CountIn::Counting { hits } => self.events.push(0, EngineEvent::CountIn { hits }),
            CountIn::Done { period, last } => {
                self.events.push(0, EngineEvent::CountIn { hits: 4 });
                let latency = self.band.latency_ms() as f64 / 1000.0 * self.sample_rate as f64;
                let start = (last + period - latency).max(self.now as f64);
                let bpm = 60.0 * self.sample_rate / period as f32;
                self.start_in = Some((start as u64 - self.now, bpm));
            }
        }
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

    /// A keys note while playing along: notes this close together are one strike.
    fn strike_note(&mut self, note: u8) {
        let pc = 1u16 << (note % 12);
        match self.strike {
            Some((at, pitches)) if self.now < at + self.strike_frames => self.strike = Some((at, pitches | pc)),
            _ => {
                self.finish_strike();
                self.strike = Some((self.now, pc));
            }
        }
    }

    /// Judges the strike gathered so far against the recording or the live source.
    fn finish_strike(&mut self) {
        let Some((at, pitches)) = self.strike.take() else { return };
        if self.live.active() {
            self.live.strike(pitches, at, &mut self.events);
        } else if self.backing.active() {
            let since_ms = (self.now - at) as f32 * 1000.0 / self.sample_rate;
            self.backing.keys_strike(pitches, self.band.latency_ms() + since_ms, self.band.song(), &mut self.events);
        }
    }

    /// Schedules this block and returns all of its events, sorted by frame.
    pub fn process(&mut self, frames: u32) -> &[Timed] {
        self.release_drums(frames);
        if self.strike.is_some_and(|(at, _)| self.now >= at + self.strike_frames) {
            self.finish_strike();
        }
        match self.start_in {
            Some((delay, bpm)) if delay >= frames as u64 => self.start_in = Some((delay - frames as u64, bpm)),
            Some((delay, bpm)) => {
                // The counted-in start falls inside this block: run the band
                // from that frame on.
                self.start_in = None;
                let mark = self.events.len();
                self.band.start_counted(bpm, &mut self.events);
                self.band.process(frames - delay as u32, &mut self.events);
                self.events.shift_from(mark, delay as u32);
            }
            None if self.live.active() => self.live.process(frames, self.now, &mut self.events),
            None if self.backing.active() => {
                let mark = self.events.len();
                self.backing.process(frames, self.band.song(), &mut self.events);
                if self.events.as_slice()[mark..].iter().any(|t| t.event == EngineEvent::Transport { playing: false }) {
                    // The recording ended.
                    self.events.push(frames - 1, EngineEvent::Tempo { bpm: self.band.bpm() });
                }
            }
            None => self.band.process(frames, &mut self.events),
        }
        self.now += frames as u64;
        self.events.sort();
        self.events.as_slice()
    }

    pub fn end_block(&mut self) {
        self.events.clear();
    }

    /// Adds the backing track to this block's output; after [`Engine::process`]
    /// and the synth, before [`Engine::end_block`].
    pub fn mix_track(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.backing.mix(left, right);
    }

    /// Swaps in a decoded recording at the engine's rate, or none, and
    /// returns the old one: free it off the audio thread where that matters.
    pub fn set_track(&mut self, track: Option<Box<Track>>) -> Option<Box<Track>> {
        let playing = self.backing.is_playing();
        let old = self.backing.set_track(track, &mut self.events);
        if playing {
            self.events.push(0, EngineEvent::Tempo { bpm: self.band.bpm() });
        }
        old
    }

    pub fn is_playing(&self) -> bool {
        self.band.is_playing() || self.backing.is_playing() || self.live.is_running()
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

    /// Runs `seconds` of blocks from absolute frame `*at`, sending each
    /// scripted command at the first block at or after its frame. Returns the
    /// events with absolute frames.
    fn run_script(engine: &mut Engine, at: &mut u64, seconds: f64, script: &[(u64, Command)]) -> Vec<(u64, EngineEvent)> {
        let end = *at + (seconds * 48_000.0) as u64;
        let mut next = script.partition_point(|(f, _)| *f < *at);
        let mut out = Vec::new();
        while *at < end {
            while next < script.len() && script[next].0 < *at + 128 {
                engine.handle(script[next].1);
                next += 1;
            }
            out.extend(engine.process(128).iter().map(|t| (*at + t.frame as u64, t.event)));
            engine.end_block();
            *at += 128;
        }
        out
    }

    fn drummer() -> Engine {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::SetInstrument { instrument: Instrument::Drums });
        engine.handle(Command::SetLatency { ms: 0.0 });
        engine.handle(Command::SetCountIn { pieces: Piece::HiHatClosed.bit() | Piece::SideStick.bit() });
        engine
    }

    fn hit(piece: Piece, velocity: u8) -> Command {
        Command::DrumHit { piece, velocity }
    }

    /// A rock groove from frame `from` for `bars` bars at `bpm`: kick on 1
    /// and 3, snare on 2 and 4, closed hi-hat eighths.
    fn groove(from: f64, bars: u32, bpm: f64) -> Vec<(u64, Command)> {
        let beat = 48_000.0 * 60.0 / bpm;
        let mut script = Vec::new();
        for eighth in 0..bars * 8 {
            let f = (from + eighth as f64 * beat / 2.0) as u64;
            script.push((f, hit(Piece::HiHatClosed, 90)));
            match eighth % 8 {
                0 | 4 => script.push((f, hit(Piece::Kick, 100))),
                2 | 6 => script.push((f, hit(Piece::Snare, 100))),
                _ => {}
            }
        }
        script
    }

    fn band_states(events: &[(u64, EngineEvent)]) -> Vec<(u64, crate::event::BandState)> {
        events.iter().filter_map(|(f, e)| if let EngineEvent::Band(s) = e { Some((*f, *s)) } else { None }).collect()
    }

    fn tempo_of(events: &[(u64, EngineEvent)]) -> Option<f32> {
        events.iter().rev().find_map(|(_, e)| if let EngineEvent::Tempo { bpm } = e { Some(*bpm) } else { None })
    }

    #[test]
    fn four_even_hi_hats_count_the_band_in() {
        for latency_ms in [0.0, 20.0] {
            let mut engine = drummer();
            engine.handle(Command::SetLatency { ms: latency_ms });
            let mut at = 0;
            // 112.5 BPM: a beat is 25 600 frames. A snare between doesn't count.
            let mut script: Vec<_> = (0..4).map(|i| (1280 + i * 25_600, hit(Piece::HiHatClosed, 80))).collect();
            script.insert(1, (9_000, hit(Piece::Snare, 100)));
            let events = run_script(&mut engine, &mut at, 3.0, &script);
            let counts: Vec<_> =
                events.iter().filter_map(|(_, e)| if let EngineEvent::CountIn { hits } = e { Some(*hits) } else { None }).collect();
            assert_eq!(counts, [0, 1, 2, 3, 4]);
            let start = 1280 + 4 * 25_600 - (latency_ms as u64 * 48);
            let first_beat = events.iter().find(|(_, e)| matches!(e, EngineEvent::Beat { bar: 0, beat: 0 })).unwrap().0;
            assert_eq!(first_beat, start, "the band starts on the next 1, early by the latency");
            assert!(events.contains(&(start, EngineEvent::Transport { playing: true })));
            assert!((tempo_of(&events).unwrap() - 112.5).abs() < 0.01);
            let second = events.iter().find(|(_, e)| matches!(e, EngineEvent::Beat { beat: 1, .. })).unwrap().0;
            assert!((second as i64 - (start + 25_600) as i64).abs() <= 1);
        }
    }

    #[test]
    fn keys_players_and_play_skip_the_count_in() {
        let mut engine = drummer();
        engine.handle(Command::SetInstrument { instrument: Instrument::Keys });
        let script: Vec<_> = (0..4).map(|i| (i * 25_600, Command::DrumNote { note: 42, velocity: 80 })).collect();
        let events = run_script(&mut engine, &mut 0, 3.0, &script);
        assert!(!events.iter().any(|(_, e)| matches!(e, EngineEvent::CountIn { hits: 1.. })));
        assert!(!engine.is_playing());
    }

    #[test]
    fn a_drummer_leads_the_tempo() {
        for player in [128.0, 112.0] {
            let mut engine = drummer();
            engine.handle(Command::SetTempo { bpm: 120.0 });
            engine.handle(Command::Play);
            let mut at = 0;
            // Starts on the band's beat, then keeps the player's tempo.
            let events = run_script(&mut engine, &mut at, 12.0, &groove(0.0, 24, player));
            let bpm = tempo_of(&events).unwrap();
            assert!((bpm - player as f32).abs() < 2.0, "player {player}, band {bpm}");
        }
    }

    #[test]
    fn a_drummer_s_fills_do_not_pull_the_tempo() {
        let mut engine = drummer();
        engine.handle(Command::SetTempo { bpm: 120.0 });
        engine.handle(Command::Play);
        let beat = 24_000.0;
        let mut script = Vec::new();
        for phrase in 0..4 {
            let from = phrase as f64 * 4.0 * 4.0 * beat;
            script.extend(groove(from, 3, 120.0));
            // Bar 4: two beats of groove, then a rushed fill: sixteenths 15 % fast, toms then snare.
            script.extend(groove(from + 12.0 * beat, 1, 120.0).into_iter().filter(|(f, _)| (*f as f64) < from + 14.0 * beat));
            for i in 0..8 {
                let piece = if i < 4 { Piece::Tom1 } else { Piece::Snare };
                script.push(((from + 14.0 * beat + i as f64 * beat / 4.0 / 1.15) as u64, hit(piece, 110)));
            }
        }
        script.sort_by_key(|(f, _)| *f);
        let events = run_script(&mut engine, &mut 0, 33.0, &script);
        let tempos: Vec<_> =
            events.iter().filter_map(|(_, e)| if let EngineEvent::Tempo { bpm } = e { Some(*bpm) } else { None }).collect();
        assert!(tempos.iter().all(|bpm| (bpm - 120.0).abs() < 1.5), "{tempos:?}");
    }

    /// A drummer plays three bars of groove, a fill in bar 4 and then, if
    /// `crash_at` is set, a crash that many frames from bar 5's 1.
    fn fill_into_bar_five(crash_at: Option<i64>) -> Vec<(u64, EngineEvent)> {
        let mut engine = drummer();
        engine.handle(Command::SetTempo { bpm: 120.0 });
        engine.handle(Command::Play);
        let beat = 24_000.0;
        let mut script = groove(0.0, 3, 120.0);
        script.extend(groove(12.0 * beat, 1, 120.0).into_iter().filter(|(f, _)| (*f as f64) < 14.0 * beat));
        for i in 0..8 {
            script.push(((14.0 * beat + i as f64 * beat / 4.0) as u64, hit(Piece::Tom2, 110)));
        }
        if let Some(offset) = crash_at {
            script.push((((16.0 * beat) as i64 + offset) as u64, hit(Piece::Crash, 120)));
        }
        script.extend(groove(16.0 * beat, 2, 120.0));
        script.sort_by_key(|(f, _)| *f);
        run_script(&mut engine, &mut 0, 9.0, &script)
    }

    #[test]
    fn a_fill_and_a_crash_on_the_1_move_the_band_on() {
        for offset in [-2_000, 0, 3_000] {
            let events = fill_into_bar_five(Some(offset));
            let states = band_states(&events);
            let cued = states.iter().find(|(_, s)| s.cued).expect("the fill is heard");
            assert!(cued.0 > 14 * 24_000 && cued.0 < 16 * 24_000, "cued at {}", cued.0);
            assert_eq!(cued.1.next_variation, 1);
            let (at, state) = states.iter().find(|(_, s)| s.variation == 1).expect("B comes in");
            assert!(*at as i64 >= 16 * 24_000 && (*at as i64) <= 16 * 24_000 + offset.max(0) + 128, "crash {offset}: B at {at}");
            assert!(!state.cued);
            assert_eq!(states.last().unwrap().1.variation, 1, "and stays");
        }
    }

    #[test]
    fn without_a_crash_the_band_stays() {
        let states = band_states(&fill_into_bar_five(None));
        assert!(states.iter().any(|(_, s)| s.cued));
        let last = states.last().unwrap().1;
        assert_eq!((last.variation, last.next_variation, last.cued), (0, 0, false));
    }

    #[test]
    fn grabbing_a_cymbal_stops_the_band() {
        let mut engine = drummer();
        engine.handle(Command::Play);
        let events =
            run_script(&mut engine, &mut 0, 0.5, &[(12_800, Command::DrumChoke { note: 42 }), (19_200, Command::DrumChoke { note: 49 })]);
        let stop = events.iter().find(|(_, e)| *e == EngineEvent::Transport { playing: false }).unwrap();
        assert_eq!(stop.0, 19_200, "the hi-hat isn't a cymbal to grab");
        assert!(!engine.is_playing());
    }

    #[test]
    fn kit_triggers_control_the_band() {
        use crate::drums::Trigger;
        let mut engine = drummer();
        engine.handle(Command::KitTrigger { trigger: Trigger::Note(60), action: Some(KitAction::StartStop) });
        engine.handle(Command::KitTrigger { trigger: Trigger::Control(64), action: Some(KitAction::NextSection) });
        engine.handle(Command::KitTrigger { trigger: Trigger::Control(65), action: Some(KitAction::Ending) });
        engine.handle(Command::NoteOn { note: 60, velocity: 100 });
        assert!(drum_events(engine.process(128)).is_empty(), "a trigger pad plays no drum");
        assert!(engine.is_playing());
        engine.end_block();
        engine.handle(Command::ControlChange { cc: 64, value: 127 });
        let state = band_states(&engine.process(128).iter().map(|t| (0, t.event)).collect::<Vec<_>>()).last().unwrap().1;
        assert_eq!((state.next_variation, state.fill_pending, state.follow), (1, true, true));
        engine.end_block();
        engine.handle(Command::ControlChange { cc: 65, value: 127 });
        assert!(band_states(&engine.process(128).iter().map(|t| (0, t.event)).collect::<Vec<_>>()).last().unwrap().1.ending_pending);
        engine.end_block();
        engine.handle(Command::DrumNote { note: 60, velocity: 90 });
        engine.process(128);
        assert!(!engine.is_playing());
    }

    #[test]
    fn a_drummer_s_band_drums_are_off_percussion_or_a_click() {
        let keys = |events: &[(u64, EngineEvent)]| -> std::collections::BTreeSet<u8> {
            events.iter().filter_map(|(_, e)| if let EngineEvent::NoteOn { channel: 9, key, .. } = e { Some(*key) } else { None }).collect()
        };
        for (mode, expected) in [
            (None, vec![]),
            (Some(BandDrums::Percussion), vec![54, 70]),
            (Some(BandDrums::Click), vec![76]),
            (Some(BandDrums::Full), vec![36, 38, 42, 49]),
        ] {
            let mut engine = drummer();
            if let Some(mode) = mode {
                engine.handle(Command::SetBandDrums { mode });
            }
            engine.handle(Command::SetTempo { bpm: 120.0 });
            engine.handle(Command::Section { request: Request::Intro });
            let events = run_script(&mut engine, &mut 0, 4.2, &[]);
            let heard: Vec<_> = keys(&events).into_iter().collect();
            if mode == Some(BandDrums::Full) {
                assert!(expected.iter().all(|k| heard.contains(k)), "{heard:?}");
            } else {
                assert_eq!(heard, expected, "{mode:?}");
            }
            if mode == Some(BandDrums::Click) {
                let clicks = events.iter().filter(|(_, e)| matches!(e, EngineEvent::NoteOn { channel: 9, .. })).count();
                assert_eq!(clicks, 9, "one per beat");
            }
        }
        let mut engine = drummer();
        engine.handle(Command::SetInstrument { instrument: Instrument::Keys });
        engine.handle(Command::Play);
        assert!(keys(&run_script(&mut engine, &mut 0, 1.0, &[])).contains(&36), "keys players get the full band");
    }

    #[test]
    fn a_drummer_s_hits_are_timed_against_the_grid() {
        let mut engine = drummer();
        engine.handle(Command::SetTempo { bpm: 120.0 });
        engine.handle(Command::Play);
        // 10 ms late on beat 2, then a crash, which isn't timed.
        let events = run_script(&mut engine, &mut 0, 1.0, &[(24_000 + 480, hit(Piece::Snare, 100)), (36_000, hit(Piece::Crash, 100))]);
        let timings: Vec<_> =
            events.iter().filter_map(|(_, e)| if let EngineEvent::Timing { ms } = e { Some(*ms) } else { None }).collect();
        assert_eq!(timings.len(), 1);
        assert!((timings[0] - 10.0).abs() < 2.7, "{timings:?}");
    }

    #[test]
    fn songs_can_play_a_recording_that_judges_the_drummer() {
        let mut engine = drummer();
        engine.handle(Command::SetLatency { ms: 10.0 });
        engine.handle(Command::SongClear { bars: 4 });
        engine.handle(Command::SongChord { beat: 0, slot: crate::Slot::Chord(Chord::new(2, Quality::Minor)) });
        engine.handle(Command::SetSong { on: true });
        // 120 BPM from 0.5 s, in a 10 s recording.
        engine.set_track(Some(Box::new(Track { main: vec![0; 2 * 480_000], drums: None })));
        engine.handle(Command::TrackClear { beats: 17 });
        for i in 0..17 {
            engine.handle(Command::TrackBeat { index: i, seconds: 0.5 + 0.5 * i as f32 });
        }
        engine.handle(Command::SetTrack { on: true });
        // Play, a fill request that the recording ignores, and a snare 6 ms
        // late on beat 2 heard 10 ms after it is played.
        let snare = 24_000 + 24_000 + 288 + 480;
        let script = [(0, Command::Play), (100, Command::Section { request: Request::Fill }), (snare, hit(Piece::Snare, 100))];
        let events = run_script(&mut engine, &mut 0, 10.5, &script);
        assert!(band_states(&events).iter().all(|(_, s)| !s.playing), "the band stays out");
        assert!(!events.iter().any(|(_, e)| matches!(e, EngineEvent::NoteOn { channel: 0..=9, .. })));
        let beats: Vec<_> = events.iter().filter_map(|(f, e)| if let EngineEvent::SongPosition { beat } = e { Some((*f, *beat)) } else { None }).collect();
        assert_eq!(beats.len(), 16);
        assert_eq!(beats[0], (24_000, 0));
        assert_eq!(chord_events(&events.iter().map(|(f, e)| Timed { frame: *f as u32, event: *e }).collect::<Vec<_>>()), [Some(Chord::new(2, Quality::Minor))]);
        let judged: Vec<_> = events.iter().filter_map(|(_, e)| if let EngineEvent::Judged { ms, piece, sixteenth, bar } = e { Some((*ms, *piece, *sixteenth, *bar)) } else { None }).collect();
        assert_eq!(judged.len(), 1);
        let (ms, piece, sixteenth, bar) = judged[0];
        assert_eq!((piece, sixteenth, bar), (Piece::Snare, 4, 0));
        // The hit is seen at the start of its block, up to 128 frames early.
        assert!(ms <= 6.01 && ms > 6.0 - 128.0 / 48.0, "{ms}");
        assert!(events.iter().any(|(_, e)| matches!(e, EngineEvent::Timing { .. })));
        assert!(!engine.is_playing(), "stopped at the end of the recording");
        assert!(events.iter().any(|(_, e)| *e == EngineEvent::Transport { playing: false }));
    }

    #[test]
    fn a_keys_player_is_judged_against_a_recording_and_its_chords() {
        let mut engine = Engine::new(48_000.0);
        engine.handle(Command::SetLatency { ms: 0.0 });
        engine.handle(Command::SongClear { bars: 4 });
        // Dm for bar 1, G from bar 2.
        engine.handle(Command::SongChord { beat: 0, slot: crate::Slot::Chord(Chord::new(2, Quality::Minor)) });
        engine.handle(Command::SongChord { beat: 4, slot: crate::Slot::Chord(Chord::new(7, Quality::Major)) });
        engine.handle(Command::SetSong { on: true });
        engine.set_track(Some(Box::new(Track { main: vec![0; 2 * 480_000], drums: None })));
        engine.handle(Command::TrackClear { beats: 17 });
        for i in 0..17 {
            engine.handle(Command::TrackBeat { index: i, seconds: 0.5 + 0.5 * i as f32 });
        }
        engine.handle(Command::SetTrack { on: true });
        let note = |note: u8| Command::NoteOn { note, velocity: 90 };
        // Song beat 0 is heard at 0.5 s (frame 24 000).
        let beat = |i: u64| 24_000 + i * 24_000;
        let script = [
            (0, Command::Play),
            // Dm on beat 2, 4 ms late, its notes 10 ms apart: one strike.
            (beat(1) + 192, note(62)),
            (beat(1) + 192 + 480, note(65)),
            (beat(1) + 192 + 960, note(69)),
            // C major on beat 4 over Dm: wrong.
            (beat(3), note(60)),
            (beat(3) + 200, note(64)),
            (beat(3) + 400, note(67)),
            // G a sixteenth early for bar 2's G: right, and meant for it.
            (beat(4) - 6_000, note(55)),
            (beat(4) - 6_000, note(59)),
            // A single note isn't checked.
            (beat(5), note(74)),
        ];
        let events = run_script(&mut engine, &mut 0, 6.0, &script);
        let judged: Vec<_> = events.iter().filter_map(|(_, e)| if let EngineEvent::KeysJudged { ms, sixteenth, bar, chord } = e { Some((*ms, *sixteenth, *bar, *chord)) } else { None }).collect();
        assert_eq!(judged.len(), 4, "{judged:?}");
        assert_eq!(judged.iter().map(|j| (j.1, j.2, j.3)).collect::<Vec<_>>(), [(4, 0, Some(true)), (12, 0, Some(false)), (15, 0, Some(true)), (4, 1, None)]);
        // Seen at the start of their block, up to 128 frames early.
        assert!(judged[0].0 <= 4.01 && judged[0].0 > 4.0 - 2.7, "{}", judged[0].0);
        assert_eq!(events.iter().filter(|(_, e)| matches!(e, EngineEvent::Timing { .. })).count(), 4);
        assert!(!events.iter().any(|(_, e)| matches!(e, EngineEvent::Judged { .. })));
    }

    #[test]
    fn live_audio_takes_over_from_the_band_and_judges_the_drummer() {
        let mut engine = drummer();
        engine.handle(Command::SetLiveOffset { ms: 0.0 });
        engine.handle(Command::SetTrack { on: true });
        engine.handle(Command::SetLive { on: true });
        // 120 BPM clicks from 0.25 s, heard as they come in.
        let click = |f: u64| {
            let into = (f as f64 / 48_000.0 - 0.25).rem_euclid(0.5);
            if f >= 12_000 && into < 0.01 { ((f * 7919) % 200) as f32 / 200.0 - 0.5 } else { 0.0 }
        };
        let snare_at = (48_000.0 * (0.25 + 20.0 * 0.5 + 0.008)) as u64;
        let mut events = Vec::new();
        let mut input = [0.0f32; 128];
        for block in 0..(48_000 * 14 / 128) as u64 {
            let at = block * 128;
            for (i, x) in input.iter_mut().enumerate() {
                *x = click(at + i as u64);
            }
            match at {
                99_968 => engine.handle(Command::Play),
                112_000 => engine.handle(Command::Section { request: Request::Fill }),
                f if f + 128 > snare_at && f <= snare_at => engine.handle(hit(Piece::Snare, 100)),
                _ => {}
            }
            engine.listen_live(&input);
            events.extend(engine.process(128).iter().map(|t| (at + t.frame as u64, t.event)));
            engine.end_block();
        }
        assert!(band_states(&events).iter().all(|(_, s)| !s.playing), "the band stays out");
        assert!(!events.iter().any(|(_, e)| matches!(e, EngineEvent::NoteOn { channel: 0..=9, .. })));
        assert!(events.contains(&(99_968, EngineEvent::Transport { playing: true })), "Play starts a run");
        let judged: Vec<_> = events.iter().filter_map(|(_, e)| if let EngineEvent::Judged { ms, piece, .. } = e { Some((*ms, *piece)) } else { None }).collect();
        assert_eq!(judged.len(), 1);
        assert!(judged[0].1 == Piece::Snare && judged[0].0 > 8.0 - 2.7 - 3.0 && judged[0].0 < 8.0 + 3.0, "{judged:?}");
        assert!(events.iter().any(|(_, e)| matches!(e, EngineEvent::Live { locked: true, .. })));
        assert!(engine.is_playing());
        // Switching to keys keeps the source's tempo on show, whatever the band's is.
        engine.handle(Command::SetInstrument { instrument: Instrument::Keys });
        let mut tempo = None;
        for _ in 0..200 {
            engine.listen_live(&[0.0; 128]);
            tempo = engine.process(128).iter().rev().find_map(|t| if let EngineEvent::Tempo { bpm } = t.event { Some(bpm) } else { None }).or(tempo);
            engine.end_block();
        }
        assert!(tempo.is_some_and(|bpm| (bpm - 120.0).abs() < 1.0), "{tempo:?}");
        engine.handle(Command::SetLive { on: false });
        let off = engine.process(128).to_vec();
        assert!(off.iter().any(|t| t.event == EngineEvent::Transport { playing: false }));
        assert!(!engine.is_playing());
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
