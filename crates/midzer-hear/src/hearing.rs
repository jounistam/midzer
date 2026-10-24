//! Listening to a song as it plays. The input (tagged with the engine frame
//! of its first sample) is resampled to 22.05 kHz and turned into log-mel and
//! constant-Q frames as it comes. Every second or so one of the networks runs
//! over the last stretch:
//!
//! - Beat This! over the last few seconds (8 by default) gives beats and
//!   downbeats. The beats before the window's last ~0.1 s (where the network
//!   can't see what follows) are fitted with a straight grid, which says where
//!   the next beats fall; the downbeats vote for which beat is the 1, and the
//!   votes carry over from run to run. A new tempo far from the last must be
//!   heard twice in a row. Chords tend to change on the 1, so where the
//!   chords heard change in the bar votes for the 1 too.
//! - BTC over the last 10 s names a chord per frame (about 11 a second);
//!   flickers shorter than two frames are smoothed away. What it hears is
//!   about a second old by the time it is shown, so once the grid is sure,
//!   the chords are read beat by beat (each beat takes the chord heard
//!   longest in it, so changes fall on the beats), and if the last bars
//!   repeat (a loop of 1 to 8 bars), the coming beats are predicted from the
//!   loop, beat by beat.
//!
//! The results are in the engine's frames, so the engine can line them up
//! with what it hears however late they arrive. This runs off the audio
//! thread (a worker in the browser, a thread on the desktop) and allocates.

use std::collections::HashMap;

use midzer_core::Chord;

use crate::beat_this::{BeatThis, FPS, MELS};
use crate::btc::{BINS, Btc, CLASSES, FRAMES as CHORD_FRAMES, chord_of};
use crate::cqt::{Cqt, HOP as CQT_HOP};
use crate::mel::{HOP as MEL_HOP, Mel, SIZE as MEL_SIZE};
use crate::resample::Resampler;

pub const RATE: f64 = 22_050.0;
/// Audio kept, in seconds: BTC's 10 s, the CQT's reach and some slack.
const KEEP_S: f64 = 16.0;
/// A chord window: BTC's 108 frames of 2048 samples.
const CHORD_SPAN: u64 = 220_500;
/// Downbeat votes fade by this much a run.
const VOTE_DECAY: f32 = 0.6;
/// A tempo this far (relative) from the last is a new one.
const TEMPO_JUMP: f64 = 0.04;
/// Chords heard are kept this long (seconds), for finding the song's loop.
const CHORD_KEEP_S: f64 = 40.0;
/// Loops tried, in beats (short ones first); the most and least seconds
/// compared, and the share that must agree; beats predicted. The chords heard in the last `CHORD_EDGE_S` are the
/// least sure, so the loop doesn't lean on them.
const LOOPS: [i64; 8] = [3, 4, 6, 8, 12, 16, 24, 32];
const LOOP_SPAN_S: f64 = 16.0;
const LOOP_MIN_S: f64 = 4.0;
const LOOP_AGREE: f32 = 0.75;
const PREDICT_BEATS: i64 = 8;
const CHORD_EDGE_S: f64 = 0.3;
/// A grid must be this sure before chords are read beat by beat.
const BEAT_CHORDS_SURE: f32 = 0.5;
/// Gaps in the input up to this long (seconds) are filled with silence.
const MAX_GAP_S: f64 = 0.25;
/// Below this level (dBFS) the input counts as silent; after this long the
/// song is forgotten, as the engine's beat tracker does.
const SILENT_DB: f32 = -60.0;
const SILENT_S: f64 = 2.5;
/// Runs in a row that agree before the grid counts as steady.
const STEADY_RUNS: u32 = 3;
/// Beats within this many seconds of the fitted grid count as on it.
const ON_GRID_S: f64 = 0.035;
/// A capture starts this long before the sound, ends this long after it,
/// and is kept only if at least `CAPTURE_MIN_S` long; it holds at most
/// `CAPTURE_MAX_S`. Quiet for `CLEAN_START_S` before the sound means the
/// song was heard from its start.
const PRE_ROLL_S: f64 = 0.5;
const CAPTURE_MIN_S: f64 = 30.0;
const CAPTURE_MAX_S: f64 = 15.0 * 60.0;
const CLEAN_START_S: f64 = 0.25;

/// How the listening runs; the defaults are what `examples/hear_eval.rs` found best.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Seconds of audio Beat This! sees, at most and at least.
    pub beat_window_s: f64,
    pub beat_min_s: f64,
    /// Seconds of new audio between runs of each network; Beat This! runs
    /// less often once it keeps hearing the same grid.
    pub beat_every_s: f64,
    pub beat_steady_every_s: f64,
    pub chord_every_s: f64,
    /// Beats this close to the window's end are left out of the grid.
    pub edge_s: f64,
    /// Seconds of beats the grid is fitted to (the newest).
    pub fit_s: f64,
    /// How much where the chords change counts for the 1, against the
    /// network's downbeats.
    pub change_weight: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { beat_window_s: 8.0, beat_min_s: 3.0, beat_every_s: 1.0, beat_steady_every_s: 2.5, chord_every_s: 0.5, edge_s: 0.12, fit_s: 6.0, change_weight: 2.0 }
    }
}

/// Where the beats fall, as a grid in the engine's frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// A beat (the newest the network heard), and the beat's length.
    pub beat: f64,
    pub period: f64,
    /// Which beat of the bar that beat is (0 is the 1), once the downbeats are clear.
    pub one: Option<u8>,
    /// How well the beats fit the grid, 0..=1.
    pub confidence: f32,
}

impl Grid {
    pub fn bpm(&self, sample_rate: f64) -> f64 {
        60.0 * sample_rate / self.period
    }
}

/// A chord from one engine frame to another; `None` is no chord.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChordSpan {
    pub from: f64,
    pub to: f64,
    pub chord: Option<Chord>,
    /// Not heard yet: what the song's loop says comes next.
    pub predicted: bool,
}

/// A song heard whole or in part, to analyse and keep (see [`Hearing::set_capture`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    /// The song at 22.05 kHz.
    pub samples: Vec<i16>,
    /// The engine frame of its first sample.
    pub from: f64,
    /// Heard from its start (quiet came before it), and to its end (quiet came after it).
    pub clean_start: bool,
    pub clean_end: bool,
}

impl Capture {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / RATE
    }

    pub fn complete(&self) -> bool {
        self.clean_start && self.clean_end
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Grid(Grid),
    /// The chords of the last 10 s, oldest first: they replace what was said before about that time.
    Chords(Vec<ChordSpan>),
}

/// The grid in 22.05 kHz samples, as runs leave it.
#[derive(Clone, Copy)]
struct State {
    anchor: f64,
    period: f64,
    confidence: f32,
    /// Votes for which beat of the bar the anchor is.
    votes: [f32; 4],
    one: Option<u8>,
    /// Runs in a row whose grid the last one's predicted.
    steady: u32,
}

pub struct Hearing {
    pub settings: Settings,
    input_rate: f64,
    resampler: Resampler,
    /// The 22.05 kHz signal; `audio[0]` is sample `audio_base` of the stream.
    audio: Vec<f32>,
    audio_base: u64,
    /// The engine frame of the stream's sample 0, and of the next input sample.
    origin: f64,
    expect: Option<u64>,
    mel: Mel,
    /// Log-mel frames; `mels[0]` is frame `mel_base` (centred on sample `mel_base · 441`).
    mels: Vec<f32>,
    mel_base: u64,
    beat_this: BeatThis,
    btc: Btc,
    cqt: Cqt,
    /// CQT frames that saw no edge, by their centre sample.
    cqt_cache: HashMap<u64, Vec<f32>>,
    next_beat: u64,
    next_chords: u64,
    state: Option<State>,
    /// A far tempo heard once, waiting to be heard again.
    candidate: Option<f64>,
    /// The chords heard, oldest first: from, to (stream samples) and chord.
    chords: Vec<(f64, f64, Option<Chord>)>,
    /// The input's power, smoothed; samples in a row below `SILENT_DB`; and
    /// whether there was sound since the last reset.
    power: f32,
    quiet: u64,
    heard: bool,
    /// Whether songs are captured, the one being captured, and the last one ended.
    capturing: bool,
    capture: Option<Capture>,
    captured: Option<Capture>,
}

impl Hearing {
    pub fn new(input_rate: f32, beat_this: BeatThis, btc: Btc) -> Self {
        let mel = Mel::new(beat_this.mel_filters.clone());
        Self {
            settings: Settings::default(),
            input_rate: input_rate as f64,
            resampler: Resampler::new(input_rate, RATE as f32),
            audio: Vec::new(),
            audio_base: 0,
            origin: 0.0,
            expect: None,
            mel,
            mels: Vec::new(),
            mel_base: 0,
            beat_this,
            btc,
            cqt: Cqt::new(),
            cqt_cache: HashMap::new(),
            next_beat: 0,
            next_chords: 0,
            state: None,
            candidate: None,
            chords: Vec::new(),
            power: 0.0,
            quiet: 0,
            heard: false,
            capturing: false,
            capture: None,
            captured: None,
        }
    }

    /// With the bundled networks.
    pub fn with_models(input_rate: f32) -> Result<Self, String> {
        Ok(Self::new(input_rate, BeatThis::load(crate::BEAT_THIS)?, Btc::load(crate::BTC)?))
    }

    /// Forgets the song: the next input starts afresh.
    pub fn reset(&mut self) {
        self.resampler = Resampler::new(self.input_rate as f32, RATE as f32);
        self.audio.clear();
        self.audio_base = 0;
        self.expect = None;
        self.mels.clear();
        self.mel_base = 0;
        self.cqt_cache.clear();
        self.next_beat = 0;
        self.next_chords = 0;
        self.state = None;
        self.candidate = None;
        self.chords.clear();
        self.power = 0.0;
        self.quiet = 0;
        self.heard = false;
        self.capture = None;
    }

    /// Captures the songs heard from now on (or stops, dropping the one
    /// under way). Each ends at a silence or [`Hearing::finish`], and
    /// [`Hearing::take_song`] hands it out.
    pub fn set_capture(&mut self, on: bool) {
        self.capturing = on;
        if !on {
            self.capture = None;
        }
    }

    /// Ends the song being captured here; if the music goes on, a new capture starts at once.
    pub fn finish(&mut self) {
        self.end_capture(None);
        if self.capturing && self.heard {
            self.capture = Some(Capture { from: self.frame_of(self.total() as f64), ..Capture::default() });
        }
    }

    /// The last song captured, if long enough to keep.
    pub fn take_song(&mut self) -> Option<Capture> {
        self.captured.take()
    }

    /// Ends the capture; `quiet` is how many of its last samples were silence (it ended cleanly).
    fn end_capture(&mut self, quiet: Option<u64>) {
        let Some(mut c) = self.capture.take() else { return };
        // A capture that filled up missed the end.
        if let Some(quiet) = quiet.filter(|_| c.samples.len() < (CAPTURE_MAX_S * RATE) as usize) {
            let keep = (c.samples.len() as u64).saturating_sub(quiet) + (PRE_ROLL_S * RATE) as u64;
            c.samples.truncate(keep as usize);
            c.clean_end = true;
        }
        if c.seconds() >= CAPTURE_MIN_S {
            self.captured = Some(c);
        }
    }

    /// Adds the stream from sample `from` on to the capture.
    fn add_to_capture(&mut self, from: u64) {
        let Some(c) = self.capture.as_mut() else { return };
        let room = ((CAPTURE_MAX_S * RATE) as usize).saturating_sub(c.samples.len());
        let at = from.saturating_sub(self.audio_base) as usize;
        let new = &self.audio[at.min(self.audio.len())..];
        c.samples.extend(new.iter().take(room).map(|&x| (x.clamp(-1.0, 1.0) * 32767.0) as i16));
    }

    /// Forgets the song but not the stream: the audio so far, the grid, the
    /// votes and the chords go, and the next input carries on from here.
    fn forget(&mut self) {
        let total = self.total();
        self.audio.clear();
        self.audio_base = total;
        self.mels.clear();
        self.cqt_cache.clear();
        self.next_beat = total + (self.settings.beat_min_s * RATE) as u64;
        self.next_chords = self.next_beat;
        self.state = None;
        self.candidate = None;
        self.chords.clear();
        self.heard = false;
    }

    /// Whether the input has been silent long enough to forget the song.
    fn silent(&self) -> bool {
        self.quiet as f64 >= SILENT_S * RATE
    }

    /// Samples received at 22.05 kHz.
    fn total(&self) -> u64 {
        self.audio_base + self.audio.len() as u64
    }

    /// The engine frame of stream sample `i` (22.05 kHz, fractional).
    fn frame_of(&self, i: f64) -> f64 {
        self.origin + i * self.input_rate / RATE
    }

    /// Takes input whose first sample is engine frame `frame`. A short gap
    /// (a few dropped samples) is filled with silence; a long one, or input
    /// going back, starts afresh.
    pub fn push(&mut self, input: &[f32], frame: u64) {
        let mut out = Vec::with_capacity(input.len());
        match self.expect {
            Some(e) if frame == e => {}
            Some(e) if frame > e && ((frame - e) as f64) < MAX_GAP_S * self.input_rate => {
                self.resampler.push(&vec![0.0; (frame - e) as usize], &mut out);
            }
            _ => {
                self.reset();
                self.origin = frame as f64;
            }
        }
        self.expect = Some(frame + input.len() as u64);
        self.resampler.push(input, &mut out);
        let floor = 10f32.powf(SILENT_DB / 10.0);
        // Where the sound began in this input, and the quiet before it.
        let mut sound = None;
        for (i, &x) in out.iter().enumerate() {
            self.power += (x * x - self.power) * (1.0 / (0.05 * RATE as f32));
            if self.power < floor {
                self.quiet += 1;
            } else {
                if !self.heard {
                    sound = Some((i, self.quiet));
                }
                self.quiet = 0;
                self.heard = true;
            }
        }
        let start = self.total();
        self.audio.extend_from_slice(&out);
        match sound {
            Some((i, quiet)) if self.capturing && self.capture.is_none() => {
                let at = (start + i as u64).saturating_sub((PRE_ROLL_S * RATE) as u64).max(self.audio_base);
                self.capture = Some(Capture { from: self.frame_of(at as f64), clean_start: quiet as f64 >= CLEAN_START_S * RATE, ..Capture::default() });
                self.add_to_capture(at);
            }
            _ => self.add_to_capture(start),
        }
        if self.silent() && self.heard {
            // The song stopped; what comes next may be another.
            self.end_capture(Some(self.quiet));
            self.forget();
        }
        if self.next_beat == 0 {
            self.next_beat = (self.settings.beat_min_s * RATE) as u64;
            self.next_chords = (self.settings.beat_min_s * RATE) as u64;
        }
        self.make_mels();
        let keep = (KEEP_S * RATE) as usize;
        if self.audio.len() > keep + RATE as usize * 4 {
            let drop = self.audio.len() - keep;
            self.audio.drain(..drop);
            self.audio_base += drop as u64;
            let base = self.audio_base;
            self.cqt_cache.retain(|&c, _| c >= base);
        }
        let keep_mels = (self.settings.beat_window_s * FPS as f64) as usize + 50;
        if self.mels.len() / MELS > keep_mels + 200 {
            let drop = self.mels.len() / MELS - keep_mels;
            self.mels.drain(..drop * MELS);
            self.mel_base += drop as u64;
        }
    }

    fn make_mels(&mut self) {
        let half = (MEL_SIZE / 2) as u64;
        let mut frame = [0.0f32; MELS];
        loop {
            let k = self.mel_base + (self.mels.len() / MELS) as u64;
            let centre = k * MEL_HOP as u64;
            if centre < half + self.audio_base {
                // Before the audio kept (or the stream's start): start after it.
                self.mel_base = (self.audio_base + half).div_ceil(MEL_HOP as u64);
                self.mels.clear();
                continue;
            }
            if centre + half > self.total() {
                break;
            }
            let from = (centre - half - self.audio_base) as usize;
            self.mel.frame(&self.audio[from..from + MEL_SIZE], &mut frame);
            self.mels.extend_from_slice(&frame);
        }
    }

    /// Runs a network if one is due, and says what it heard. Call it after
    /// each [`Hearing::push`]; a run takes a while (up to about half a second).
    pub fn step(&mut self) -> Option<Heard> {
        if self.silent() {
            return None;
        }
        let total = self.total();
        let beat_due = total >= self.next_beat && self.mels.len() / MELS >= (self.settings.beat_min_s * FPS as f64) as usize;
        let chords_due = total >= self.next_chords && total >= (self.settings.beat_min_s * RATE) as u64;
        // Chords first: their run is short, and the beat grid carries on by itself meanwhile.
        let beat = match (beat_due, chords_due) {
            (_, true) => false,
            (true, false) => true,
            (false, false) => return None,
        };
        if beat {
            let steady = self.state.is_some_and(|s| s.steady >= STEADY_RUNS && s.one.is_some());
            let every = if steady { self.settings.beat_steady_every_s } else { self.settings.beat_every_s };
            self.next_beat = total + (every * RATE) as u64;
            self.hear_beats().map(Heard::Grid)
        } else {
            self.next_chords = total + (self.settings.chord_every_s * RATE) as u64;
            Some(Heard::Chords(self.hear_chords()))
        }
    }

    fn hear_beats(&mut self) -> Option<Grid> {
        let available = self.mels.len() / MELS;
        let n = available.min((self.settings.beat_window_s * FPS as f64) as usize);
        let first = available - n;
        let logits = self.beat_this.run(&self.mels[first * MELS..], n);
        // Frame j of the window is centred on stream sample (mel_base + first + j) · 441.
        let at = |j: f64| ((self.mel_base + first as u64) as f64 + j) * MEL_HOP as f64;
        let edge = (self.settings.edge_s * FPS as f64).round() as usize;
        let beats: Vec<(f64, f32)> = peaks(&logits.beat).into_iter().filter(|p| p.0 >= 6.0 && p.0 < (n - edge) as f64).map(|(j, v)| (at(j), v)).collect();
        let downbeats: Vec<(f64, f32)> = peaks(&logits.downbeat).into_iter().filter(|p| p.0 >= 6.0 && p.0 < (n - edge) as f64).map(|(j, v)| (at(j), v)).collect();
        let newest = beats.last()?.0;
        let fit_from = newest - self.settings.fit_s * RATE;
        let fitted: Vec<f64> = beats.iter().map(|b| b.0).filter(|&t| t >= fit_from).collect();
        let (anchor, period, confidence) = fit(&fitted)?;
        let state = self.merge(anchor, period, confidence, &downbeats)?;
        Some(Grid { beat: self.frame_of(state.anchor), period: state.period * self.input_rate / RATE, one: state.one, confidence: state.confidence })
    }

    /// Brings a run's grid together with what earlier runs heard.
    fn merge(&mut self, anchor: f64, period: f64, confidence: f32, downbeats: &[(f64, f32)]) -> Option<State> {
        let mut state = match self.state {
            Some(prev) if (period / prev.period - 1.0).abs() > TEMPO_JUMP => {
                // A new tempo: only when heard twice, or when the old grid was weak.
                let again = self.candidate.is_some_and(|c| (period / c - 1.0).abs() <= TEMPO_JUMP);
                if !again && prev.confidence >= 0.5 * confidence {
                    self.candidate = Some(period);
                    return None;
                }
                self.candidate = None;
                State { anchor, period, confidence, votes: [0.0; 4], one: None, steady: 0 }
            }
            Some(prev) => {
                self.candidate = None;
                // The votes move with the anchor, a whole number of beats on.
                let k = ((anchor - prev.anchor) / period).round() as i64;
                let mut votes = [0.0; 4];
                for s in 0..4 {
                    votes[(s as i64 + k).rem_euclid(4) as usize] = prev.votes[s] * VOTE_DECAY;
                }
                let one = prev.one.map(|o| (o as i64 + k).rem_euclid(4) as u8);
                // Did the last grid see this beat coming?
                let predicted = prev.anchor + k as f64 * prev.period;
                let steady = if (anchor - predicted).abs() < ON_GRID_S * RATE { prev.steady + 1 } else { 0 };
                let w = 0.5 * prev.confidence;
                let period = (period * confidence as f64 + prev.period * w as f64) / (confidence + w).max(1e-6) as f64;
                State { anchor, period, confidence, votes, one, steady }
            }
            None => State { anchor, period, confidence, votes: [0.0; 4], one: None, steady: 0 },
        };
        for &(d, logit) in downbeats {
            let n = ((d - state.anchor) / state.period).round();
            if ((d - state.anchor) - n * state.period).abs() < ON_GRID_S * RATE {
                // The anchor is beat (−n) of that bar.
                state.votes[(-(n as i64)).rem_euclid(4) as usize] += crate::nn::sigmoid(logit);
            }
        }
        // Where the chords change: a share of the changes on each beat of the bar.
        let changes = self.changes(state.anchor, state.period);
        let total: f32 = changes.iter().sum();
        let votes: [f32; 4] = std::array::from_fn(|v| state.votes[v] + if total > 0.0 { self.settings.change_weight * changes[v] / total } else { 0.0 });
        let mut order = [0usize, 1, 2, 3];
        order.sort_by(|&a, &b| votes[b].total_cmp(&votes[a]));
        let (best, second) = (votes[order[0]], votes[order[1]]);
        if best >= 1.0 && best > 1.5 * second {
            state.one = Some(order[0] as u8);
        }
        self.state = Some(state);
        Some(state)
    }

    fn hear_chords(&mut self) -> Vec<ChordSpan> {
        let total = self.total();
        let hop = CQT_HOP as u64;
        // A window of BTC's 108 frames on the stream's grid of 2048 samples,
        // ending at the newest sample, or shorter at the start.
        let start = (total.saturating_sub(CHORD_SPAN) / hop * hop).max(self.audio_base.div_ceil(hop) * hop);
        let end = (start + CHORD_SPAN).min(total);
        let n = (((end - start) / hop) as usize + 1).min(CHORD_FRAMES);
        let reach = self.cqt.reach as u64;
        let mut features = vec![0.0f32; n * BINS];
        for t in 0..n {
            let centre = start + t as u64 * hop;
            let inside = centre >= start + reach && centre + reach <= end;
            let row = &mut features[t * BINS..(t + 1) * BINS];
            if inside && let Some(cached) = self.cqt_cache.get(&centre) {
                row.copy_from_slice(cached);
                continue;
            }
            let base = self.audio_base;
            self.cqt.frame(&self.audio, centre as isize - base as isize, (start - base) as usize, (end - base) as usize, row);
            if inside {
                self.cqt_cache.insert(centre, row.to_vec());
            }
        }
        let logits = self.btc.run(&features, n);
        let mut classes: Vec<usize> = logits.as_chunks::<CLASSES>().0.iter().map(|r| (0..CLASSES).max_by(|&a, &b| r[a].total_cmp(&r[b])).unwrap()).collect();
        smooth(&mut classes);
        // What this window heard replaces what earlier ones said about its time.
        let (from, to) = (start as f64, end as f64);
        self.chords.retain(|c| c.0 < from);
        if let Some(last) = self.chords.last_mut() {
            last.1 = last.1.min(from);
        }
        for (t, &c) in classes.iter().enumerate() {
            let centre = (start + t as u64 * hop) as f64;
            let (a, b) = ((centre - hop as f64 / 2.0).max(from), (centre + hop as f64 / 2.0).min(to));
            let chord = chord_of(c);
            match self.chords.last_mut() {
                Some(last) if last.2 == chord && (a - last.1).abs() < 1.0 => last.1 = b,
                _ => self.chords.push((a, b, chord)),
            }
        }
        let keep_from = to - CHORD_KEEP_S * RATE;
        self.chords.retain(|c| c.1 > keep_from);

        let span = |(a, b, chord): (f64, f64, Option<Chord>), predicted: bool| ChordSpan { from: self.frame_of(a), to: self.frame_of(b), chord, predicted };
        let Some(state) = self.state.filter(|s| s.confidence >= BEAT_CHORDS_SURE) else {
            return self.chords.iter().filter(|c| c.1 > from).map(|&(a, b, chord)| span((a.max(from), b, chord), false)).collect();
        };
        // Beat by beat: each takes the chord heard longest in it. The beat
        // at or after the least sure last moments is where the loop's chords
        // take over; the chord heard last carries on to it.
        let beat = |k: i64| state.anchor + k as f64 * state.period;
        let edge = to - CHORD_EDGE_S * RATE;
        let mut k = ((from - state.anchor) / state.period).ceil() as i64;
        let mut cuts = vec![from];
        while beat(k) < edge {
            if beat(k) > from {
                cuts.push(beat(k));
            }
            k += 1;
        }
        let held = beat(k).max(from);
        let mut heard: Vec<(f64, f64, Option<Chord>)> = Vec::new();
        for (i, &a) in cuts.iter().enumerate() {
            let b = cuts.get(i + 1).copied().unwrap_or(held);
            let Some(chord) = chord_over(&self.chords, a, b.min(to)) else { continue };
            match heard.last_mut() {
                Some(last) if last.2 == chord => last.1 = b,
                _ => heard.push((a, b, chord)),
            }
        }
        if let Some(last) = heard.last_mut() {
            last.1 = last.1.max(to);
        }
        let predicted = self.predict(state, k, to);
        if let (Some(last), Some(first)) = (heard.last_mut(), predicted.first()) {
            last.1 = first.0;
        }
        heard.into_iter().map(|c| span(c, false)).chain(predicted.into_iter().map(|c| span(c, true))).collect()
    }

    /// The chords of the coming beats (from beat `first` on `state`'s grid),
    /// from the song's loop, if the last seconds repeat clearly: from, to
    /// (stream samples) and chord, a beat at a time.
    fn predict(&self, state: State, first: i64, end: f64) -> Vec<(f64, f64, Option<Chord>)> {
        let Some(length) = find_loop(&self.chords, state.period, end) else { return Vec::new() };
        let heard = end - CHORD_EDGE_S * RATE;
        let beat = |k: i64| state.anchor + k as f64 * state.period;
        let mut out: Vec<(f64, f64, Option<Chord>)> = Vec::new();
        for k in first..first + PREDICT_BEATS {
            // The same beat a loop or more earlier, heard in full.
            let mut then = k - length;
            while beat(then + 1) > heard {
                then -= length;
            }
            let Some(chord) = chord_over(&self.chords, beat(then), beat(then + 1)) else { break };
            match out.last_mut() {
                Some(last) if last.2 == chord => last.1 = beat(k + 1),
                _ => out.push((beat(k), beat(k + 1), chord)),
            }
        }
        out
    }

    /// How often the chords heard in the last seconds changed on each beat
    /// of the bar, read beat by beat on the grid through `anchor`: index `v`
    /// counts the changes a bar starting `v` beats before the anchor puts on its 1.
    fn changes(&self, anchor: f64, period: f64) -> [f32; 4] {
        let mut out = [0.0; 4];
        let (Some(f), Some(l)) = (self.chords.first(), self.chords.last()) else { return out };
        let (first, last) = (f.0, l.1 - CHORD_EDGE_S * RATE);
        let beat = |k: i64| anchor + k as f64 * period;
        let mut k = ((first.max(last - LOOP_SPAN_S * RATE) - anchor) / period).ceil() as i64;
        let mut before = None;
        while beat(k + 1) <= last {
            let now = chord_over(&self.chords, beat(k), beat(k + 1)).flatten().map(triad);
            if let (Some(a), Some(b)) = (before, now)
                && a != b
            {
                out[(-k).rem_euclid(4) as usize] += 1.0;
            }
            before = now;
            k += 1;
        }
        out
    }
}

/// A chord's root and whether it is minor: what a loop must repeat (the
/// network wavers between a triad and its seventh).
fn triad(c: Chord) -> (u8, bool) {
    use midzer_core::Quality::*;
    (c.root, matches!(c.quality, Minor | Minor6 | Minor7 | HalfDiminished7 | Diminished | Diminished7))
}

/// The chord heard longest from `a` to `b` (in the chords' own time unit),
/// or `None` when less than half of that was heard.
pub fn chord_over(chords: &[(f64, f64, Option<Chord>)], a: f64, b: f64) -> Option<Option<Chord>> {
    let mut tally: Vec<(Option<Chord>, f64)> = Vec::new();
    let mut heard = 0.0;
    for c in &chords[chords.partition_point(|c| c.1 <= a)..] {
        if c.0 >= b {
            break;
        }
        let d = c.1.min(b) - c.0.max(a);
        if d <= 0.0 {
            continue;
        }
        heard += d;
        match tally.iter_mut().find(|t| t.0 == c.2) {
            Some(t) => t.1 += d,
            None => tally.push((c.2, d)),
        }
    }
    if heard < 0.5 * (b - a) {
        return None;
    }
    tally.into_iter().max_by(|x, y| x.1.total_cmp(&y.1)).map(|t| t.0)
}

/// The chord kept for stream sample `t`, or `None` when nothing was heard then.
fn chord_at(chords: &[(f64, f64, Option<Chord>)], t: f64) -> Option<Option<Chord>> {
    let i = chords.partition_point(|c| c.1 <= t);
    chords.get(i).filter(|c| c.0 <= t).map(|c| c.2)
}

/// The shortest loop (a whole number of beats) the
/// chords of the last seconds repeat with: sampled every tenth of a second,
/// most moments with a chord match the chord a loop earlier. Compared in
/// time rather than beat by beat, so a slightly wrong tempo blurs nothing.
/// Chords count as the same with the same root and third.
fn find_loop(chords: &[(f64, f64, Option<Chord>)], period: f64, end: f64) -> Option<i64> {
    let first = chords.first()?.0;
    let last = end - CHORD_EDGE_S * RATE;
    let step = 0.1 * RATE;
    LOOPS.into_iter().find(|&beats| {
        let length = beats as f64 * period;
        let from = (first + length).max(last - LOOP_SPAN_S * RATE);
        if last - from < tune("LOOP_MIN", LOOP_MIN_S as f32) as f64 * RATE {
            return false;
        }
        let (mut pairs, mut same) = (0usize, 0usize);
        let mut t = from;
        while t < last {
            if let (Some(a), Some(b)) = (chord_at(chords, t), chord_at(chords, t - length))
                && (a.is_some() || b.is_some())
            {
                pairs += 1;
                same += (a.map(triad) == b.map(triad)) as usize;
            }
            t += step;
        }
        pairs > 0 && same as f32 >= tune("LOOP_AGREE", LOOP_AGREE) * pairs as f32
    })
}

fn tune(k: &str, d: f32) -> f32 { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

/// Beat This!'s own peak picking: frames above zero that are the highest
/// within three frames each way, neighbours merged. (frame, logit).
pub(crate) fn peaks(logits: &[f32]) -> Vec<(f64, f32)> {
    let n = logits.len();
    let mut out: Vec<(f64, f32, usize)> = Vec::new();
    for j in 0..n {
        let v = logits[j];
        if v <= 0.0 {
            continue;
        }
        let (lo, hi) = (j.saturating_sub(3), (j + 3).min(n - 1));
        if logits[lo..=hi].iter().all(|&u| u <= v) {
            match out.last_mut() {
                Some(last) if j - (last.0.round() as usize) <= 1 && last.2 == j - 1 => {
                    last.0 = (last.0 + j as f64) / 2.0;
                    last.2 = j;
                }
                _ => out.push((j as f64, v, j)),
            }
        }
    }
    out.into_iter().map(|(t, v, _)| (t, v)).collect()
}

/// A straight grid through beat times: the newest beat on it, the period,
/// and the share of beats within [`ON_GRID_S`] of it (less with few beats).
fn fit(beats: &[f64]) -> Option<(f64, f64, f32)> {
    if beats.len() < 4 {
        return None;
    }
    let mut intervals: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_by(f64::total_cmp);
    let p0 = intervals[intervals.len() / 2];
    if p0 <= 0.0 {
        return None;
    }
    let last = beats[beats.len() - 1];
    // Least squares of time on beat number, twice: the second time without the beats far off the first grid.
    let mut keep: Vec<(f64, f64)> = beats.iter().map(|&b| (((b - last) / p0).round(), b)).collect();
    let mut grid = (last, p0);
    for _ in 0..2 {
        let n = keep.len() as f64;
        if n < 3.0 {
            break;
        }
        let (sx, sy) = keep.iter().fold((0.0, 0.0), |s, &(x, y)| (s.0 + x, s.1 + y));
        let (mx, my) = (sx / n, sy / n);
        let (sxy, sxx) = keep.iter().fold((0.0, 0.0), |s, &(x, y)| (s.0 + (x - mx) * (y - my), s.1 + (x - mx) * (x - mx)));
        if sxx <= 0.0 {
            break;
        }
        let p = sxy / sxx;
        let a = my - p * mx;
        grid = (a, p);
        keep.retain(|&(x, y)| (y - (a + p * x)).abs() < 2.0 * ON_GRID_S * RATE);
    }
    let (a, p) = grid;
    let on = beats.iter().filter(|&&b| {
        let x = ((b - a) / p).round();
        (b - (a + p * x)).abs() < ON_GRID_S * RATE
    });
    let share = on.count() as f32 / beats.len() as f32;
    let enough = (beats.len() as f32 / 8.0).min(1.0);
    Some((a, p, share * enough))
}

/// Chords that last a single frame take their neighbour's.
pub(crate) fn smooth(classes: &mut [usize]) {
    for t in 1..classes.len().saturating_sub(1) {
        if classes[t] != classes[t - 1] && classes[t] != classes[t + 1] {
            classes[t] = classes[t - 1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loop_of_two_bars_is_found_and_a_muddle_is_not() {
        use midzer_core::Quality;
        let c = |root: u8| Some(Chord { root, quality: Quality::Major, bass: root });
        let beat = 0.5 * RATE;
        // C for a bar, G for a bar, over and over for 16 s: a loop of 8 beats, not 4.
        let song: Vec<_> = (0..8).map(|i| (i as f64 * 4.0 * beat, (i + 1) as f64 * 4.0 * beat, if i % 2 == 0 { c(0) } else { c(7) })).collect();
        let end = 16.0 * RATE;
        assert_eq!(find_loop(&song, beat, end), Some(8));
        // Slightly off in tempo, it still holds.
        assert_eq!(find_loop(&song, beat * 1.005, end), Some(8));
        let mut seed = 7u32;
        let muddle: Vec<_> = (0..32)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (i as f64 * beat, (i + 1) as f64 * beat, c(((seed >> 16) % 12) as u8))
            })
            .collect();
        assert_eq!(find_loop(&muddle, beat, end), None);
        assert_eq!(chord_at(&song, 4.5 * beat), Some(c(7)));
        assert_eq!(chord_at(&song, 40.0 * beat), None);
    }

    /// 120 BPM clicks over a held C major chord, then `quiet` seconds of silence.
    fn song(seconds: f64, quiet: f64) -> Vec<f32> {
        let len = (seconds * RATE) as usize;
        let mut out: Vec<f32> = (0..len)
            .map(|i| {
                let t = i as f64 / RATE;
                let tones: f64 = [130.8, 164.8, 196.0].iter().map(|hz| 0.08 * (std::f64::consts::TAU * hz * t).sin()).sum();
                let click = (-(t % 0.5) / 0.01).exp() * (std::f64::consts::TAU * 1000.0 * t).sin() * 0.5;
                (tones + click) as f32
            })
            .collect();
        out.extend(std::iter::repeat_n(0.0, (quiet * RATE) as usize));
        out
    }

    fn listen(hearing: &mut Hearing, signal: &[f32], from: u64) -> usize {
        let mut runs = 0;
        for (i, block) in signal.chunks(2048).enumerate() {
            hearing.push(block, from + (i * 2048) as u64);
            while hearing.step().is_some() {
                runs += 1;
            }
        }
        runs
    }

    #[test]
    fn silence_forgets_the_song_and_a_short_dip_does_not() {
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        let loud = song(8.0, 0.0);
        listen(&mut hearing, &loud, 0);
        assert!(!hearing.chords.is_empty() && hearing.state.is_some());
        let dip = vec![0.0; (2.0 * RATE) as usize];
        listen(&mut hearing, &dip, loud.len() as u64);
        assert!(!hearing.chords.is_empty() && hearing.state.is_some(), "a 2 s dip keeps the song");

        // A second more makes 2.5 s of silence (the level takes ~0.5 s to fall); then the networks rest.
        let quiet = vec![0.0; (4.0 * RATE) as usize];
        let at = (loud.len() + dip.len()) as u64;
        let half = (1.0 * RATE) as usize;
        listen(&mut hearing, &quiet[..half], at);
        assert!(hearing.chords.is_empty() && hearing.state.is_none());
        let runs = listen(&mut hearing, &quiet[half..], at + half as u64);
        assert_eq!(runs, 0, "the networks rest in silence");
        // The stream carries on: the next song is heard in the engine's frames.
        let at = at + quiet.len() as u64;
        assert_eq!(hearing.expect, Some(at));
        listen(&mut hearing, &loud, at);
        let before = |c: &(f64, f64, Option<Chord>)| c.2.is_some() && hearing.frame_of(c.1) < at as f64;
        assert!(hearing.state.is_some() && !hearing.chords.is_empty() && !hearing.chords.iter().any(before));
    }

    /// Pushes without running the networks: capturing needs only the input.
    fn feed(hearing: &mut Hearing, signal: &[f32], from: u64) -> u64 {
        for (i, block) in signal.chunks(2048).enumerate() {
            hearing.push(block, from + (i * 2048) as u64);
        }
        from + signal.len() as u64
    }

    #[test]
    fn a_song_from_silence_to_silence_is_captured_whole() {
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        hearing.set_capture(true);
        let at = feed(&mut hearing, &vec![0.0; RATE as usize], 0);
        let at = feed(&mut hearing, &song(35.0, 3.0), at);
        let c = hearing.take_song().expect("a song");
        assert!(c.complete());
        // Half a second either side of the music, plus the level's fall after it.
        assert!((36.0..36.6).contains(&c.seconds()), "{} s", c.seconds());
        assert!((c.from - 0.5 * RATE).abs() < 2048.0, "from {}", c.from);
        assert!(hearing.take_song().is_none());

        // Opened mid-song: the start is missing.
        hearing.reset();
        feed(&mut hearing, &song(35.0, 3.0), at + 100_000);
        let c = hearing.take_song().expect("a song");
        assert!(!c.clean_start && c.clean_end);
    }

    #[test]
    fn finishing_ends_the_capture_and_the_music_goes_on_in_a_new_one() {
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        hearing.set_capture(true);
        let music = song(66.0, 3.0);
        let at = feed(&mut hearing, &vec![0.0; RATE as usize], 0);
        let cut = (35.0 * RATE) as usize;
        let at2 = feed(&mut hearing, &music[..cut], at);
        hearing.finish();
        let c = hearing.take_song().expect("the first part");
        assert!(c.clean_start && !c.clean_end);
        feed(&mut hearing, &music[cut..], at2);
        let c = hearing.take_song().expect("the rest");
        assert!(!c.clean_start && c.clean_end);
        assert!((31.5..32.1).contains(&c.seconds()), "{} s", c.seconds());
    }

    #[test]
    fn short_songs_a_reset_and_capture_off_keep_nothing() {
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        hearing.set_capture(true);
        let at = feed(&mut hearing, &song(10.0, 3.0), 0);
        assert!(hearing.take_song().is_none(), "too short");
        let music = song(40.0, 3.0);
        let at = feed(&mut hearing, &music[..(35.0 * RATE) as usize], at);
        hearing.reset();
        feed(&mut hearing, &vec![0.0; 3 * RATE as usize], at);
        assert!(hearing.take_song().is_none(), "reset drops it");
        hearing.set_capture(false);
        feed(&mut hearing, &music, at + 200_000);
        assert!(hearing.take_song().is_none() && hearing.capture.is_none());
    }

    #[test]
    fn a_beat_takes_the_chord_heard_longest_in_it() {
        use midzer_core::Quality;
        let c = |root: u8| Some(Chord { root, quality: Quality::Major, bass: root });
        let heard = [(0.0, 1.2, c(0)), (1.2, 1.4, c(2)), (1.4, 3.0, c(7))];
        assert_eq!(chord_over(&heard, 1.0, 2.0), Some(c(7)));
        assert_eq!(chord_over(&heard, 0.0, 1.0), Some(c(0)));
        // Less than half of it heard.
        assert_eq!(chord_over(&heard, 2.6, 4.0), None);
    }

    #[test]
    fn peaks_are_local_maxima_above_zero() {
        let l = [-1.0, 0.5, 2.0, 2.0, 0.1, -3.0, -1.0, 1.0, 0.2, -1.0, -1.0, -1.0, -0.5];
        assert_eq!(peaks(&l), vec![(2.5, 2.0), (7.0, 1.0)]);
    }

    #[test]
    fn a_grid_through_beats_with_one_missing_and_one_off() {
        let p = 0.5 * RATE;
        let mut beats: Vec<f64> = (0..10).filter(|&i| i != 4).map(|i| 1000.0 + i as f64 * p).collect();
        beats[6] += 0.2 * RATE;
        let (a, period, share) = fit(&beats).unwrap();
        assert!((period - p).abs() < 1.0, "period {period}");
        assert!((a - (1000.0 + 9.0 * p)).abs() < 2.0, "anchor {a}");
        assert!((share - 8.0 / 9.0).abs() < 1e-6);
    }
}
