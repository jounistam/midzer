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
//! With a library of stored songs, the last seconds are also looked up in
//! their fingerprints every couple of seconds. Once a song is recognised,
//! the listener follows it: the grids come from its stored beat map (its
//! beat, the tempo there and its 1) and the chords from its stored chart,
//! known ahead, while Beat This! only keeps the place in the song exact.
//! Losing the song, or hearing another, is a change of song: the capture
//! is cut there.
//!
//! The results are in the engine's frames, so the engine can line them up
//! with what it hears however late they arrive. This runs off the audio
//! thread (a worker in the browser, a thread on the desktop) and allocates.

use std::collections::HashMap;

use midzer_core::Chord;

use crate::beat_this::{BeatThis, FPS, MELS};
use crate::btc::{BINS, Btc, CLASSES, FRAMES as CHORD_FRAMES, chord_of};
use crate::cqt::{Cqt, HOP as CQT_HOP};
use crate::fingerprint::{FPS as MARK_FPS, fingerprint};
use crate::mel::{HOP as MEL_HOP, Mel, SIZE as MEL_SIZE};
use crate::offline::refine;
use crate::recognise::{Library, MATCH_VOTES, Match};
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
/// Seconds of audio looked up in the library, and how often.
const RECOGNISE_S: f64 = 8.0;
const RECOGNISE_EVERY_S: f64 = 2.0;
/// Looks-ups in a row that miss the song followed before it counts as lost;
/// a look-up counts only with this many landmarks (music, not a quiet bit).
const LOST_MISSES: u32 = 2;
const MISS_MIN_MARKS: usize = 120;
/// The landmark frames around the place expected that still count as it.
const NEAR_FRAMES: i32 = 2;
/// How often a song followed is told, in seconds; its chords are told from
/// this far back and this many beats ahead.
const FOLLOW_EVERY_S: f64 = 1.0;
const FOLLOW_BACK_S: f64 = 10.0;
const FOLLOW_AHEAD: usize = 8;
/// Network beats this close to the stored ones (s) move the place in the
/// song, when at least `REFINE_BEATS` are.
const REFINE_S: f64 = 0.07;
const REFINE_BEATS: usize = 4;
/// How much a stored 1 counts for where the 1 is while following, against
/// the network's downbeats (one bar phase is stored for a whole song, so
/// it is wrong after odd bars).
const STORED_ONE: f32 = 0.3;
/// A capture is of a stored song if it was followed for this long (s).
const FOLLOWED_MIN_S: f64 = 10.0;

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
    /// The stored song followed for the most of it (the library's id), if any was for long.
    pub followed: Option<u32>,
}

impl Capture {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / RATE
    }

    pub fn complete(&self) -> bool {
        self.clean_start && self.clean_end
    }
}

/// A stored song being followed: where it was at an engine frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Following {
    /// The library's id for it.
    pub song: u32,
    pub frame: f64,
    /// Seconds into the song at `frame`.
    pub seconds: f64,
    pub votes: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Grid(Grid),
    /// The chords of the last 10 s, oldest first: they replace what was said before about that time.
    Chords(Vec<ChordSpan>),
    /// A stored song is followed from here (on recognising it, or a jump in
    /// it), or none is any more.
    Follow(Option<Following>),
    /// The bar lines of the song followed, from the bar playing to a few bars
    /// ahead: the engine frame of the beat each starts on and its length in beats.
    Bars(Vec<(f64, u8)>),
}

/// The song followed.
#[derive(Clone, Copy)]
struct Follow {
    song: u32,
    /// Seconds into the song at stream sample `i`: `i / RATE + delta`.
    delta: f64,
    /// Look-ups in a row that missed it, and the end of the last that found it (a stream sample).
    misses: u32,
    found_until: u64,
    next_out: u64,
    /// The last grid told (anchor and period in stream samples), the votes
    /// for how many beats after the stored bar lines the 1s are heard, and
    /// the shift taken.
    grid: Option<(f64, f64)>,
    votes: [f32; 4],
    one: Option<u8>,
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
    /// The stream sample of the capture's first, and how long each song was followed in it.
    capture_at: u64,
    followed: Vec<(u32, u64)>,
    /// The songs kept, the one followed, and those not to recognise till the next silence.
    library: Library,
    follow: Option<Follow>,
    ignore: Vec<u32>,
    next_recognise: u64,
    /// What `step` has still to say.
    pending: Vec<Heard>,
    /// The network's downbeats from its last run while following, not yet counted (stream sample, sureness).
    follow_downbeats: Vec<(f64, f32)>,
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
            capture_at: 0,
            followed: Vec::new(),
            library: Library::default(),
            follow: None,
            ignore: Vec::new(),
            next_recognise: 0,
            pending: Vec::new(),
            follow_downbeats: Vec::new(),
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
        self.next_recognise = 0;
        self.pending.clear();
        self.unfollow();
        self.ignore.clear();
    }

    /// The stored songs to recognise; changing them takes effect at the next look-up.
    pub fn library(&mut self) -> &mut Library {
        &mut self.library
    }

    /// Stops following `song` and doesn't recognise it again until the next silence or reset.
    pub fn ignore(&mut self, song: u32) {
        if !self.ignore.contains(&song) {
            self.ignore.push(song);
        }
        if self.follow.is_some_and(|f| f.song == song) {
            self.end_follow();
        }
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
        self.finish_at(self.total());
    }

    /// Ends the capture at stream sample `cut`; what came after starts the next one.
    fn finish_at(&mut self, cut: u64) {
        let rest = match self.capture.as_mut() {
            Some(c) => {
                let keep = cut.saturating_sub(self.capture_at) as usize;
                if keep < c.samples.len() { c.samples.split_off(keep) } else { Vec::new() }
            }
            None => Vec::new(),
        };
        let at = cut.max(self.capture_at);
        self.end_capture(None);
        if self.capturing && self.heard {
            self.start_capture(at, false);
            if let Some(c) = self.capture.as_mut() {
                c.samples = rest;
            }
        }
    }

    fn start_capture(&mut self, at: u64, clean_start: bool) {
        self.capture = Some(Capture { from: self.frame_of(at as f64), clean_start, ..Capture::default() });
        self.capture_at = at;
        self.followed.clear();
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
        c.followed = self.followed.iter().filter(|f| f.1 as f64 >= FOLLOWED_MIN_S * RATE).max_by_key(|f| f.1).map(|f| f.0);
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
        self.next_recognise = 0;
        self.unfollow();
        self.ignore.clear();
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
                self.start_capture(at, quiet as f64 >= CLEAN_START_S * RATE);
                self.add_to_capture(at);
            }
            _ => self.add_to_capture(start),
        }
        if let (Some(f), Some(_)) = (self.follow, &self.capture) {
            match self.followed.iter_mut().find(|e| e.0 == f.song) {
                Some(e) => e.1 += out.len() as u64,
                None => self.followed.push((f.song, out.len() as u64)),
            }
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
        if !self.pending.is_empty() {
            return Some(self.pending.remove(0));
        }
        if self.silent() {
            return None;
        }
        let total = self.total();
        if !self.library.is_empty() && self.heard && total >= self.next_recognise && self.audio.len() as f64 >= RECOGNISE_S * RATE {
            self.next_recognise = total + (RECOGNISE_EVERY_S * RATE) as u64;
            self.recognise();
        }
        if let Some(f) = self.follow {
            if total >= f.next_out {
                self.tell_follow();
            } else if total >= self.next_beat && self.mels.len() / MELS >= (self.settings.beat_min_s * FPS as f64) as usize {
                // Beat This! still runs, only to keep the place in the song exact.
                self.next_beat = total + (self.settings.beat_steady_every_s * RATE) as u64;
                self.refine_follow();
            }
            return (!self.pending.is_empty()).then(|| self.pending.remove(0));
        }
        if !self.pending.is_empty() {
            return Some(self.pending.remove(0));
        }
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

    /// Looks the last seconds up in the library: finds a song, keeps
    /// following one, or notices it jumped, stopped or changed.
    fn recognise(&mut self) {
        let n = (RECOGNISE_S * RATE) as usize;
        let total = self.total();
        let window = total - n as u64;
        let marks = fingerprint(&self.audio[self.audio.len() - n..]);
        let votes = self.library.votes(&marks);
        // A landmark frame is this many stream samples.
        let frame = RATE / MARK_FPS;
        let delta = |offset: f64| (offset * frame - window as f64) / RATE;
        if let Some(f) = self.follow.as_mut() {
            let expected = (f.delta * RATE + window as f64) / frame;
            let (near, _) = self.library.near(&votes, f.song, expected, NEAR_FRAMES);
            if near >= MATCH_VOTES / 2 {
                f.misses = 0;
                // Where it was last heard: the window may already hold the next song.
                if let Some(t) = self.library.last_heard(&marks, f.song, expected, NEAR_FRAMES) {
                    f.found_until = window + ((t as f64 + 1.0) * frame) as u64;
                }
                return;
            }
        }
        match (self.library.recognise(&votes, &self.ignore), self.follow) {
            // The same song somewhere else: a jump in it.
            (Some(m), Some(f)) if m.song == f.song => self.start_follow(m, delta(m.offset)),
            (Some(m), Some(f)) => {
                self.change_song(f.found_until);
                self.start_follow(m, delta(m.offset));
            }
            (Some(m), None) => self.start_follow(m, delta(m.offset)),
            (None, Some(f)) if marks.len() >= MISS_MIN_MARKS => {
                let misses = f.misses + 1;
                if misses >= LOST_MISSES {
                    self.end_follow();
                    self.change_song(f.found_until);
                } else if let Some(f) = self.follow.as_mut() {
                    f.misses = misses;
                }
            }
            _ => {}
        }
    }

    fn start_follow(&mut self, m: Match, delta: f64) {
        let total = self.total();
        self.follow = Some(Follow { song: m.song, delta, misses: 0, found_until: total, next_out: 0, grid: None, votes: [0.0; 4], one: None });
        self.follow_downbeats.clear();
        self.state = None;
        self.candidate = None;
        self.pending.push(Heard::Follow(Some(Following { song: m.song, frame: self.frame_of(total as f64), seconds: total as f64 / RATE + delta, votes: m.votes })));
    }

    /// Stops following; the networks listen afresh.
    fn end_follow(&mut self) {
        self.unfollow();
        let total = self.total();
        self.state = None;
        self.candidate = None;
        self.chords.clear();
        self.next_beat = total;
        self.next_chords = total;
    }

    /// Forgets the song followed, saying so if there was one.
    fn unfollow(&mut self) {
        if self.follow.take().is_some() {
            self.pending.push(Heard::Follow(None));
        }
    }

    /// Another song began around stream sample `cut`: the capture ends there.
    fn change_song(&mut self, cut: u64) {
        self.finish_at(cut);
        self.state = None;
        self.candidate = None;
        self.chords.clear();
    }

    /// Says where the song followed is: a grid through its stored beats, its
    /// bar lines (moved if the network's downbeats outvote the stored 1s),
    /// and its chords.
    fn tell_follow(&mut self) {
        let total = self.total();
        let Some(mut f) = self.follow else { return };
        f.next_out = total + (FOLLOW_EVERY_S * RATE) as u64;
        let now = total as f64 / RATE + f.delta;
        let Some(song) = self.library.song(f.song) else {
            self.end_follow();
            return;
        };
        let beats = &song.beats;
        if beats.len() < 2 || now >= song.seconds.min(beats[beats.len() - 1]) {
            // Past what was stored of it (a song heard in part before): listen afresh.
            self.ignore(f.song);
            return;
        }
        let k = beats.partition_point(|&b| b <= now);
        if k == 0 {
            self.follow = Some(f);
            return;
        }
        let k = k - 1;
        let stream = |s: f64| (s - f.delta) * RATE;
        // A straight grid through the last stored beats: a stray beat in the map doesn't move it.
        let from = beats.partition_point(|&b| b < now - self.settings.fit_s);
        let near: Vec<f64> = beats[from..=k].iter().map(|&b| stream(b)).collect();
        // The stored beat nearest a stream time.
        let beat_at = |t: f64| {
            let s = t / RATE + f.delta;
            let i = beats.partition_point(|&b| b < s);
            [i.checked_sub(1), Some(i)].into_iter().flatten().filter(|&i| i < beats.len()).min_by(|&a, &b| (beats[a] - s).abs().total_cmp(&(beats[b] - s).abs())).unwrap_or(0)
        };
        if let Some((anchor, period, _)) = fit(&near).filter(|g| g.2 >= 0.5) {
            // The votes fade; the stored 1s (no shift) and the network's downbeats add to them.
            for v in &mut f.votes {
                *v *= VOTE_DECAY;
            }
            let on = |t: f64| ((t - anchor) - ((t - anchor) / period).round() * period).abs() < ON_GRID_S * RATE;
            for (j, &t) in near.iter().enumerate() {
                if song.bar_at(from + j).0 == from + j && on(t) {
                    f.votes[0] += STORED_ONE;
                }
            }
            for (t, w) in self.follow_downbeats.drain(..) {
                let j = beat_at(t);
                let shift = j - song.bar_at(j).0;
                if on(t) && shift < 4 {
                    f.votes[shift] += w;
                }
            }
            let mut order = [0usize, 1, 2, 3];
            order.sort_by(|&a, &b| f.votes[b].total_cmp(&f.votes[a]));
            if f.votes[order[0]] >= 1.0 && f.votes[order[0]] > 1.5 * f.votes[order[1]] {
                f.one = Some(order[0] as u8);
            }
            f.grid = Some((anchor, period));
        }
        let (anchor, period) = f.grid.unwrap_or((stream(beats[k]), (beats[k + 1] - beats[k]) * RATE));
        // Until the votes are clear, the stored bar lines.
        let shift = f.one.unwrap_or(0) as usize;
        let in_bar = |j: usize| j.checked_sub(shift).map_or((j + 4 - shift) % 4, |j| j - song.bar_at(j).0);
        let one = (in_bar(beat_at(anchor)) % 4) as u8;
        let grid = Grid { beat: self.frame_of(anchor), period: period * self.input_rate / RATE, one: Some(one), confidence: 1.0 };
        let mut bars = Vec::new();
        let mut j = k.saturating_sub(in_bar(k));
        while j <= k + FOLLOW_AHEAD && j < beats.len() - 1 {
            let len = j.checked_sub(shift).map_or(shift - j, |j| song.bar_at(j).1).max(1);
            bars.push((self.frame_of(stream(beats[j])), len as u8));
            j += len;
        }
        let first = beats.partition_point(|&b| b <= now - FOLLOW_BACK_S).saturating_sub(1);
        let last = (k + FOLLOW_AHEAD).min(song.chords.len()).min(beats.len() - 1);
        let mut spans: Vec<ChordSpan> = Vec::new();
        for j in first..last {
            let (from, to, chord) = (self.frame_of(stream(beats[j])), self.frame_of(stream(beats[j + 1])), song.chords[j]);
            match spans.last_mut() {
                Some(s) if s.chord == chord => s.to = to,
                _ => spans.push(ChordSpan { from, to, chord, predicted: false }),
            }
        }
        self.follow = Some(f);
        self.pending.push(Heard::Grid(grid));
        self.pending.push(Heard::Bars(bars));
        self.pending.push(Heard::Chords(spans));
    }

    /// Moves the place in the song followed halfway to where the network's beats put it.
    fn refine_follow(&mut self) {
        let available = self.mels.len() / MELS;
        let n = available.min((self.settings.beat_window_s * FPS as f64) as usize);
        let first = available - n;
        let logits = self.beat_this.run(&self.mels[first * MELS..], n);
        let edge = (self.settings.edge_s * FPS as f64).round() as usize;
        let base = (self.mel_base + first as u64) as f64;
        let Some(id) = self.follow.map(|f| f.song) else { return };
        let (Some(f), Some(song)) = (self.follow.as_mut(), self.library.song(id)) else { return };
        self.follow_downbeats = peaks(&logits.downbeat)
            .into_iter()
            .filter(|p| p.0 >= 6.0 && p.0 < (n - edge) as f64)
            .map(|(j, v)| ((base + refine(&logits.downbeat, j)) * MEL_HOP as f64, crate::nn::sigmoid(v)))
            .collect();
        let mut off: Vec<f64> = peaks(&logits.beat)
            .into_iter()
            .filter(|p| p.0 >= 6.0 && p.0 < (n - edge) as f64)
            .filter_map(|(j, _)| {
                let t = (base + refine(&logits.beat, j)) * MEL_HOP as f64 / RATE + f.delta;
                let i = song.beats.partition_point(|&b| b < t);
                let near = [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|i| song.beats.get(i)).map(|&b| b - t).min_by(|a, b| a.abs().total_cmp(&b.abs()))?;
                (near.abs() < REFINE_S).then_some(near)
            })
            .collect();
        if off.len() >= REFINE_BEATS {
            off.sort_by(f64::total_cmp);
            f.delta += 0.5 * off[off.len() / 2];
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

    /// Pushes and steps, keeping all that is heard.
    fn hear_all(hearing: &mut Hearing, signal: &[f32], from: u64) -> (u64, Vec<Heard>) {
        let mut heard = Vec::new();
        for (i, block) in signal.chunks(2048).enumerate() {
            hearing.push(block, from + (i * 2048) as u64);
            while let Some(h) = hearing.step() {
                heard.push(h);
            }
        }
        (from + signal.len() as u64, heard)
    }

    fn follows(heard: &[Heard]) -> Vec<Option<Following>> {
        heard.iter().filter_map(|h| if let Heard::Follow(f) = h { Some(*f) } else { None }).collect()
    }

    /// A stored song of 60 s: the fingerprint test's tune, a beat every half second and C throughout.
    fn stored(hearing: &mut Hearing, seconds: f64) -> Vec<f32> {
        use crate::fingerprint::tests::tune;
        use crate::recognise::StoredSong;
        use midzer_core::Quality;
        let music = tune(60.0, 1);
        let beats: Vec<f64> = (0..=(seconds * 2.0) as usize).map(|i| i as f64 * 0.5).collect();
        let chords = vec![Some(Chord { root: 0, quality: Quality::Major, bass: 0 }); beats.len() - 1];
        let marks = fingerprint(&music[..(seconds * RATE) as usize]);
        hearing.library().add(StoredSong { id: 7, beats, chords, seconds, bar_beats: Vec::new() }, marks);
        music
    }

    #[test]
    fn a_stored_song_is_followed_on_its_beats_and_across_a_jump() {
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        let music = stored(&mut hearing, 60.0);
        // Opened 10 s into the song, at engine frame 1000.
        let (at, heard) = hear_all(&mut hearing, &music[(10.0 * RATE) as usize..(30.0 * RATE) as usize], 1000);
        let f = follows(&heard);
        assert_eq!(f.len(), 1, "{f:?}");
        let f = f[0].unwrap();
        assert_eq!(f.song, 7);
        // Song time s is engine frame 1000 + (s − 10) · RATE.
        let frame = |s: f64| 1000.0 + (s - 10.0) * RATE;
        assert!((frame(f.seconds) - f.frame).abs() < 0.03 * RATE, "{f:?}");
        // The grids from the stored song: those after it was recognised.
        let start = heard.iter().position(|h| matches!(h, Heard::Follow(_))).unwrap();
        let grids: Vec<Grid> = heard[start..].iter().filter_map(|h| if let Heard::Grid(g) = h { Some(*g) } else { None }).collect();
        assert!(grids.len() >= 5);
        for (i, g) in grids.iter().enumerate() {
            let s = 10.0 + (g.beat - 1000.0) / RATE;
            let k = (s / 0.5).round();
            assert!((s - k * 0.5).abs() < 0.01, "a grid at {s} s");
            // The stored 1 first; then the network's downbeats have a say (this tune has no bars).
            assert!(g.one.is_some());
            if i == 0 {
                assert_eq!(g.one, Some((k as i64 % 4) as u8));
            }
            assert!((g.period - 0.5 * RATE).abs() < 1.0 && g.confidence == 1.0);
        }
        assert!(heard.iter().any(|h| matches!(h, Heard::Chords(c) if !c.is_empty() && c.iter().all(|c| !c.predicted && c.chord.is_some_and(|c| c.root == 0)))));

        // The player jumps to 45 s: followed there, with no change of song.
        let (_, heard) = hear_all(&mut hearing, &music[(45.0 * RATE) as usize..(58.0 * RATE) as usize], at);
        let f: Vec<_> = follows(&heard).into_iter().flatten().collect();
        assert_eq!(f.len(), 1, "{f:?}");
        let s = 45.0 + (f[0].frame - at as f64) / RATE;
        assert!((f[0].seconds - s).abs() < 0.03, "{:?} at {s}", f[0]);
        assert!(!follows(&heard).contains(&None));
    }

    #[test]
    fn other_music_ends_the_song_and_cuts_the_capture_there() {
        use crate::fingerprint::tests::tune;
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        hearing.set_capture(true);
        let music = stored(&mut hearing, 60.0);
        let (at, _) = hear_all(&mut hearing, &vec![0.0; RATE as usize], 0);
        let (at, heard) = hear_all(&mut hearing, &music[..(40.0 * RATE) as usize], at);
        assert!(follows(&heard).first().is_some_and(|f| f.is_some()));
        let (at, heard) = hear_all(&mut hearing, &tune(40.0, 2), at);
        assert_eq!(follows(&heard), vec![None]);
        let c = hearing.take_song().expect("the stored song's capture");
        assert!(c.clean_start && !c.clean_end && c.followed == Some(7));
        assert!((39.0..43.0).contains(&c.seconds()), "{} s", c.seconds());
        hear_all(&mut hearing, &vec![0.0; 3 * RATE as usize], at);
        let c = hearing.take_song().expect("the other music");
        assert!(!c.clean_start && c.clean_end && c.followed.is_none());
        assert!((36.0..42.0).contains(&c.seconds()), "{} s", c.seconds());
    }

    #[test]
    fn not_this_song_and_the_end_of_a_partial_song_stop_following() {
        let mut hearing = Hearing::with_models(RATE as f32).unwrap();
        hearing.set_capture(true);
        // Only its first 30 s were stored.
        let music = stored(&mut hearing, 30.0);
        let (at, heard) = hear_all(&mut hearing, &music[..(29.0 * RATE) as usize], 0);
        assert!(follows(&heard).first().is_some_and(|f| f.is_some()));
        let (at, heard) = hear_all(&mut hearing, &music[(29.0 * RATE) as usize..(45.0 * RATE) as usize], at);
        assert_eq!(follows(&heard), vec![None], "stops quietly past the end");
        assert!(hearing.take_song().is_none(), "the capture goes on");

        hearing.reset();
        let (at, heard) = hear_all(&mut hearing, &music[..(15.0 * RATE) as usize], at + 10_000);
        assert!(follows(&heard).first().is_some_and(|f| f.is_some()));
        hearing.ignore(7);
        let (_, heard) = hear_all(&mut hearing, &music[(15.0 * RATE) as usize..(28.0 * RATE) as usize], at);
        let f = follows(&heard);
        assert!(f.first() == Some(&None) && f.len() == 1, "{f:?}");
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
