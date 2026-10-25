//! Improvising for a player who can't yet: the player taps, midzer solos.
//!
//! On level 1 the player taps one key, the song's home note, and a solo
//! plays itself on the beat grid the player hears. The faster the taps, the
//! busier, higher and louder it gets; the harder, the louder. When they
//! stop, the phrase ends on a chord tone and the solo rests. Holding the key
//! lands on a long note until it is let go.
//!
//! Notes come from the song's scale (a pentatonic the UI sends with the chord
//! heard, see [`crate::Command::SoloHarmony`]), chord tones on the strong
//! beats. Everything is fixed state: nothing allocates.

use crate::engine::PLAYER_CHANNEL;
use crate::event::{EngineEvent, EventBuffer};

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

    fn sixteenth_at(&self, frame: f64) -> f64 {
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
    /// The last note played, and when.
    pitch: u8,
    rising: bool,
    last_note: f64,
    rng: u32,
    feel_in: u32,
    feel_every: u32,
    feel: Option<u8>,
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
            next: None,
            mask: 0,
            last_mask: 0,
            phrase_left: 0,
            rest_left: 0,
            ending: false,
            sounding: None,
            grace: None,
            pitch: 60,
            rising: true,
            last_note: f64::NEG_INFINITY,
            rng: 0x9e37_79b9,
            feel_in: 0,
            feel_every: (FEEL_EVERY_S * sample_rate) as u32,
            feel: None,
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// Turns the solo on at `level` (0 turns it off). `lead_ms` is how long
    /// the output takes to be heard, so the notes are sent that much early.
    pub fn set_level(&mut self, level: u8, lead_ms: f32, out: &mut EventBuffer) {
        if level != self.level {
            self.silence(0, out);
            self.level = level.min(MAX_SOLO_LEVEL);
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

    /// A key pressed at engine frame `now`. Returns whether the solo took it
    /// (the key itself doesn't sound then).
    pub fn press(&mut self, key: u8, velocity: u8, now: u64, grid: Option<Grid>, out: &mut EventBuffer) -> bool {
        if self.level == 0 {
            return false;
        }
        if key % 12 != self.tonic {
            // Level 1 is the home note alone.
            return true;
        }
        self.held = Some((key, now));
        let Some(grid) = grid else { return true };
        let since = self.last_tap.map(|last| (now - last) as f64 / grid.period);
        self.rate = match since {
            Some(dt) if dt < self.gap() => 0.5 * self.rate + 0.5 * (1.0 / dt.max(1.0 / MAX_RATE)),
            // The first tap, or the first after a while: eighths to start with.
            _ => 1.0,
        };
        self.last_tap = Some(now);
        self.touch = if since.is_some() { 0.6 * self.touch + 0.4 * velocity as f32 } else { velocity as f32 };
        if (now as f64 - self.last_note) / grid.period > PICKUP_AFTER_BEATS {
            // Straight away, so the tap is heard to start it.
            self.energy = self.energy.max(energy_of(self.rate));
            self.phrase_left = 0;
            self.rest_left = 0;
            let key = self.pick(true);
            self.note_on(key, 0, now as f64, now, grid.period * 0.5, out);
        }
        true
    }

    /// A key let go at engine frame `now`.
    pub fn release(&mut self, key: u8, now: u64, out: &mut EventBuffer) -> bool {
        if self.level == 0 {
            return false;
        }
        if self.held.is_some_and(|(held, _)| held == key) {
            self.held = None;
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
        for (key, _) in [self.sounding.take(), self.grace.take()].into_iter().flatten() {
            out.push(at, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key });
        }
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
    }

    /// Plays this block's notes on `grid`, the beats the player hears (none
    /// while the beat isn't found).
    pub fn process(&mut self, frames: u32, now: u64, grid: Option<Grid>, out: &mut EventBuffer) {
        let end = (now + frames as u64) as f64;
        self.release_due(now, end, out);
        if self.level == 0 {
            return;
        }
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
                    self.sixteenth(q, t, now, &g, out);
                }
                q += 1;
            }
            self.next = Some(q);
            self.release_due(now, end, out);
        } else {
            self.next = None;
            self.energy = 0.0;
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

    /// How many beats may pass between taps before the solo winds down.
    fn gap(&self) -> f64 {
        1.25 / self.rate.max(0.25) + 0.5
    }

    /// Whether the taps still drive the solo at engine frame `at`.
    fn driven(&self, at: u64, period: f64) -> bool {
        self.held.is_some() || self.last_tap.is_some_and(|last| at.saturating_sub(last) as f64 / period <= self.gap())
    }

    fn update_energy(&mut self, frames: u32, now: u64, g: Grid) {
        if self.held.is_some_and(|(_, since)| (now - since) as f64 / g.period >= HOLD_BEATS) && self.long == Long::None {
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
                return;
            }
            Long::Waiting | Long::Sounding => return,
            Long::None => {}
        }
        if self.mask & 1 << pos == 0 || t - self.last_note < g.period / 8.0 {
            // Nothing here, or a tap's note just took this sixteenth.
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
                out.push((at - now as f64) as u32, EngineEvent::NoteOn { channel: PLAYER_CHANNEL, key: from, velocity: self.velocity(1) });
            }
        }
        self.note_on(key, pos, t, now, length, out);
        if ending {
            self.ending = false;
        }
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
        let home = self.home() as f32;
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
        out.push((t - now as f64).max(0.0) as u32, EngineEvent::NoteOn { channel: PLAYER_CHANNEL, key, velocity });
        self.sounding = Some((key, t + length));
        self.last_note = t;
        self.pitch = key;
    }

    fn end_note(&mut self, t: f64, now: u64, out: &mut EventBuffer) {
        for (key, _) in [self.grace.take(), self.sounding.take()].into_iter().flatten() {
            out.push((t - now as f64).max(0.0) as u32, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key });
        }
    }

    /// Ends the notes whose time is up before `end`.
    fn release_due(&mut self, now: u64, end: f64, out: &mut EventBuffer) {
        for slot in [&mut self.grace, &mut self.sounding] {
            if let Some((key, off)) = *slot
                && off < end
            {
                out.push((off - now as f64).max(0.0) as u32, EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key });
                *slot = None;
            }
        }
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
        ((self.next_random() >> 8) as f32 / (1u32 << 24) as f32) < p
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
        solo.set_level(1, 0.0, &mut EventBuffer::default());
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
        let mut notes: Vec<Played> = Vec::new();
        let mut open: Vec<(u8, u64, u8)> = Vec::new();
        let mut releases: Vec<u64> = Vec::new();
        let mut next = 0;
        let mut now = 0u64;
        while now < (beats * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            releases.retain(|&r| {
                if r < now + BLOCK as u64 {
                    solo.release(60, now, &mut out);
                    false
                } else {
                    true
                }
            });
            while next < taps.len() && taps[next].0 < now + BLOCK as u64 {
                solo.press(60, 100, now, Some(GRID), &mut out);
                releases.push(taps[next].0 + taps[next].1);
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
                    _ => {}
                }
            }
            now += BLOCK as u64;
        }
        notes.extend(open.into_iter().map(|(key, from, velocity)| (from, u64::MAX, key, velocity)));
        notes.sort();
        notes
    }

    #[test]
    fn other_keys_and_no_beat_play_nothing() {
        let mut s = solo();
        let mut out = EventBuffer::default();
        assert!(s.press(62, 100, 0, Some(GRID), &mut out));
        assert!(s.press(60, 100, 0, None, &mut out));
        assert!(out.is_empty());
        let mut off = Solo::new(SR);
        assert!(!off.press(60, 100, 0, Some(GRID), &mut out));
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

    #[test]
    fn rotates_scales() {
        assert_eq!(rotate(DEFAULT_SCALE, 0), DEFAULT_SCALE);
        // G major pentatonic: G A B D E.
        assert_eq!(rotate(DEFAULT_SCALE, 7), 1 << 7 | 1 << 9 | 1 << 11 | 1 << 2 | 1 << 4);
    }
}
