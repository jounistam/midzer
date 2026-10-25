//! Playing back the solo the player just played ("Replay my solo").
//!
//! The UI records a run's notes from the solo's [`EngineEvent::SoloNote`]s
//! and loads them back through commands, in ticks ([`PPQ`] per beat) from
//! the run's first bar. Over live audio whose beat is found, the replay
//! starts on the next bar's 1 of the beats the player hears and follows
//! them; without a beat it keeps its own clock at the run's tempo, with a
//! soft click. Everything is fixed state: nothing allocates.

use crate::engine::PLAYER_CHANNEL;
use crate::event::{DRUM_CHANNEL, EngineEvent, EventBuffer, PLAYER_DRUM_CHANNEL};
use crate::solo::Grid;
use crate::style::{BEATS_PER_BAR, PPQ, WOOD_BLOCK};

pub const MAX_REPLAY_NOTES: usize = 1024;
/// Notes sounding at once at most; past this the one ending first ends now.
const MAX_SOUNDING: usize = 16;
/// The click without a beat: soft, the 1s a little louder, a sixteenth long.
const CLICK_VELOCITY: u8 = 35;
const CLICK_ONE_VELOCITY: u8 = 50;
const CLICK_BEATS: f64 = 0.25;
/// A note found this many beats late (the beat jumped) still plays, at once.
const LATE_BEATS: f64 = 0.25;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayNote {
    /// Ticks from the run's first bar.
    pub tick: u32,
    pub len: u32,
    pub key: u8,
    pub velocity: u8,
    /// A drum hit, on [`PLAYER_DRUM_CHANNEL`].
    pub drum: bool,
}

/// Where the replay's tick 0 is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Clock {
    /// Beat `from` (a 1) of the live beats the player hears.
    Grid { from: i64 },
    /// Engine frame `at`, `period` frames a beat.
    Free { at: f64, period: f64 },
}

pub struct Replay {
    notes: [ReplayNote; MAX_REPLAY_NOTES],
    count: usize,
    /// The last note's end, in ticks.
    end_tick: u32,
    sample_rate: f32,
    playing: bool,
    /// The run's tempo, for playing without a beat.
    bpm: f32,
    /// Set once the replay knows where it starts.
    clock: Option<Clock>,
    /// The grid last seen, to carry on at its tempo if the beat is lost.
    last_grid: Option<Grid>,
    /// Tick 0 was reached and said so.
    started: bool,
    /// The next note to start.
    next: usize,
    /// Notes sounding: channel, key and the beat it ends on.
    sounding: [Option<(u8, u8, f64)>; MAX_SOUNDING],
}

impl Replay {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            notes: [ReplayNote::default(); MAX_REPLAY_NOTES],
            count: 0,
            end_tick: 0,
            sample_rate,
            playing: false,
            bpm: 120.0,
            clock: None,
            last_grid: None,
            started: false,
            next: 0,
            sounding: [None; MAX_SOUNDING],
        }
    }

    /// Forgets the notes, stopping first.
    pub fn clear(&mut self, out: &mut EventBuffer) {
        self.stop(0, out);
        self.count = 0;
        self.end_tick = 0;
    }

    /// Adds a note, in order of its tick; past [`MAX_REPLAY_NOTES`] it is dropped.
    pub fn add(&mut self, note: ReplayNote) {
        if self.count == MAX_REPLAY_NOTES {
            return;
        }
        let at = self.notes[..self.count].partition_point(|n| n.tick <= note.tick);
        self.notes.copy_within(at..self.count, at + 1);
        self.notes[at] = note;
        self.count += 1;
        self.end_tick = self.end_tick.max(note.tick.saturating_add(note.len));
    }

    #[cfg(test)]
    fn is_playing(&self) -> bool {
        self.playing
    }

    /// Plays from the start (again), at `bpm` when there is no beat, or stops.
    pub fn set_playing(&mut self, on: bool, bpm: f32, out: &mut EventBuffer) {
        self.stop(0, out);
        if on {
            self.playing = true;
            self.bpm = bpm;
        }
    }

    /// Stops at frame `at` of this block, ending the notes sounding.
    pub fn stop(&mut self, at: u32, out: &mut EventBuffer) {
        for (channel, key, _) in self.sounding.iter_mut().filter_map(Option::take) {
            out.push(at, EngineEvent::NoteOff { channel, key });
        }
        if std::mem::take(&mut self.playing) {
            out.push(at, EngineEvent::Replaying { playing: false });
        }
        self.clock = None;
        self.started = false;
        self.next = 0;
    }

    /// Plays this block's notes. `grid` is the live beats the player hears
    /// (none while the beat isn't found), `lead` how many frames early the
    /// output must be sent to be heard on them.
    pub fn process(&mut self, frames: u32, now: u64, grid: Option<Grid>, lead: f64, out: &mut EventBuffer) {
        if !self.playing {
            return;
        }
        let (start, end) = (now as f64, (now + frames as u64) as f64);
        let clock = match (self.clock, grid, self.last_grid) {
            (None, Some(g), _) => {
                // The next bar's 1.
                let beat = (g.sixteenth_at(start + lead) / 4.0).ceil() as i64;
                Clock::Grid { from: beat + (g.one - beat).rem_euclid(BEATS_PER_BAR as i64) }
            }
            (None, None, _) => Clock::Free { at: start, period: self.sample_rate as f64 * 60.0 / self.bpm.max(1.0) as f64 },
            // The beat was lost: on at its last tempo.
            (Some(Clock::Grid { from }), None, Some(last)) => {
                let beat = last.sixteenth_at(start + lead) / 4.0 - from as f64;
                Clock::Free { at: start - beat * last.period, period: last.period }
            }
            (Some(clock), _, _) => clock,
        };
        self.clock = Some(clock);
        if grid.is_some() {
            self.last_grid = grid;
        }
        let beat_at = |frame: f64| match (clock, grid) {
            (Clock::Grid { from }, Some(g)) => g.sixteenth_at(frame + lead) / 4.0 - from as f64,
            (Clock::Free { at, period }, _) => (frame - at) / period,
            (Clock::Grid { .. }, None) => unreachable!("a grid clock without a grid became free"),
        };
        let frame_of = |beat: f64| {
            let frame = match (clock, grid) {
                (Clock::Grid { from }, Some(g)) => g.at + (from as f64 + beat - g.index as f64) * g.period - lead,
                (Clock::Free { at, period }, _) => at + beat * period,
                (Clock::Grid { .. }, None) => unreachable!("a grid clock without a grid became free"),
            };
            (frame - start).clamp(0.0, frames as f64 - 1.0) as u32
        };
        let (from, to) = (beat_at(start), beat_at(end));
        if !self.started && to > 0.0 {
            self.started = true;
            out.push(frame_of(0.0), EngineEvent::Replaying { playing: true });
        }
        for slot in &mut self.sounding {
            if let Some((channel, key, off)) = *slot
                && off < to
            {
                out.push(frame_of(off), EngineEvent::NoteOff { channel, key });
                *slot = None;
            }
        }
        if let Clock::Free { .. } = clock {
            let last = (self.end_tick as f64 / PPQ as f64).ceil();
            let mut beat = from.ceil().max(0.0);
            while beat < to && beat < last {
                let velocity = if (beat as i64).rem_euclid(BEATS_PER_BAR as i64) == 0 { CLICK_ONE_VELOCITY } else { CLICK_VELOCITY };
                self.sound(DRUM_CHANNEL, WOOD_BLOCK, velocity, frame_of(beat), beat + CLICK_BEATS, out);
                beat += 1.0;
            }
        }
        while self.next < self.count {
            let note = self.notes[self.next];
            let beat = note.tick as f64 / PPQ as f64;
            if beat >= to {
                break;
            }
            self.next += 1;
            if beat < from - LATE_BEATS {
                continue;
            }
            let channel = if note.drum { PLAYER_DRUM_CHANNEL } else { PLAYER_CHANNEL };
            self.sound(channel, note.key, note.velocity, frame_of(beat), beat + note.len.max(1) as f64 / PPQ as f64, out);
        }
        if self.next == self.count && self.sounding.iter().all(Option::is_none) && to * PPQ as f64 >= self.end_tick as f64 {
            self.stop(frames - 1, out);
        }
    }

    /// Starts `key` at frame `at` until beat `off`, ending it first if it sounds.
    fn sound(&mut self, channel: u8, key: u8, velocity: u8, at: u32, off: f64, out: &mut EventBuffer) {
        let same = self.sounding.iter().position(|s| s.is_some_and(|(c, k, _)| c == channel && k == key));
        let free = self.sounding.iter().position(Option::is_none);
        let first = || (0..MAX_SOUNDING).min_by(|&a, &b| self.sounding[a].map_or(0.0, |s| s.2).total_cmp(&self.sounding[b].map_or(0.0, |s| s.2)));
        let Some(i) = same.or(free).or_else(first) else { return };
        if let Some((channel, key, _)) = self.sounding[i] {
            out.push(at, EngineEvent::NoteOff { channel, key });
        }
        out.push(at, EngineEvent::NoteOn { channel, key, velocity });
        self.sounding[i] = Some((channel, key, off));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    /// 120 BPM.
    const PERIOD: f64 = 24_000.0;
    const BLOCK: u32 = 128;

    /// A note played: start and end frame, channel, key.
    type Played = (u64, u64, u8, u8);

    /// Plays `replay` for `beats`, on `grid` until frame `lose` (or never);
    /// returns the notes, when it said it started and stopped.
    fn run(replay: &mut Replay, beats: f64, grid: Option<Grid>, lose: u64) -> (Vec<Played>, Vec<(u64, bool)>) {
        let mut notes = Vec::new();
        let mut open: Vec<(u8, u8, u64)> = Vec::new();
        let mut said = Vec::new();
        let mut now = 0;
        while now < (beats * PERIOD) as u64 {
            let mut out = EventBuffer::default();
            replay.process(BLOCK, now, grid.filter(|_| now < lose), 0.0, &mut out);
            out.sort();
            for t in out.as_slice() {
                let at = now + t.frame as u64;
                match t.event {
                    EngineEvent::NoteOn { channel, key, .. } => open.push((channel, key, at)),
                    EngineEvent::NoteOff { channel, key } => {
                        let i = open.iter().position(|o| o.0 == channel && o.1 == key).expect("an off for a note on");
                        let (channel, key, from) = open.remove(i);
                        notes.push((from, at, channel, key));
                    }
                    EngineEvent::Replaying { playing } => said.push((at, playing)),
                    _ => {}
                }
            }
            now += BLOCK as u64;
        }
        assert!(open.is_empty(), "notes left sounding: {open:?}");
        notes.sort();
        (notes, said)
    }

    /// A one-bar lick: C on the 1, E on the "and" of 2, G on 3 for a beat.
    fn lick() -> Replay {
        let mut r = Replay::new(SR);
        for (tick, len, key) in [(1200, 240, 64), (0, 240, 60), (1920, 480, 67)] {
            r.add(ReplayNote { tick, len, key, velocity: 90, drum: false });
        }
        r
    }

    fn keys(notes: &[Played]) -> Vec<(u64, u64, u8)> {
        notes.iter().filter(|n| n.2 == PLAYER_CHANNEL).map(|n| (n.0, n.1, n.3)).collect()
    }

    #[test]
    fn on_the_beat_it_starts_on_the_next_bars_one() {
        let mut r = lick();
        r.set_playing(true, 90.0, &mut EventBuffer::default());
        // Beat 2 is a 1; the player hears beat 0 at frame 1000.
        let grid = Grid { at: 1000.0, index: 0, period: PERIOD, one: 2 };
        let (notes, said) = run(&mut r, 12.0, Some(grid), u64::MAX);
        let one = 1000 + 2 * PERIOD as u64;
        let near = |a: u64, b: f64| (a as f64 - b).abs() <= 1.0;
        let played = keys(&notes);
        assert_eq!(played.iter().map(|n| n.2).collect::<Vec<_>>(), [60, 64, 67]);
        assert!(near(played[0].0, one as f64) && near(played[0].1, one as f64 + PERIOD / 2.0), "{played:?}");
        assert!(near(played[1].0, one as f64 + 2.5 * PERIOD), "{played:?}");
        assert!(near(played[2].0, one as f64 + 4.0 * PERIOD) && near(played[2].1, one as f64 + 5.0 * PERIOD), "{played:?}");
        assert!(notes.iter().all(|n| n.2 == PLAYER_CHANNEL), "no click on the beat");
        assert_eq!(said.len(), 2);
        assert!(near(said[0].0, one as f64) && said[0].1);
        assert!(!said[1].1 && said[1].0 >= played[2].1);
        assert!(!r.is_playing());
    }

    #[test]
    fn without_a_beat_it_keeps_the_runs_tempo_with_a_click() {
        let mut r = lick();
        r.set_playing(true, 120.0, &mut EventBuffer::default());
        let (notes, said) = run(&mut r, 8.0, None, 0);
        let played = keys(&notes);
        assert_eq!(played.iter().map(|n| (n.0, n.2)).collect::<Vec<_>>(), [(0, 60), (30_000 * 2, 64), (24_000 * 4, 67)]);
        let clicks: Vec<_> = notes.iter().filter(|n| n.2 == DRUM_CHANNEL).map(|n| n.0).collect();
        assert_eq!(clicks, (0..5).map(|b| b * PERIOD as u64).collect::<Vec<_>>(), "a click a beat to the end");
        assert_eq!(said.iter().map(|s| s.1).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn losing_the_beat_carries_on_at_its_tempo() {
        let mut r = lick();
        r.set_playing(true, 60.0, &mut EventBuffer::default());
        let grid = Grid { at: 0.0, index: 0, period: PERIOD, one: 0 };
        // Lost after the first note.
        let (notes, _) = run(&mut r, 8.0, Some(grid), PERIOD as u64);
        let played = keys(&notes);
        assert_eq!(played.iter().map(|n| (n.0, n.2)).collect::<Vec<_>>(), [(0, 60), (60_000, 64), (96_000, 67)]);
    }

    #[test]
    fn stopping_ends_the_notes_and_playing_again_starts_over() {
        let mut r = lick();
        r.set_playing(true, 120.0, &mut EventBuffer::default());
        let mut out = EventBuffer::default();
        r.process(BLOCK, 0, None, 0.0, &mut out);
        let mut out = EventBuffer::default();
        r.set_playing(false, 120.0, &mut out);
        let events: Vec<_> = out.as_slice().iter().map(|t| t.event).collect();
        assert!(events.contains(&EngineEvent::NoteOff { channel: PLAYER_CHANNEL, key: 60 }));
        assert!(events.contains(&EngineEvent::Replaying { playing: false }));
        r.set_playing(true, 120.0, &mut EventBuffer::default());
        let (notes, _) = run(&mut r, 8.0, None, 0);
        assert_eq!(keys(&notes).len(), 3);
    }

    #[test]
    fn drum_hits_replay_on_the_players_kit() {
        let mut r = Replay::new(SR);
        r.add(ReplayNote { tick: 0, len: 120, key: 38, velocity: 90, drum: true });
        r.add(ReplayNote { tick: 0, len: 240, key: 60, velocity: 90, drum: false });
        r.set_playing(true, 120.0, &mut EventBuffer::default());
        let (notes, _) = run(&mut r, 2.0, None, 0);
        assert!(notes.contains(&(0, (PERIOD / 4.0) as u64, PLAYER_DRUM_CHANNEL, 38)), "{notes:?}");
        assert!(notes.contains(&(0, (PERIOD / 2.0) as u64, PLAYER_CHANNEL, 60)), "{notes:?}");
    }

    #[test]
    fn keeps_to_its_size() {
        let mut r = Replay::new(SR);
        for i in 0..MAX_REPLAY_NOTES as u32 + 10 {
            r.add(ReplayNote { tick: i * 60, len: 60, key: 60, velocity: 80, drum: false });
        }
        assert_eq!(r.count, MAX_REPLAY_NOTES);
        r.clear(&mut EventBuffer::default());
        assert_eq!(r.count, 0);
    }
}
