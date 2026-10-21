//! The backing band: plays a style's patterns over the current chord, in the
//! section the conductor picks.

use crate::chord::Chord;
use crate::conductor::{Conductor, IdleMode, Request, Section};
use crate::drummer::{FillDetector, keeps_time, time_weight};
use crate::drums::Piece;
use crate::event::{BandState, DRUM_CHANNEL, EngineEvent, EventBuffer};
use crate::song::{Slot, SongPlayer};
use crate::style::{
    BEATS_PER_BAR, CRASH, HAT, Hit, OPEN_HAT, PPQ, Part, PartKind, Pattern, RIDE, SHAKER, SIDE_STICK, SNARE, Step, Style, TAMBOURINE,
    TICKS_PER_BAR, WOOD_BLOCK, styles,
};
use crate::voicing::{Voicing, bass_note, voice_near};

const MAX_SOUNDING: usize = 64;
const KEYS_RANGE: (u8, u8) = (55, 79);
const PAD_RANGE: (u8, u8) = (48, 72);
const KEYS_HOME: f32 = 64.0;
const PAD_HOME: f32 = 60.0;
/// Human slack on top of the measured latency when latching a late chord.
const LATE_SLACK_MS: f32 = 60.0;
/// Tempo follow: what counts as one attack (a chord's notes come within this
/// many seconds).
const ONSET_MERGE_S: f64 = 0.05;
/// How far the band may drift from the set tempo, and how fast it moves.
#[derive(Clone, Copy)]
struct FollowProfile {
    range: f32,
    gain: f32,
}
const KEYS_FOLLOW: FollowProfile = FollowProfile { range: 0.15, gain: 0.3 };
/// A drummer owns the time: a wider range and a quicker response.
const DRUMS_FOLLOW: FollowProfile = FollowProfile { range: 0.25, gain: 0.45 };
/// A crash this close to a bar line counts as on its 1.
const CRASH_WINDOW: f64 = PPQ as f64 / 4.0;
/// Tempo follow reports a new tempo to the UI once it moved this much.
const TEMPO_REPORT_STEP: f32 = 0.5;

/// Tempo follow: compares the player's attacks with the band's eighth-note grid.
#[derive(Clone, Copy, Default)]
struct TempoFollow {
    on: bool,
    /// The tempo the player set; following stays within range of it.
    base_bpm: f32,
    /// The last attack: seconds since Play and the grid tick it was nearest.
    last: Option<(f64, f64)>,
    /// Smoothed tempo of the player's attacks.
    player_bpm: f32,
    /// Smoothed timing error in beats; negative when the player is early.
    phase: f32,
    reported_bpm: f32,
}

/// What the band's own drums play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BandDrums {
    #[default]
    Full,
    Off,
    /// Shaker and tambourine along the style's groove.
    Percussion,
    /// A wood block on every beat.
    Click,
}

impl BandDrums {
    pub const ALL: [BandDrums; 4] = [BandDrums::Full, BandDrums::Off, BandDrums::Percussion, BandDrums::Click];
}

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
    /// A song tick where the band stops and waits for the player.
    wait: Option<u32>,
    /// While waiting: the band tick it stopped at and the song tick.
    hold: Option<(f64, u32)>,
    /// The band tick of the song beat playing, and that song beat.
    song_beat: Option<(f64, u32)>,
    tempo: TempoFollow,
    /// A drummer leads: tempo follow is always on and tuned for drums, and
    /// fills move the band on.
    drummer: bool,
    drum_layer: BandDrums,
    fills: FillDetector,
    /// The band tick the drummer's last crash was meant for.
    last_crash: Option<f64>,
    /// While a fill is cued: the latest band tick a crash still takes it.
    cue_deadline: Option<f64>,
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
            wait: None,
            hold: None,
            song_beat: None,
            tempo: TempoFollow { base_bpm: style.default_bpm, player_bpm: style.default_bpm, reported_bpm: style.default_bpm, ..TempoFollow::default() },
            drummer: false,
            drum_layer: BandDrums::Full,
            fills: FillDetector::default(),
            last_crash: None,
            cue_deadline: None,
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

    /// The player set the tempo; tempo follow starts again from here.
    pub fn set_tempo(&mut self, bpm: f32) {
        self.retime(bpm);
        let bpm = self.bpm;
        self.tempo = TempoFollow { on: self.tempo.on, base_bpm: bpm, player_bpm: bpm, reported_bpm: bpm, ..TempoFollow::default() };
    }

    fn retime(&mut self, bpm: f32) {
        self.anchor_tick = self.clock(0).tick_at(self.frame);
        self.anchor_frame = self.frame;
        self.bpm = bpm.clamp(30.0, 300.0);
    }

    /// Whether the band speeds up and slows down with the player. Turning it
    /// off goes back to the set tempo.
    pub fn set_tempo_follow(&mut self, on: bool, out: &mut EventBuffer) {
        self.tempo.on = on;
        if !on {
            let base = self.tempo.base_bpm;
            self.set_tempo(base);
            out.push(0, EngineEvent::Tempo { bpm: base });
        }
    }

    /// A drummer leads the band, or no longer does. Without the tempo follow
    /// setting the band goes back to the set tempo.
    pub fn set_drummer(&mut self, on: bool, out: &mut EventBuffer) {
        if on == self.drummer {
            return;
        }
        self.drummer = on;
        self.conductor.set_drummer(on);
        self.fills.reset();
        if !on && !self.tempo.on {
            self.set_tempo_follow(false, out);
        }
        self.publish(0, out);
    }

    pub fn set_drum_layer(&mut self, layer: BandDrums, out: &mut EventBuffer) {
        if layer != self.drum_layer {
            self.drum_layer = layer;
            self.release_all(|ch| ch == DRUM_CHANNEL, 0, out);
        }
    }

    /// The player attacked a note or strum, `late_ms` after it was played on
    /// top of the usual latency. With tempo follow on, attacks near the
    /// eighth-note grid pull the tempo towards the player's.
    pub fn onset(&mut self, late_ms: f32) {
        self.follow_onset(late_ms, (1.0, 1.0));
    }

    /// Tempo follow from one attack, weighted by how much it tells about the
    /// tempo: `weight.0` on a beat, `weight.1` between beats.
    fn follow_onset(&mut self, late_ms: f32, weight: (f32, f32)) {
        if !(self.tempo.on || self.drummer) || !self.playing || self.hold.is_some() || self.click {
            return;
        }
        let profile = if self.drummer { DRUMS_FOLLOW } else { KEYS_FOLLOW };
        let late_s = ((self.latency_ms + late_ms) / 1000.0) as f64;
        let seconds = self.frame as f64 / self.sample_rate as f64 - late_s;
        let tick = self.clock(1).tick_at(self.frame) - late_s * self.ticks_per_second();
        let grid = PPQ as f64 / 2.0;
        let nearest = (tick / grid).round() * grid;
        let error = tick - nearest;
        let w = if nearest % PPQ as f64 == 0.0 { weight.0 } else { weight.1 };
        let t = &mut self.tempo;
        if w <= 0.0 {
            return;
        }
        if let Some((last_s, _)) = t.last
            && seconds - last_s < ONSET_MERGE_S
        {
            return;
        }
        if error.abs() > grid * 0.3 {
            return;
        }
        let gain = profile.gain * w;
        if let Some((last_s, last_tick)) = t.last {
            let beats = (nearest - last_tick) / PPQ as f64;
            let dt = seconds - last_s;
            if beats >= 0.5 && (0.15..=2.5).contains(&dt) {
                let heard = (60.0 * beats / dt) as f32;
                if (heard / self.bpm - 1.0).abs() < 0.25 {
                    t.player_bpm += gain * (heard - t.player_bpm);
                }
            }
        }
        t.last = Some((seconds, nearest));
        t.phase += gain * ((error / PPQ as f64) as f32 - t.phase);
        // Match the player's tempo, a little faster while they are ahead.
        let target = t.player_bpm * (1.0 - 0.5 * t.phase);
        let (lo, hi) = (t.base_bpm * (1.0 - profile.range), t.base_bpm * (1.0 + profile.range));
        let bpm = (self.bpm + 0.5 * w * (target - self.bpm)).clamp(lo, hi);
        self.retime(bpm);
    }

    /// The drummer hit `piece`. Reports how far off the band's grid it was,
    /// listens for fills and the crash that confirms one, and follows the tempo.
    pub fn drum_hit(&mut self, piece: Piece, velocity: u8, out: &mut EventBuffer) {
        if !self.playing || self.hold.is_some() || self.click {
            return;
        }
        let clock = self.clock(1);
        let now = clock.tick_at(self.frame);
        let tick = now - self.latency_ms as f64 / 1000.0 * self.ticks_per_second();
        if keeps_time(piece, velocity) {
            // Sixteenths, or triplets in a shuffle.
            let grid = PPQ as f64 / if self.style.swung() { 3.0 } else { 4.0 };
            let error = tick - (tick / grid).round() * grid;
            out.push(0, EngineEvent::Timing { ms: (error / self.ticks_per_second() * 1000.0) as f32 });
        }
        let bar = (tick / TICKS_PER_BAR as f64).floor();
        // In a song the chart has the sections; a fill there is only a fill.
        if self.fills.hit(piece, velocity, bar as i64, tick - bar * TICKS_PER_BAR as f64) && !self.song.active() {
            self.conductor.cue_next();
            self.publish(0, out);
        }
        if piece.is_crash() {
            self.last_crash = Some(tick);
            if self.conductor.cued() && self.cue_deadline.is_some_and(|d| tick <= d) {
                // Just after the 1: the band changes now.
                self.cue_deadline = None;
                self.conductor.take_cue();
                self.restrike(now, &clock, out);
                self.publish(0, out);
            }
        }
        let weight = if self.fills.in_fill(bar as i64) { (0.0, 0.0) } else { time_weight(piece, velocity) };
        self.follow_onset(0.0, weight);
    }

    /// Starts the band at `bpm`, from a count-in.
    pub fn start_counted(&mut self, bpm: f32, out: &mut EventBuffer) {
        self.set_tempo(bpm);
        self.tempo.reported_bpm = self.bpm;
        out.push(0, EngineEvent::Tempo { bpm: self.bpm });
        self.play(out);
    }

    #[cfg(test)]
    fn bpm(&self) -> f32 {
        self.bpm
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
        let Some(style) = styles().get(index as usize) else {
            return;
        };
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

    pub fn latency_ms(&self) -> f32 {
        self.latency_ms
    }

    /// The variation a "next section" control moves to.
    pub fn next_section(&self) -> u8 {
        self.conductor.following_variation()
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

    /// Where the band stops and waits for the player, in song ticks; `None`
    /// clears it. A wait that moves while the band holds lets it go on.
    pub fn set_song_wait(&mut self, tick: Option<u32>) {
        self.wait = tick;
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
    /// `late_ms` is how much later than a key press this chord was recognised
    /// (e.g. from a microphone).
    pub fn set_chord(&mut self, chord: Chord, late_ms: f32, out: &mut EventBuffer) {
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
        let grace = ((self.latency_ms + late_ms + LATE_SLACK_MS) / 1000.0) as f64 * self.ticks_per_second();
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
        self.hold = None;
        self.song_beat = None;
        self.tempo.last = None;
        self.tempo.phase = 0.0;
        self.fills.reset();
        self.last_crash = None;
        self.cue_deadline = None;
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
        self.hold = None;
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
        if let Some((_, song_tick)) = self.hold {
            if self.song.active() && self.wait == Some(song_tick) {
                self.hold_block(frames);
                return;
            }
            self.hold = None;
            out.push(0, EngineEvent::Hold { holding: false, tick: song_tick });
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
            if let Some(at) = self.wait_between(seg_start, seg_end) {
                self.schedule_range(seg_start, at, &clock, out);
                self.start_hold(at, &clock, frames, out);
                return;
            }
            self.schedule_range(seg_start, seg_end, &clock, out);
            if beat_tick >= end {
                break;
            }
            if self.waits_on_beat(beat) {
                self.start_hold(beat_tick, &clock, frames, out);
                return;
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

    /// A wait inside the song beat playing, as a band tick in `from..to`.
    fn wait_between(&self, from: f64, to: f64) -> Option<f64> {
        let wait = self.wait.filter(|_| self.song.active() && !self.click)?;
        let (beat_tick, beat) = self.song_beat?;
        let into = wait.checked_sub(beat * PPQ).filter(|t| *t > 0 && *t < PPQ)?;
        let at = beat_tick + into as f64;
        (at >= from && at < to).then_some(at)
    }

    /// Whether the band waits right at `beat`, before playing it.
    fn waits_on_beat(&self, beat: u64) -> bool {
        let beat_in_bar = (beat % BEATS_PER_BAR as u64) as u8;
        self.song.active() && !self.click && self.wait == Some(self.song.upcoming(beat_in_bar) * PPQ)
    }

    /// Stops the clock at `tick` for the rest of the block and until the wait
    /// moves. Drums and bass stop; the harmony rings on.
    fn start_hold(&mut self, tick: f64, clock: &BlockClock, frames: u32, out: &mut EventBuffer) {
        let song_tick = self.wait.unwrap_or(0);
        let frame = clock.frame(tick);
        let style = self.style;
        let rhythm = |ch: u8| style.parts.iter().any(|p| p.channel == ch && matches!(p.kind, PartKind::Drums | PartKind::Bass));
        self.release_all(rhythm, frame, out);
        out.push(frame, EngineEvent::Hold { holding: true, tick: song_tick });
        self.hold = Some((tick, song_tick));
        self.anchor_tick = tick;
        self.hold_block(frames);
    }

    /// Lets frames pass without moving the band's clock.
    fn hold_block(&mut self, frames: u32) {
        self.frame += frames as u64;
        self.anchor_frame = self.frame;
    }

    fn on_beat(&mut self, beat: u64, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        let frame = clock.frame(tick);
        let beat_in_bar = (beat % BEATS_PER_BAR as u64) as u8;
        out.push(frame, EngineEvent::Beat { bar: (beat / BEATS_PER_BAR as u64) as u32, beat: beat_in_bar });
        if (self.bpm - self.tempo.reported_bpm).abs() >= TEMPO_REPORT_STEP {
            self.tempo.reported_bpm = self.bpm;
            out.push(frame, EngineEvent::Tempo { bpm: self.bpm });
        }

        let velocity = if beat_in_bar == 0 { 120 } else { 90 };
        if self.click {
            self.note_on(DRUM_CHANNEL, WOOD_BLOCK, velocity, tick, tick + PPQ as f64 / 4.0, frame, out);
            return;
        }
        if self.drum_layer == BandDrums::Click && self.muted & (1 << DRUM_CHANNEL) == 0 {
            self.note_on(DRUM_CHANNEL, WOOD_BLOCK, self.scale(velocity), tick, tick + PPQ as f64 / 4.0, frame, out);
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
            self.song_beat = Some((tick, song.beat));
            out.push(frame, EngineEvent::SongPosition { beat: song.beat });
            if let Some(chord) = song.chord {
                self.pending = chord;
            }
            if let Some(request) = song.request {
                self.conductor.request(request, true);
            }
        }
        self.check_cue(beat_in_bar, tick);
        if cue.crash && self.drum_layer == BandDrums::Full && self.muted & (1 << DRUM_CHANNEL) == 0 {
            self.note_on(DRUM_CHANNEL, CRASH, self.scale(110), tick, tick + PPQ as f64, frame, out);
        }

        if self.pending != self.chord {
            self.change_chord(tick, clock, out);
        }
        self.publish(frame, out);
    }

    /// A cued fill on the 1: a crash just before the bar line takes it now;
    /// otherwise one may still come a little after it, until the next beat.
    fn check_cue(&mut self, beat_in_bar: u8, tick: f64) {
        if !self.conductor.cued() {
            self.cue_deadline = None;
        } else if beat_in_bar == 0 {
            if self.last_crash.is_some_and(|t| t >= tick - CRASH_WINDOW && t <= tick) {
                self.conductor.take_cue();
            } else {
                self.cue_deadline = Some(tick + CRASH_WINDOW);
            }
        } else if self.cue_deadline.take().is_some() {
            self.conductor.drop_cue();
        }
    }

    /// Applies the pending chord at `tick`.
    fn change_chord(&mut self, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        self.chord = self.pending;
        out.push(clock.frame(tick), EngineEvent::ChordChanged(self.chord));
        self.restrike(tick, clock, out);
    }

    /// Cuts the harmony at `tick` and re-strikes the notes the patterns still
    /// hold there, for a new chord or variation. Steps starting exactly at
    /// `tick` are left to the regular schedule.
    fn restrike(&mut self, tick: f64, clock: &BlockClock, out: &mut EventBuffer) {
        let frame = clock.frame(tick);
        let style = self.style;
        let harmonic = |ch: u8| style.parts.iter().any(|p| p.kind != PartKind::Drums && p.channel == ch);
        self.release_all(harmonic, frame, out);

        let bar = (tick / TICKS_PER_BAR as f64).floor();
        let bar_tick = bar * TICKS_PER_BAR as f64;
        for part in style.parts.iter().filter(|p| p.kind != PartKind::Drums) {
            let Some(pattern) = self.pattern(part) else {
                continue;
            };
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
                let Some(pattern) = self.pattern(part) else {
                    continue;
                };
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
            (Hit::Drum(key), _) => {
                if let Some((key, velocity)) = self.layer_drum(key, velocity) {
                    self.note_on(part.channel, key, velocity, tick, off_tick, frame, out);
                }
            }
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

    /// What the band's drum layer plays for a drum step of the style.
    fn layer_drum(&self, key: u8, velocity: u8) -> Option<(u8, u8)> {
        /// The GM pedal hi-hat, rim shot and ride bell.
        const PEDAL_HAT: u8 = 44;
        const RIM: u8 = 40;
        const RIDE_BELL: u8 = 53;
        match self.drum_layer {
            BandDrums::Full => Some((key, velocity)),
            BandDrums::Off | BandDrums::Click => None,
            BandDrums::Percussion => {
                let key = match key {
                    HAT | OPEN_HAT | PEDAL_HAT | RIDE | RIDE_BELL | SHAKER => SHAKER,
                    SNARE | RIM | SIDE_STICK | TAMBOURINE => TAMBOURINE,
                    _ => return None,
                };
                Some((key, (velocity as u16 * 3 / 4).max(1) as u8))
            }
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
            cued: c.cued(),
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
        band.set_chord(Chord::new(9, Quality::Minor), 0.0, &mut out);
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
        band.set_chord(Chord::new(9, Quality::Minor), 0.0, &mut out);
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
            band.set_chord(Chord::new(0, Quality::Major), 0.0, &mut out);
            let mut events = run(&mut band, 3.3, &mut frame);
            band.set_chord(Chord::new(7, Quality::Dominant7), 0.0, &mut out);
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
        band.set_chord(Chord::new(0, Quality::Major), 0.0, &mut out);
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
        band.set_chord(Chord::new(2, Quality::Minor), 0.0, &mut out);
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
        band.set_chord(Chord::new(0, Quality::Major), 0.0, &mut out);
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
        band.set_chord(Chord::new(0, Quality::Major), 0.0, &mut out);
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
        band.set_chord(Chord::new(2, Quality::Minor), 0.0, &mut out); // ignored while the song plays
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
            .filter_map(|(f, e)| matches!(e, EngineEvent::SongPosition { beat } if beat % 4 == 0).then_some(*f))
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

    fn holds(events: &[(u64, EngineEvent)]) -> Vec<(u64, bool, u32)> {
        events
            .iter()
            .filter_map(|(f, e)| match e {
                EngineEvent::Hold { holding, tick } => Some((*f, *holding, *tick)),
                _ => None,
            })
            .collect()
    }

    fn song_beats(events: &[(u64, EngineEvent)]) -> Vec<(u64, u32)> {
        events
            .iter()
            .filter_map(|(f, e)| match e {
                EngineEvent::SongPosition { beat } => Some((*f, *beat)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn waits_on_a_beat_until_the_wait_moves() {
        let (mut band, mut frame) = song_band();
        band.set_song_wait(Some(4 * PPQ));
        let events = run(&mut band, 4.0, &mut frame);
        assert_eq!(holds(&events), [(96_000, true, 4 * PPQ)]);
        assert_eq!(song_beats(&events).last(), Some(&(72_000, 3)), "bar 1 is not played yet");
        assert!(note_ons(&events, 9).iter().all(|(f, _)| *f < 96_000), "the drums stop");
        assert!(band.is_playing());

        band.set_song_wait(None);
        let resume = frame;
        let events = run(&mut band, 1.0, &mut frame);
        assert_eq!(holds(&events), [(resume, false, 4 * PPQ)]);
        assert_eq!(song_beats(&events)[0], (resume, 4));
        let chord = events.iter().find(|(_, e)| matches!(e, EngineEvent::ChordChanged(_))).unwrap();
        assert_eq!((chord.0, chord.1), (resume, EngineEvent::ChordChanged(Some(Chord::new(7, Quality::Major)))));
        assert!(note_ons(&events, 9).iter().any(|(f, _)| *f == resume), "the band comes back in on the beat");
    }

    #[test]
    fn waits_inside_a_beat() {
        let (mut band, mut frame) = song_band();
        band.set_song_wait(Some(2 * PPQ + PPQ / 2));
        let events = run(&mut band, 3.0, &mut frame);
        assert_eq!(holds(&events), [(60_000, true, 2 * PPQ + PPQ / 2)]);
        band.set_song_wait(Some(6 * PPQ)); // the next note to wait for
        let resume = frame;
        let events = run(&mut band, 0.5, &mut frame);
        assert_eq!(holds(&events)[0], (resume, false, 2 * PPQ + PPQ / 2));
        assert_eq!(song_beats(&events)[0], (resume + 12_000, 3), "half a beat after going on");
    }

    #[test]
    fn a_wait_already_passed_does_not_hold() {
        let (mut band, mut frame) = song_band();
        run(&mut band, 0.75, &mut frame);
        band.set_song_wait(Some(PPQ));
        let events = run(&mut band, 2.0, &mut frame);
        assert!(holds(&events).is_empty());
        let mut out = EventBuffer::default();
        band.stop(&mut out);
        band.set_song_wait(Some(0));
        band.play(&mut out);
        let events = run(&mut band, 1.0, &mut frame);
        assert_eq!(holds(&events).len(), 1, "a new performance can wait right at its start");
        assert!(song_beats(&events).is_empty());
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

    /// Plays attacks every `beats` at `player_bpm` for `seconds`, starting on
    /// the band's first beat. Returns the reported tempos.
    fn play_along(band: &mut Band, frame: &mut u64, player_bpm: f32, beats: f32, seconds: f32) -> Vec<f32> {
        let period = (60.0 / player_bpm * beats * SR) as u64;
        let mut next = *frame;
        let mut tempos = Vec::new();
        let mut out = EventBuffer::default();
        for _ in 0..(seconds * SR / BLOCK as f32) as usize {
            if *frame >= next {
                band.onset(0.0);
                next += period;
            }
            band.process(BLOCK, &mut out);
            tempos.extend(out.as_slice().iter().filter_map(|t| match t.event {
                EngineEvent::Tempo { bpm } => Some(bpm),
                _ => None,
            }));
            out.clear();
            *frame += BLOCK as u64;
        }
        tempos
    }

    fn following_band() -> (Band, u64) {
        let (mut band, frame) = started_band();
        band.set_latency(0.0);
        band.set_tempo_follow(true, &mut EventBuffer::default());
        (band, frame)
    }

    #[test]
    fn tempo_follow_is_off_by_default() {
        let (mut band, mut frame) = started_band();
        band.set_latency(0.0);
        assert!(play_along(&mut band, &mut frame, 130.0, 1.0, 8.0).is_empty());
        assert_eq!(band.bpm(), 120.0);
    }

    #[test]
    fn steady_player_keeps_the_tempo() {
        let (mut band, mut frame) = following_band();
        assert!(play_along(&mut band, &mut frame, 120.0, 0.5, 8.0).is_empty());
        assert!((band.bpm() - 120.0).abs() < 0.5, "{}", band.bpm());
    }

    #[test]
    fn follows_a_player_who_speeds_up_or_slows_down() {
        for player in [128.0, 112.0] {
            let (mut band, mut frame) = following_band();
            let tempos = play_along(&mut band, &mut frame, player, 1.0, 16.0);
            assert!((band.bpm() - player).abs() < 2.0, "player {player}, band {}", band.bpm());
            assert!(!tempos.is_empty(), "tempo changes are reported");
        }
    }

    #[test]
    fn tempo_follow_stays_near_the_set_tempo_and_turns_off_cleanly() {
        let (mut band, mut frame) = following_band();
        play_along(&mut band, &mut frame, 150.0, 1.0, 20.0);
        assert!(band.bpm() <= 120.0 * 1.15 + 0.01, "{}", band.bpm());
        let mut out = EventBuffer::default();
        band.set_tempo_follow(false, &mut out);
        assert_eq!(band.bpm(), 120.0);
        assert!(out.as_slice().iter().any(|t| t.event == EngineEvent::Tempo { bpm: 120.0 }));
    }
}
