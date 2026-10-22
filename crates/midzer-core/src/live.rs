//! Playing along with live audio that midzer doesn't play: a browser tab, the
//! system's output or a line input. The [`BeatTracker`] finds its beats, and
//! a drummer's hits are judged against them.
//!
//! Judging looks back: a hit waits until the tracker has confirmed the beat
//! after it, about a beat later, and is then placed between the two beats
//! around it. The beats shown to the player are predicted ahead from the last
//! confirmed one, so they come on time. The source reaches the player and the
//! engine with different delays; a calibrated offset says how much later the
//! player hears it.
//!
//! Nothing here allocates after [`Live::new`].

use crate::drummer::keeps_time;
use crate::drums::Piece;
use crate::event::{EngineEvent, EventBuffer};
use crate::tracker::{BeatList, BeatTracker, TrackedBeat};

const BEATS: u32 = 4;
/// Confirmed beats kept for judging.
const KEPT: usize = 8;
/// Hits waiting for the beat after them.
const PENDING: usize = 64;
/// How often the status goes out at most, in seconds.
const STATUS_EVERY_S: f32 = 0.1;
/// The downbeat guess: how fast it forgets, when it starts, and how much
/// better another beat must look to take over.
const DOWNBEAT_DECAY: f32 = 0.9;
const DOWNBEAT_AFTER: u32 = 8;
const DOWNBEAT_MARGIN: f32 = 1.15;
/// The tempo shown changes when the tracked one moves by this much.
const TEMPO_STEP: f32 = 0.5;

#[derive(Debug, Clone, Copy)]
struct Hit {
    /// When the player heard the source at the moment of the hit, as the engine
    /// got it: the hit's frame less the offset.
    at: f64,
    piece: Piece,
    velocity: u8,
}

pub struct Live {
    tracker: BeatTracker,
    sample_rate: f32,
    active: bool,
    /// A run: between Play (or the first hit once locked) and Stop.
    running: bool,
    /// How much later the player hears the source than the engine gets it, in frames.
    offset: f64,
    /// Confirmed beats, oldest first: engine frame and index.
    beats: [(f64, u32); KEPT],
    beat_count: usize,
    pending: [Hit; PENDING],
    pending_count: usize,
    /// Beat indices that are a 1, modulo 4, and whether the player set it.
    downbeat: u32,
    downbeat_told: bool,
    low: [f32; 4],
    high: [f32; 4],
    beats_heard: u32,
    /// The bar the run started in, counted from the tracker's first beat.
    first_bar: i64,
    /// The next beat to show the player.
    next_shown: Option<u32>,
    status: Option<(bool, u8, u8)>,
    status_in: u32,
    status_every: u32,
    bpm: f32,
}

impl Live {
    pub fn new(sample_rate: f32) -> Self {
        let hit = Hit { at: 0.0, piece: Piece::Kick, velocity: 0 };
        let status_every = (STATUS_EVERY_S * sample_rate) as u32;
        Self {
            tracker: BeatTracker::new(sample_rate),
            sample_rate,
            active: false,
            running: false,
            offset: 0.0,
            beats: [(0.0, 0); KEPT],
            beat_count: 0,
            pending: [hit; PENDING],
            pending_count: 0,
            downbeat: 0,
            downbeat_told: false,
            low: [0.0; 4],
            high: [0.0; 4],
            beats_heard: 0,
            first_bar: 0,
            next_shown: None,
            status: None,
            status_in: 0,
            status_every,
            bpm: 0.0,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Turns live mode on or off; either way it starts listening afresh.
    pub fn set_active(&mut self, on: bool, out: &mut EventBuffer) {
        self.stop(0, out);
        self.active = on;
        self.forget();
        out.push(0, self.status_event());
    }

    fn forget(&mut self) {
        self.tracker.reset();
        self.beat_count = 0;
        self.pending_count = 0;
        self.downbeat = 0;
        self.downbeat_told = false;
        self.low = [0.0; 4];
        self.high = [0.0; 4];
        self.beats_heard = 0;
        self.next_shown = None;
        self.status = None;
        self.bpm = 0.0;
    }

    /// How much later, in ms, the player hears the source than the engine gets it.
    pub fn set_offset(&mut self, ms: f32) {
        self.offset = ms as f64 / 1000.0 * self.sample_rate as f64;
    }

    /// The player says the beat is twice (or half) as fast as the one found.
    pub fn scale_tempo(&mut self, double: bool) {
        self.tracker.scale_tempo(if double { 2.0 } else { 0.5 });
        self.beat_count = 0;
        self.next_shown = None;
    }

    /// The index of the beat the player hears nearest to engine frame `now`.
    fn beat_heard_at(&self, now: u64) -> Option<i64> {
        let (at, index, period) = self.tracker.last_beat()?;
        Some(index as i64 + ((now as f64 - self.offset - at) / period).round() as i64)
    }

    /// The player marks the beat heard nearest to engine frame `now` as a 1.
    pub fn set_downbeat(&mut self, now: u64) {
        if let Some(beat) = self.beat_heard_at(now) {
            self.downbeat = beat.rem_euclid(BEATS as i64) as u32;
            self.downbeat_told = true;
        }
    }

    /// Starts a run at engine frame `now`; its bars count from the one heard then.
    pub fn play(&mut self, now: u64, out: &mut EventBuffer) {
        if !self.active || self.running {
            return;
        }
        self.running = true;
        self.pending_count = 0;
        self.first_bar = self.beat_heard_at(now).map_or(0, |beat| (beat - self.downbeat as i64).div_euclid(BEATS as i64));
        out.push(0, EngineEvent::Transport { playing: true });
    }

    /// Ends the run at frame `at` of this block, judging the hits still waiting.
    pub fn stop(&mut self, at: u32, out: &mut EventBuffer) {
        if !self.running {
            return;
        }
        if let Some(&(last, index)) = self.beats[..self.beat_count].last() {
            // Past the last confirmed beat the grid carries on at the tempo.
            let period = self.tracker.last_beat().map_or(0.0, |b| b.2);
            if period > 0.0 {
                for i in 0..self.pending_count {
                    let hit = self.pending[i];
                    let k = ((hit.at - last) / period).floor().max(0.0);
                    let a = last + k * period;
                    self.judge(hit, (a, index + k as u32), a + period, out);
                }
            }
        }
        self.pending_count = 0;
        self.running = false;
        out.push(at, EngineEvent::Transport { playing: false });
    }

    /// A drummer's hit at engine frame `now`. The first one once the beat is
    /// found starts a run.
    pub fn hit(&mut self, piece: Piece, velocity: u8, now: u64, out: &mut EventBuffer) {
        if !self.active {
            return;
        }
        if !self.running {
            if !self.tracker.locked() {
                return;
            }
            self.play(now, out);
        }
        if self.pending_count == PENDING {
            self.pending.copy_within(1.., 0);
            self.pending_count -= 1;
        }
        self.pending[self.pending_count] = Hit { at: now as f64 - self.offset, piece, velocity };
        self.pending_count += 1;
    }

    /// Feeds a block of the source, whose first sample is at engine frame `start`.
    pub fn listen(&mut self, input: &[f32], start: u64, out: &mut EventBuffer) {
        if !self.active {
            return;
        }
        let mut beats = BeatList::default();
        self.tracker.push(input, start, &mut beats);
        for beat in beats.as_slice() {
            self.on_beat(beat, out);
        }
    }

    fn on_beat(&mut self, beat: &TrackedBeat, out: &mut EventBuffer) {
        if self.beats[..self.beat_count].last().is_some_and(|b| b.1 >= beat.index) {
            // The tracker started over.
            self.beat_count = 0;
        }
        if self.beat_count == KEPT {
            self.beats.copy_within(1.., 0);
            self.beat_count -= 1;
        }
        self.beats[self.beat_count] = (beat.at, beat.index);
        self.beat_count += 1;
        self.guess_downbeat(beat);

        if self.running {
            // Hits before this beat now have a beat on each side.
            let mut kept = 0;
            for i in 0..self.pending_count {
                let hit = self.pending[i];
                if hit.at < beat.at {
                    self.judge_between(hit, out);
                } else {
                    self.pending[kept] = hit;
                    kept += 1;
                }
            }
            self.pending_count = kept;
        }
    }

    /// Accumulates where the low and high onsets fall in the bar and picks
    /// the 1: kick and bass on 1 and 3, snare on 2 and 4.
    fn guess_downbeat(&mut self, beat: &TrackedBeat) {
        let slot = (beat.index % BEATS) as usize;
        self.low[slot] = self.low[slot] * DOWNBEAT_DECAY + beat.low;
        self.high[slot] = self.high[slot] * DOWNBEAT_DECAY + beat.high;
        self.beats_heard += 1;
        if self.downbeat_told || self.beats_heard < DOWNBEAT_AFTER {
            return;
        }
        let (low, high) = (self.low, self.high);
        let norm = |v: [f32; 4]| {
            let sum: f32 = v.iter().sum();
            v.map(|x| if sum > 0.0 { x / sum } else { 0.0 })
        };
        let (low, high) = (norm(low), norm(high));
        let score = |d: usize| low[d] + 0.5 * low[(d + 2) % 4] + 0.5 * (high[(d + 1) % 4] + high[(d + 3) % 4]) - 0.25 * (high[d] + high[(d + 2) % 4]);
        let best = (0..4).max_by(|&a, &b| score(a).total_cmp(&score(b))).unwrap_or(0);
        let current = self.downbeat as usize;
        if best != current && score(best) > score(current).max(0.0) * DOWNBEAT_MARGIN + 1e-3 {
            self.downbeat = best as u32;
        }
    }

    /// Judges a hit between the confirmed beats around it; dropped when it
    /// came before the oldest one kept.
    fn judge_between(&mut self, hit: Hit, out: &mut EventBuffer) {
        let beats = &self.beats[..self.beat_count];
        let after = beats.partition_point(|b| b.0 <= hit.at);
        if after == 0 || after == beats.len() {
            return;
        }
        let (a, b) = (beats[after - 1], beats[after]);
        // A beat skipped in between leaves an index gap: split it evenly.
        let gap = (b.1 - a.1).max(1);
        let step = (b.0 - a.0) / gap as f64;
        let k = (((hit.at - a.0) / step).floor() as u32).min(gap - 1);
        let start = (a.0 + k as f64 * step, a.1 + k);
        self.judge(hit, start, start.0 + step, out);
    }

    /// Places a hit on the sixteenths of the beat that starts at `beat` (frame, index) and ends at `end`.
    fn judge(&self, hit: Hit, beat: (f64, u32), end: f64, out: &mut EventBuffer) {
        let length = end - beat.0;
        if length <= 0.0 {
            return;
        }
        let into = (hit.at - beat.0) / length * 4.0;
        let nearest = into.round();
        let ms = ((into - nearest) / 4.0 * length / self.sample_rate as f64 * 1000.0) as f32;
        let sixteenth = (beat.1 as i64 - self.downbeat as i64) * 4 + nearest as i64;
        let bar = (sixteenth.div_euclid(16) - self.first_bar).clamp(0, u16::MAX as i64) as u16;
        let sixteenth = sixteenth.rem_euclid(16) as u8;
        out.push(0, EngineEvent::Judged { ms, piece: hit.piece, sixteenth, bar });
        if keeps_time(hit.piece, hit.velocity) {
            out.push(0, EngineEvent::Timing { ms });
        }
    }

    /// The beats the player hears in this block, predicted from the last
    /// confirmed one; the tempo and the tracker's status.
    pub fn process(&mut self, frames: u32, now: u64, out: &mut EventBuffer) {
        if !self.active {
            return;
        }
        if self.running && self.tracker.last_beat().is_none() {
            // The source went quiet and the tracker let go: the run is over.
            self.stop(0, out);
        }
        self.show_beats(frames, now, out);
        self.status_in = self.status_in.saturating_sub(frames);
        if self.status_in == 0 {
            self.status_in = self.status_every;
            let event = self.status_event();
            if let EngineEvent::Live { locked, confidence, level } = event
                && self.status != Some((locked, confidence, level))
            {
                self.status = Some((locked, confidence, level));
                out.push(0, event);
            }
        }
    }

    fn show_beats(&mut self, frames: u32, now: u64, out: &mut EventBuffer) {
        let Some((at, index, period)) = self.tracker.last_beat().filter(|_| self.tracker.locked()) else {
            self.next_shown = None;
            return;
        };
        let heard = |j: u32| at + (j as f64 - index as f64) * period + self.offset;
        let end = (now + frames as u64) as f64;
        let mut j = match self.next_shown {
            Some(j) if heard(j) >= now as f64 - 0.5 * period => j,
            // Starting, or after a jump: the first beat from now on.
            _ => (index as i64 + ((now as f64 - heard(index)) / period).ceil() as i64).max(0) as u32,
        };
        while heard(j) < end {
            let t = heard(j);
            if t >= now as f64 {
                let rel = j as i64 - self.downbeat as i64;
                let bar = (rel.div_euclid(BEATS as i64) - if self.running { self.first_bar } else { 0 }).max(0) as u32;
                out.push((t - now as f64) as u32, EngineEvent::Beat { bar, beat: rel.rem_euclid(BEATS as i64) as u8 });
            }
            j += 1;
        }
        self.next_shown = Some(j);
        let bpm = (60.0 * self.sample_rate as f64 / period) as f32;
        if (bpm - self.bpm).abs() >= TEMPO_STEP {
            self.bpm = bpm;
            out.push(0, EngineEvent::Tempo { bpm });
        }
    }

    fn status_event(&self) -> EngineEvent {
        let level = ((self.tracker.level_db() + 80.0) / 80.0 * 255.0).clamp(0.0, 255.0) as u8;
        EngineEvent::Live { locked: self.tracker.locked(), confidence: (self.tracker.confidence() * 255.0).clamp(0.0, 255.0) as u8, level }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// 120 BPM clicks from 0.25 s, the 1s louder with a low thump like a kick.
    fn source(seconds: f32) -> Vec<f32> {
        let len = (seconds * SR) as usize;
        let mut out = vec![0.0f32; len];
        let mut seed = 3u32;
        for i in 0.. {
            let start = ((0.25 + i as f32 * 0.5) * SR) as usize;
            if start >= len {
                break;
            }
            for k in 0..(0.15 * SR) as usize {
                if start + k >= len {
                    break;
                }
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
                let t = k as f32 / SR;
                let thump = if i % 4 == 0 { (2.0 * std::f32::consts::PI * 60.0 * t).sin() * 0.8 * (-t / 0.04).exp() } else { 0.0 };
                out[start + k] += noise * 0.3 * (-t / 0.006).exp() + thump;
            }
        }
        out
    }

    /// Runs `signal` through in blocks of 128 from frame `from`, with hits at
    /// the given frames; returns the events with absolute frames.
    fn run(live: &mut Live, signal: &[f32], from: u64, hits: &[(u64, Piece)]) -> Vec<(u64, EngineEvent)> {
        let mut events = Vec::new();
        let mut next = 0;
        for (i, block) in signal.chunks(128).enumerate() {
            let now = from + (i * 128) as u64;
            let mut out = EventBuffer::default();
            while next < hits.len() && hits[next].0 < now + 128 {
                live.hit(hits[next].1, 100, now, &mut out);
                next += 1;
            }
            live.listen(block, now, &mut out);
            live.process(128, now, &mut out);
            out.sort();
            events.extend(out.as_slice().iter().map(|t| (now + t.frame as u64, t.event)));
        }
        events
    }

    fn judged(events: &[(u64, EngineEvent)]) -> Vec<(f32, Piece, u8, u16)> {
        events
            .iter()
            .filter_map(|(_, e)| if let EngineEvent::Judged { ms, piece, sixteenth, bar } = e { Some((*ms, *piece, *sixteenth, *bar)) } else { None })
            .collect()
    }

    fn frame(seconds: f64) -> u64 {
        (seconds * SR as f64) as u64
    }

    #[test]
    fn judges_hits_a_beat_later_against_the_source() {
        let mut live = Live::new(SR);
        live.set_active(true, &mut EventBuffer::default());
        // The player hears the source 20 ms after the engine gets it.
        live.set_offset(20.0);
        let beat = |i: u32| 0.25 + i as f64 * 0.5 + 0.020;
        // From 12 s: snare 10 ms late on beats 2 and 4, kick 5 ms early on 1
        // and 3, hi-hat eighths on time.
        let mut hits = Vec::new();
        for i in 24..40 {
            let piece = if i % 2 == 0 { Piece::Kick } else { Piece::Snare };
            let ms = if piece == Piece::Kick { -0.005 } else { 0.010 };
            hits.push((frame(beat(i) + ms), piece));
            hits.push((frame(beat(i) + 0.25), Piece::HiHatClosed));
        }
        hits.sort_by_key(|h| h.0);
        let events = run(&mut live, &source(21.0), 0, &hits);
        let started = events.iter().find(|(_, e)| *e == EngineEvent::Transport { playing: true }).expect("the first hit starts a run");
        assert!(started.0 <= hits[0].0 && started.0 + 128 > hits[0].0);
        let j = judged(&events);
        assert_eq!(j.len(), hits.len(), "every hit judged");
        for &(ms, piece, sixteenth, _) in &j {
            let (want, at) = match piece {
                Piece::Kick => (-5.0, [0, 8]),
                Piece::Snare => (10.0, [4, 12]),
                _ => (0.0, [2, 6]),
            };
            // Hits are seen at the start of their block, up to 128 frames early.
            assert!(ms <= want + 1.5 && ms > want - 2.7 - 1.5, "{piece:?} at {sixteenth}: {ms} ms");
            if piece != Piece::HiHatClosed {
                assert!(at.contains(&sixteenth), "{piece:?} on sixteenth {sixteenth}");
            }
        }
        // The kick's low thump marks the 1, and bars count from the run's start.
        let kicks: Vec<u8> = j.iter().filter(|h| h.1 == Piece::Kick).map(|h| h.2).collect();
        assert_eq!(kicks, [0, 8].repeat(4));
        assert_eq!((j[0].3, j.last().unwrap().3), (0, 3));
        // Judged about a beat after the hit, not later.
        let last_hit = hits.last().unwrap().0;
        let last_judged = events.iter().rev().find(|(_, e)| matches!(e, EngineEvent::Judged { .. })).unwrap().0;
        assert!(last_judged > last_hit && last_judged < last_hit + frame(1.0), "{}", (last_judged - last_hit) as f64 / SR as f64);
        // Timing reaches the meter for timekeeping hits.
        assert_eq!(events.iter().filter(|(_, e)| matches!(e, EngineEvent::Timing { .. })).count(), hits.len());
    }

    #[test]
    fn shows_beats_when_the_player_hears_them_and_reports_its_status() {
        let mut live = Live::new(SR);
        live.set_active(true, &mut EventBuffer::default());
        live.set_offset(30.0);
        let events = run(&mut live, &source(12.0), 0, &[]);
        let beats: Vec<_> = events.iter().filter_map(|(f, e)| if let EngineEvent::Beat { beat, .. } = e { Some((*f, *beat)) } else { None }).collect();
        assert!(beats.len() >= 8, "{} beats", beats.len());
        for (f, b) in &beats {
            let heard = (*f as f64 / SR as f64 - 0.25 - 0.030) / 0.5;
            assert!((heard - heard.round()).abs() * 500.0 < 4.0, "beat at {} s", *f as f64 / SR as f64);
            assert_eq!(*b as i64, (heard.round() as i64).rem_euclid(4), "the thump is the 1");
        }
        assert!(beats.windows(2).all(|w| w[1].0 - w[0].0 > 23_000 && w[1].0 - w[0].0 < 25_000), "no beat twice or skipped");
        let statuses: Vec<_> = events.iter().filter_map(|(_, e)| if let EngineEvent::Live { locked, .. } = e { Some(*locked) } else { None }).collect();
        assert_eq!(statuses.first(), Some(&false));
        assert_eq!(statuses.last(), Some(&true));
        let tempo = events.iter().rev().find_map(|(_, e)| if let EngineEvent::Tempo { bpm } = e { Some(*bpm) } else { None }).unwrap();
        assert!((tempo - 120.0).abs() < 0.5, "{tempo}");
        assert!(!events.iter().any(|(_, e)| matches!(e, EngineEvent::Transport { .. })), "no run without hits");
    }

    #[test]
    fn the_player_can_move_the_1_and_silence_ends_the_run() {
        let mut live = Live::new(SR);
        live.set_active(true, &mut EventBuffer::default());
        let mut signal = source(12.0);
        signal.extend(std::iter::repeat_n(0.0, (4.0 * SR) as usize));
        let mut out = EventBuffer::default();
        let warm = &signal[..frame(10.0) as usize];
        run(&mut live, warm, 0, &[]);
        // Beat 2 of a bar (10.25 s + 0.5 s) is now the 1.
        live.set_downbeat(frame(10.75));
        live.play(frame(10.0), &mut out);
        let rest = run(&mut live, &signal[frame(10.0) as usize..], frame(10.0), &[]);
        let ones: Vec<_> = rest.iter().filter(|(_, e)| matches!(e, EngineEvent::Beat { beat: 0, .. })).map(|(f, _)| *f as f64 / SR as f64).collect();
        assert!(!ones.is_empty() && ones.iter().all(|t| ((t - 10.75) / 2.0 - ((t - 10.75) / 2.0).round()).abs() < 0.01), "{ones:?}");
        assert!(rest.iter().any(|(_, e)| *e == EngineEvent::Transport { playing: false }), "the run ends in silence");
        assert!(!live.is_running());
    }
}
