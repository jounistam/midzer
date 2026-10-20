//! The backing band: plays a style's patterns over the current chord.

use crate::chord::Chord;
use crate::event::{EngineEvent, EventBuffer};
use crate::style::{BEATS_PER_BAR, Hit, Part, PartKind, PPQ, Step, Style, TICKS_PER_BAR};
use crate::voicing::{Voicing, bass_note, voice_near};

const MAX_SOUNDING: usize = 64;
const KEYS_RANGE: (u8, u8) = (55, 79);
const PAD_RANGE: (u8, u8) = (48, 72);
const KEYS_HOME: f32 = 64.0;
const PAD_HOME: f32 = 60.0;

#[derive(Clone, Copy, Default)]
struct Sounding {
    active: bool,
    channel: u8,
    key: u8,
    off_tick: f64,
}

pub struct Band {
    style: &'static Style,
    sample_rate: f32,
    bpm: f32,
    playing: bool,
    /// Frames rendered since Play; the next block starts here.
    frame: u64,
    /// Song position is `anchor_tick` at `anchor_frame`, advancing at the
    /// current tempo. Re-anchored on tempo changes, so float error never
    /// accumulates.
    anchor_frame: u64,
    anchor_tick: f64,
    chord: Option<Chord>,
    /// The latest chord the player formed; latched on the next beat.
    pending: Option<Chord>,
    keys: Voicing,
    pad: Voicing,
    sounding: [Sounding; MAX_SOUNDING],
    /// Bit per MIDI channel.
    muted: u16,
}

/// Converts song positions within the current block to frame offsets.
#[derive(Clone, Copy)]
struct BlockClock {
    block_start: u64,
    anchor_frame: u64,
    anchor_tick: f64,
    ticks_per_frame: f64,
    frames: u32,
}

impl BlockClock {
    fn tick_at(&self, frame: u64) -> f64 {
        self.anchor_tick + (frame - self.anchor_frame) as f64 * self.ticks_per_frame
    }

    fn frame(&self, tick: f64) -> u32 {
        // The epsilon absorbs float error so a tick exactly on a frame boundary
        // lands on that frame, not the one before it.
        let absolute = self.anchor_frame as f64 + (tick - self.anchor_tick) / self.ticks_per_frame + 1e-6;
        let offset = (absolute.floor() - self.block_start as f64).max(0.0) as u32;
        offset.min(self.frames.saturating_sub(1))
    }
}

impl Band {
    pub fn new(style: &'static Style, sample_rate: f32) -> Self {
        Self {
            style,
            sample_rate,
            bpm: style.default_bpm,
            playing: false,
            frame: 0,
            anchor_frame: 0,
            anchor_tick: 0.0,
            chord: None,
            pending: None,
            keys: Voicing::default(),
            pad: Voicing::default(),
            sounding: [Sounding::default(); MAX_SOUNDING],
            muted: 0,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn chord(&self) -> Option<Chord> {
        self.chord
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        self.anchor_tick = self.clock(0).tick_at(self.frame);
        self.anchor_frame = self.frame;
        self.bpm = bpm.clamp(30.0, 300.0);
    }

    fn clock(&self, frames: u32) -> BlockClock {
        BlockClock {
            block_start: self.frame,
            anchor_frame: self.anchor_frame,
            anchor_tick: self.anchor_tick,
            ticks_per_frame: self.bpm as f64 / 60.0 * PPQ as f64 / self.sample_rate as f64,
            frames,
        }
    }

    pub fn set_muted(&mut self, channel: u8, muted: bool, out: &mut EventBuffer) {
        let bit = 1u16 << (channel & 15);
        if muted {
            self.muted |= bit;
            self.release_all(|ch| ch == channel, 0, out);
        } else {
            self.muted &= !bit;
        }
    }

    /// The player formed a new chord. While stopped it applies immediately;
    /// while playing it is latched on the next beat so the band stays in time.
    pub fn set_chord(&mut self, chord: Chord, out: &mut EventBuffer) {
        self.pending = Some(chord);
        if !self.playing && self.chord != self.pending {
            self.chord = self.pending;
            out.push(0, EngineEvent::ChordChanged(self.chord));
        }
    }

    pub fn play(&mut self, out: &mut EventBuffer) {
        if self.playing {
            return;
        }
        self.playing = true;
        self.frame = 0;
        self.anchor_frame = 0;
        self.anchor_tick = 0.0;
        for part in self.style.parts {
            if part.kind != PartKind::Drums {
                out.push(0, EngineEvent::ProgramChange { channel: part.channel, program: part.program });
            }
        }
        out.push(0, EngineEvent::Transport { playing: true });
    }

    pub fn stop(&mut self, out: &mut EventBuffer) {
        if !self.playing {
            return;
        }
        self.playing = false;
        self.release_all(|_| true, 0, out);
        out.push(0, EngineEvent::Transport { playing: false });
    }

    /// Schedules everything that happens during the next `frames` frames.
    pub fn process(&mut self, frames: u32, out: &mut EventBuffer) {
        if !self.playing || frames == 0 {
            return;
        }
        let clock = self.clock(frames);
        let start = clock.tick_at(self.frame);
        let end = clock.tick_at(self.frame + frames as u64);

        // Walk the block beat by beat so a chord latched on a beat applies to
        // everything scheduled from that beat on.
        let mut seg_start = start;
        let mut beat = (start / PPQ as f64 - 1e-9).ceil().max(0.0) as u64;
        loop {
            let beat_tick = (beat * PPQ as u64) as f64;
            let seg_end = beat_tick.min(end);
            self.schedule_range(seg_start, seg_end, &clock, out);
            if beat_tick >= end {
                break;
            }
            self.on_beat(beat, beat_tick, &clock, out);
            seg_start = beat_tick;
            beat += 1;
        }
        self.frame += frames as u64;
    }

    fn on_beat(&mut self, beat: u64, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        let frame = clock.frame(tick);
        let beat_in_bar = (beat % BEATS_PER_BAR as u64) as u8;
        out.push(frame, EngineEvent::Beat { bar: (beat / BEATS_PER_BAR as u64) as u32, beat: beat_in_bar });

        if self.pending == self.chord {
            return;
        }
        self.chord = self.pending;
        out.push(frame, EngineEvent::ChordChanged(self.chord));

        // Cut the harmony that no longer fits. Patterns on this beat retrigger
        // naturally; the pad is re-struck for the rest of the bar.
        let harmonic = self.harmonic_channels();
        self.release_all(|ch| harmonic.contains(&ch), frame, out);
        if beat_in_bar != 0 {
            let bar_end = (tick / TICKS_PER_BAR as f64).ceil() * TICKS_PER_BAR as f64;
            for part in self.style.parts.iter().filter(|p| p.kind == PartKind::Pad) {
                for step in part.steps {
                    let retrigger = Step { dur: (bar_end - tick) as u32, ..*step };
                    self.trigger(part, &retrigger, tick, clock, out);
                }
            }
        }
    }

    fn schedule_range(&mut self, from: f64, to: f64, clock: &BlockClock, out: &mut EventBuffer) {
        if to <= from {
            return;
        }
        for i in 0..MAX_SOUNDING {
            let s = self.sounding[i];
            if s.active && s.off_tick < to {
                out.push(clock.frame(s.off_tick), EngineEvent::NoteOff { channel: s.channel, key: s.key });
                self.sounding[i].active = false;
            }
        }

        let first_bar = (from / TICKS_PER_BAR as f64).floor() as u64;
        let last_bar = (to / TICKS_PER_BAR as f64).ceil() as u64;
        for bar in first_bar..last_bar {
            let bar_tick = (bar * TICKS_PER_BAR as u64) as f64;
            for part in self.style.parts {
                for step in part.steps {
                    let tick = bar_tick + step.tick as f64;
                    if tick >= from && tick < to {
                        self.trigger(part, step, tick, clock, out);
                    }
                }
            }
        }
    }

    fn trigger(&mut self, part: &Part, step: &Step, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        if self.muted & (1 << part.channel) != 0 {
            return;
        }
        let off_tick = tick + step.dur as f64;
        let frame = clock.frame(tick);
        match (step.hit, self.chord) {
            (Hit::Drum(key), _) => self.note_on(part.channel, key, step.velocity, frame, off_tick, out),
            (Hit::Bass(tone), Some(chord)) => {
                let key = bass_note(&chord, tone);
                self.note_on(part.channel, key, step.velocity, frame, off_tick, out);
            }
            (Hit::Chord, Some(chord)) => {
                let voicing = match part.kind {
                    PartKind::Pad => {
                        self.pad = voice_near(&chord, self.pad.center().unwrap_or(PAD_HOME) * 0.7 + PAD_HOME * 0.3, PAD_RANGE.0, PAD_RANGE.1);
                        self.pad
                    }
                    _ => {
                        self.keys = voice_near(&chord, self.keys.center().unwrap_or(KEYS_HOME) * 0.7 + KEYS_HOME * 0.3, KEYS_RANGE.0, KEYS_RANGE.1);
                        self.keys
                    }
                };
                for &key in voicing.notes() {
                    self.note_on(part.channel, key, step.velocity, frame, off_tick, out);
                }
            }
            // Harmonic parts wait for the first chord.
            (_, None) => {}
        }
    }

    fn note_on(&mut self, channel: u8, key: u8, velocity: u8, frame: u32, off_tick: f64, out: &mut EventBuffer) {
        // Retriggering a sounding key: end the old note first.
        if let Some(i) = self.sounding.iter().position(|s| s.active && s.channel == channel && s.key == key) {
            out.push(frame, EngineEvent::NoteOff { channel, key });
            self.sounding[i].active = false;
        }
        let Some(slot) = self.sounding.iter_mut().find(|s| !s.active) else {
            return; // Polyphony budget exhausted; skip rather than hang a note.
        };
        *slot = Sounding { active: true, channel, key, off_tick };
        out.push(frame, EngineEvent::NoteOn { channel, key, velocity });
    }

    fn release_all(&mut self, mut filter: impl FnMut(u8) -> bool, frame: u32, out: &mut EventBuffer) {
        for s in self.sounding.iter_mut().filter(|s| s.active && filter(s.channel)) {
            out.push(frame, EngineEvent::NoteOff { channel: s.channel, key: s.key });
            s.active = false;
        }
    }

    fn harmonic_channels(&self) -> [u8; 4] {
        let mut channels = [u8::MAX; 4];
        for (slot, part) in channels.iter_mut().zip(self.style.parts.iter().filter(|p| p.kind != PartKind::Drums)) {
            *slot = part.channel;
        }
        channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Quality;
    use crate::style::POP;

    const SR: f32 = 48_000.0;
    const BLOCK: u32 = 128;

    /// Runs the band for `seconds`, returning every event with its absolute frame.
    fn run(band: &mut Band, seconds: f32, start_frame: &mut u64) -> Vec<(u64, EngineEvent)> {
        let mut all = Vec::new();
        let mut out = EventBuffer::default();
        for _ in 0..(seconds * SR / BLOCK as f32) as usize {
            band.process(BLOCK, &mut out);
            out.sort();
            all.extend(out.as_slice().iter().map(|t| (*start_frame + t.frame as u64, t.event)));
            out.clear();
            *start_frame += BLOCK as u64;
        }
        all
    }

    fn started_band() -> (Band, u64) {
        let mut band = Band::new(&POP, SR);
        band.set_tempo(120.0); // one beat = 24000 frames
        let mut out = EventBuffer::default();
        band.play(&mut out);
        (band, 0)
    }

    fn note_ons(events: &[(u64, EngineEvent)], channel: u8) -> Vec<(u64, u8)> {
        events
            .iter()
            .filter_map(|(f, e)| match e {
                EngineEvent::NoteOn { channel: c, key, .. } if *c == channel => Some((*f, *key)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn beats_land_on_exact_frames() {
        let (mut band, mut frame) = started_band();
        let events = run(&mut band, 2.1, &mut frame);
        let beats: Vec<_> = events
            .iter()
            .filter_map(|(f, e)| matches!(e, EngineEvent::Beat { .. }).then_some(*f))
            .collect();
        assert_eq!(beats, [0, 24_000, 48_000, 72_000, 96_000]);
    }

    #[test]
    fn only_drums_play_until_a_chord_arrives() {
        let (mut band, mut frame) = started_band();
        let events = run(&mut band, 2.0, &mut frame);
        assert!(!note_ons(&events, 9).is_empty());
        for channel in [1, 2, 3] {
            assert!(note_ons(&events, channel).is_empty(), "channel {channel} played without a chord");
        }
    }

    #[test]
    fn chord_is_latched_on_the_next_beat() {
        let (mut band, mut frame) = started_band();
        run(&mut band, 0.6, &mut frame); // just past beat 1 (frame 24000)
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(9, Quality::Minor), &mut out);
        assert!(out.as_slice().is_empty(), "must not change mid-beat while playing");

        let events = run(&mut band, 0.5, &mut frame);
        let change = events.iter().find(|(_, e)| matches!(e, EngineEvent::ChordChanged(_))).unwrap();
        assert_eq!(change.0, 48_000);

        // The pad is re-struck with A minor tones at the change.
        let pad: Vec<_> = note_ons(&events, 3).into_iter().filter(|(f, _)| *f == 48_000).collect();
        let mut pcs: Vec<_> = pad.iter().map(|(_, k)| k % 12).collect();
        pcs.sort();
        assert_eq!(pcs, [0, 4, 9]);
    }

    #[test]
    fn every_note_on_gets_a_note_off() {
        let (mut band, mut frame) = started_band();
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(0, Quality::Major), &mut out);
        let mut events = run(&mut band, 3.3, &mut frame);
        band.set_chord(Chord::new(7, Quality::Dominant7), &mut out);
        events.extend(run(&mut band, 2.2, &mut frame));
        band.stop(&mut out);
        events.extend(out.as_slice().iter().map(|t| (frame, t.event)));

        let mut open = std::collections::HashSet::new();
        for (_, e) in &events {
            match e {
                EngineEvent::NoteOn { channel, key, .. } => assert!(open.insert((*channel, *key)), "double note-on"),
                EngineEvent::NoteOff { channel, key } => {
                    open.remove(&(*channel, *key));
                }
                _ => {}
            }
        }
        assert!(open.is_empty(), "hanging notes: {open:?}");
    }

    #[test]
    fn muted_parts_go_silent_and_come_back() {
        let (mut band, mut frame) = started_band();
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(0, Quality::Major), &mut out);
        run(&mut band, 0.3, &mut frame);
        band.set_muted(2, true, &mut out);
        assert!(out.as_slice().iter().any(|t| matches!(t.event, EngineEvent::NoteOff { channel: 2, .. })));
        let events = run(&mut band, 2.0, &mut frame);
        assert!(note_ons(&events, 2).is_empty());
        assert!(!note_ons(&events, 9).is_empty());
        band.set_muted(2, false, &mut out);
        assert!(!note_ons(&run(&mut band, 2.0, &mut frame), 2).is_empty());
    }

    #[test]
    fn chord_applies_immediately_while_stopped() {
        let mut band = Band::new(&POP, SR);
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(2, Quality::Minor), &mut out);
        assert_eq!(band.chord(), Some(Chord::new(2, Quality::Minor)));
        assert_eq!(out.as_slice().len(), 1);
    }
}
