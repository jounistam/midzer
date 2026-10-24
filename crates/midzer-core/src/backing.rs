//! Backing tracks: a recording the engine plays instead of the band.
//!
//! The recording comes with a tempo map from its analysis (the time of every
//! song beat), so the beats, the chart's chords and the song position follow
//! the recording even where its tempo drifts, and a drummer's hits are judged
//! against its own grid. Seeks and loops jump on bar lines with a short
//! cross-fade. On the desktop the drum stem can be kept apart and muted.
//!
//! The samples are handed over already decoded at the engine's rate (see
//! [`Backing::set_track`]); swapping them is the only thing that allocates or
//! frees, and the host does that off the audio thread where it can.

use crate::chord::Chord;
use crate::drummer::keeps_time;
use crate::drums::Piece;
use crate::event::{EngineEvent, EventBuffer};
use crate::song::{MAX_SONG_BEATS, SongPlayer};

/// Every song beat plus the end of the last bar.
pub const MAX_TRACK_BEATS: usize = MAX_SONG_BEATS + 1;
/// Frames of the cross-fade on starts, stops and jumps, so nothing clicks.
const FADE: u32 = 256;
/// The tempo shown changes when the recording's moves by this much.
const TEMPO_STEP: f32 = 0.5;

/// A decoded recording: interleaved stereo `i16` at the engine's rate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    /// The whole mix, or everything but the drums when `drums` is set.
    pub main: Vec<i16>,
    /// The drum stem, as long as `main`.
    pub drums: Option<Vec<i16>>,
}

impl Track {
    pub fn frames(&self) -> u64 {
        (self.main.len() / 2) as u64
    }
}

/// A hit placed on the recording's grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Judged {
    /// How far from the nearest sixteenth, in ms; negative is early.
    pub ms: f32,
    /// The sixteenth of the bar it was meant for, 0..28 (four a beat).
    pub sixteenth: u8,
    pub bar: u16,
    /// The song beat it was meant for, counting the sixteenth after it as that beat.
    pub beat: u32,
}

/// Whether the pitch classes played fit `chord`: every one a chord tone, or
/// all but one when four or more are played (a colour note). Single notes
/// aren't checked: a melody moves through notes outside the chord.
pub(crate) fn fits(pitches: u16, chord: Chord) -> Option<bool> {
    let count = pitches.count_ones();
    if count < 2 {
        return None;
    }
    let tones = chord.pitch_classes().fold(0u16, |set, pc| set | 1 << pc);
    let outside = (pitches & !tones).count_ones();
    Some(outside == 0 || (outside == 1 && count >= 4))
}

/// What the mixer does at a frame of this block.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cue {
    Start { at: u32, from: u64 },
    Jump { at: u32, to: u64 },
    Stop { at: u32 },
}

pub struct Backing {
    track: Option<Box<Track>>,
    sample_rate: f64,
    /// Song beat `i` is heard `beats[i]` seconds into the recording.
    beats: [f32; MAX_TRACK_BEATS],
    beat_count: usize,
    /// Whether the recording, rather than the band, plays songs.
    active: bool,
    playing: bool,
    /// The recording's frame at the start of the next block.
    pos: u64,
    /// The next song beat to report.
    next_beat: u32,
    start_bar: u32,
    seek: Option<u32>,
    loop_bars: Option<(u32, u32)>,
    drums_on: bool,
    gain: f32,
    chord: Option<Option<Chord>>,
    bpm: f32,
    /// This block's cues for the mixer, in order.
    cues: [Option<Cue>; 2],
    /// The mixer: where it reads, frames into its fade-in, and a fading-out read.
    cursor: Option<u64>,
    ramp: u32,
    tail: Option<(u64, u32)>,
}

impl Backing {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            track: None,
            sample_rate: sample_rate as f64,
            beats: [0.0; MAX_TRACK_BEATS],
            beat_count: 0,
            active: false,
            playing: false,
            pos: 0,
            next_beat: 0,
            start_bar: 0,
            seek: None,
            loop_bars: None,
            drums_on: true,
            gain: 1.0,
            chord: None,
            bpm: 0.0,
            cues: [None; 2],
            cursor: None,
            ramp: 0,
            tail: None,
        }
    }

    /// Swaps in a recording (or none) and returns the old one, for the host
    /// to free off the audio thread. Playing stops.
    pub fn set_track(&mut self, track: Option<Box<Track>>, out: &mut EventBuffer) -> Option<Box<Track>> {
        self.stop(0, out);
        self.cursor = None;
        self.tail = None;
        self.cues = [None; 2];
        std::mem::replace(&mut self.track, track)
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, on: bool, out: &mut EventBuffer) {
        if !on {
            self.stop(0, out);
        }
        self.active = on;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Empties the tempo map and sets how many beat times follow.
    pub fn clear_beats(&mut self, count: u32) {
        self.beat_count = (count as usize).min(MAX_TRACK_BEATS);
        self.beats[..self.beat_count].fill(0.0);
    }

    pub fn set_beat(&mut self, index: u32, seconds: f32) {
        if (index as usize) < self.beat_count {
            self.beats[index as usize] = seconds.max(0.0);
        }
    }

    pub fn set_drums(&mut self, on: bool) {
        self.drums_on = on;
    }

    /// The master volume; at the band's default the recording plays as recorded.
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = (gain / 0.6).clamp(0.0, 1.5);
    }

    pub fn set_loop(&mut self, start: u32, end: u32) {
        self.loop_bars = (start < end).then_some((start, end));
    }

    /// Stopped: where playing starts. Playing: a jump at the next bar line.
    pub fn seek(&mut self, bar: u32) {
        if self.playing {
            self.seek = Some(bar);
        } else {
            self.start_bar = bar;
        }
    }

    /// Seconds into the recording of song beat `i`, extrapolated from the
    /// first or last beats outside the map.
    fn beat_time(&self, i: i64) -> Option<f64> {
        let n = self.beat_count;
        if n < 2 {
            return None;
        }
        let b = |k: usize| self.beats[k] as f64;
        Some(if i < 0 {
            b(0) + i as f64 * (b(1) - b(0))
        } else if (i as usize) < n {
            b(i as usize)
        } else {
            b(n - 1) + (i as usize - (n - 1)) as f64 * (b(n - 1) - b(n - 2))
        })
    }

    fn beat_frame(&self, i: u32) -> Option<u64> {
        self.beat_time(i as i64).map(|t| (t * self.sample_rate).round().max(0.0) as u64)
    }

    /// Starts from the start bar, a bar early so the player hears it coming;
    /// from the very start at bar 0.
    pub fn play(&mut self, band_bpm: f32, chart: &SongPlayer, out: &mut EventBuffer) {
        if self.playing || self.track.is_none() || self.beat_count < 2 {
            return;
        }
        let from = match self.start_bar {
            0 => 0,
            bar => self.beat_frame(chart.bar_start(bar - 1)).unwrap_or(0),
        };
        self.pos = from;
        self.next_beat = (0..).find(|&i| self.beat_frame(i).is_none_or(|f| f >= from)).unwrap_or(0);
        self.seek = None;
        self.playing = true;
        self.chord = None;
        self.bpm = band_bpm;
        self.cue(Cue::Start { at: 0, from });
        out.push(0, EngineEvent::Transport { playing: true });
    }

    /// Stops at frame `at` of this block.
    pub fn stop(&mut self, at: u32, out: &mut EventBuffer) {
        if !self.playing {
            return;
        }
        self.playing = false;
        self.seek = None;
        self.cue(Cue::Stop { at });
        out.push(at, EngineEvent::Transport { playing: false });
    }

    fn cue(&mut self, cue: Cue) {
        // A start and a stop in one block keep the later.
        match self.cues.iter_mut().find(|c| c.is_none()) {
            Some(slot) => *slot = Some(cue),
            None => self.cues[1] = Some(cue),
        }
    }

    /// Where a bar line at song beat `beat` jumps to: a seek, or the loop's start.
    fn jump_target(&mut self, beat: u32, chart: &SongPlayer) -> Option<u32> {
        if chart.bar_of(beat).1 != 0 {
            return None;
        }
        if let Some(bar) = self.seek.take() {
            return Some(bar);
        }
        match self.loop_bars {
            Some((start, end)) if beat >= chart.bar_start(end) => Some(start),
            _ => None,
        }
    }

    /// Reports the beats in the next `frames` and queues the jumps on them.
    pub fn process(&mut self, frames: u32, chart: &SongPlayer, out: &mut EventBuffer) {
        if !self.playing || frames == 0 {
            return;
        }
        let chart_beats = chart.beats();
        let mut offset = 0u32;
        let mut pos = self.pos;
        while let Some(f) = self.beat_frame(self.next_beat) {
            let end = pos + (frames - offset) as u64;
            if f >= end {
                break;
            }
            let beat = self.next_beat;
            let at = offset + f.saturating_sub(pos) as u32;
            if let Some(bar) = self.jump_target(beat, chart)
                && let Some(to) = self.beat_frame(chart.bar_start(bar))
                && chart.bar_start(bar) != beat
            {
                self.cue(Cue::Jump { at, to });
                self.next_beat = chart.bar_start(bar);
                pos = to;
                offset = at;
                continue;
            }
            if beat >= chart_beats {
                // Past the chart: the recording plays out without beats.
                if self.loop_bars.is_none() {
                    break;
                }
                self.next_beat += 1;
                continue;
            }
            self.report_beat(chart, beat, at, out);
            self.next_beat += 1;
        }
        self.pos = pos + (frames - offset) as u64;
        let len = self.track.as_ref().map_or(0, |t| t.frames());
        if self.pos >= len {
            let at = frames - (self.pos - len).min(frames as u64) as u32;
            self.stop(at, out);
        }
    }

    fn report_beat(&mut self, chart: &SongPlayer, beat: u32, at: u32, out: &mut EventBuffer) {
        let (bar, beat_in_bar) = chart.bar_of(beat);
        out.push(at, EngineEvent::Beat { bar, beat: beat_in_bar });
        out.push(at, EngineEvent::SongPosition { beat });
        self.report_chord(chart, beat, at, out);
        let len = chart.bar_start(bar + 1).saturating_sub(beat).max(1);
        if beat_in_bar == 0
            && let (Some(a), Some(b)) = (self.beat_time(beat as i64), self.beat_time((beat + len) as i64))
            && b > a
        {
            let bpm = (60.0 * len as f64 / (b - a)) as f32;
            if (bpm - self.bpm).abs() >= TEMPO_STEP {
                self.bpm = bpm;
                out.push(at, EngineEvent::Tempo { bpm });
            }
        }
    }

    fn report_chord(&mut self, chart: &SongPlayer, beat: u32, at: u32, out: &mut EventBuffer) {
        let chord = chart.chord_at(beat);
        if self.chord != Some(chord) {
            self.chord = Some(chord);
            out.push(at, EngineEvent::ChordChanged(chord));
        }
    }

    /// Places a hit made `late_ms` before now (the player's latency) on the
    /// recording's grid of sixteenths, in the chart's bars.
    pub fn judge(&self, late_ms: f32, chart: &SongPlayer) -> Option<Judged> {
        if !self.playing {
            return None;
        }
        let t = self.pos as f64 / self.sample_rate - late_ms as f64 / 1000.0;
        let n = self.beat_count;
        let first = self.beat_time(0)?;
        // The beat the hit falls in, then the nearest sixteenth inside it.
        let k = if t < first {
            -1
        } else {
            let i = self.beats[..n].partition_point(|&b| b as f64 <= t) as i64 - 1;
            if i as usize >= n - 1 {
                let last = self.beat_time(n as i64 - 1)?;
                n as i64 - 1 + ((t - last) / (last - self.beat_time(n as i64 - 2)?)).floor() as i64
            } else {
                i
            }
        };
        let (a, b) = (self.beat_time(k)?, self.beat_time(k + 1)?);
        let into = (t - a) / (b - a) * 4.0;
        let nearest = into.round();
        let ms = ((into - nearest) / 4.0 * (b - a) * 1000.0) as f32;
        let sixteenth = k * 4 + nearest as i64;
        if sixteenth < 0 {
            // Well before the first beat: nothing to judge against yet.
            return None;
        }
        let (bar, beat_in_bar) = chart.bar_of((sixteenth / 4) as u32);
        let beat = ((sixteenth + 1) / 4) as u32;
        let sixteenth = beat_in_bar as u32 * 4 + (sixteenth % 4) as u32;
        Some(Judged { ms, sixteenth: sixteenth as u8, bar: bar.min(u16::MAX as u32) as u16, beat })
    }

    /// Judges a drummer's hit; the meter only shows hits that keep time.
    pub fn drum_hit(&self, piece: Piece, velocity: u8, late_ms: f32, chart: &SongPlayer, out: &mut EventBuffer) {
        let Some(j) = self.judge(late_ms, chart) else { return };
        out.push(0, EngineEvent::Judged { ms: j.ms, piece, sixteenth: j.sixteenth, bar: j.bar });
        if keeps_time(piece, velocity) {
            out.push(0, EngineEvent::Timing { ms: j.ms });
        }
    }

    /// Judges a keys player's strike (the pitch classes played together, a
    /// bit set) made `late_ms` before now, and whether it fits the chart's
    /// chord on the beat it was meant for.
    pub fn keys_strike(&self, pitches: u16, late_ms: f32, chart: &SongPlayer, out: &mut EventBuffer) {
        let Some(j) = self.judge(late_ms, chart) else { return };
        // A strike a sixteenth before a beat is meant for that beat's chord.
        let chord = chart.chord_at(j.beat).and_then(|c| fits(pitches, c));
        out.push(0, EngineEvent::KeysJudged { ms: j.ms, sixteenth: j.sixteenth, bar: j.bar, chord });
        out.push(0, EngineEvent::Timing { ms: j.ms });
    }

    /// Adds this block of the recording to `left` and `right`, after
    /// [`Backing::process`] for the same block.
    pub fn mix(&mut self, left: &mut [f32], right: &mut [f32]) {
        let cues = std::mem::take(&mut self.cues);
        let Some(track) = self.track.as_deref() else { return };
        if self.cursor.is_none() && self.tail.is_none() && cues[0].is_none() {
            return;
        }
        let len = track.frames();
        let drums = if self.drums_on { track.drums.as_deref() } else { None };
        let read = |pos: u64| -> (f32, f32) {
            if pos >= len {
                return (0.0, 0.0);
            }
            let i = pos as usize * 2;
            let mut l = track.main[i] as f32;
            let mut r = track.main[i + 1] as f32;
            if let Some(d) = drums {
                l += d[i] as f32;
                r += d[i + 1] as f32;
            }
            (l, r)
        };
        let scale = self.gain / 32_768.0;
        let fade = FADE as f32;
        for i in 0..left.len().min(right.len()) {
            for cue in cues.iter().flatten() {
                match *cue {
                    Cue::Start { at, from } if at as usize == i => {
                        self.tail = self.cursor.map(|c| (c, FADE));
                        self.cursor = Some(from);
                        self.ramp = 0;
                    }
                    Cue::Jump { at, to } if at as usize == i => {
                        self.tail = self.cursor.map(|c| (c, FADE));
                        self.cursor = Some(to);
                        self.ramp = 0;
                    }
                    Cue::Stop { at } if at as usize == i => {
                        self.tail = self.cursor.take().map(|c| (c, FADE));
                    }
                    _ => {}
                }
            }
            let (mut l, mut r) = (0.0, 0.0);
            if let Some(c) = self.cursor {
                let g = (self.ramp as f32 / fade).min(1.0);
                let (a, b) = read(c);
                l += a * g;
                r += b * g;
                self.cursor = Some(c + 1);
                self.ramp = self.ramp.saturating_add(1);
            }
            if let Some((t, left_frames)) = self.tail {
                let g = left_frames as f32 / fade;
                let (a, b) = read(t);
                l += a * g;
                r += b * g;
                self.tail = (left_frames > 1).then_some((t + 1, left_frames - 1));
            }
            left[i] += l * scale;
            right[i] += r * scale;
        }
        if self.cursor.is_some_and(|c| c >= len) {
            self.cursor = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Quality;
    use crate::song::Slot;

    const RATE: f32 = 48_000.0;

    /// A recording of `seconds` whose left channel holds its own frame number
    /// (mod 30 000), so the mixer's reads can be checked.
    fn track(seconds: f32, drums: bool) -> Box<Track> {
        let frames = (seconds * RATE) as usize;
        let main: Vec<i16> = (0..frames).flat_map(|f| [(f % 30_000) as i16, 0]).collect();
        let drums = drums.then(|| (0..frames).flat_map(|_| [0, 1000]).collect());
        Box::new(Track { main, drums })
    }

    /// A tempo map starting at `first` seconds whose beats get `drift` seconds longer each.
    fn backing(beats: u32, first: f32, beat: f32, drift: f32) -> Backing {
        let mut b = Backing::new(RATE);
        b.set_track(Some(track(first + beats as f32 * (beat + drift * beats as f32) + 2.0, false)), &mut EventBuffer::default());
        b.clear_beats(beats + 1);
        let mut t = first;
        for i in 0..=beats {
            b.set_beat(i, t);
            t += beat + drift * i as f32;
        }
        b.set_active(true, &mut EventBuffer::default());
        b
    }

    fn chart(bars: u32) -> SongPlayer {
        let mut s = SongPlayer::default();
        s.clear(bars);
        s.set_active(true);
        s.set_slot(0, Slot::Chord(Chord::new(0, Quality::Major)));
        s.set_slot(4, Slot::Chord(Chord::new(7, Quality::Major)));
        s
    }

    /// Runs `frames` in blocks of 128, mixing each; returns events with absolute frames.
    fn run(b: &mut Backing, chart: &SongPlayer, frames: u64) -> Vec<(u64, EngineEvent)> {
        let mut out = Vec::new();
        let mut at = 0;
        let (mut l, mut r) = ([0.0f32; 128], [0.0f32; 128]);
        while at < frames {
            let mut events = EventBuffer::default();
            b.process(128, chart, &mut events);
            b.mix(&mut l, &mut r);
            events.sort();
            out.extend(events.as_slice().iter().map(|t| (at + t.frame as u64, t.event)));
            at += 128;
        }
        out
    }

    fn song_beats(events: &[(u64, EngineEvent)]) -> Vec<(u64, u32)> {
        events.iter().filter_map(|(f, e)| if let EngineEvent::SongPosition { beat } = e { Some((*f, *beat)) } else { None }).collect()
    }

    #[test]
    fn beats_follow_the_tempo_map_where_it_drifts() {
        // From 100 BPM getting slower by 2 ms a beat.
        let mut b = backing(16, 0.3, 0.6, 0.002);
        let c = chart(4);
        let mut out = EventBuffer::default();
        b.play(120.0, &c, &mut out);
        let events = run(&mut b, &c, (RATE * 13.0) as u64);
        let beats = song_beats(&events);
        assert_eq!(beats.len(), 16, "one per chart beat");
        for (frame, beat) in beats {
            let expected = (b.beat_time(beat as i64).unwrap() * RATE as f64).round() as u64;
            assert_eq!(frame, expected, "beat {beat}");
        }
        let tempos: Vec<f32> = events.iter().filter_map(|(_, e)| if let EngineEvent::Tempo { bpm } = e { Some(*bpm) } else { None }).collect();
        assert!(tempos.len() >= 2 && tempos[0] < 100.0 && tempos.windows(2).all(|w| w[1] < w[0]), "{tempos:?}");
        let chords: Vec<_> = events.iter().filter_map(|(f, e)| if let EngineEvent::ChordChanged(c) = e { Some((*f, c.unwrap().root)) } else { None }).collect();
        assert_eq!(chords.iter().map(|c| c.1).collect::<Vec<_>>(), [0, 7]);
        assert!(events.iter().any(|(_, e)| *e == EngineEvent::Transport { playing: false }), "stops at the end of the recording");
    }

    #[test]
    fn starts_a_bar_early_and_loops_on_the_bar_line() {
        let mut b = backing(32, 0.5, 0.5, 0.0);
        let c = chart(8);
        b.seek(2);
        b.set_loop(2, 4);
        b.play(120.0, &c, &mut EventBuffer::default());
        let events = run(&mut b, &c, (RATE * 7.0) as u64);
        let beats: Vec<u32> = song_beats(&events).iter().map(|b| b.1).collect();
        // Bar 1 as the lead-in, then bars 2–3 twice and more.
        assert_eq!(&beats[..4], [4, 5, 6, 7]);
        assert_eq!(&beats[4..12], [8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(&beats[12..14], [8, 9]);
        // The jump is heard on the bar line: beat 16 of the recording, counted from the lead-in at beat 4.
        let jump_at = song_beats(&events)[12].0;
        assert_eq!(jump_at, ((b.beat_time(16).unwrap() - b.beat_time(4).unwrap()) * RATE as f64).round() as u64);
    }

    #[test]
    fn a_seek_while_playing_jumps_at_the_next_bar() {
        let mut b = backing(32, 0.0, 0.5, 0.0);
        let c = chart(8);
        b.play(120.0, &c, &mut EventBuffer::default());
        let first = run(&mut b, &c, (RATE * 0.7) as u64);
        assert_eq!(song_beats(&first).last().unwrap().1, 1);
        b.seek(6);
        let beats: Vec<u32> = song_beats(&run(&mut b, &c, (RATE * 2.0) as u64)).iter().map(|b| b.1).collect();
        assert_eq!(&beats[..4], [2, 3, 24, 25]);
    }

    #[test]
    fn mixes_the_recording_with_fades_and_the_drum_stem() {
        let mut b = Backing::new(RATE);
        b.set_track(Some(track(1.0, true)), &mut EventBuffer::default());
        b.clear_beats(3);
        for (i, t) in [0.0, 0.5, 1.0].into_iter().enumerate() {
            b.set_beat(i as u32, t);
        }
        b.set_gain(0.6);
        let c = chart(1);
        b.play(120.0, &c, &mut EventBuffer::default());
        let (mut l, mut r) = ([0.0f32; 128], [0.0f32; 128]);
        // Two blocks to get past the fade-in.
        for _ in 0..3 {
            l.fill(0.0);
            r.fill(0.0);
            b.process(128, &c, &mut EventBuffer::default());
            b.mix(&mut l, &mut r);
        }
        assert!((l[0] * 32_768.0 - 256.0).abs() < 0.01, "reads frame 256: {}", l[0] * 32_768.0);
        assert!((r[0] * 32_768.0 - 1000.0).abs() < 0.01, "the drum stem is mixed in");
        b.set_drums(false);
        l.fill(0.0);
        r.fill(0.0);
        b.process(128, &c, &mut EventBuffer::default());
        b.mix(&mut l, &mut r);
        assert_eq!(r[0], 0.0, "drums muted");
        // Stopping fades out over FADE frames and then is silent.
        let mut out = EventBuffer::default();
        b.stop(0, &mut out);
        let mut last = f32::MAX;
        for block in 0..4 {
            l.fill(0.0);
            b.process(128, &c, &mut EventBuffer::default());
            b.mix(&mut l, &mut r);
            if block < 2 {
                assert!(l[0] < last && l[0] > l[127] && l[127] > 0.0, "block {block}");
                last = l[127];
            } else {
                assert!(l.iter().all(|x| *x == 0.0), "block {block}");
            }
        }
    }

    #[test]
    fn judges_hits_against_the_recording_s_sixteenths() {
        let mut b = backing(16, 0.25, 0.5, 0.0);
        let c = chart(4);
        b.play(120.0, &c, &mut EventBuffer::default());
        // Bar 1, beat 2 (sixteenth 4 of bar 1) is 0.25 + 6 × 0.5 s in.
        let at = |seconds: f64, b: &mut Backing| b.pos = (seconds * RATE as f64) as u64;
        at(3.25 + 0.012, &mut b);
        let j = b.judge(0.0, &c).unwrap();
        assert_eq!((j.bar, j.sixteenth), (1, 8));
        assert!((j.ms - 12.0).abs() < 0.1, "{j:?}");
        // The player's latency moves the hit earlier: 20 ms of it makes this 8 ms early.
        assert!((b.judge(20.0, &c).unwrap().ms + 8.0).abs() < 0.1);
        // Just before the second sixteenth of beat 0, and in the pickup.
        at(0.25 + 0.125 - 0.005, &mut b);
        let j = b.judge(0.0, &c).unwrap();
        assert_eq!((j.bar, j.sixteenth), (0, 1));
        assert!((j.ms + 5.0).abs() < 0.1);
        at(0.25 - 0.03, &mut b);
        let j = b.judge(0.0, &c).unwrap();
        assert_eq!((j.bar, j.sixteenth), (0, 0));
        assert!((j.ms + 30.0).abs() < 0.1);
        at(0.0, &mut b);
        assert_eq!(b.judge(0.0, &c), None, "too long before the first beat");
        // Past the map the grid carries on at the last tempo.
        at(0.25 + 20.0 * 0.5 + 0.003, &mut b);
        let j = b.judge(0.0, &c).unwrap();
        assert_eq!((j.bar, j.sixteenth), (5, 0));
        assert!((j.ms - 3.0).abs() < 0.1);
    }

    #[test]
    fn bars_of_other_lengths_set_the_bar_lines_jumps_and_judging() {
        // Bars of 4, 2, 4 and 3 beats, half a second each from 0.25 s.
        let mut b = backing(13, 0.25, 0.5, 0.0);
        let mut c = chart(4);
        c.set_bar_beats(1, 2);
        c.set_bar_beats(3, 3);
        b.seek(2);
        b.set_loop(2, 4);
        b.play(120.0, &c, &mut EventBuffer::default());
        let events = run(&mut b, &c, (RATE * 6.0) as u64);
        let beats: Vec<u32> = song_beats(&events).iter().map(|b| b.1).collect();
        // Bar 1 (2 beats) leads in, then bars 2–3 (7 beats) loop.
        assert_eq!(&beats[..11], [4, 5, 6, 7, 8, 9, 10, 11, 12, 6, 7]);
        let bars: Vec<(u32, u8)> = events.iter().filter_map(|(_, e)| if let EngineEvent::Beat { bar, beat } = e { Some((*bar, *beat)) } else { None }).collect();
        assert_eq!(&bars[..7], [(1, 0), (1, 1), (2, 0), (2, 1), (2, 2), (2, 3), (3, 0)]);

        // Beat 11 is the second of bar 3: its first sixteenth is that bar's fifth.
        b.pos = ((0.25 + 11.0 * 0.5) * RATE as f64) as u64;
        let j = b.judge(0.0, &c).unwrap();
        assert_eq!((j.bar, j.sixteenth, j.beat), (3, 4, 11));
    }
}
