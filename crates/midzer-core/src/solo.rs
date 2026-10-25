//! Improvising for a player who can't yet: the player taps, midzer solos.
//!
//! On level 1 the player taps one key, the song's home note, and a solo
//! plays itself on the beat grid the player hears. The faster the taps, the
//! busier, higher and louder it gets; the harder, the louder. When they
//! stop, the phrase ends on a chord tone and the solo rests. Holding the key
//! lands on a long note until it is let go, with vibrato.
//!
//! Each level hands over more: on level 2 three keys (home, fifth, octave)
//! steer the register; on level 3 the player plays the notes, snapped to the
//! scale, and midzer fills the gaps on the grid; on level 4 it only snaps;
//! on level 5 the notes pass unchanged.
//!
//! With call and response on, midzer plays a one-bar call and the player
//! answers it in the next bar: on levels 1–2 each tap plays the call's next
//! note, so tapping its rhythm brings the lick back; from level 3 the player
//! plays the notes. The answer is scored on rhythm, and from level 3 on the
//! notes too, and the calls grow longer as the answers get them right.
//!
//! Every note the player hears goes out as a [`EngineEvent::SoloNote`] too,
//! the player's and midzer's, so the UI can record the solo and replay it
//! (see [`crate::replay`]).
//!
//! Notes come from the song's scale (a pentatonic the UI sends with the chord
//! heard, see [`crate::Command::SoloHarmony`]), chord tones on the strong
//! beats. Everything is fixed state: nothing allocates.

use crate::engine::PLAYER_CHANNEL;
use crate::event::{CallPhase, EngineEvent, EventBuffer};
use crate::style::{PPQ, TICKS_PER_BAR};

/// The highest level the UI may ask for.
pub const MAX_SOLO_LEVEL: u8 = 5;
/// The most a lead may be, in ms (see [`Solo::set_level`]).
pub const MAX_LEAD_MS: f32 = 500.0;

/// Taps per beat counted at most.
const MAX_RATE: f64 = 4.0;
/// How fast the energy follows the taps, up and down, in beats.
const RISE_BEATS: f64 = 0.25;
const FALL_BEATS: f64 = 0.75;
/// Below this energy the solo is resting.
const SILENT: f32 = 0.06;
/// A tap after this many beats without a note sounds at once instead of waiting for the grid.
const PICKUP_AFTER_BEATS: f64 = 1.0;
/// Holding the key this many beats asks for a long note.
const HOLD_BEATS: f64 = 0.3;
/// How often the energy goes out at most, in seconds.
const FEEL_EVERY_S: f32 = 0.1;
/// The scale used before the UI says which: C major pentatonic.
const DEFAULT_SCALE: u16 = 0b0010_1001_0101;
/// From this level the player plays the notes; from the next one midzer adds nothing.
const OWN_NOTES_FROM: u8 = 3;
/// From this level the player's notes are judged like any playing along.
const JUDGED_FROM: u8 = 4;
/// Level 5 plays the notes as they are.
const AS_PLAYED: u8 = 5;
/// Level 2's low and high keys move the solo's range this many semitones.
const STEER_LOW: f32 = -5.0;
const STEER_HIGH: f32 = 7.0;
/// Level 3 fills only this long after the player's last note.
const FILL_AFTER_BEATS: f64 = 0.5;
/// Notes this close together are one chord, not faster playing.
const CHORD_BEATS: f64 = 0.1;
/// A long note's vibrato starts this far in and reaches its depth (CC1) this much later.
const VIBRATO_AFTER_BEATS: f64 = 0.5;
const VIBRATO_BEATS: f64 = 1.0;
const VIBRATO_DEPTH: f64 = 64.0;
/// No key held, in the own-note map.
const NONE: u8 = 0xff;
/// A call's notes: the first call's, the fewest and the most.
const FIRST_CALL: u8 = 3;
const MIN_CALL: u8 = 2;
const MAX_CALL: usize = 8;
/// Calls are played at this energy, whatever the taps.
const CALL_ENERGY: f32 = 0.45;
/// A call's last note starts at this sixteenth of its bar at the latest, so
/// the answer's last note is in before the next call.
const CALL_LAST: u8 = 12;
/// An answer's note counts for the call's within this many sixteenths.
const ANSWER_SLACK: f64 = 0.75;
/// The most notes of an answer counted.
const MAX_ANSWER: usize = 32;
/// After this many answers in a row with no notes the calls wait for a tap.
const UNANSWERED_REST: u8 = 2;
/// Sixteenths in a bar.
const BAR: i64 = 16;

/// What a key press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    /// Not soloing: the key plays as usual.
    Free,
    /// It drives the solo, or is silent on this level.
    Drove,
    /// The player's own note, sounding as this key.
    Played(u8),
}

/// Where the player hears the beats: beat `index` at engine frame `at`,
/// `period` frames apart; beat `one` is a 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub at: f64,
    pub index: i64,
    pub period: f64,
    pub one: i64,
}

impl Grid {
    /// The frame of sixteenth `q` (four per beat, counted like the beats).
    fn frame(&self, q: i64) -> f64 {
        self.at + (q as f64 / 4.0 - self.index as f64) * self.period
    }

    pub(crate) fn sixteenth_at(&self, frame: f64) -> f64 {
        (self.index as f64 + (frame - self.at) / self.period) * 4.0
    }

    fn beat_in_bar(&self, q: i64) -> i64 {
        (q.div_euclid(4) - self.one).rem_euclid(4)
    }
}

/// A long note asked for by holding the key.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Long {
    None,
    /// It starts on the next strong sixteenth.
    Waiting,
    Sounding,
}

pub struct Solo {
    level: u8,
    sample_rate: f32,
    /// How much earlier than heard the notes are sent: the output's latency, in frames.
    lead: f64,
    tonic: u8,
    scale: u16,
    chord: u16,
    /// Taps per beat, smoothed, and the frame of the last tap.
    rate: f64,
    last_tap: Option<u64>,
    /// The taps' velocity, smoothed.
    touch: f32,
    energy: f32,
    /// The driving key held, and since when.
    held: Option<(u8, u64)>,
    long: Long,
    /// When the long note started, and the vibrato (CC1) sent for it.
    long_from: f64,
    modulation: u8,
    /// Level 2: the low (-1), middle (0) or high (1) key last tapped.
    steer: i8,
    /// The key sounding for each key the player holds (levels 3–5), how many
    /// held keys sound each key, how many are held and when the last began.
    own: [u8; 128],
    own_count: [u8; 128],
    own_held: u8,
    last_own: f64,
    /// The next sixteenth to play or skip.
    next: Option<i64>,
    /// This beat's sixteenths to play (a bit each), and the last beat's.
    mask: u8,
    last_mask: u8,
    phrase_left: u8,
    rest_left: u8,
    /// The phrase ends on this beat's first note.
    ending: bool,
    /// The note sounding and when it ends (engine frames), and a grace note's.
    sounding: Option<(u8, f64)>,
    grace: Option<(u8, f64)>,
    /// The note sounding is the player's: an answer's tap (levels 1–2).
    sounding_own: bool,
    /// The beats last heard, and the engine frame last seen, to place the notes for the replay.
    grid: Option<Grid>,
    now: u64,
    /// The last note played, and when.
    pitch: u8,
    rising: bool,
    last_note: f64,
    rng: u32,
    feel_in: u32,
    feel_every: u32,
    feel: Option<u8>,
    /// Call and response is on, and where it is.
    call: bool,
    phase: CallPhase,
    /// A tap asked for a call.
    armed: bool,
    /// The call: each note's sixteenth into its bar, and its key.
    call_notes: [(u8, u8); MAX_CALL],
    call_len: u8,
    /// How many notes the next call has; grows as the answers get them.
    call_size: u8,
    /// The sixteenth the call's bar starts on; the answer's bar follows it.
    call_from: i64,
    /// The answer's notes: sixteenths from the answer's bar, and keys.
    answers: [(f32, u8); MAX_ANSWER],
    answer_len: u8,
    /// Answers in a row with no notes.
    unanswered: u8,
}

impl Solo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            level: 0,
            sample_rate,
            lead: 0.0,
            tonic: 0,
            scale: DEFAULT_SCALE,
            chord: 0,
            rate: 0.0,
            last_tap: None,
            touch: 0.0,
            energy: 0.0,
            held: None,
            long: Long::None,
            long_from: 0.0,
            modulation: 0,
            steer: 0,
            own: [NONE; 128],
            own_count: [0; 128],
            own_held: 0,
            last_own: f64::NEG_INFINITY,
            next: None,
            mask: 0,
            last_mask: 0,
            phrase_left: 0,
            rest_left: 0,
            ending: false,
            sounding: None,
            grace: None,
            sounding_own: false,
            grid: None,
            now: 0,
            pitch: 60,
            rising: true,
            last_note: f64::NEG_INFINITY,
            rng: 0x9e37_79b9,
            feel_in: 0,
            feel_every: (FEEL_EVERY_S * sample_rate) as u32,
            feel: None,
            call: false,
            phase: CallPhase::Idle,
            armed: false,
            call_notes: [(0, 0); MAX_CALL],
            call_len: 0,
            call_size: FIRST_CALL,
            call_from: 0,
            answers: [(0.0, 0); MAX_ANSWER],
            answer_len: 0,
            unanswered: 0,
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// Turns the solo on at `level` (0 turns it off), with call and response
    /// or not. `lead_ms` is how long the output takes to be heard, so the
    /// notes are sent that much early.
    pub fn set_level(&mut self, level: u8, call: bool, lead_ms: f32, out: &mut EventBuffer) {
        if level != self.level || call != self.call {
            self.silence(0, out);
            self.level = level.min(MAX_SOLO_LEVEL);
            self.call = call;
        }
        self.lead = lead_ms.clamp(0.0, MAX_LEAD_MS) as f64 / 1000.0 * self.sample_rate as f64;
    }

    /// The song's home note (0 is C), its scale and the chord heard now, as pitch-class bit sets.
    pub fn set_harmony(&mut self, tonic: u8, scale: u16, chord: u16) {
        self.tonic = tonic % 12;
        self.scale = if scale & 0xfff == 0 { rotate(DEFAULT_SCALE, self.tonic) } else { scale & 0xfff };
        self.chord = chord & 0xfff;
    }

    #[cfg(test)]
    fn seed(&mut self, seed: u32) {
        self.rng = seed.max(1);
    }

    /// The key the player taps on level 1: the home note from middle C up.
    pub fn home(&self) -> u8 {
        60 + self.tonic
    }

    /// How much earlier than heard the notes are sent, in frames.
    pub fn lead(&self) -> f64 {
        self.lead
    }

    /// Whether the player's notes are judged on this level.
    pub fn judged(&self) -> bool {
        self.level >= JUDGED_FROM
    }

    /// A key pressed at engine frame `now`.
    pub fn press(&mut self, key: u8, velocity: u8, now: u64, grid: Option<Grid>, out: &mut EventBuffer) -> Pressed {
        if self.level == 0 {
            return Pressed::Free;
        }
        (self.grid, self.now) = (grid, now);
        if self.level >= OWN_NOTES_FROM {
            let pressed = self.own_note(key, velocity, now, grid, out);
            if let (true, Pressed::Played(sound), Some(grid)) = (self.call, pressed, grid) {
                self.answered(sound, now, grid, out);
            }
            return pressed;
        }
        let Some(steer) = self.steer_of(key) else {
            // Only the lit keys drive the solo.
            return Pressed::Drove;
        };
        self.held = Some((key, now));
        let Some(grid) = grid else { return Pressed::Drove };
        if self.call {
            self.tap(velocity, now, grid);
            if let Some(i) = self.answered(key, now, grid, out)
                && i < self.call_len as usize
            {
                // The call's next note, for as long as the key is held.
                let key = self.call_notes[i].1;
                self.end_note(now as f64, now, out);
                self.key_on(0, key, velocity, now as f64, true, out);
                self.sounding = Some((key, f64::INFINITY));
                self.sounding_own = true;
                self.last_note = now as f64;
                self.pitch = key;
            }
            return Pressed::Drove;
        }
        self.steer = steer;
        self.tap(velocity, now, grid);
        if (now as f64 - self.last_note) / grid.period > PICKUP_AFTER_BEATS {
            // Straight away, so the tap is heard to start it.
            self.energy = self.energy.max(energy_of(self.rate));
            self.phrase_left = 0;
            self.rest_left = 0;
            let key = self.pick(true);
            self.note_on(key, 0, now as f64, now, grid.period * 0.5, out);
        }
        Pressed::Drove
    }

    /// Which way a driving key steers (levels 1 and 2), or `None` for a key that doesn't drive.
    fn steer_of(&self, key: u8) -> Option<i8> {
        let pc = key % 12;
        if self.level == 1 {
            return (pc == self.tonic).then_some(0);
        }
        if pc == (self.tonic + 7) % 12 {
            Some(0)
        } else if pc == self.tonic {
            Some(if key >= self.home() + 12 { 1 } else { -1 })
        } else {
            None
        }
    }

    /// Counts a tap (or a note of the player's) towards the rate and the touch.
    fn tap(&mut self, velocity: u8, now: u64, grid: Grid) {
        let since = self.last_tap.map(|last| (now - last) as f64 / grid.period);
        self.rate = match since {
            // A chord's notes are one tap.
            Some(dt) if dt < CHORD_BEATS && self.level >= OWN_NOTES_FROM => self.rate,
            Some(dt) if dt < self.gap() => 0.5 * self.rate + 0.5 * (1.0 / dt.max(1.0 / MAX_RATE)),
            // The first tap, or the first after a while: eighths to start with.
            _ => 1.0,
        };
        self.last_tap = Some(now);
        self.touch = if since.is_some() { 0.6 * self.touch + 0.4 * velocity as f32 } else { velocity as f32 };
    }

    /// Levels 3–5: the player's own note, snapped to the scale below level 5, sounding at once.
    fn own_note(&mut self, key: u8, velocity: u8, now: u64, grid: Option<Grid>, out: &mut EventBuffer) -> Pressed {
        let sound = if self.level >= AS_PLAYED { key } else { self.snap(key) };
        if let Some(grid) = grid {
            self.tap(velocity, now, grid);
        }
        // midzer's note makes room, so its end can't cut the player's short.
        self.end_note(now as f64, now, out);
        self.end_own(key, now, out);
        self.own[key as usize] = sound;
        self.own_count[sound as usize] += 1;
        self.own_held += 1;
        self.last_own = now as f64;
        self.last_note = now as f64;
        self.pitch = sound;
        self.key_on(0, sound, velocity, now as f64, true, out);
        Pressed::Played(sound)
    }

    /// Ends the note the player's `key` sounds, if any.
    fn end_own(&mut self, key: u8, now: u64, out: &mut EventBuffer) {
        let sound = std::mem::replace(&mut self.own[key as usize], NONE);
        if sound == NONE {
            return;
        }
        self.own_held = self.own_held.saturating_sub(1);
        self.own_count[sound as usize] = self.own_count[sound as usize].saturating_sub(1);
        if self.own_count[sound as usize] == 0 {
            self.key_off(0, sound, now as f64, true, out);
        }
        if self.own_held == 0 {
            // The solo carries on from the note let go.
            self.last_tap = self.last_tap.map(|_| now);
        }
    }

    /// The scale note nearest `key`; between two, the chord tone, else the lower.
    fn snap(&self, key: u8) -> u8 {
        let fits = |k: i16| (0..=127).contains(&k) && self.scale & 1 << (k as u16 % 12) != 0;
        let tones = self.chord_tones();
        let key = key as i16;
        for d in 0..12 {
            let (down, up) = (key - d, key + d);
            match (fits(down), fits(up)) {
                (true, true) if tones & 1 << (up as u16 % 12) != 0 && tones & 1 << (down as u16 % 12) == 0 => return up as u8,
                (true, _) => return down as u8,
                (_, true) => return up as u8,
                _ => {}
            }
        }
        key as u8
    }

    /// A key let go at engine frame `now`.
    pub fn release(&mut self, key: u8, now: u64, out: &mut EventBuffer) -> bool {
        if self.level == 0 {
            return false;
        }
        self.now = now;
        self.end_own(key, now, out);
        if self.held.is_some_and(|(held, _)| held == key) {
            self.held = None;
            if self.call && self.sounding.is_some_and(|(_, off)| off == f64::INFINITY) {
                // An answer's note ends with its tap.
                self.end_note(now as f64, now, out);
            }
            if self.long == Long::Sounding {
                self.end_note(now as f64, now, out);
            }
            if self.long != Long::None {
                // The solo carries on from here as if just tapped.
                self.last_tap = Some(now);
            }
            self.long = Long::None;
        }
        true
    }

    /// Stops every note at frame `at` of this block and forgets the taps.
    pub fn silence(&mut self, at: u32, out: &mut EventBuffer) {
        let t = (self.now + at as u64) as f64;
        self.end_note(t, self.now, out);
        for key in 0..128u8 {
            if std::mem::take(&mut self.own_count[key as usize]) > 0 {
                self.key_off(at, key, t, true, out);
            }
        }
        self.own = [NONE; 128];
        self.own_held = 0;
        self.steer = 0;
        self.held = None;
        self.long = Long::None;
        self.last_tap = None;
        self.rate = 0.0;
        self.energy = 0.0;
        self.next = None;
        self.phrase_left = 0;
        self.rest_left = 0;
        self.mask = 0;
        self.ending = false;
        self.rest_calls(at, out);
        self.call_size = FIRST_CALL;
    }

    /// Call and response waits for a tap again.
    fn rest_calls(&mut self, at: u32, out: &mut EventBuffer) {
        self.armed = false;
        self.answer_len = 0;
        self.unanswered = 0;
        if std::mem::replace(&mut self.phase, CallPhase::Idle) != CallPhase::Idle {
            out.push(at, EngineEvent::SoloCall { phase: CallPhase::Idle, hit: 0, of: 0, key: None });
        }
    }

    /// Plays this block's notes on `grid`, the beats the player hears (none
    /// while the beat isn't found).
    pub fn process(&mut self, frames: u32, now: u64, grid: Option<Grid>, out: &mut EventBuffer) {
        let end = (now + frames as u64) as f64;
        self.release_due(now, end, out);
        if self.level == 0 {
            return;
        }
        (self.grid, self.now) = (grid, now);
        if let Some(g) = grid {
            self.update_energy(frames, now, g);
            let lead = self.lead;
            let mut q = match self.next {
                Some(q) if g.frame(q) - lead >= now as f64 - g.period => q,
                // Starting, or the grid jumped.
                _ => (g.sixteenth_at(now as f64 + lead)).ceil() as i64,
            };
            while g.frame(q) - lead < end {
                let t = g.frame(q) - lead;
                if t >= now as f64 {
                    if self.call {
                        self.call_sixteenth(q, t, now, &g, out);
                    } else if self.level < JUDGED_FROM {
                        self.sixteenth(q, t, now, &g, out);
                    }
                }
                q += 1;
            }
            self.next = Some(q);
            self.release_due(now, end, out);
            self.vibrato(now, &g, out);
        } else {
            self.next = None;
            self.energy = 0.0;
            self.rest_calls(0, out);
        }
        self.feel_in = self.feel_in.saturating_sub(frames);
        if self.feel_in == 0 {
            self.feel_in = self.feel_every;
            let feel = (self.energy * 255.0).round() as u8;
            if self.feel != Some(feel) {
                self.feel = Some(feel);
                out.push(0, EngineEvent::SoloFeel { energy: feel });
            }
        }
    }

    /// Deepens a long note's vibrato as it sounds on.
    fn vibrato(&mut self, now: u64, g: &Grid, out: &mut EventBuffer) {
        if self.long != Long::Sounding {
            return;
        }
        let into = (now as f64 - self.long_from) / g.period - VIBRATO_AFTER_BEATS;
        let depth = ((into / VIBRATO_BEATS).clamp(0.0, 1.0) * VIBRATO_DEPTH) as u8;
        // In steps, not on every block.
        if depth > self.modulation && (depth - self.modulation >= 4 || depth == VIBRATO_DEPTH as u8) {
            self.modulation = depth;
            out.push(0, EngineEvent::Control { channel: PLAYER_CHANNEL, controller: 1, value: depth });
        }
    }

    /// Takes the vibrato away again.
    fn unbend(&mut self, at: u32, out: &mut EventBuffer) {
        if std::mem::take(&mut self.modulation) > 0 {
            out.push(at, EngineEvent::Control { channel: PLAYER_CHANNEL, controller: 1, value: 0 });
        }
    }

    /// How many beats may pass between taps before the solo winds down.
    fn gap(&self) -> f64 {
        1.25 / self.rate.max(0.25) + 0.5
    }

    /// Whether the taps still drive the solo at engine frame `at`.
    fn driven(&self, at: u64, period: f64) -> bool {
        self.held.is_some() || self.own_held > 0 || self.last_tap.is_some_and(|last| at.saturating_sub(last) as f64 / period <= self.gap())
    }

    fn update_energy(&mut self, frames: u32, now: u64, g: Grid) {
        if !self.call && self.held.is_some_and(|(_, since)| (now - since) as f64 / g.period >= HOLD_BEATS) && self.long == Long::None {
            self.long = Long::Waiting;
        }
        if self.long != Long::None {
            return;
        }
        let target = if self.driven(now, g.period) { energy_of(self.rate) } else { 0.0 };
        let beats = frames as f64 / g.period;
        let k = (beats / if target > self.energy { RISE_BEATS } else { FALL_BEATS }).min(1.0) as f32;
        self.energy += (target - self.energy) * k;
    }

    /// Sixteenth `q`, heard at engine frame `t` (sent `lead` before).
    fn sixteenth(&mut self, q: i64, t: f64, now: u64, g: &Grid, out: &mut EventBuffer) {
        let pos = q.rem_euclid(4) as u8;
        if pos == 0 {
            self.new_beat(g.beat_in_bar(q), now, g.period);
        }
        match self.long {
            Long::Waiting if pos.is_multiple_of(2) => {
                let key = self.pick(true);
                self.note_on(key, 0, t, now, f64::INFINITY, out);
                self.long = Long::Sounding;
                self.long_from = t;
                return;
            }
            Long::Waiting | Long::Sounding => return,
            Long::None => {}
        }
        if self.mask & 1 << pos == 0 || t - self.last_note < g.period / 8.0 {
            // Nothing here, or a tap's note just took this sixteenth.
            return;
        }
        if self.level >= OWN_NOTES_FROM && (self.own_held > 0 || t - self.last_own < FILL_AFTER_BEATS * g.period) {
            // The player's playing: fills go in the gaps.
            return;
        }
        let ending = self.ending && pos == 0;
        let strong = ending || (pos == 0 && g.beat_in_bar(q) % 2 == 0);
        let key = self.pick(strong);
        let length = if ending { 2.0 } else { 0.5 + (1.0 - self.energy as f64) } * g.period;
        if self.energy > 0.8 && pos == 0 && !ending && self.chance(0.25) {
            // A quick slide in from the scale note below.
            let from = self.ladder_step(key, -1);
            let at = t - g.period / 8.0;
            if from != key && at >= now as f64 {
                self.end_note(at, now, out);
                self.grace = Some((from, t));
                let velocity = self.velocity(1);
                self.key_on((at - now as f64) as u32, from, velocity, at + self.lead, false, out);
            }
        }
        self.note_on(key, pos, t, now, length, out);
        if ending {
            self.ending = false;
        }
    }

    /// Sixteenth `q` with call and response on: a new phase on each bar's 1,
    /// and the call's notes while it plays.
    fn call_sixteenth(&mut self, q: i64, t: f64, now: u64, g: &Grid, out: &mut EventBuffer) {
        let at = (t - now as f64).max(0.0) as u32;
        if q.rem_euclid(4) == 0 && g.beat_in_bar(q) == 0 {
            self.call_bar(q, at, out);
        }
        if self.phase != CallPhase::Listen {
            return;
        }
        let into = q - self.call_from;
        let len = self.call_len as usize;
        let Some(i) = self.call_notes[..len].iter().position(|n| n.0 as i64 == into) else { return };
        let key = self.call_notes[i].1;
        // Up to the next note, a beat at most.
        let gap = self.call_notes.get(i + 1).filter(|_| i + 1 < len).map_or(4, |n| (n.0 as i64 - into).min(4));
        self.note_on(key, (into % 4) as u8, t, now, gap as f64 * 0.9 * g.period / 4.0, out);
        out.push(at, EngineEvent::SoloCall { phase: CallPhase::Listen, hit: i as u8 + 1, of: self.call_len, key: Some(key) });
    }

    /// A bar starts on sixteenth `q`: the call, the answer, or the answer's score and the next call.
    fn call_bar(&mut self, q: i64, at: u32, out: &mut EventBuffer) {
        match self.phase {
            CallPhase::Idle if self.armed => {
                self.armed = false;
                self.next_call(q, at, out);
            }
            CallPhase::Listen if q >= self.call_from + BAR => {
                self.phase = CallPhase::Answer;
                out.push(at, self.answer_event());
            }
            CallPhase::Answer | CallPhase::Scored if q >= self.call_from + 2 * BAR => {
                let hit = self.score();
                let of = self.call_len;
                out.push(at, EngineEvent::SoloCall { phase: CallPhase::Scored, hit, of, key: None });
                self.unanswered = if self.answer_len == 0 { self.unanswered + 1 } else { 0 };
                if hit == of {
                    self.call_size = (self.call_size + 1).min(MAX_CALL as u8);
                } else if hit * 2 < of && self.answer_len > 0 {
                    self.call_size = (self.call_size - 1).max(MIN_CALL);
                }
                if self.unanswered >= UNANSWERED_REST {
                    self.rest_calls(at, out);
                } else {
                    self.next_call(q, at, out);
                }
            }
            _ => {}
        }
    }

    fn next_call(&mut self, q: i64, at: u32, out: &mut EventBuffer) {
        self.make_call(q);
        self.answer_len = 0;
        self.phase = CallPhase::Listen;
        out.push(at, EngineEvent::SoloCall { phase: CallPhase::Listen, hit: 0, of: self.call_len, key: None });
    }

    /// Makes up a call of `call_size` notes for the bar from sixteenth `q`:
    /// on the 1 or its "and", then mostly on the beats and their "and"s,
    /// from the home note along the scale, ending on a chord tone.
    fn make_call(&mut self, q: i64) {
        self.call_from = q;
        let size = self.call_size as usize;
        let mut slots = [false; CALL_LAST as usize + 1];
        slots[if self.chance(0.7) { 0 } else { 2 }] = true;
        for _ in 1..size {
            let weight = |slot: usize| density(CALL_ENERGY, (slot % 4) as u8, (slot / 4) as i64) + 0.05;
            let total: f32 = (1..slots.len()).filter(|&s| !slots[s]).map(weight).sum();
            let mut r = self.unit() * total;
            for (slot, on) in slots.iter_mut().enumerate().skip(1) {
                if *on {
                    continue;
                }
                r -= weight(slot);
                if r <= 0.0 {
                    *on = true;
                    break;
                }
            }
        }
        let energy = std::mem::replace(&mut self.energy, CALL_ENERGY);
        let steer = std::mem::take(&mut self.steer);
        self.pitch = self.home();
        let mut len = 0;
        for (slot, _) in slots.iter().enumerate().filter(|(_, on)| **on) {
            let last = len + 1 == slots.iter().filter(|on| **on).count();
            let key = self.pick(slot % 8 == 0 || last);
            self.call_notes[len] = (slot as u8, key);
            len += 1;
        }
        self.call_len = len as u8;
        self.energy = energy;
        self.steer = steer;
    }

    /// The player played `key` at frame `now`: part of the answer if it is
    /// due, or a call asked for. Returns which note of the answer it is.
    fn answered(&mut self, key: u8, now: u64, g: Grid, out: &mut EventBuffer) -> Option<usize> {
        if self.phase == CallPhase::Idle {
            self.armed = true;
            return None;
        }
        let into = g.sixteenth_at(now as f64) - (self.call_from + BAR) as f64;
        let i = self.answer_len as usize;
        if !(-ANSWER_SLACK..BAR as f64 - ANSWER_SLACK).contains(&into) || i >= MAX_ANSWER {
            return None;
        }
        // Tapping, the note is the call's; playing, the player's own.
        let key = if self.level >= OWN_NOTES_FROM { key } else { self.call_notes.get(i).filter(|_| i < self.call_len as usize).map_or(NONE, |n| n.1) };
        self.answers[i] = (into as f32, key);
        self.answer_len += 1;
        out.push(0, self.answer_event());
        Some(i)
    }

    /// The answer so far, with the key to play next from level 3.
    fn answer_event(&self) -> EngineEvent {
        let i = self.answer_len as usize;
        let key = (self.level >= OWN_NOTES_FROM && i < self.call_len as usize).then(|| self.call_notes[i].1);
        EngineEvent::SoloCall { phase: CallPhase::Answer, hit: self.score(), of: self.call_len, key }
    }

    /// The call's notes the answer got: each answered within the slack, on
    /// the same note from level 3; notes beyond the call's count against it.
    fn score(&self) -> u8 {
        let answers = &self.answers[..self.answer_len as usize];
        let mut used = [false; MAX_ANSWER];
        let mut hit = 0u8;
        for &(slot, key) in &self.call_notes[..self.call_len as usize] {
            let best = (0..answers.len())
                .filter(|&j| !used[j] && (answers[j].0 - slot as f32).abs() as f64 <= ANSWER_SLACK)
                .filter(|&j| self.level < OWN_NOTES_FROM || answers[j].1 % 12 == key % 12)
                .min_by(|&a, &b| (answers[a].0 - slot as f32).abs().total_cmp(&(answers[b].0 - slot as f32).abs()));
            if let Some(j) = best {
                used[j] = true;
                hit += 1;
            }
        }
        hit.saturating_sub(self.answer_len.saturating_sub(self.call_len))
    }

    /// Decides what this beat plays: part of a phrase, a rest between
    /// phrases, or the phrase's last note once the taps stop.
    fn new_beat(&mut self, beat_in_bar: i64, now: u64, period: f64) {
        self.mask = 0;
        self.ending = false;
        if self.long != Long::None {
            return;
        }
        let driven = self.driven(now, period) && self.energy >= SILENT;
        if !driven {
            if self.phrase_left > 0 {
                // Wound down: one last chord tone.
                self.phrase_left = 0;
                self.mask = 1;
                self.ending = true;
            }
            return;
        }
        if self.rest_left > 0 {
            self.rest_left -= 1;
            return;
        }
        if self.phrase_left == 0 {
            self.phrase_left = 2 + self.below(3) as u8 + (self.energy * 4.0) as u8;
        }
        self.phrase_left -= 1;
        if self.phrase_left == 0 {
            self.mask = 1;
            self.ending = true;
            self.rest_left = ((1.0 - self.energy) * 2.0).round() as u8;
            return;
        }
        // Repeating the last beat's rhythm now and then is what makes it a groove.
        self.mask = if self.last_mask != 0 && self.chance(0.4) {
            self.last_mask
        } else {
            let mut mask = 0;
            for pos in 0..4u8 {
                if self.chance(density(self.energy, pos, beat_in_bar)) {
                    mask |= 1 << pos;
                }
            }
            mask
        };
        self.last_mask = self.mask;
    }

    /// The next note: a step or a small leap along the scale from the last
    /// one, within a range that climbs with the energy; on a strong beat the
    /// nearest chord tone.
    fn pick(&mut self, strong: bool) -> u8 {
        let mut ladder = [0u8; 40];
        let len = self.ladder(&mut ladder);
        if len == 0 {
            return self.home();
        }
        let ladder = &ladder[..len];
        let mut i = ladder.iter().enumerate().min_by_key(|(_, k)| (**k as i16 - self.pitch as i16).abs()).map_or(0, |(i, _)| i) as i64;
        if !self.chance(0.7) {
            self.rising = !self.rising;
        }
        if self.steer != 0 && self.chance(0.3) {
            // Level 2's low and high keys lean the line that way.
            self.rising = self.steer > 0;
        }
        let r = self.below(100);
        let size = if r < 10 { 0 } else if r < 70 { 1 } else if r < 90 { 2 } else { 3 };
        i += if self.rising { size } else { -size };
        let last = len as i64 - 1;
        if i > last {
            i = 2 * last - i;
            self.rising = false;
        } else if i < 0 {
            i = -i;
            self.rising = true;
        }
        let mut i = i.clamp(0, last) as usize;
        if strong {
            let tones = self.chord_tones();
            let near = (0..=2).flat_map(|d| [i as i64 + d, i as i64 - d]).filter(|&j| (0..=last).contains(&j)).find(|&j| tones & 1 << (ladder[j as usize] % 12) != 0);
            if let Some(j) = near {
                i = j as usize;
            }
        }
        self.pitch = ladder[i];
        self.pitch
    }

    /// The scale's keys in the solo's range, low to high.
    fn ladder(&self, out: &mut [u8; 40]) -> usize {
        let e = self.energy.clamp(0.0, 1.0);
        let home = self.home() as f32
            + match self.steer {
                -1 => STEER_LOW,
                1 => STEER_HIGH,
                _ => 0.0,
            };
        let low = (home - 3.0 + 5.0 * e) as u8;
        let high = (home + 10.0 + 9.0 * e) as u8;
        let mut len = 0;
        for key in low..=high.min(127) {
            if self.scale & 1 << (key % 12) != 0 && len < out.len() {
                out[len] = key;
                len += 1;
            }
        }
        len
    }

    /// The scale note `steps` away from `key`, or `key` at the range's end.
    fn ladder_step(&self, key: u8, steps: i64) -> u8 {
        let mut ladder = [0u8; 40];
        let len = self.ladder(&mut ladder);
        let ladder = &ladder[..len];
        ladder.iter().position(|&k| k == key).and_then(|i| ladder.get((i as i64 + steps).max(0) as usize)).copied().unwrap_or(key)
    }

    /// The chord's tones, or the home chord's from the scale when no chord is heard.
    fn chord_tones(&self) -> u16 {
        if self.chord != 0 {
            return self.chord;
        }
        let at = |i: u8| 1u16 << ((self.tonic + i) % 12);
        let third = if self.scale & at(4) != 0 { at(4) } else { at(3) };
        at(0) | third | at(7)
    }

    fn velocity(&mut self, pos: u8) -> u8 {
        let accent = match pos {
            0 => 8.0,
            2 => 0.0,
            _ => -10.0,
        };
        let jitter = self.below(11) as f32 - 5.0;
        (0.6 * self.touch + 30.0 + 20.0 * self.energy + accent + jitter).clamp(30.0, 127.0) as u8
    }

    /// Starts `key` at engine frame `t`, on sixteenth `pos` of its beat, for
    /// `length` frames, ending the note before it.
    fn note_on(&mut self, key: u8, pos: u8, t: f64, now: u64, length: f64, out: &mut EventBuffer) {
        self.end_note(t, now, out);
        let velocity = self.velocity(pos);
        self.key_on((t - now as f64).max(0.0) as u32, key, velocity, t + self.lead, false, out);
        self.sounding = Some((key, t + length));
        self.sounding_own = false;
        self.last_note = t;
        self.pitch = key;
    }

    fn end_note(&mut self, t: f64, now: u64, out: &mut EventBuffer) {
        let at = (t - now as f64).max(0.0) as u32;
        if let Some((key, _)) = self.grace.take() {
            self.key_off(at, key, t + self.lead, false, out);
        }
        if let Some((key, _)) = self.sounding.take() {
            let own = std::mem::take(&mut self.sounding_own);
            self.key_off(at, key, if own { t } else { t + self.lead }, own, out);
        }
        self.unbend(at, out);
    }

    /// Ends the notes whose time is up before `end`.
    fn release_due(&mut self, now: u64, end: f64, out: &mut EventBuffer) {
        for grace in [true, false] {
            let slot = if grace { &mut self.grace } else { &mut self.sounding };
            if let Some((key, off)) = *slot
                && off < end
            {
                *slot = None;
                // Only midzer's notes end on time; an answer's tap ends with its key.
                self.key_off((off - now as f64).max(0.0) as u32, key, off + self.lead, false, out);
            }
        }
    }

    /// Starts `key` at frame `at` of this block, heard at engine frame
    /// `heard`, and says so for the replay.
    fn key_on(&self, at: u32, key: u8, velocity: u8, heard: f64, own: bool, out: &mut EventBuffer) {
        out.push(at, EngineEvent::NoteOn { channel: PLAYER_CHANNEL, key, velocity });
        self.recorded(at, key, velocity, heard, own, out);
    }

    fn key_off(&self, at: u32, key: u8, heard: f64, own: bool, out: &mut EventBuffer) {
        out.push(at, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key });
        self.recorded(at, key, 0, heard, own, out);
    }

    /// A note started or ended, placed on the beats heard; nothing without a beat.
    fn recorded(&self, at: u32, key: u8, velocity: u8, heard: f64, own: bool, out: &mut EventBuffer) {
        let Some(g) = self.grid else { return };
        // Counted in bars of the beats found, on the 1s' phase: `one` itself moves on each bar.
        let ticks = ((g.sixteenth_at(heard) / 4.0 - g.one.rem_euclid(4) as f64) * PPQ as f64).round() as i64;
        let bar = TICKS_PER_BAR as i64;
        // Bars wrap: only differences between them count.
        out.push(at, EngineEvent::SoloNote { key, velocity, bar: ticks.div_euclid(bar) as u16, tick: ticks.rem_euclid(bar) as u16, own });
    }

    fn next_random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    /// A random number in 0..1.
    fn unit(&mut self) -> f32 {
        (self.next_random() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next_random() % n
    }
}

/// 0.5 taps a beat is calm (0.25), one is eighths (0.5), two sixteenths
/// (0.75), four and more everything (1).
fn energy_of(rate: f64) -> f32 {
    if rate <= 0.0 {
        return 0.0;
    }
    (0.25 + 0.25 * (rate / 0.5).log2()).clamp(0.0, 1.0) as f32
}

/// How likely sixteenth `pos` of a beat is played at `energy`: the beats
/// first, then the "and"s, then the anticipations and the rest.
fn density(energy: f32, pos: u8, beat_in_bar: i64) -> f32 {
    let threshold = match pos {
        0 => 0.1,
        2 => 0.35,
        3 => 0.55,
        _ => 0.65,
    };
    let strong = if pos == 0 && beat_in_bar % 2 == 0 { 0.1 } else { 0.0 };
    (((energy - threshold) / 0.2).clamp(0.0, 1.0) * 0.9 + strong).min(1.0)
}

/// `bits` (pitch classes) moved up `by` semitones.
fn rotate(bits: u16, by: u8) -> u16 {
    let by = by % 12;
    ((bits << by) | (bits >> (12 - by))) & 0xfff
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    /// 120 BPM.
    const PERIOD: f64 = 24_000.0;
    const BLOCK: u32 = 128;
    const GRID: Grid = Grid { at: 0.0, index: 0, period: PERIOD, one: 0 };

    /// A note played: start and end frame, key, velocity.
    type Played = (u64, u64, u8, u8);

    /// Runs for `beats` with taps at the given frames, each held `hold` frames; returns the notes.
    fn run(solo: &mut Solo, beats: f64, taps: &[u64], hold: u64) -> Vec<Played> {
        run_holds(solo, beats, &taps.iter().map(|&t| (t, hold)).collect::<Vec<_>>())
    }

    fn solo() -> Solo {
        let mut solo = Solo::new(SR);
        solo.seed(7);
        solo.set_level(1, false, 0.0, &mut EventBuffer::default());
        // C major pentatonic over C.
        solo.set_harmony(0, 0b0010_1001_0101, 0b0000_1001_0001);
        solo
    }

    /// Taps every `every` beats from beat 1 to beat `until`.
    fn taps(every: f64, until: f64) -> Vec<u64> {
        let mut out = Vec::new();
        let mut b = 1.0;
        while b < until {
            out.push((b * PERIOD) as u64);
            b += every;
        }
        out
    }

    #[test]
    fn silent_without_taps() {
        assert!(run(&mut solo(), 16.0, &[], 0).is_empty());
    }

    #[test]
    fn faster_taps_play_busier_and_higher() {
        let mut counts = Vec::new();
        let mut pitches = Vec::new();
        for every in [2.0, 1.0, 0.5, 0.3] {
            // Summed over seeds, so one lucky phrase doesn't decide it.
            let (mut count, mut sum) = (0usize, 0f64);
            for seed in 1..=8 {
                let mut s = solo();
                s.seed(seed);
                let notes = run(&mut s, 18.0, &taps(every, 17.0), 1000);
                let middle: Vec<_> = notes.iter().filter(|n| (5.0 * PERIOD) as u64 <= n.0 && n.0 < (17.0 * PERIOD) as u64).collect();
                count += middle.len();
                sum += middle.iter().map(|n| n.2 as f64).sum::<f64>();
            }
            counts.push(count);
            pitches.push(sum / count.max(1) as f64);
        }
        assert!(counts.windows(2).all(|w| w[0] < w[1]), "notes per tap rate: {counts:?}");
        assert!(pitches[3] > pitches[0] + 2.0, "mean pitch: {pitches:?}");
    }

    #[test]
    fn notes_stay_in_the_scale() {
        for seed in 1..=6 {
            let mut s = solo();
            s.seed(seed);
            for n in run(&mut s, 18.0, &taps(0.3, 17.0), 1000) {
                assert!([0, 2, 4, 7, 9].contains(&(n.2 % 12)), "key {} off the scale", n.2);
            }
        }
    }

    #[test]
    fn notes_fall_on_the_sixteenths_after_the_pickup() {
        let sixteenth = PERIOD / 4.0;
        let notes = run(&mut solo(), 18.0, &taps(0.3, 17.0), 1000);
        assert!(notes.len() > 20);
        // The first sounds with the first tap, in its block.
        assert!(notes[0].0 + BLOCK as u64 > PERIOD as u64 && notes[0].0 <= PERIOD as u64);
        for n in &notes[1..] {
            let q = n.0 as f64 / sixteenth;
            // On a sixteenth, or a slide a thirty-second before one.
            let off = (q - q.round()).abs().min((q + 0.5 - (q + 0.5).round()).abs());
            assert!(off * sixteenth < 1.5, "note at {} is off the grid", n.0);
        }
    }

    #[test]
    fn stops_soon_after_the_taps() {
        for every in [1.0, 0.5, 0.3] {
            let notes = run(&mut solo(), 16.0, &taps(every, 8.0), 1000);
            let last_tap = *taps(every, 8.0).last().unwrap() as f64;
            let last = notes.last().unwrap().0 as f64;
            assert!(last <= last_tap + 2.5 * PERIOD, "tapping every {every}: last note {} beats after the last tap", (last - last_tap) / PERIOD);
            assert!(notes.iter().all(|n| n.1 != u64::MAX), "a note hangs: {:?}", notes.iter().filter(|n| n.1 == u64::MAX).map(|n| (n.0 as f64 / PERIOD, n.2)).collect::<Vec<_>>());
        }
    }

    #[test]
    fn holding_the_key_plays_one_long_note() {
        // Taps every half beat, the last one held for three beats.
        let mut taps = taps(0.5, 5.0);
        let hold_from = (5.0 * PERIOD) as u64;
        taps.push(hold_from);
        let mut s = solo();
        let holds: Vec<_> = taps.iter().enumerate().map(|(i, &t)| (t, if i + 1 == taps.len() { (3.0 * PERIOD) as u64 } else { 1000 })).collect();
        let notes = run_holds(&mut s, 12.0, &holds);
        let during: Vec<_> = notes.iter().filter(|n| n.0 > hold_from + (0.6 * PERIOD) as u64 && n.0 < hold_from + (3.0 * PERIOD) as u64).collect();
        assert_eq!(during.len(), 0, "nothing new while the long note sounds: {during:?}");
        let long = notes.iter().find(|n| n.0 >= hold_from && n.0 <= hold_from + (0.75 * PERIOD) as u64 && n.1 >= hold_from + (2.9 * PERIOD) as u64);
        assert!(long.is_some(), "a note held to the release: {notes:?}");
        assert!([0, 4, 7].contains(&(long.unwrap().2 % 12)), "on a chord tone");
    }

    /// Like `run`, with each tap's own hold.
    fn run_holds(solo: &mut Solo, beats: f64, taps: &[(u64, u64)]) -> Vec<Played> {
        run_keys(solo, beats, &taps.iter().map(|&(t, hold)| (t, hold, 60)).collect::<Vec<_>>(), &mut Vec::new())
    }

    /// Like `run_holds`, each tap on its own key; the vibrato sent goes to `controls` as (frame, value).
    fn run_keys(solo: &mut Solo, beats: f64, taps: &[(u64, u64, u8)], controls: &mut Vec<(u64, u8)>) -> Vec<Played> {
        let mut notes: Vec<Played> = Vec::new();
        let mut open: Vec<(u8, u64, u8)> = Vec::new();
        let mut releases: Vec<(u64, u8)> = Vec::new();
        let mut next = 0;
        let mut now = 0u64;
        while now < (beats * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            releases.retain(|&(r, key)| {
                if r < now + BLOCK as u64 {
                    solo.release(key, now, &mut out);
                    false
                } else {
                    true
                }
            });
            while next < taps.len() && taps[next].0 < now + BLOCK as u64 {
                let (at, hold, key) = taps[next];
                solo.press(key, 100, now, Some(GRID), &mut out);
                releases.push((at + hold, key));
                next += 1;
            }
            solo.process(BLOCK, now, Some(GRID), &mut out);
            out.sort();
            for t in out.as_slice() {
                let at = now + t.frame as u64;
                match t.event {
                    EngineEvent::NoteOn { key, velocity, .. } => open.push((key, at, velocity)),
                    EngineEvent::NoteOff { key, .. } => {
                        if let Some(i) = open.iter().position(|o| o.0 == key) {
                            let (key, from, velocity) = open.remove(i);
                            notes.push((from, at, key, velocity));
                        }
                    }
                    EngineEvent::Control { controller: 1, value, .. } => controls.push((at, value)),
                    _ => {}
                }
            }
            now += BLOCK as u64;
        }
        notes.extend(open.into_iter().map(|(key, from, velocity)| (from, u64::MAX, key, velocity)));
        notes.sort();
        notes
    }

    fn at_level(level: u8) -> Solo {
        let mut s = solo();
        s.set_level(level, false, 0.0, &mut EventBuffer::default());
        s
    }

    /// Notes on `key` every `every` beats from beat 1 to `until`, each held `hold` beats.
    fn notes_on(key: u8, every: f64, until: f64, hold: f64) -> Vec<(u64, u64, u8)> {
        taps(every, until).into_iter().map(|t| (t, (hold * PERIOD) as u64, key)).collect()
    }

    #[test]
    fn holding_adds_vibrato_and_letting_go_takes_it_away() {
        let mut taps: Vec<_> = notes_on(60, 0.5, 5.0, 0.04);
        let hold_from = (5.0 * PERIOD) as u64;
        taps.push((hold_from, (3.0 * PERIOD) as u64, 60));
        let mut controls = Vec::new();
        run_keys(&mut solo(), 10.0, &taps, &mut controls);
        let first = controls.first().expect("vibrato on the long note");
        assert!(first.0 > hold_from + (0.75 * PERIOD) as u64, "not before half a beat in: {controls:?}");
        assert!(controls.iter().any(|c| c.1 == VIBRATO_DEPTH as u8), "it deepens: {controls:?}");
        let last = controls.last().unwrap();
        assert_eq!(last.1, 0, "let go, it goes");
        assert!(last.0 >= hold_from + (3.0 * PERIOD) as u64 - BLOCK as u64);
        assert!(controls.len() < 24, "in steps: {}", controls.len());
    }

    #[test]
    fn level_two_steers_with_the_low_and_high_keys() {
        let mean = |key: u8| {
            let (mut count, mut sum) = (0usize, 0f64);
            for seed in 1..=8 {
                let mut s = at_level(2);
                s.seed(seed);
                for n in run_keys(&mut s, 14.0, &notes_on(key, 0.5, 13.0, 0.04), &mut Vec::new()) {
                    assert!([0, 2, 4, 7, 9].contains(&(n.2 % 12)), "key {} off the scale", n.2);
                    count += 1;
                    sum += n.2 as f64;
                }
            }
            assert!(count > 50, "key {key} drives the solo: {count} notes");
            sum / count as f64
        };
        let (low, middle, high) = (mean(60), mean(67), mean(72));
        assert!(low + 3.0 < middle && middle + 3.0 < high, "mean pitch low {low}, middle {middle}, high {high}");
        assert!(mean(48) < middle, "a lower home note is low too");
        assert!(run_keys(&mut at_level(2), 8.0, &notes_on(62, 0.5, 7.0, 0.04), &mut Vec::new()).is_empty(), "other keys are silent");
    }

    #[test]
    fn level_three_snaps_the_notes_and_fills_the_gaps() {
        let mut fills = 0;
        for seed in 1..=6 {
            let mut s = at_level(3);
            s.seed(seed);
            // C sharp, once a beat for a tenth of a beat.
            let own = notes_on(61, 1.0, 13.0, 0.1);
            let notes = run_keys(&mut s, 14.0, &own, &mut Vec::new());
            for &(at, hold, _) in &own {
                let note = notes.iter().find(|n| n.0 <= at && at < n.0 + BLOCK as u64).expect("the player's note sounds at once");
                // Between C and D, the chord tone.
                assert_eq!(note.2, 60);
                assert!(note.1 >= at + hold - BLOCK as u64, "and as long as held");
            }
            for n in notes.iter().filter(|n| !own.iter().any(|o| n.0 <= o.0 && o.0 < n.0 + BLOCK as u64)) {
                fills += 1;
                assert!([0, 2, 4, 7, 9].contains(&(n.2 % 12)), "fill {} off the scale", n.2);
                let after = own.iter().filter(|o| o.0 <= n.0).map(|o| n.0 - o.0).min().unwrap_or(u64::MAX);
                assert!(after as f64 >= FILL_AFTER_BEATS * PERIOD - BLOCK as f64, "a fill {} beats after the player's note", after as f64 / PERIOD);
            }
        }
        assert!(fills > 10, "midzer fills the gaps: {fills}");
    }

    #[test]
    fn level_four_only_snaps_and_level_five_plays_as_played() {
        for (level, key, sounds) in [(4, 61, 60), (4, 66, 67), (4, 65, 64), (5, 61, 61)] {
            let mut s = at_level(level);
            let own = notes_on(key, 0.5, 9.0, 0.1);
            let notes = run_keys(&mut s, 10.0, &own, &mut Vec::new());
            assert_eq!(notes.len(), own.len(), "level {level}: the player's notes only");
            assert!(notes.iter().all(|n| n.2 == sounds), "level {level}: {key} sounds as {sounds}: {notes:?}");
        }
        assert!(at_level(4).judged() && !at_level(3).judged());
    }

    #[test]
    fn keys_snapped_to_one_note_end_it_together() {
        let mut s = at_level(4);
        let mut out = EventBuffer::default();
        assert_eq!(s.press(61, 90, 0, Some(GRID), &mut out), Pressed::Played(60));
        assert_eq!(s.press(60, 90, 10, Some(GRID), &mut out), Pressed::Played(60));
        let offs = |out: &EventBuffer| out.as_slice().iter().filter(|t| matches!(t.event, EngineEvent::NoteOff { .. })).count();
        let mut out = EventBuffer::default();
        s.release(61, 20, &mut out);
        assert_eq!(offs(&out), 0, "C still held");
        s.release(60, 30, &mut out);
        assert_eq!(offs(&out), 1);
        s.press(63, 90, 40, Some(GRID), &mut out);
        let mut out = EventBuffer::default();
        s.silence(0, &mut out);
        assert_eq!(out.as_slice().iter().map(|t| t.event).filter(|e| matches!(e, EngineEvent::NoteOff { .. })).collect::<Vec<_>>(), [EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key: 64 }], "E flat sounded as E, the chord tone");
    }

    #[test]
    fn other_keys_and_no_beat_play_nothing() {
        let mut s = solo();
        let mut out = EventBuffer::default();
        assert_eq!(s.press(62, 100, 0, Some(GRID), &mut out), Pressed::Drove);
        assert_eq!(s.press(60, 100, 0, None, &mut out), Pressed::Drove);
        assert!(out.is_empty());
        let mut off = Solo::new(SR);
        assert_eq!(off.press(60, 100, 0, Some(GRID), &mut out), Pressed::Free);
    }

    #[test]
    fn harder_taps_play_louder() {
        let loudness = |velocity: u8| {
            let mut s = solo();
            let mut out = EventBuffer::default();
            s.press(60, velocity, PERIOD as u64, Some(GRID), &mut out);
            out.as_slice().iter().find_map(|t| if let EngineEvent::NoteOn { velocity, .. } = t.event { Some(velocity) } else { None }).unwrap()
        };
        assert!(loudness(120) > loudness(40) + 15);
    }

    #[test]
    fn the_feel_goes_out() {
        let mut s = solo();
        let mut feels = Vec::new();
        let mut now = 0u64;
        for _ in 0..16 {
            let mut out = EventBuffer::default();
            s.press(60, 100, now, Some(GRID), &mut out);
            for _ in 0..(PERIOD as u64 / 2 / BLOCK as u64) {
                s.process(BLOCK, now, Some(GRID), &mut out);
                now += BLOCK as u64;
            }
            feels.extend(out.as_slice().iter().filter_map(|t| if let EngineEvent::SoloFeel { energy } = t.event { Some(energy) } else { None }));
        }
        assert!(feels.last().is_some_and(|&f| f > 150), "two taps a beat feel busy: {feels:?}");
    }

    #[test]
    fn energy_rises_with_the_rate() {
        assert_eq!(energy_of(0.0), 0.0);
        assert!((energy_of(0.5) - 0.25).abs() < 1e-6);
        assert!((energy_of(1.0) - 0.5).abs() < 1e-6);
        assert!((energy_of(2.0) - 0.75).abs() < 1e-6);
        assert_eq!(energy_of(8.0), 1.0);
    }

    /// What call and response did: each call's notes (frame, key), each
    /// score (hit, of), every note played, and the phases in order.
    #[derive(Default)]
    struct Calls {
        calls: Vec<Vec<(u64, u8)>>,
        scores: Vec<(u8, u8)>,
        notes: Vec<Played>,
        phases: Vec<CallPhase>,
    }

    /// Runs call and response for `beats`: one tap on `start` at beat 0.5
    /// asks for a call, then the player answers each call with the notes
    /// `answer` makes of it (frame, key), each held a tenth of a beat.
    fn call_and_response(solo: &mut Solo, beats: f64, start: u8, answer: impl Fn(&[(u64, u8)]) -> Vec<(u64, u8)>) -> Calls {
        let mut got = Calls::default();
        let mut presses: Vec<(u64, u8)> = vec![((0.5 * PERIOD) as u64, start)];
        let mut releases: Vec<(u64, u8)> = Vec::new();
        let mut open: Vec<(u8, u64, u8)> = Vec::new();
        let mut now = 0u64;
        while now < (beats * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            releases.retain(|&(r, key)| r >= now + BLOCK as u64 || !solo.release(key, now, &mut out));
            presses.retain(|&(at, key)| {
                if at >= now + BLOCK as u64 {
                    return true;
                }
                solo.press(key, 100, now, Some(GRID), &mut out);
                releases.push((at + (0.1 * PERIOD) as u64, key));
                false
            });
            solo.process(BLOCK, now, Some(GRID), &mut out);
            out.sort();
            for t in out.as_slice() {
                let at = now + t.frame as u64;
                match t.event {
                    EngineEvent::NoteOn { key, velocity, .. } => open.push((key, at, velocity)),
                    EngineEvent::NoteOff { key, .. } => {
                        if let Some(i) = open.iter().position(|o| o.0 == key) {
                            let (key, from, velocity) = open.remove(i);
                            got.notes.push((from, at, key, velocity));
                        }
                    }
                    EngineEvent::SoloCall { phase, hit, of, key } => {
                        if got.phases.last() != Some(&phase) {
                            got.phases.push(phase);
                        }
                        match (phase, key) {
                            (CallPhase::Listen, None) => got.calls.push(Vec::new()),
                            (CallPhase::Listen, Some(key)) => {
                                let call = got.calls.last_mut().unwrap();
                                call.push((at, key));
                                if hit == of {
                                    presses.extend(answer(call));
                                }
                            }
                            (CallPhase::Scored, _) => got.scores.push((hit, of)),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            now += BLOCK as u64;
        }
        got.notes.sort();
        got
    }

    fn calling(level: u8) -> Solo {
        let mut s = solo();
        s.set_level(level, true, 0.0, &mut EventBuffer::default());
        s
    }

    /// The call's notes a bar later, on `key` (or the call's own keys).
    fn echo(call: &[(u64, u8)], key: Option<u8>) -> Vec<(u64, u8)> {
        call.iter().map(|&(at, k)| (at + (4.0 * PERIOD) as u64, key.unwrap_or(k))).collect()
    }

    #[test]
    fn a_call_waits_for_a_tap() {
        let mut s = calling(1);
        assert!(run(&mut s, 12.0, &[], 0).is_empty());
        let got = call_and_response(&mut calling(1), 12.0, 60, |_| Vec::new());
        assert_eq!(got.phases.first(), Some(&CallPhase::Listen), "one tap asks for a call");
        assert_eq!(got.calls[0].first().map(|c| c.0), Some((4.0 * PERIOD) as u64), "on the next bar's 1, or its \"and\"");
    }

    #[test]
    fn calls_are_one_bar_of_the_scale_on_the_grid() {
        for seed in 1..=8 {
            let mut s = calling(1);
            s.seed(seed);
            let got = call_and_response(&mut s, 40.0, 60, |call| echo(call, Some(60)));
            assert!(got.calls.len() >= 4);
            for call in &got.calls {
                let bar = call[0].0 / (4 * PERIOD as u64) * (4 * PERIOD as u64);
                assert!(call.len() >= MIN_CALL as usize && call.len() <= MAX_CALL);
                for &(at, key) in call {
                    let q = (at - bar) as f64 / (PERIOD / 4.0);
                    assert!((q - q.round()).abs() < 0.01 && q <= CALL_LAST as f64, "call note at sixteenth {q}");
                    assert!([0, 2, 4, 7, 9].contains(&(key % 12)), "key {key} off the scale");
                }
                assert!([0, 4, 7].contains(&(call.last().unwrap().1 % 12)), "ends on a chord tone: {call:?}");
            }
        }
    }

    #[test]
    fn tapping_the_calls_rhythm_plays_it_back_and_the_calls_grow() {
        let got = call_and_response(&mut calling(1), 44.0, 60, |call| echo(call, Some(60)));
        assert!(got.scores.len() >= 4, "{:?}", got.phases);
        assert!(got.scores.iter().all(|&(hit, of)| hit == of), "every note answered: {:?}", got.scores);
        assert!(got.scores.windows(2).all(|w| w[1].1 == w[0].1 + 1), "each call a note longer: {:?}", got.scores);
        // The taps sound the call's notes.
        for (call, _) in got.calls.iter().zip(&got.scores) {
            for &(at, key) in &echo(call, None) {
                assert!(got.notes.iter().any(|n| n.2 == key && n.0 <= at && at < n.0 + BLOCK as u64), "the call's {key} at {at}");
            }
        }
    }

    #[test]
    fn an_answer_off_the_rhythm_scores_less() {
        let late = |call: &[(u64, u8)]| echo(call, Some(60)).into_iter().map(|(at, k)| (at + (PERIOD / 2.0) as u64, k)).collect();
        let got = call_and_response(&mut calling(1), 44.0, 60, late);
        let (hit, of) = got.scores.iter().fold((0, 0), |s, &(h, o)| (s.0 + h as u32, s.1 + o as u32));
        assert!(hit * 2 < of, "an eighth late: {hit} of {of}");
        assert!(got.scores.last().unwrap().1 <= FIRST_CALL, "the calls stay short: {:?}", got.scores);
        let doubled = |call: &[(u64, u8)]| echo(call, Some(60)).into_iter().flat_map(|(at, k)| [(at, k), (at + (PERIOD / 4.0) as u64 * 3 / 2, k)]).collect();
        let got = call_and_response(&mut calling(1), 20.0, 60, doubled);
        assert!(got.scores.iter().all(|&(hit, of)| hit < of), "extra notes count against: {:?}", got.scores);
    }

    #[test]
    fn unanswered_calls_wait_for_a_tap_again() {
        let got = call_and_response(&mut calling(2), 40.0, 67, |_| Vec::new());
        assert_eq!(got.scores, [(0, FIRST_CALL), (0, FIRST_CALL)]);
        assert_eq!(got.phases.last(), Some(&CallPhase::Idle));
        let last = got.notes.last().unwrap().0 as f64 / PERIOD;
        assert!(last < 16.0, "silent after: last note at beat {last}");
    }

    #[test]
    fn from_level_three_the_answer_is_the_notes() {
        let got = call_and_response(&mut calling(3), 44.0, 60, |call| echo(call, None));
        assert!(got.scores.len() >= 4 && got.scores.iter().all(|&(hit, of)| hit == of), "the call's notes: {:?}", got.scores);
        let (mut hit, mut of) = (0, 0);
        for seed in 1..=4 {
            let mut s = calling(3);
            s.seed(seed);
            // The call's rhythm, a scale step up.
            let up = |call: &[(u64, u8)]| echo(call, None).into_iter().map(|(at, k)| (at, k + if [4, 9].contains(&(k % 12)) { 3 } else { 2 })).collect();
            for (h, o) in call_and_response(&mut s, 30.0, 60, up).scores {
                hit += h as u32;
                of += o as u32;
            }
        }
        assert!(hit * 4 < of, "the wrong notes: {hit} of {of}");
    }

    #[test]
    fn the_key_to_play_next_goes_out_from_level_three() {
        let mut s = calling(3);
        let mut out = EventBuffer::default();
        s.press(60, 90, 0, Some(GRID), &mut out);
        s.process(BLOCK, 0, Some(GRID), &mut out);
        let mut now = BLOCK as u64;
        let mut answer_keys = Vec::new();
        while now < (8.5 * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            s.process(BLOCK, now, Some(GRID), &mut out);
            answer_keys.extend(out.as_slice().iter().filter_map(|t| match t.event {
                EngineEvent::SoloCall { phase: CallPhase::Answer, key, .. } => Some(key),
                _ => None,
            }));
            now += BLOCK as u64;
        }
        assert_eq!(answer_keys, [Some(s.call_notes[0].1)]);
    }

    /// Every note on and off with its `SoloNote`: (frame, key, velocity, ticks from bar 0, own).
    fn recorded(solo: &mut Solo, beats: f64, taps: &[(u64, u64, u8)]) -> Vec<(u64, u8, u8, i64, bool)> {
        let mut got = Vec::new();
        let mut releases: Vec<(u64, u8)> = Vec::new();
        let mut next = 0;
        let mut now = 0u64;
        while now < (beats * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            releases.retain(|&(r, key)| r >= now + BLOCK as u64 || !solo.release(key, now, &mut out));
            while next < taps.len() && taps[next].0 < now + BLOCK as u64 {
                let (at, hold, key) = taps[next];
                solo.press(key, 100, now, Some(GRID), &mut out);
                releases.push((at + hold, key));
                next += 1;
            }
            solo.process(BLOCK, now, Some(GRID), &mut out);
            out.sort();
            let mut sounds = Vec::new();
            for t in out.as_slice() {
                match t.event {
                    EngineEvent::NoteOn { key, velocity, .. } => sounds.push((now + t.frame as u64, key, velocity)),
                    EngineEvent::NoteOff { key, .. } => sounds.push((now + t.frame as u64, key, 0)),
                    EngineEvent::SoloNote { key, velocity, bar, tick, own } => {
                        let i = sounds.iter().position(|s| s.1 == key && s.2 == velocity).expect("a note went out for each record");
                        let (at, key, velocity) = sounds.remove(i);
                        assert_eq!(at, now + t.frame as u64, "on the note's frame");
                        got.push((at, key, velocity, bar as i64 * TICKS_PER_BAR as i64 + tick as i64, own));
                    }
                    _ => {}
                }
            }
            assert!(sounds.is_empty(), "every note recorded: {sounds:?}");
            now += BLOCK as u64;
        }
        got
    }

    #[test]
    fn every_note_goes_out_for_the_replay_where_it_is_heard() {
        let lead = 960.0;
        let mut s = solo();
        s.set_level(1, false, lead as f32 / SR * 1000.0, &mut EventBuffer::default());
        let notes = recorded(&mut s, 12.0, &notes_on(60, 0.3, 9.0, 0.04));
        assert!(notes.len() > 20 && notes.iter().all(|n| !n.4), "midzer's notes: {}", notes.len());
        for &(at, _, velocity, ticks, _) in &notes[1..] {
            // Heard a lead after sent, on a sixteenth (a slide a thirty-second before one).
            let heard = (at as f64 + lead) / PERIOD * PPQ as f64;
            assert!((ticks as f64 - heard).abs() <= 0.5, "tick {ticks} for a note heard at {heard}");
            assert!(velocity == 0 || ticks % (PPQ as i64 / 8) == 0, "on the grid: {ticks}");
        }
        let mut s = at_level(4);
        let own = notes_on(61, 0.5, 5.0, 0.1);
        let notes = recorded(&mut s, 6.0, &own);
        assert_eq!(notes.len(), 2 * own.len());
        for (n, o) in notes.iter().step_by(2).zip(&own) {
            assert!(n.4 && n.1 == 60, "the player's note, as it sounds: {n:?}");
            let played = o.0 / BLOCK as u64 * BLOCK as u64;
            assert_eq!(n.3, (played as f64 / PERIOD * PPQ as f64).round() as i64, "when played");
        }
    }

    #[test]
    fn rotates_scales() {
        assert_eq!(rotate(DEFAULT_SCALE, 0), DEFAULT_SCALE);
        // G major pentatonic: G A B D E.
        assert_eq!(rotate(DEFAULT_SCALE, 7), 1 << 7 | 1 << 9 | 1 << 11 | 1 << 2 | 1 << 4);
    }
}
