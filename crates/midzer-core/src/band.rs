//! The backing band: plays a style's patterns over the current chord, in the
//! section the conductor picks.

use crate::chord::Chord;
use crate::conductor::{Conductor, IdleMode, Request, Section};
use crate::event::{BandState, DRUM_CHANNEL, EngineEvent, EventBuffer};
use crate::song::{Slot, SongPlayer};
use crate::style::{BEATS_PER_BAR, CRASH, Hit, PPQ, Part, PartKind, Pattern, Step, Style, TICKS_PER_BAR, WOOD_BLOCK, styles};
use crate::voicing::{Voicing, bass_note, voice_near};

const MAX_SOUNDING: usize = 64;
const KEYS_RANGE: (u8, u8) = (55, 79);
const PAD_RANGE: (u8, u8) = (48, 72);
const KEYS_HOME: f32 = 64.0;
const PAD_HOME: f32 = 60.0;
/// Human slack on top of the measured latency when latching a late chord.
const LATE_SLACK_MS: f32 = 60.0;

#[derive(Clone, Copy, Default)]
struct Sounding {
    active: bool,
    channel: u8,
    key: u8,
    on_tick: f64,
    off_tick: f64,
}

pub struct Band {
    style: &'static Style,
    style_index: u8,
    /// A style change waiting for the next bar line.
    pending_style: Option<u8>,
    conductor: Conductor,
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
    keys_held: bool,
    /// How late the player's notes arrive after the beat they meant, from
    /// output latency and reaction time.
    latency_ms: f32,
    /// Plays only a click on every beat, for latency calibration.
    click: bool,
    /// The state last reported to the UI.
    published: Option<BandState>,
    /// A chord chart that replaces the player's chords while active.
    song: SongPlayer,
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
    pub fn new(sample_rate: f32) -> Self {
        let style = &styles()[0];
        Self {
            style,
            style_index: 0,
            pending_style: None,
            conductor: Conductor::default(),
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
            keys_held: false,
            latency_ms: 30.0,
            click: false,
            published: None,
            song: SongPlayer::default(),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn chord(&self) -> Option<Chord> {
        self.chord
    }

    #[cfg(test)]
    pub fn conductor(&self) -> &Conductor {
        &self.conductor
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
            ticks_per_frame: self.ticks_per_second() / self.sample_rate as f64,
            frames,
        }
    }

    fn ticks_per_second(&self) -> f64 {
        self.bpm as f64 / 60.0 * PPQ as f64
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

    /// Switches style now while stopped, or on the next bar line while playing.
    pub fn set_style(&mut self, index: u8, out: &mut EventBuffer) {
        let Some(style) = styles().get(index as usize) else { return };
        if self.playing {
            self.pending_style = (index != self.style_index).then_some(index);
        } else {
            self.style = style;
            self.style_index = index;
            self.pending_style = None;
        }
        self.publish(0, out);
    }

    pub fn request(&mut self, request: Request, out: &mut EventBuffer) {
        if request == Request::Intro && !self.playing {
            self.start(true, out);
        } else {
            self.conductor.request(request, self.playing);
        }
        self.publish(0, out);
    }

    pub fn set_follow(&mut self, follow: bool, out: &mut EventBuffer) {
        self.conductor.set_follow(follow);
        self.publish(0, out);
    }

    pub fn set_energy(&mut self, energy: f32, out: &mut EventBuffer) {
        self.conductor.set_energy(energy);
        self.publish(0, out);
    }

    pub fn set_idle_mode(&mut self, mode: IdleMode) {
        self.conductor.set_idle_mode(mode);
    }

    pub fn set_latency(&mut self, ms: f32) {
        self.latency_ms = ms.clamp(0.0, 500.0);
    }

    pub fn set_click(&mut self, on: bool, out: &mut EventBuffer) {
        self.click = on;
        self.release_all(|_| true, 0, out);
    }

    pub fn set_song(&mut self, on: bool) {
        self.song.set_active(on);
        self.conductor.set_song(self.song.active());
    }

    pub fn clear_song(&mut self, bars: u32) {
        self.song.clear(bars);
        self.conductor.set_song(self.song.active());
    }

    pub fn set_song_slot(&mut self, beat: u32, slot: Slot) {
        self.song.set_slot(beat, slot);
    }

    pub fn set_song_mark(&mut self, bar: u32, mark: Option<Request>) {
        self.song.set_mark(bar, mark);
    }

    pub fn set_song_loop(&mut self, start: u32, end: u32) {
        self.song.set_loop(start, end);
    }

    /// Stopped: where the song starts. Playing: a jump on the next bar line.
    pub fn seek_song(&mut self, bar: u32) {
        self.song.seek(bar, self.playing);
    }

    /// The player pressed a key; `keys_held` tells whether any key is down.
    pub fn note_played(&mut self, velocity: u8) {
        self.conductor.note_played(velocity);
    }

    pub fn set_keys_held(&mut self, held: bool) {
        self.keys_held = held;
    }

    /// The player formed a new chord. While stopped it applies immediately.
    /// While playing it is latched on the next beat so the band stays in time,
    /// unless it came just after a beat: then the player meant that beat, and
    /// the harmony changes right away.
    /// While a song plays, its chart has the harmony and this is ignored.
    pub fn set_chord(&mut self, chord: Chord, out: &mut EventBuffer) {
        if self.playing && self.song.active() {
            return;
        }
        self.pending = Some(chord);
        if self.chord == self.pending {
            return;
        }
        if !self.playing {
            self.chord = self.pending;
            out.push(0, EngineEvent::ChordChanged(self.chord));
            return;
        }
        let clock = self.clock(1);
        let now = clock.tick_at(self.frame);
        let since_beat = now - (now / PPQ as f64).floor() * PPQ as f64;
        let grace = ((self.latency_ms + LATE_SLACK_MS) / 1000.0) as f64 * self.ticks_per_second();
        if since_beat > 1e-6 && since_beat <= grace.min(PPQ as f64 / 3.0) {
            self.change_chord(now, &clock, out);
        }
    }

    pub fn play(&mut self, out: &mut EventBuffer) {
        self.start(false, out);
    }

    fn start(&mut self, intro: bool, out: &mut EventBuffer) {
        if self.playing {
            return;
        }
        self.playing = true;
        self.frame = 0;
        self.anchor_frame = 0;
        self.anchor_tick = 0.0;
        let mut intro = intro;
        if self.song.active() {
            let (song_intro, variation) = self.song.start();
            intro |= song_intro;
            if let Some(v) = variation {
                self.conductor.request(Request::Variation(v), false);
            }
        }
        self.conductor.start(intro);
        self.program_changes(out);
        out.push(0, EngineEvent::Transport { playing: true });
        self.publish(0, out);
    }

    pub fn stop(&mut self, out: &mut EventBuffer) {
        self.stop_at(0, out);
    }

    fn stop_at(&mut self, frame: u32, out: &mut EventBuffer) {
        if !self.playing {
            return;
        }
        self.playing = false;
        self.release_all(|_| true, frame, out);
        out.push(frame, EngineEvent::Transport { playing: false });
        if let Some(index) = self.pending_style.take() {
            self.style = &styles()[index as usize];
            self.style_index = index;
        }
        self.publish(frame, out);
    }

    /// Schedules everything that happens during the next `frames` frames.
    pub fn process(&mut self, frames: u32, out: &mut EventBuffer) {
        if !self.playing || frames == 0 {
            return;
        }
        let clock = self.clock(frames);
        let start = clock.tick_at(self.frame);
        let end = clock.tick_at(self.frame + frames as u64);

        // Walk the block beat by beat: chords, sections and styles change on
        // beats, and apply to everything scheduled from that beat on.
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
            if !self.playing {
                return;
            }
            seg_start = beat_tick;
            beat += 1;
        }
        self.frame += frames as u64;
    }

    fn on_beat(&mut self, beat: u64, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        let frame = clock.frame(tick);
        let beat_in_bar = (beat % BEATS_PER_BAR as u64) as u8;
        out.push(frame, EngineEvent::Beat { bar: (beat / BEATS_PER_BAR as u64) as u32, beat: beat_in_bar });

        if self.click {
            let velocity = if beat_in_bar == 0 { 120 } else { 90 };
            self.note_on(DRUM_CHANNEL, WOOD_BLOCK, velocity, tick, tick + PPQ as f64 / 4.0, frame, out);
            return;
        }

        let before = self.conductor.section();
        let cue = self.conductor.on_beat(beat_in_bar, self.keys_held);
        if cue.stop {
            self.stop_at(frame, out);
            return;
        }
        if beat_in_bar == 0
            && let Some(index) = self.pending_style.take()
        {
            self.release_all(|_| true, frame, out);
            self.style = &styles()[index as usize];
            self.style_index = index;
            self.program_changes_at(frame, out);
        }
        if self.conductor.section() == Section::Rest && before != Section::Rest {
            // Everyone but the pad stops; the rest pattern takes over.
            self.release_all(|_| true, frame, out);
        }
        if self.song.active() {
            let song = self.song.on_beat(beat_in_bar);
            if let Some(bar) = song.bar {
                out.push(frame, EngineEvent::SongPosition { bar });
            }
            if let Some(chord) = song.chord {
                self.pending = chord;
            }
            if let Some(request) = song.request {
                self.conductor.request(request, true);
            }
        }
        if cue.crash && self.muted & (1 << DRUM_CHANNEL) == 0 {
            self.note_on(DRUM_CHANNEL, CRASH, self.scale(110), tick, tick + PPQ as f64, frame, out);
        }

        if self.pending != self.chord {
            self.change_chord(tick, clock, out);
        }
        self.publish(frame, out);
    }

    /// Applies the pending chord at `tick`: cuts the harmony that no longer
    /// fits and re-strikes notes the pattern still holds at that point.
    /// Steps starting exactly at `tick` are left to the regular schedule.
    fn change_chord(&mut self, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        self.chord = self.pending;
        let frame = clock.frame(tick);
        out.push(frame, EngineEvent::ChordChanged(self.chord));
        let style = self.style;
        let harmonic = |ch: u8| style.parts.iter().any(|p| p.kind != PartKind::Drums && p.channel == ch);
        self.release_all(harmonic, frame, out);

        let bar = (tick / TICKS_PER_BAR as f64).floor();
        let bar_tick = bar * TICKS_PER_BAR as f64;
        for part in style.parts.iter().filter(|p| p.kind != PartKind::Drums) {
            let Some(pattern) = self.pattern(part) else { continue };
            for step in pattern.bar(bar as u64) {
                let (on, off) = (bar_tick + step.tick as f64, bar_tick + (step.tick + step.dur) as f64);
                if on < tick && off > tick + 1.0 {
                    self.trigger(part, &Step { dur: (off - tick) as u32, ..step }, tick, clock, out);
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
        if self.click {
            return;
        }

        let first_bar = (from / TICKS_PER_BAR as f64).floor() as u64;
        let last_bar = (to / TICKS_PER_BAR as f64).ceil() as u64;
        let style = self.style;
        for bar in first_bar..last_bar {
            let bar_tick = (bar * TICKS_PER_BAR as u64) as f64;
            for part in &style.parts {
                let Some(pattern) = self.pattern(part) else { continue };
                for step in pattern.bar(bar) {
                    let tick = bar_tick + step.tick as f64;
                    if tick >= from && tick < to {
                        self.trigger(part, &step, tick, clock, out);
                    }
                }
            }
        }
    }

    /// The pattern `part` plays in the current section, if any.
    fn pattern<'a>(&self, part: &'a Part) -> Option<&'a Pattern> {
        let main = &part.main[self.conductor.variation() as usize];
        match self.conductor.section() {
            Section::Intro => Some(&part.intro),
            Section::Main => Some(main),
            Section::Fill => Some(part.fill.as_ref().unwrap_or(main)),
            Section::Ending => Some(&part.ending),
            Section::Rest => part.rest.as_ref(),
        }
    }

    fn scale(&self, velocity: u8) -> u8 {
        (velocity as f32 * self.conductor.velocity_scale()).round().clamp(1.0, 127.0) as u8
    }

    fn trigger(&mut self, part: &Part, step: &Step, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        if self.muted & (1 << part.channel) != 0 {
            return;
        }
        let off_tick = tick + step.dur as f64;
        let frame = clock.frame(tick);
        let velocity = self.scale(step.velocity);
        match (step.hit, self.chord) {
            (Hit::Drum(key), _) => self.note_on(part.channel, key, velocity, tick, off_tick, frame, out),
            (Hit::Bass(tone), Some(chord)) => {
                let key = bass_note(&chord, tone);
                self.note_on(part.channel, key, velocity, tick, off_tick, frame, out);
            }
            (Hit::Chord, Some(chord)) => {
                let voicing = self.voicing(part.kind, &chord);
                for &key in voicing.notes() {
                    self.note_on(part.channel, key, velocity, tick, off_tick, frame, out);
                }
            }
            (Hit::ChordNote(index), Some(chord)) => {
                // Arpeggios keep the voicing the part already has, so all
                // notes of one pass come from the same inversion.
                let current = if part.kind == PartKind::Pad { self.pad } else { self.keys };
                let voicing = if index == 0 || !current.contains_chord(&chord) { self.voicing(part.kind, &chord) } else { current };
                let notes = voicing.notes();
                if let Some(&base) = notes.get(index as usize % notes.len().max(1)) {
                    let key = base + 12 * (index as usize / notes.len()) as u8;
                    self.note_on(part.channel, key.min(127), velocity, tick, off_tick, frame, out);
                }
            }
            // Harmonic parts wait for the first chord.
            (_, None) => {}
        }
    }

    /// Voices `chord` near the part's previous voicing, drifting back home.
    fn voicing(&mut self, kind: PartKind, chord: &Chord) -> Voicing {
        let (voicing, home, (low, high)) = match kind {
            PartKind::Pad => (&mut self.pad, PAD_HOME, PAD_RANGE),
            _ => (&mut self.keys, KEYS_HOME, KEYS_RANGE),
        };
        *voicing = voice_near(chord, voicing.center().unwrap_or(home) * 0.7 + home * 0.3, low, high);
        *voicing
    }

    #[allow(clippy::too_many_arguments)]
    fn note_on(&mut self, channel: u8, key: u8, velocity: u8, tick: f64, off_tick: f64, frame: u32, out: &mut EventBuffer) {
        if let Some(i) = self.sounding.iter().position(|s| s.active && s.channel == channel && s.key == key) {
            let s = &mut self.sounding[i];
            if (s.on_tick - tick).abs() < 1.0 {
                // Two hits on the same key at once (a crash cue and the
                // pattern's own crash): play one, as long as the longer.
                s.off_tick = s.off_tick.max(off_tick);
                return;
            }
            // Retriggering a sounding key: end the old note first.
            out.push(frame, EngineEvent::NoteOff { channel, key });
            s.active = false;
        }
        let Some(slot) = self.sounding.iter_mut().find(|s| !s.active) else {
            return; // Polyphony budget exhausted; skip rather than hang a note.
        };
        *slot = Sounding { active: true, channel, key, on_tick: tick, off_tick };
        out.push(frame, EngineEvent::NoteOn { channel, key, velocity });
    }

    fn release_all(&mut self, mut filter: impl FnMut(u8) -> bool, frame: u32, out: &mut EventBuffer) {
        for s in self.sounding.iter_mut().filter(|s| s.active && filter(s.channel)) {
            out.push(frame, EngineEvent::NoteOff { channel: s.channel, key: s.key });
            s.active = false;
        }
    }

    fn program_changes(&self, out: &mut EventBuffer) {
        self.program_changes_at(0, out);
    }

    fn program_changes_at(&self, frame: u32, out: &mut EventBuffer) {
        for part in &self.style.parts {
            out.push(frame, EngineEvent::ProgramChange { channel: part.channel, program: part.program });
        }
    }

    fn state(&self) -> BandState {
        let c = &self.conductor;
        BandState {
            style: self.style_index,
            next_style: self.pending_style,
            playing: self.playing,
            section: c.section(),
            variation: c.variation(),
            next_variation: c.next_variation(),
            fill_pending: c.fill_pending(),
            intro_pending: c.intro_pending(),
            ending_pending: c.ending_pending(),
            follow: c.follows(),
            energy: (c.energy() * 255.0).round() as u8,
        }
    }

    /// Reports the band's state to the UI if it changed.
    fn publish(&mut self, frame: u32, out: &mut EventBuffer) {
        let state = self.state();
        if self.published != Some(state) {
            self.published = Some(state);
            out.push(frame, EngineEvent::Band(state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Quality;

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
        let mut band = Band::new(SR);
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

    fn states(events: &[(u64, EngineEvent)]) -> Vec<(u64, BandState)> {
        events
            .iter()
            .filter_map(|(f, e)| match e {
                EngineEvent::Band(s) => Some((*f, *s)),
                _ => None,
            })
            .collect()
    }

    fn assert_no_hanging_notes(events: &[(u64, EngineEvent)]) {
        let mut open = std::collections::HashSet::new();
        for (_f, e) in events {
            match e {
                EngineEvent::NoteOn { channel, key, .. } => assert!(open.insert((*channel, *key)), "double note-on {channel} {key} at {}", _f),
                EngineEvent::NoteOff { channel, key } => {
                    open.remove(&(*channel, *key));
                }
                _ => {}
            }
        }
        assert!(open.is_empty(), "hanging notes: {open:?}");
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
        run(&mut band, 0.6, &mut frame); // well past beat 1 (frame 24000)
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
    fn a_chord_just_after_the_beat_counts_for_that_beat() {
        let (mut band, mut frame) = started_band();
        band.set_latency(20.0);
        run(&mut band, 0.53, &mut frame); // 30 ms after beat 1
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(9, Quality::Minor), &mut out);
        assert!(out.as_slice().iter().any(|t| t.event == EngineEvent::ChordChanged(Some(Chord::new(9, Quality::Minor)))));
        // The bass note of beat 1 is re-struck as A right away.
        assert!(out.as_slice().iter().any(|t| matches!(t.event, EngineEvent::NoteOn { channel: 2, key, .. } if key % 12 == 9)));
    }

    #[test]
    fn every_note_on_gets_a_note_off() {
        for index in 0..styles().len() as u8 {
            let (mut band, mut frame) = started_band();
            let mut out = EventBuffer::default();
            band.set_style(index, &mut out);
            band.stop(&mut out);
            band.play(&mut out);
            out.clear();
            band.set_chord(Chord::new(0, Quality::Major), &mut out);
            let mut events = run(&mut band, 3.3, &mut frame);
            band.set_chord(Chord::new(7, Quality::Dominant7), &mut out);
            band.request(Request::Variation(2), &mut out);
            events.extend(run(&mut band, 4.2, &mut frame));
            band.request(Request::Ending, &mut out);
            events.extend(run(&mut band, 5.0, &mut frame));
            assert!(!band.is_playing(), "{}: the ending stops the band", styles()[index as usize].name);
            assert_no_hanging_notes(&events);
        }
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
        let mut band = Band::new(SR);
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(2, Quality::Minor), &mut out);
        assert_eq!(band.chord(), Some(Chord::new(2, Quality::Minor)));
        assert_eq!(out.as_slice().len(), 1);
    }

    #[test]
    fn style_changes_wait_for_the_bar_line() {
        let (mut band, mut frame) = started_band();
        run(&mut band, 0.6, &mut frame);
        let mut out = EventBuffer::default();
        band.set_style(1, &mut out);
        assert_eq!(states(&out.as_slice().iter().map(|t| (0, t.event)).collect::<Vec<_>>())[0].1.next_style, Some(1));
        let events = run(&mut band, 2.0, &mut frame);
        let change = events.iter().find(|(_, e)| matches!(e, EngineEvent::ProgramChange { channel: 2, program: 34 })).unwrap();
        assert_eq!(change.0, 96_000, "rock bass arrives on bar 2");
        assert_eq!(states(&events).last().unwrap().1.style, 1);
    }

    #[test]
    fn intro_runs_one_bar_and_reports_sections() {
        let mut band = Band::new(SR);
        band.set_tempo(120.0);
        let mut out = EventBuffer::default();
        band.request(Request::Intro, &mut out);
        assert!(band.is_playing());
        assert_eq!(band.conductor().section(), Section::Intro);
        let mut frame = 0;
        let events = run(&mut band, 2.2, &mut frame);
        let main = states(&events).into_iter().find(|(_, s)| s.section == Section::Main).unwrap();
        assert_eq!(main.0, 96_000);
        assert!(note_ons(&events, 9).contains(&(96_000, CRASH)), "crash on the first bar after the intro");
    }

    #[test]
    fn rest_keeps_only_the_pad() {
        let (mut band, mut frame) = started_band();
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(0, Quality::Major), &mut out);
        band.note_played(90);
        run(&mut band, 2.0, &mut frame);
        let events = run(&mut band, 6.0, &mut frame);
        let rest_at = states(&events).into_iter().find(|(_, s)| s.section == Section::Rest).unwrap().0;
        let after: Vec<_> = events.iter().filter(|(f, _)| *f > rest_at).cloned().collect();
        assert!(note_ons(&after, 9).is_empty() && note_ons(&after, 2).is_empty());
        assert!(!note_ons(&after, 3).is_empty(), "the pad holds the chord");

        band.note_played(90);
        let events = run(&mut band, 0.6, &mut frame);
        assert!(!note_ons(&events, 9).is_empty(), "drums come back");
    }

    #[test]
    fn click_mode_plays_one_click_per_beat() {
        let (mut band, mut frame) = started_band();
        let mut out = EventBuffer::default();
        band.set_click(true, &mut out);
        band.set_chord(Chord::new(0, Quality::Major), &mut out);
        let events = run(&mut band, 2.1, &mut frame);
        let ons: Vec<_> = events.iter().filter(|(_, e)| matches!(e, EngineEvent::NoteOn { .. })).collect();
        assert_eq!(ons.len(), 5);
        assert!(note_ons(&events, 9).iter().all(|(_, k)| *k == WOOD_BLOCK));
    }

    /// Am for two beats then F, then G | C, in variation B from bar 1.
    fn song_band() -> (Band, u64) {
        let mut band = Band::new(SR);
        band.set_tempo(120.0);
        band.set_song(true);
        band.clear_song(2);
        band.set_song_slot(0, Slot::Chord(Chord::new(9, Quality::Minor)));
        band.set_song_slot(2, Slot::Chord(Chord::new(5, Quality::Major)));
        band.set_song_slot(4, Slot::Chord(Chord::new(7, Quality::Major)));
        band.set_song_slot(6, Slot::Chord(Chord::new(0, Quality::Major)));
        band.set_song_mark(1, Some(Request::Variation(1)));
        let mut out = EventBuffer::default();
        band.play(&mut out);
        (band, 0)
    }

    #[test]
    fn songs_play_their_chart_and_end() {
        let (mut band, mut frame) = song_band();
        let mut out = EventBuffer::default();
        band.set_chord(Chord::new(2, Quality::Minor), &mut out); // ignored while the song plays
        let events = run(&mut band, 6.5, &mut frame);
        let chords: Vec<_> = events
            .iter()
            .filter_map(|(f, e)| match e {
                EngineEvent::ChordChanged(Some(c)) => Some((*f, c.to_string())),
                _ => None,
            })
            .collect();
        let expected = [(0, "Am"), (48_000, "F"), (96_000, "G"), (144_000, "C")];
        assert_eq!(chords, expected.map(|(f, n)| (f, n.to_string())));

        let bars: Vec<_> = events
            .iter()
            .filter_map(|(f, e)| matches!(e, EngineEvent::SongPosition { .. }).then_some(*f))
            .collect();
        assert_eq!(bars, [0, 96_000, 192_000], "the ending bar follows the chart");

        let sections: Vec<_> = states(&events).iter().map(|(f, s)| (*f, s.section, s.variation)).collect();
        assert!(sections.contains(&(48_000, Section::Fill, 0)), "{sections:?}");
        assert!(sections.contains(&(96_000, Section::Main, 1)), "{sections:?}");
        assert!(sections.contains(&(192_000, Section::Ending, 1)), "{sections:?}");
        assert!(!band.is_playing(), "the band stops after the ending");
        assert_no_hanging_notes(&events);
    }

    #[test]
    fn songs_play_through_silence() {
        let (mut band, mut frame) = song_band();
        band.set_song_loop(0, 2);
        band.note_played(90);
        let events = run(&mut band, 12.0, &mut frame);
        assert!(states(&events).iter().all(|(_, s)| s.section != Section::Rest && s.section != Section::Ending));
        assert!(band.is_playing());
    }

    #[test]
    fn energy_scales_velocity() {
        let velocities = |energy: f32| {
            let (mut band, mut frame) = started_band();
            let mut out = EventBuffer::default();
            band.set_follow(false, &mut out);
            band.set_energy(energy, &mut out);
            run(&mut band, 1.0, &mut frame)
                .iter()
                .filter_map(|(_, e)| match e {
                    EngineEvent::NoteOn { velocity, .. } => Some(*velocity as u32),
                    _ => None,
                })
                .sum::<u32>()
        };
        assert!(velocities(1.0) > velocities(0.0) * 13 / 10);
    }
}
