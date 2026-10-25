//! Accompanying for a player who can't yet: the player taps the chord's
//! root, midzer plays a keys pattern over it.
//!
//! On level 1 each tap on the root keeps the pattern going for two bars; on
//! level 2 it runs while the root is held. The faster the taps, the busier
//! the pattern's variation; the harder, the louder. When the taps stop, the
//! pattern ends on the next 1 with the root and the chord, and rests. A tap
//! after the rest sounds the root and the chord at once, so it is heard to
//! start the pattern.
//!
//! Patterns are a bass line and a chord line in the band's step grids (see
//! [`crate::style::bass`] and [`crate::style::chords`]), three variations
//! each, on the beat grid the player hears, on [`ACCOMP_CHANNEL`]. The
//! chord comes from the UI (see [`crate::Command::AccompChord`]).
//! Everything is fixed state: nothing allocates after construction.

use std::sync::OnceLock;

use crate::chord::Chord;
use crate::event::{EngineEvent, EventBuffer};
use crate::solo::{Grid, MAX_LEAD_MS, Pressed};
use crate::style::{Hit, PPQ, Pattern, Tone, bass, chords};
use crate::taps::{Taps, follow};
use crate::voicing::{Voicing, bass_note, voice_near};

/// The MIDI channel the accompaniment plays on: its own, so its notes never
/// cut the player's or the solo's.
pub const ACCOMP_CHANNEL: u8 = 5;
/// The highest level the UI may ask for.
pub const MAX_ACCOMP_LEVEL: u8 = 2;
/// How many patterns there are (see `ACCOMP_PATTERNS` in the UI).
pub const PATTERNS: u8 = 5;

/// Level 2 runs the pattern while the root is held.
const HOLD_LEVEL: u8 = 2;
/// Level 1 keeps the pattern going this many beats after the last tap.
const KEEP_BEATS: f64 = 8.0;
/// A tap on the last chord's root still counts this long after a change, in beats.
const CHANGE_SLACK_BEATS: f64 = 0.5;
/// Where the variations change: A below the first, C from the second, with
/// this much hysteresis.
const B_FROM: f32 = 0.4;
const C_FROM: f32 = 0.75;
const HYSTERESIS: f32 = 0.05;
/// How often the energy goes out at most, in seconds.
const FEEL_EVERY_S: f32 = 0.1;
/// The bass's lowest key: its root sits in C2–B2.
const BASS_LOW: u8 = 36;
/// The chord's home and range alone, and below a split with the solo.
const CHORD_HOME: f32 = 60.0;
const CHORD_RANGE: (u8, u8) = (52, 72);
const SPLIT_HOME: f32 = 52.0;
const SPLIT_RANGE: (u8, u8) = (45, 59);
/// The pickup's and the ending's length, in beats.
const PICKUP_BEATS: f64 = 1.0;
const ENDING_BEATS: f64 = 2.0;
/// Notes sounding at most.
const MAX_SOUNDING: usize = 8;
/// Ticks in a sixteenth.
const SIXTEENTH: u32 = PPQ / 4;

/// Each pattern's variations A, B and C, as (bass, chord) grids; an empty
/// line plays the previous variation's.
const LINES: [[(&str, &str); 3]; PATTERNS as usize] = [
    // Arpeggio.
    [("R-------R-------", "1---2---3---2---"), ("", "1-2-3-4-3-2-1-2-"), ("", "1234321212343212")],
    // Broken chord (Alberti).
    [("R-------5-------", "1-3-2-3-1-3-2-3-"), ("R---5---R---5---", ""), ("", "1323132313231323")],
    // Pop stabs.
    [("R-------R-------", "x-------x-------"), ("R-----R-5---R---", "x--x--x---x-x---"), ("R-.RR-.R5-.58-5-", "X-ox-ox-x-ox-oxo")],
    // Ballad.
    [("R---------------", "--1-2-3-4-3-2---"), ("R-------5-------", ""), ("", "--1234-3-2341-3-")],
    // Bounce (stride).
    [("R-------5-------", "----x-------x---"), ("R---3---5---3---", ""), ("R-5-8-5-R-5-8-5-", "--x-x-x---x-x-x-")],
];

/// One variation of a pattern.
struct Line {
    bass: Pattern,
    chords: Pattern,
}

/// The patterns, built once.
fn patterns() -> &'static [[Line; 3]] {
    static PATTERNS: OnceLock<Vec<[Line; 3]>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        LINES
            .iter()
            .map(|variations| {
                let (mut b, mut c) = ("", "");
                variations.map(|(bass_line, chord_line)| {
                    b = if bass_line.is_empty() { b } else { bass_line };
                    c = if chord_line.is_empty() { c } else { chord_line };
                    Line { bass: bass(b), chords: chords(c) }
                })
            })
            .collect()
    })
}

pub struct Accomp {
    level: u8,
    pattern: u8,
    /// Keys below this accompany a solo; 0 is the whole keyboard.
    split: u8,
    sample_rate: f32,
    /// How much earlier than heard the notes are sent: the output's latency, in frames.
    lead: f64,
    chord: Option<Chord>,
    /// The chord's root before the last change, and the frame it changed.
    last_root: Option<u8>,
    changed: u64,
    voicing: Voicing,
    taps: Taps,
    energy: f32,
    /// The variation playing: 0 (A) to 2 (C).
    variation: u8,
    /// Level 2: the root held.
    held: Option<u8>,
    /// The pattern is playing, and since the pickup at this engine frame.
    running: bool,
    pickup: f64,
    /// The next sixteenth to play.
    next: Option<i64>,
    /// The notes sounding and the engine frames they end.
    sounding: [Option<(u8, f64)>; MAX_SOUNDING],
    now: u64,
    feel_in: u32,
    feel_every: u32,
    feel: Option<u8>,
}

impl Accomp {
    pub fn new(sample_rate: f32) -> Self {
        // Built here, not on the audio path.
        patterns();
        Self {
            level: 0,
            pattern: 0,
            split: 0,
            sample_rate,
            lead: 0.0,
            chord: None,
            last_root: None,
            changed: 0,
            voicing: Voicing::default(),
            taps: Taps::default(),
            energy: 0.0,
            variation: 0,
            held: None,
            running: false,
            pickup: f64::NEG_INFINITY,
            next: None,
            sounding: [None; MAX_SOUNDING],
            now: 0,
            feel_in: 0,
            feel_every: (FEEL_EVERY_S * sample_rate) as u32,
            feel: None,
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// Turns the accompaniment on at `level` (0 turns it off) with `pattern`,
    /// below `split` (0 for all keys). `lead_ms` is how long the output takes
    /// to be heard, so the notes are sent that much early.
    pub fn set(&mut self, level: u8, pattern: u8, split: u8, lead_ms: f32, out: &mut EventBuffer) {
        let level = level.min(MAX_ACCOMP_LEVEL);
        if level != self.level || split != self.split {
            self.silence(0, out);
            self.level = level;
            self.split = split;
            self.revoice();
        }
        self.pattern = pattern.min(PATTERNS - 1);
        self.lead = lead_ms.clamp(0.0, MAX_LEAD_MS) as f64 / 1000.0 * self.sample_rate as f64;
    }

    /// The chord to accompany from now on; `None` for no chord, which ends the notes.
    pub fn set_chord(&mut self, chord: Option<Chord>, out: &mut EventBuffer) {
        if chord == self.chord {
            return;
        }
        self.last_root = self.chord.map(|c| c.root);
        self.changed = self.now;
        self.chord = chord;
        // The old chord's notes end at the change.
        self.end_all(0, out);
        self.revoice();
    }

    /// Voices the chord near the last voicing, drifting back home.
    fn revoice(&mut self) {
        let Some(chord) = self.chord else { return };
        let (home, (low, high)) = if self.split > 0 { (SPLIT_HOME, SPLIT_RANGE) } else { (CHORD_HOME, CHORD_RANGE) };
        let center = self.voicing.center().filter(|c| (low as f32..=high as f32).contains(c)).unwrap_or(home);
        self.voicing = voice_near(&chord, center * 0.7 + home * 0.3, low, high);
    }

    #[cfg(test)]
    fn voicing(&self) -> Voicing {
        self.voicing
    }

    /// A key pressed at engine frame `now`.
    pub fn press(&mut self, key: u8, velocity: u8, now: u64, grid: Option<Grid>, out: &mut EventBuffer) -> Pressed {
        if self.level == 0 {
            return Pressed::Free;
        }
        self.now = now;
        let Some(grid) = grid.filter(|g| self.drives(key, now, g.period)) else {
            // Only the lit root drives the pattern.
            return Pressed::Drove;
        };
        if self.level == HOLD_LEVEL {
            self.held = Some(key);
        }
        // Calm to start with, so a tap a bar keeps it calm.
        self.taps.count(velocity, now, grid.period, false, KEEP_BEATS, 0.25);
        if !self.running {
            self.start(now, grid.period, out);
        }
        Pressed::Drove
    }

    /// Whether `key` drives the pattern: the chord's root in any octave, or
    /// the last chord's just after a change; any key before the first chord.
    fn drives(&self, key: u8, now: u64, period: f64) -> bool {
        let pc = key % 12;
        let Some(chord) = self.chord else { return true };
        pc == chord.root || (self.last_root == Some(pc) && (now - self.changed.min(now)) as f64 / period < CHANGE_SLACK_BEATS)
    }

    /// The pickup: the root and the chord at once, then the pattern on the grid.
    fn start(&mut self, now: u64, period: f64, out: &mut EventBuffer) {
        let Some(chord) = self.chord else { return };
        self.energy = self.energy.max(energy_of(self.taps.rate));
        self.variation = variation_of(self.energy, 0);
        self.running = true;
        self.pickup = now as f64;
        let velocity = self.velocity(90);
        self.root_and_chord(&chord, now as f64, now, PICKUP_BEATS * period, velocity, out);
    }

    /// A key let go at engine frame `now`.
    pub fn release(&mut self, key: u8, now: u64) -> bool {
        if self.level == 0 {
            return false;
        }
        self.now = now;
        if self.held == Some(key) {
            self.held = None;
        }
        true
    }

    /// Stops every note at frame `at` of this block and forgets the taps.
    pub fn silence(&mut self, at: u32, out: &mut EventBuffer) {
        self.end_all(at, out);
        self.taps.clear();
        self.energy = 0.0;
        self.variation = 0;
        self.held = None;
        self.running = false;
        self.next = None;
    }

    /// Whether the player still drives the pattern at engine frame `at`.
    fn driven(&self, at: u64, period: f64) -> bool {
        if self.level == HOLD_LEVEL {
            return self.held.is_some();
        }
        self.taps.last_tap.is_some_and(|last| at.saturating_sub(last) as f64 / period <= KEEP_BEATS)
    }

    /// Plays this block's notes on `grid`, the beats the player hears (none
    /// while the beat isn't found).
    pub fn process(&mut self, frames: u32, now: u64, grid: Option<Grid>, out: &mut EventBuffer) {
        let end = (now + frames as u64) as f64;
        self.release_due(now, end, out);
        if self.level == 0 {
            return;
        }
        self.now = now;
        if let Some(g) = grid {
            // The energy holds between taps while they drive the pattern.
            let target = if self.driven(now, g.period) { energy_of(self.taps.rate) } else { 0.0 };
            self.energy = follow(self.energy, target, frames as f64 / g.period);
            let lead = self.lead;
            let mut q = match self.next {
                Some(q) if g.frame(q) - lead >= now as f64 - g.period => q,
                // Starting, or the grid jumped.
                _ => g.sixteenth_at(now as f64 + lead).ceil() as i64,
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
        } else if self.running || self.next.is_some() {
            // Lost the beat: nothing to play on.
            self.end_all(0, out);
            self.running = false;
            self.next = None;
            self.energy = 0.0;
        }
        self.feel_in = self.feel_in.saturating_sub(frames);
        if self.feel_in == 0 {
            self.feel_in = self.feel_every;
            let feel = (self.energy * 255.0).round() as u8;
            if self.feel != Some(feel) {
                self.feel = Some(feel);
                out.push(0, EngineEvent::AccompFeel { energy: feel });
            }
        }
    }

    /// Sixteenth `q`, heard at engine frame `t` (sent `lead` before): the
    /// variation is decided on each 1, and the ending plays there once the
    /// player stops driving.
    fn sixteenth(&mut self, q: i64, t: f64, now: u64, g: &Grid, out: &mut EventBuffer) {
        if !self.running {
            return;
        }
        let Some(chord) = self.chord else { return };
        let slot = (g.beat_in_bar(q) * 4 + q.rem_euclid(4)) as u32;
        if slot == 0 {
            if !self.driven(now, g.period) {
                self.running = false;
                self.end_all((t - now as f64).max(0.0) as u32, out);
                let velocity = self.velocity(96);
                self.root_and_chord(&chord, t, now, ENDING_BEATS * g.period, velocity, out);
                return;
            }
            self.variation = variation_of(self.energy, self.variation);
        }
        if t - self.pickup < g.period / 4.0 {
            // The pickup just sounded here.
            return;
        }
        let line = &patterns()[self.pattern as usize][self.variation as usize];
        let tick = slot * SIXTEENTH;
        for pattern in [&line.bass, &line.chords] {
            for step in pattern.steps.iter().filter(|s| s.tick == tick) {
                let length = step.dur as f64 / PPQ as f64 * g.period;
                let velocity = self.velocity(step.velocity);
                match step.hit {
                    Hit::Bass(tone) => self.note_on(bass_key(&chord, tone), velocity, t, now, length, out),
                    Hit::Chord => {
                        for i in 0..self.voicing.notes().len() {
                            let key = self.voicing.notes()[i];
                            self.note_on(key, velocity, t, now, length, out);
                        }
                    }
                    Hit::ChordNote(i) => {
                        let notes = self.voicing.notes();
                        if let Some(&base) = notes.get(i as usize % notes.len().max(1)) {
                            let key = (base + 12 * (i as usize / notes.len()) as u8).min(127);
                            self.note_on(key, velocity, t, now, length, out);
                        }
                    }
                    Hit::Drum(_) => {}
                }
            }
        }
    }

    /// The root in the bass and the chord, from engine frame `t` for `length` frames.
    fn root_and_chord(&mut self, chord: &Chord, t: f64, now: u64, length: f64, velocity: u8, out: &mut EventBuffer) {
        self.note_on(bass_key(chord, Tone::Root), velocity, t, now, length, out);
        for i in 0..self.voicing.notes().len() {
            let key = self.voicing.notes()[i];
            self.note_on(key, velocity, t, now, length, out);
        }
    }

    /// A step's velocity, as loud as the player taps.
    fn velocity(&self, step: u8) -> u8 {
        (step as f32 * (0.45 + 0.55 * self.taps.touch / 100.0)).clamp(20.0, 127.0) as u8
    }

    /// Starts `key` at engine frame `t` for `length` frames, ending it first
    /// if it is sounding, or the note ending soonest when all slots are taken.
    fn note_on(&mut self, key: u8, velocity: u8, t: f64, now: u64, length: f64, out: &mut EventBuffer) {
        let at = (t - now as f64).max(0.0) as u32;
        let same = self.sounding.iter().position(|s| s.is_some_and(|(k, _)| k == key));
        let free = self.sounding.iter().position(Option::is_none);
        let soonest = || (0..MAX_SOUNDING).min_by(|&a, &b| self.sounding[a].map_or(0.0, |s| s.1).total_cmp(&self.sounding[b].map_or(0.0, |s| s.1)));
        let Some(i) = same.or(free).or_else(soonest) else { return };
        if let Some((old, _)) = self.sounding[i] {
            out.push(at, EngineEvent::NoteOff { channel: ACCOMP_CHANNEL, key: old });
        }
        out.push(at, EngineEvent::NoteOn { channel: ACCOMP_CHANNEL, key, velocity });
        self.sounding[i] = Some((key, t + length));
    }

    /// Ends the notes whose time is up before `end`.
    fn release_due(&mut self, now: u64, end: f64, out: &mut EventBuffer) {
        for slot in &mut self.sounding {
            if let Some((key, off)) = *slot
                && off < end
            {
                *slot = None;
                out.push((off - now as f64).max(0.0) as u32, EngineEvent::NoteOff { channel: ACCOMP_CHANNEL, key });
            }
        }
    }

    fn end_all(&mut self, at: u32, out: &mut EventBuffer) {
        for (key, _) in self.sounding.iter_mut().filter_map(Option::take) {
            out.push(at, EngineEvent::NoteOff { channel: ACCOMP_CHANNEL, key });
        }
    }
}

/// The bass key for `tone` of `chord`, the root in C2–B2.
fn bass_key(chord: &Chord, tone: Tone) -> u8 {
    BASS_LOW + chord.root % 12 + bass_note(chord, tone) - bass_note(chord, Tone::Root)
}

/// A quarter of a tap a beat is calm (0.15), one is middling (0.5), two
/// and more busy (0.85); in between along the doublings.
fn energy_of(rate: f64) -> f32 {
    if rate <= 0.0 {
        return 0.0;
    }
    let doublings = rate.log2().clamp(-2.0, 1.0) as f32;
    if doublings < 0.0 { 0.5 + 0.175 * doublings } else { 0.5 + 0.35 * doublings }
}

/// The variation for `energy`, moving away from `current` only past the hysteresis.
fn variation_of(energy: f32, current: u8) -> u8 {
    [B_FROM, C_FROM].iter().enumerate().filter(|&(i, &from)| energy >= if current as usize > i { from - HYSTERESIS } else { from + HYSTERESIS }).count() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Quality;

    const SR: f32 = 48_000.0;
    /// 120 BPM.
    const PERIOD: f64 = 24_000.0;
    const BAR: u64 = 4 * PERIOD as u64;
    const BLOCK: u32 = 128;
    const GRID: Grid = Grid { at: 0.0, index: 0, period: PERIOD, one: 0 };
    const C: Chord = Chord::new(0, Quality::Major);
    const A_MINOR: Chord = Chord::new(9, Quality::Minor);

    /// What a run played: notes (start, end, key), and the variation at each block's start.
    #[derive(Default)]
    struct Run {
        notes: Vec<(u64, u64, u8)>,
        variations: Vec<(u64, u8)>,
        feels: Vec<u8>,
    }

    fn accomp(level: u8, pattern: u8) -> Accomp {
        let mut accomp = Accomp::new(SR);
        accomp.set(level, pattern, 0, 0.0, &mut EventBuffer::default());
        accomp.set_chord(Some(C), &mut EventBuffer::default());
        accomp
    }

    /// Runs for `beats` with presses (frame, held frames, key) and chord changes (frame, chord).
    fn run(accomp: &mut Accomp, beats: f64, presses: &[(u64, u64, u8)], changes: &[(u64, Chord)]) -> Run {
        let mut run = Run::default();
        let mut open: Vec<(u8, u64)> = Vec::new();
        let mut releases: Vec<(u64, u8)> = Vec::new();
        let (mut next, mut change) = (0, 0);
        let mut now = 0u64;
        while now < (beats * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            while change < changes.len() && changes[change].0 < now + BLOCK as u64 {
                accomp.set_chord(Some(changes[change].1), &mut out);
                change += 1;
            }
            releases.retain(|&(r, key)| {
                if r < now + BLOCK as u64 {
                    accomp.release(key, now);
                    false
                } else {
                    true
                }
            });
            while next < presses.len() && presses[next].0 < now + BLOCK as u64 {
                let (at, hold, key) = presses[next];
                accomp.press(key, 100, now, Some(GRID), &mut out);
                releases.push((at + hold, key));
                next += 1;
            }
            accomp.process(BLOCK, now, Some(GRID), &mut out);
            run.variations.push((now, accomp.variation));
            out.sort();
            for t in out.as_slice() {
                let at = now + t.frame as u64;
                match t.event {
                    EngineEvent::NoteOn { channel, key, .. } => {
                        assert_eq!(channel, ACCOMP_CHANNEL);
                        open.push((key, at));
                    }
                    EngineEvent::NoteOff { key, .. } => {
                        if let Some(i) = open.iter().position(|o| o.0 == key) {
                            let (key, from) = open.remove(i);
                            run.notes.push((from, at, key));
                        }
                    }
                    EngineEvent::AccompFeel { energy } => run.feels.push(energy),
                    _ => {}
                }
            }
            now += BLOCK as u64;
        }
        run.notes.extend(open.into_iter().map(|(key, from)| (from, u64::MAX, key)));
        run.notes.sort();
        run
    }

    /// Taps on C from beat 1, every `every` beats, until beat `until`.
    fn taps(every: f64, until: f64) -> Vec<(u64, u64, u8)> {
        let mut out = Vec::new();
        let mut b = 1.0;
        while b < until {
            out.push(((b * PERIOD) as u64, 1000, 60));
            b += every;
        }
        out
    }

    /// The variations played in the bars from `from` up to `to`.
    fn variations(run: &Run, from: u64, to: u64) -> Vec<u8> {
        let mut v: Vec<u8> = run.variations.iter().filter(|(at, _)| (from * BAR..to * BAR).contains(at)).map(|(_, v)| *v).collect();
        v.dedup();
        v
    }

    #[test]
    fn silent_without_taps() {
        for pattern in 0..PATTERNS {
            assert!(run(&mut accomp(1, pattern), 16.0, &[], &[]).notes.is_empty());
        }
    }

    #[test]
    fn a_tap_a_bar_keeps_it_calm_and_it_ends_two_bars_after() {
        let presses = taps(4.0, 16.0);
        let last = presses.last().unwrap().0;
        for pattern in 0..PATTERNS {
            let r = run(&mut accomp(1, pattern), 40.0, &presses, &[]);
            assert_eq!(variations(&r, 0, 6), [0], "pattern {pattern} stays on A");
            // The pickup sounds with the first tap.
            assert!(r.notes[0].0 <= PERIOD as u64 && r.notes[0].0 + BLOCK as u64 > PERIOD as u64);
            // Still playing a bar and a half after the last tap.
            assert!(r.notes.iter().any(|n| n.0 > last + 6 * PERIOD as u64), "pattern {pattern} keeps going");
            // The ending: on the first 1 more than two bars after the last tap, then silence.
            let ending = (last + 2 * BAR).div_ceil(BAR) * BAR;
            let after: Vec<_> = r.notes.iter().filter(|n| n.0 >= ending).collect();
            assert!(!after.is_empty() && after.iter().all(|n| n.0 == ending), "pattern {pattern} ends on the 1: {after:?}");
            assert!(after.iter().all(|n| n.1 <= ending + 2 * PERIOD as u64));
        }
    }

    #[test]
    fn faster_taps_play_busier_variations_switching_on_the_one() {
        // A tap a beat, then two a beat.
        let mut presses = taps(1.0, 12.0);
        presses.extend(taps(0.5, 28.0).into_iter().filter(|p| p.0 >= (12.0 * PERIOD) as u64));
        let r = run(&mut accomp(1, 0), 30.0, &presses, &[]);
        assert_eq!(variations(&r, 0, 1), [0], "starts calm");
        assert!(variations(&r, 2, 3).contains(&1), "a tap a beat plays B: {:?}", variations(&r, 0, 8));
        assert_eq!(variations(&r, 6, 7), [2], "two a beat play C");
        for w in r.variations.windows(2).filter(|w| w[0].1 != w[1].1) {
            // The change shows in the block that plays the 1.
            assert!(w[1].0 % BAR < BLOCK as u64, "changed off the 1 at {}", w[1].0);
        }
        let counts: Vec<_> = [(1, 3), (5, 7)].iter().map(|&(a, b)| r.notes.iter().filter(|n| (a * BAR..b * BAR).contains(&n.0)).count()).collect();
        assert!(counts[0] < counts[1], "busier: {counts:?}");
        assert!(r.feels.iter().any(|&f| f > 200), "the feel rises: {:?}", r.feels);
    }

    #[test]
    fn notes_are_chord_tones_and_revoice_on_a_change() {
        for pattern in 0..PATTERNS {
            let mut a = accomp(1, pattern);
            let before = a.voicing();
            let presses = taps(0.5, 16.0);
            let r = run(&mut a, 16.0, &presses, &[(2 * BAR, A_MINOR)]);
            assert_ne!(a.voicing(), before);
            assert!(a.voicing().notes().iter().all(|n| (CHORD_RANGE.0..=CHORD_RANGE.1).contains(n)));
            for n in &r.notes {
                let tones: &[u8] = if n.0 < 2 * BAR { &[0, 4, 7] } else { &[9, 0, 4] };
                assert!(tones.contains(&(n.2 % 12)), "pattern {pattern}: key {} at {}", n.2, n.0);
                // Nothing rings over the change.
                assert!(n.0 >= 2 * BAR || n.1 <= 2 * BAR + BLOCK as u64);
                if n.2 < 48 {
                    assert!(n.2 >= BASS_LOW);
                }
            }
            assert!(r.notes.iter().any(|n| n.2 < 48 && n.2 % 12 == 9), "pattern {pattern}: the bass follows");
        }
    }

    #[test]
    fn only_the_root_drives_it() {
        let presses: Vec<_> = taps(1.0, 8.0).into_iter().map(|(at, hold, _)| (at, hold, 62)).collect();
        assert!(run(&mut accomp(1, 0), 12.0, &presses, &[]).notes.is_empty());
        let presses: Vec<_> = taps(1.0, 8.0).into_iter().map(|(at, hold, _)| (at, hold, 48)).collect();
        assert!(!run(&mut accomp(1, 0), 12.0, &presses, &[]).notes.is_empty(), "any octave");
    }

    #[test]
    fn level_two_runs_while_held() {
        let held = 3 * BAR;
        let r = run(&mut accomp(2, 1), 32.0, &[(PERIOD as u64, held, 60)], &[]);
        let release = PERIOD as u64 + held;
        // Plays the whole time it is held.
        for bar in 0..3 {
            assert!(r.notes.iter().any(|n| (bar * BAR + BAR / 2..(bar + 1) * BAR).contains(&n.0)), "bar {bar}");
        }
        // Ends on the next 1 after the release.
        let ending = release.div_ceil(BAR) * BAR;
        assert!(r.notes.iter().filter(|n| n.0 >= ending).all(|n| n.0 == ending));
        assert!(r.notes.iter().any(|n| n.0 == ending));
        let r = run(&mut accomp(2, 1), 16.0, &[(PERIOD as u64, 1000, 60)], &[]);
        assert!(r.notes.iter().all(|n| n.0 <= BAR), "a short press ends on the next 1");
    }

    #[test]
    fn the_energy_maps_the_tap_rate() {
        assert_eq!(energy_of(0.0), 0.0);
        assert!((energy_of(0.25) - 0.15).abs() < 1e-6);
        assert!((energy_of(1.0) - 0.5).abs() < 1e-6);
        assert!((energy_of(2.0) - 0.85).abs() < 1e-6);
        assert_eq!(energy_of(4.0), 0.85);
        assert_eq!(variation_of(0.42, 0), 0);
        assert_eq!(variation_of(0.46, 0), 1);
        assert_eq!(variation_of(0.38, 1), 1);
        assert_eq!(variation_of(0.9, 0), 2);
    }

    #[test]
    fn every_pattern_line_parses() {
        assert_eq!(patterns().len(), PATTERNS as usize);
        for line in patterns().iter().flatten() {
            assert!(!line.bass.steps.is_empty() && !line.chords.steps.is_empty());
            assert!(line.bass.steps.iter().chain(&line.chords.steps).all(|s| s.tick % SIXTEENTH == 0));
        }
    }
}
