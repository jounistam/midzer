//! Live beat tracking, for audio midzer doesn't play itself: a browser tab,
//! the system's output or a line input.
//!
//! - Onset strength: spectral flux over log-spaced bands, about 200 times a second.
//! - Tempo: a running, slowly forgetting autocorrelation of the onset strength,
//!   read through a comb over four multiples of each period with a log-normal
//!   prior around 120 BPM.
//! - Phase: the causal dynamic programme of Stark, Davies and Plumbley (2009):
//!   every frame scores how well a beat there continues the best earlier beat
//!   about a period before. Following those links back from the best recent
//!   frame gives the beats, each confirmed a quarter of a beat after it.
//! - The confirmed beats go through a phase-locked loop, so the grid a drummer
//!   is judged against is smooth rather than following every onset.
//! - A guide from outside ([`BeatTracker::guide`]: a network that heard the
//!   last seconds, see `midzer-hear`) overrides the programme while it is
//!   fresh: the beats come from its grid, still through the loop, and the
//!   tempo estimate is pulled to it so the programme carries on from there.
//!
//! Every buffer is allocated in [`BeatTracker::new`]; [`BeatTracker::push`]
//! never allocates.

use crate::fft::Fft;

/// Onset frames per second, roughly; the hop is the nearest whole number of samples.
const FRAME_HZ: f32 = 200.0;
const MIN_BPM: f32 = 60.0;
const MAX_BPM: f32 = 200.0;
/// The prior's centre and width (in octaves), and its width once the player
/// has said the tempo is twice or half what the tracker found.
const PRIOR_BPM: f32 = 100.0;
const PRIOR_OCTAVES: f32 = 0.9;
const TOLD_OCTAVES: f32 = 0.3;
/// Onset frames kept: about 10 s.
const RING: usize = 2048;
/// Multiples of a period the tempo comb reads.
const COMB: usize = 4;
/// Bands of the onset detector, log-spaced between these.
const BANDS: usize = 24;
const BAND_LO_HZ: f32 = 40.0;
const BAND_HI_HZ: f32 = 12_000.0;
/// Band levels are compressed as ln(1 + COMPRESSION × magnitude).
const COMPRESSION: f32 = 1000.0;
/// Bands below this are the kick and bass, above that the snare and cymbals.
const LOW_HZ: f32 = 200.0;
const HIGH_HZ: f32 = 1_500.0;
/// How long the autocorrelation remembers, and how often the tempo is re-read.
const ACF_SECONDS: f32 = 6.0;
const TEMPO_EVERY_S: f32 = 0.25;
/// A new tempo more than this far away must win twice in a row.
const TEMPO_JUMP: f32 = 0.04;
/// How many hops before the end of its first window an onset is, on average.
const ONSET_HOPS: f64 = 1.2;
/// The dynamic programme's balance of history and onset, and how tightly it holds the period.
const DP_ALPHA: f32 = 0.9;
const DP_TIGHTNESS: f32 = 5.0;
/// A beat is confirmed this many periods after it.
const CONFIRM: f32 = 0.25;
/// The loop's phase and period gains, and the error past which it starts over.
const PLL_PHASE: f64 = 0.5;
const PLL_PERIOD: f64 = 0.15;
const PLL_RESYNC: f64 = 0.3;
/// Below this level (dBFS) the input counts as silent; after this long it is lost.
const SILENCE_DB: f32 = -60.0;
const LOST_AFTER_S: f32 = 2.5;
/// Locking: this many beats in a row, and either clear periodicity or some
/// periodicity with this many tempo readings (3 s) in a row without a jump.
/// Real songs have a noisier onset strength than clicks, so their
/// periodicity stays low (0.1–0.4); the steady tempo makes up for it.
const LOCK_BEATS: u32 = 8;
const SURE_CONFIDENCE: f32 = 0.35;
const LOCK_CONFIDENCE: f32 = 0.15;
const LOCK_READS: u32 = 12;
const UNLOCK_CONFIDENCE: f32 = 0.05;
/// Confidence follows the periodicity with this time constant.
const CONFIDENCE_SECONDS: f32 = 1.0;
/// A guide steers for this long after it came, if it is at least this sure.
const GUIDE_KEEP_S: f32 = 5.0;
const GUIDE_SURE: f32 = 0.3;
/// A guided beat moves to a clear onset (this many times the usual onset
/// strength) at most this far from it: the guide says which beat, the onset
/// says exactly when.
const GUIDE_SNAP_S: f32 = 0.025;
const GUIDE_SNAP_ONSET: f32 = 3.0;

/// A grid from outside: a beat's engine frame, the period in frames, how
/// sure it is (0..=1) and the engine frame it came at.
#[derive(Debug, Clone, Copy)]
struct Guide {
    at: f64,
    period: f64,
    confidence: f32,
    since: u64,
}

/// A confirmed beat of the input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackedBeat {
    /// When it was in the input, in the engine's frames (fractional).
    pub at: f64,
    /// Counts up from the first beat since the tracker last started over.
    pub index: u32,
    /// The beat's length in frames, as the loop has it now.
    pub period: f64,
    /// Onset strength at the beat in the low (kick, bass) and high (snare) bands.
    pub low: f32,
    pub high: f32,
}

/// Most beats one push of up to 128 frames can confirm.
const MAX_BEATS: usize = 8;

/// Fixed-capacity list of the beats one push confirmed.
pub struct BeatList {
    items: [TrackedBeat; MAX_BEATS],
    len: usize,
}

impl Default for BeatList {
    fn default() -> Self {
        Self { items: [TrackedBeat { at: 0.0, index: 0, period: 0.0, low: 0.0, high: 0.0 }; MAX_BEATS], len: 0 }
    }
}

impl BeatList {
    fn push(&mut self, beat: TrackedBeat) {
        if self.len < MAX_BEATS {
            self.items[self.len] = beat;
            self.len += 1;
        }
    }

    pub fn as_slice(&self) -> &[TrackedBeat] {
        &self.items[..self.len]
    }
}

pub struct BeatTracker {
    sample_rate: f32,
    hop: usize,
    /// Onset frames per second.
    rate: f32,
    fft: Fft,
    window: Box<[f32]>,
    /// The last `window.len()` samples, as a ring.
    samples: Box<[f32]>,
    write: usize,
    /// Samples since the last onset frame.
    since_hop: usize,
    /// The engine frame of the next input sample.
    clock: u64,
    re: Box<[f32]>,
    im: Box<[f32]>,
    /// Each band's FFT bins, and its frequency class: 0 low, 1 mid, 2 high.
    bands: Box<[(usize, usize, u8)]>,
    prev: Box<[f32]>,
    flux_mean: f32,
    onset_mean: f32,
    /// Per onset frame, as rings: onset strength, low and high flux, the
    /// mean-free strength for the autocorrelation, the engine frame at the
    /// end of its window, the dynamic programme's score and its link back.
    onset: Box<[f32]>,
    low: Box<[f32]>,
    high: Box<[f32]>,
    centred: Box<[f32]>,
    time: Box<[u64]>,
    score: Box<[f32]>,
    back: Box<[u16]>,
    /// Onset frames so far.
    frames: u64,
    acf: Box<[f32]>,
    acf_decay: f32,
    min_lag: usize,
    max_lag: usize,
    /// The beat period in onset frames, once there is one.
    period: Option<f32>,
    /// A far tempo that won once, waiting for a second win.
    candidate: Option<f32>,
    /// The programme's window over links back, for the current period.
    weights: Box<[f32]>,
    prior_bpm: f32,
    prior_octaves: f32,
    tempo_every: u32,
    until_tempo: u32,
    /// The latest beat the programme confirmed, as an onset frame.
    confirmed: Option<u64>,
    /// The loop: the last beat's frame and index, and the period.
    beat: Option<(f64, u32)>,
    beat_period: f64,
    beats_in_lock: u32,
    /// Tempo readings in a row without a jump.
    steady_reads: u32,
    confidence: f32,
    locked: bool,
    power: f32,
    quiet_frames: u32,
    lost_after: u32,
    /// Sound since the last reset, and whether silence then made it let go.
    heard: bool,
    lost: bool,
    level_db: f32,
    guide: Option<Guide>,
    guide_keep: u64,
}

impl BeatTracker {
    pub fn new(sample_rate: f32) -> Self {
        let hop = (sample_rate / FRAME_HZ).round().max(1.0) as usize;
        let rate = sample_rate / hop as f32;
        // About 11 ms of window, a power of two.
        let n = ((sample_rate * 0.011) as usize).next_power_of_two().max(256);
        let window = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
        let min_lag = (rate * 60.0 / MAX_BPM).floor() as usize;
        let max_lag = (rate * 60.0 / MIN_BPM).ceil() as usize;
        let bin_hz = sample_rate / n as f32;
        let top = BAND_HI_HZ.min(sample_rate * 0.45);
        let mut bands: Vec<(usize, usize, u8)> = Vec::with_capacity(BANDS);
        for b in 0..BANDS {
            let edge = |k: usize| BAND_LO_HZ * (top / BAND_LO_HZ).powf(k as f32 / BANDS as f32);
            let lo = ((edge(b) / bin_hz).round() as usize).max(1);
            let hi = ((edge(b + 1) / bin_hz).round() as usize).max(lo + 1);
            let lo = bands.last().map_or(lo, |l| lo.max(l.1));
            if lo >= hi {
                continue;
            }
            let mid = (lo + hi) as f32 * 0.5 * bin_hz;
            let class = if mid < LOW_HZ { 0 } else if mid < HIGH_HZ { 1 } else { 2 };
            bands.push((lo, hi, class));
        }
        let band_count = bands.len();
        let tempo_every = (rate * TEMPO_EVERY_S).round() as u32;
        Self {
            sample_rate,
            hop,
            rate,
            fft: Fft::new(n),
            window,
            samples: vec![0.0; n].into(),
            write: 0,
            since_hop: 0,
            clock: 0,
            re: vec![0.0; n].into(),
            im: vec![0.0; n].into(),
            bands: bands.into(),
            prev: vec![0.0; band_count].into(),
            flux_mean: 0.0,
            onset_mean: 0.0,
            onset: vec![0.0; RING].into(),
            low: vec![0.0; RING].into(),
            high: vec![0.0; RING].into(),
            centred: vec![0.0; RING].into(),
            time: vec![0; RING].into(),
            score: vec![0.0; RING].into(),
            back: vec![0; RING].into(),
            frames: 0,
            acf: vec![0.0; COMB * max_lag + 2].into(),
            acf_decay: (-1.0 / (rate * ACF_SECONDS)).exp(),
            min_lag,
            max_lag,
            period: None,
            candidate: None,
            weights: vec![0.0; 2 * max_lag + 2].into(),
            prior_bpm: PRIOR_BPM,
            prior_octaves: PRIOR_OCTAVES,
            tempo_every,
            until_tempo: tempo_every,
            confirmed: None,
            beat: None,
            beat_period: 0.0,
            beats_in_lock: 0,
            steady_reads: 0,
            confidence: 0.0,
            locked: false,
            power: 0.0,
            quiet_frames: 0,
            lost_after: (LOST_AFTER_S * rate) as u32,
            heard: false,
            lost: false,
            level_db: -100.0,
            guide: None,
            guide_keep: (GUIDE_KEEP_S * sample_rate) as u64,
        }
    }

    /// Forgets everything heard: the tempo, the beats and the lock.
    pub fn reset(&mut self) {
        self.samples.fill(0.0);
        self.since_hop = 0;
        self.prev.fill(0.0);
        self.flux_mean = 0.0;
        self.onset_mean = 0.0;
        for ring in [&mut self.onset, &mut self.low, &mut self.high, &mut self.centred, &mut self.score] {
            ring.fill(0.0);
        }
        self.back.fill(0);
        self.frames = 0;
        self.acf.fill(0.0);
        self.period = None;
        self.candidate = None;
        self.prior_bpm = PRIOR_BPM;
        self.prior_octaves = PRIOR_OCTAVES;
        self.until_tempo = self.tempo_every;
        self.confirmed = None;
        self.beat = None;
        self.beats_in_lock = 0;
        self.steady_reads = 0;
        self.confidence = 0.0;
        self.locked = false;
        self.quiet_frames = 0;
        self.heard = false;
        self.lost = false;
        self.guide = None;
    }

    /// Whether silence made the tracker let go of what it heard since the
    /// last call: once per quiet stretch after sound.
    pub fn take_lost(&mut self) -> bool {
        std::mem::take(&mut self.lost)
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    /// How sure the tracker is of the beat, 0..=1.
    pub fn confidence(&self) -> f32 {
        self.confidence
    }

    /// The input's loudness in dBFS, smoothed.
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    /// The tempo the loop runs at, once it has beats.
    pub fn bpm(&self) -> Option<f32> {
        self.beat.map(|_| (60.0 * self.sample_rate as f64 / self.beat_period) as f32)
    }

    /// The tempo the autocorrelation reads now, before the loop.
    pub fn estimate_bpm(&self) -> Option<f32> {
        self.period.map(|p| 60.0 * self.rate / p)
    }

    /// The last confirmed beat (engine frame, index) and the period in frames.
    pub fn last_beat(&self) -> Option<(f64, u32, f64)> {
        self.beat.map(|(at, index)| (at, index, self.beat_period))
    }

    /// Takes `input`, whose first sample is at engine frame `start`, and adds
    /// the beats it confirms to `out`.
    pub fn push(&mut self, input: &[f32], start: u64, out: &mut BeatList) {
        self.clock = start;
        let n = self.samples.len();
        for &x in input {
            self.samples[self.write] = x;
            self.write = (self.write + 1) % n;
            self.power += (x * x - self.power) * (1.0 / (0.05 * self.sample_rate));
            self.clock += 1;
            self.since_hop += 1;
            if self.since_hop >= self.hop {
                self.since_hop = 0;
                self.frame(out);
            }
        }
    }

    /// One onset frame: the spectrum, the onset strength and everything that follows from it.
    fn frame(&mut self, out: &mut BeatList) {
        let n = self.samples.len();
        for i in 0..n {
            self.re[i] = self.samples[(self.write + i) % n] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);
        let scale = 2.0 / n as f32;
        let (mut flux, mut low, mut high) = (0.0, 0.0, 0.0);
        for (b, &(lo, hi, class)) in self.bands.iter().enumerate() {
            let mut sum = 0.0;
            for k in lo..hi {
                sum += (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt();
            }
            let level = (1.0 + COMPRESSION * sum * scale).ln();
            let rise = (level - self.prev[b]).max(0.0);
            self.prev[b] = level;
            flux += rise;
            match class {
                0 => low += rise,
                2 => high += rise,
                _ => {}
            }
        }
        let rate = self.rate;
        self.flux_mean += (flux - self.flux_mean) / (0.5 * rate);
        let onset = (flux - self.flux_mean).max(0.0);
        self.onset_mean += (onset - self.onset_mean) / (3.0 * rate);

        let m = self.frames;
        let at = (m as usize) % RING;
        self.onset[at] = onset;
        self.low[at] = low;
        self.high[at] = high;
        self.centred[at] = onset - self.onset_mean;
        self.time[at] = self.clock;
        self.frames += 1;

        self.level_db = 10.0 * self.power.max(1e-10).log10();
        if self.level_db < SILENCE_DB {
            self.quiet_frames += 1;
            if self.quiet_frames >= self.lost_after {
                if self.frames > 1 {
                    let heard = self.heard;
                    self.reset();
                    self.lost = heard;
                }
                return;
            }
        } else {
            self.quiet_frames = 0;
            self.heard = true;
        }

        self.autocorrelate(m);
        self.until_tempo -= 1;
        if self.until_tempo == 0 {
            self.until_tempo = self.tempo_every;
            self.estimate_tempo();
        }
        if let Some(period) = self.period {
            self.programme(m, period);
        }
        if let Some(guide) = self.fresh_guide() {
            self.confirm_guided(guide, out);
        } else if let Some(period) = self.period {
            self.confirm(m, period, out);
        }
    }

    fn ring(&self, ring: &[f32], m: u64) -> f32 {
        ring[(m as usize) % RING]
    }

    fn autocorrelate(&mut self, m: u64) {
        let x = self.ring(&self.centred, m);
        let lags = (self.acf.len() as u64).min(m + 1) as usize;
        let decay = self.acf_decay;
        for lag in 0..lags {
            let y = self.centred[((m - lag as u64) as usize) % RING];
            self.acf[lag] = self.acf[lag] * decay + x * y;
        }
    }

    /// The autocorrelation at a fractional lag.
    fn acf_at(&self, lag: f32) -> f32 {
        let i = lag.floor() as usize;
        if i + 1 >= self.acf.len() {
            return 0.0;
        }
        let f = lag - i as f32;
        self.acf[i] * (1.0 - f) + self.acf[i + 1] * f
    }

    fn tempo_score(&self, lag: f32) -> f32 {
        let comb: f32 = (1..=COMB).map(|p| self.acf_at(lag * p as f32)).sum();
        let octaves = (60.0 * self.rate / lag / self.prior_bpm).log2() / self.prior_octaves;
        comb * (-0.5 * octaves * octaves).exp()
    }

    fn estimate_tempo(&mut self) {
        let energy = self.acf[0];
        if energy <= 1e-9 || self.frames < (2.0 * self.rate) as u64 {
            return;
        }
        let mut best = (self.min_lag, f32::MIN);
        for lag in self.min_lag..=self.max_lag {
            let s = self.tempo_score(lag as f32);
            if s > best.1 {
                best = (lag, s);
            }
        }
        // A parabola through the peak's neighbours gives the fractional lag.
        let (l, s) = best;
        let (a, c) = (self.tempo_score(l as f32 - 1.0), self.tempo_score(l as f32 + 1.0));
        let curve = a - 2.0 * s + c;
        let found = l as f32 + if curve < 0.0 { (0.5 * (a - c) / curve).clamp(-0.5, 0.5) } else { 0.0 };
        let periodicity = (self.acf_at(found) / energy).clamp(0.0, 1.0);
        let k = (self.tempo_every as f32 / (CONFIDENCE_SECONDS * self.rate)).min(1.0);
        self.confidence += (periodicity - self.confidence) * k;

        match self.period {
            Some(p) if (found / p - 1.0).abs() <= TEMPO_JUMP => self.steady_reads += 1,
            _ => self.steady_reads = 0,
        }
        let next = match self.period {
            None => found,
            Some(p) if (found / p - 1.0).abs() <= TEMPO_JUMP => p + 0.5 * (found - p),
            Some(p) => match self.candidate.take() {
                // A jump must win twice, and clearly.
                Some(c) if (found / c - 1.0).abs() <= TEMPO_JUMP && s > 1.1 * self.tempo_score(p) => found,
                _ => {
                    self.candidate = Some(found);
                    p
                }
            },
        };
        if (self.period.unwrap_or(0.0) - next).abs() > 1e-3 {
            if self.period.is_some_and(|p| (next / p - 1.0).abs() > TEMPO_JUMP) {
                // A new tempo: the loop starts over from the programme's beats.
                self.unlock();
            }
            self.set_period(next);
        }
        if self.locked && (self.confidence < UNLOCK_CONFIDENCE) {
            self.unlock();
        }
    }

    /// A grid from outside: a beat at engine frame `at`, `period` frames
    /// apart, `confidence` 0..=1. While it is fresh, the beats come from it.
    pub fn guide(&mut self, at: f64, period: f64, confidence: f32) {
        if confidence < GUIDE_SURE || period <= 0.0 {
            return;
        }
        let onset_period = (period / self.hop as f64) as f32;
        if !(self.min_lag as f32..=self.max_lag as f32).contains(&onset_period) {
            return;
        }
        // The programme follows the guide's tempo, so it carries on when the guide stops.
        self.prior_bpm = 60.0 * self.rate / onset_period;
        self.prior_octaves = TOLD_OCTAVES;
        self.candidate = None;
        if self.period.is_none_or(|p| (onset_period / p - 1.0).abs() > TEMPO_JUMP) {
            self.set_period(onset_period);
        }
        self.guide = Some(Guide { at, period, confidence, since: self.clock });
        // Sure enough to show beats now; the loop moves to the guide's grid with its next beat.
        if self.beat.is_some() {
            self.locked = true;
        }
    }

    fn fresh_guide(&self) -> Option<Guide> {
        self.guide.filter(|g| self.clock < g.since + self.guide_keep)
    }

    /// The player says the beat is `factor` (2 or ½) times what was found:
    /// the tempo moves there and stays near it until the input is lost.
    pub fn scale_tempo(&mut self, factor: f32) {
        let Some(period) = self.period else { return };
        // The player knows better than the guide.
        self.guide = None;
        let next = (period / factor).clamp(self.min_lag as f32, self.max_lag as f32);
        self.prior_bpm = 60.0 * self.rate / next;
        self.prior_octaves = TOLD_OCTAVES;
        self.candidate = None;
        self.set_period(next);
        self.unlock();
        if self.beat.is_some() {
            self.beat_period = next as f64 * self.hop as f64;
        }
    }

    fn set_period(&mut self, period: f32) {
        self.period = Some(period);
        for (tau, w) in self.weights.iter_mut().enumerate() {
            let x = DP_TIGHTNESS * (tau.max(1) as f32 / period).ln();
            *w = (-0.5 * x * x).exp();
        }
    }

    fn unlock(&mut self) {
        self.locked = false;
        self.beats_in_lock = 0;
    }

    /// The programme's score for frame `m`: its onset plus the best earlier
    /// beat about a period back.
    fn programme(&mut self, m: u64, period: f32) {
        let lo = (period * 0.5).round().max(1.0) as u64;
        let hi = ((period * 2.0).round() as u64).min(self.weights.len() as u64 - 1).min(m).min(RING as u64 - 1);
        let mut best = (0u64, 0.0f32);
        let mut tau = lo;
        while tau <= hi {
            let s = self.weights[tau as usize] * self.score[((m - tau) as usize) % RING];
            if s > best.1 {
                best = (tau, s);
            }
            tau += 1;
        }
        let at = (m as usize) % RING;
        self.score[at] = (1.0 - DP_ALPHA) * self.onset[at] + DP_ALPHA * best.1;
        self.back[at] = best.0 as u16;
    }

    /// Follows the links back from the best frame of the last period and
    /// confirms the beats old enough, in order.
    fn confirm(&mut self, m: u64, period: f32, out: &mut BeatList) {
        let span = (period.round() as u64).min(m + 1);
        let mut anchor = m;
        for f in (m + 1 - span)..=m {
            if self.ring(&self.score, f) > self.ring(&self.score, anchor) {
                anchor = f;
            }
        }
        let ready = m.saturating_sub((CONFIRM * period).round() as u64);
        let after = self.confirmed.map_or(0, |c| c + (period * 0.5) as u64);
        let oldest = m.saturating_sub(RING as u64 - 1);
        // The chain, newest first, down to the last confirmed beat.
        let mut chain = [0u64; MAX_BEATS];
        let mut len = 0;
        let mut f = anchor;
        while f > after && f >= oldest && len < MAX_BEATS {
            if f <= ready {
                chain[len] = f;
                len += 1;
            }
            let back = self.back[(f as usize) % RING] as u64;
            if back == 0 || back > f {
                break;
            }
            f -= back;
        }
        if self.confirmed.is_none() {
            // Starting: only the newest beat, nothing older.
            len = len.min(1);
        }
        for i in (0..len).rev() {
            self.confirmed = Some(chain[i]);
            self.beat(chain[i], period, out);
        }
    }

    /// A confirmed beat at onset frame `f`: refined between frames, then
    /// smoothed by the loop.
    fn beat(&mut self, f: u64, period: f32, out: &mut BeatList) {
        let o = |k: u64| self.ring(&self.onset, k);
        let mut delta = 0.0;
        if f >= 1 && f + 1 < self.frames {
            let (a, b, c) = (o(f - 1), o(f), o(f + 1));
            let curve = a - 2.0 * b + c;
            if b > 0.0 && b >= a && b >= c && curve < 0.0 {
                delta = (0.5 * (a - c) / curve).clamp(-0.5, 0.5) as f64;
            }
        }
        let peak = |ring: &[f32]| {
            let lo = f.saturating_sub(2).max(self.frames.saturating_sub(RING as u64 - 1));
            (lo..=(f + 2).min(self.frames - 1)).map(|k| ring[(k as usize) % RING]).fold(0.0f32, f32::max)
        };
        let (low, high) = (peak(&self.low), peak(&self.high));
        // The first window an onset reaches peaks: it lies within the last hop.
        let raw = self.ring_time(f) as f64 + (delta - ONSET_HOPS) * self.hop as f64;
        let measured = period as f64 * self.hop as f64;
        let (at, index) = match self.beat {
            None => {
                self.beat_period = measured;
                (raw, 0)
            }
            Some((last, index)) => {
                let steps = ((raw - last) / self.beat_period).round().clamp(1.0, 4.0);
                let predicted = last + steps * self.beat_period;
                let error = raw - predicted;
                if error.abs() > PLL_RESYNC * self.beat_period {
                    self.beat_period = measured;
                    self.unlock();
                    (raw, index + steps as u32)
                } else {
                    // Missed beats in between are filled in on the loop's grid.
                    for k in 1..steps as u32 {
                        let at = last + k as f64 * self.beat_period;
                        out.push(TrackedBeat { at, index: index + k, period: self.beat_period, low: 0.0, high: 0.0 });
                    }
                    self.beat_period += PLL_PERIOD * error / steps;
                    // The period never strays far from the tempo estimate.
                    self.beat_period = self.beat_period.clamp(measured * 0.97, measured * 1.03);
                    (predicted + PLL_PHASE * error, index + steps as u32)
                }
            }
        };
        self.beat = Some((at, index));
        self.beats_in_lock += 1;
        let sure = self.confidence >= SURE_CONFIDENCE || (self.confidence >= LOCK_CONFIDENCE && self.steady_reads >= LOCK_READS);
        if !self.locked && sure && self.beats_in_lock >= LOCK_BEATS {
            self.locked = true;
        }
        out.push(TrackedBeat { at, index, period: self.beat_period, low, high });
    }

    /// Confirms the guide's beats just after they pass, in order: the
    /// listener has already settled them, so unlike the programme's they only
    /// wait for the onset that may pin them down.
    fn confirm_guided(&mut self, g: Guide, out: &mut BeatList) {
        let ready = self.clock as f64 - (GUIDE_SNAP_S as f64 + 0.01) * self.sample_rate as f64;
        let mut k = match self.beat {
            Some((last, _)) => ((last + 0.5 * g.period - g.at) / g.period).ceil(),
            // Starting: the newest beat only.
            None => ((ready - g.at) / g.period).floor(),
        };
        while g.at + k * g.period <= ready {
            self.guided_beat(g.at + k * g.period, g, out);
            k += 1.0;
        }
    }

    /// A beat of the guide's grid at engine frame `t`, through the loop.
    fn guided_beat(&mut self, t: f64, g: Guide, out: &mut BeatList) {
        // The onsets around it, for the downbeat guess: the frame nearest `t`.
        let back = ((self.clock as f64 - t) / self.hop as f64).round().max(0.0) as u64;
        let f = self.frames.saturating_sub(1 + back);
        let t = self.snap(t, f);
        let peak = |ring: &[f32]| {
            let lo = f.saturating_sub(2).max(self.frames.saturating_sub(RING as u64 - 1));
            (lo..=(f + 2).min(self.frames.saturating_sub(1))).map(|k| ring[(k as usize) % RING]).fold(0.0f32, f32::max)
        };
        let (low, high) = (peak(&self.low), peak(&self.high));
        let (at, index) = match self.beat {
            None => {
                self.beat_period = g.period;
                (t, 0)
            }
            Some((last, index)) => {
                let steps = ((t - last) / self.beat_period).round().clamp(1.0, 4.0);
                let predicted = last + steps * self.beat_period;
                let error = t - predicted;
                if error.abs() > PLL_RESYNC * self.beat_period {
                    self.beat_period = g.period;
                    (t, index + steps as u32)
                } else {
                    for k in 1..steps as u32 {
                        let at = last + k as f64 * self.beat_period;
                        out.push(TrackedBeat { at, index: index + k, period: self.beat_period, low: 0.0, high: 0.0 });
                    }
                    self.beat_period += PLL_PERIOD * error / steps;
                    self.beat_period = self.beat_period.clamp(g.period * 0.97, g.period * 1.03);
                    (predicted + PLL_PHASE * error, index + steps as u32)
                }
            }
        };
        self.beat = Some((at, index));
        self.beats_in_lock += 1;
        self.locked = true;
        self.confidence = self.confidence.max(g.confidence);
        out.push(TrackedBeat { at, index, period: self.beat_period, low, high });
    }

    /// `t` moved to the clear onset nearest it (onset frame `f`), if there is one close by.
    fn snap(&self, t: f64, f: u64) -> f64 {
        let reach = (GUIDE_SNAP_S * self.rate).round() as u64;
        let oldest = self.frames.saturating_sub(RING as u64 - 1);
        let (lo, hi) = (f.saturating_sub(reach).max(oldest).max(1), (f + reach).min(self.frames.saturating_sub(2)));
        let o = |k: u64| self.ring(&self.onset, k);
        let Some(best) = (lo..=hi).filter(|&k| o(k) >= o(k - 1) && o(k) >= o(k + 1)).max_by(|&a, &b| o(a).total_cmp(&o(b))) else { return t };
        if o(best) < GUIDE_SNAP_ONSET * self.onset_mean.max(1e-6) {
            return t;
        }
        let (a, b, c) = (o(best - 1), o(best), o(best + 1));
        let curve = a - 2.0 * b + c;
        let delta = if curve < 0.0 { (0.5 * (a - c) / curve).clamp(-0.5, 0.5) as f64 } else { 0.0 };
        let at = self.ring_time(best) as f64 + (delta - ONSET_HOPS) * self.hop as f64;
        if (at - t).abs() <= GUIDE_SNAP_S as f64 * self.sample_rate as f64 { at } else { t }
    }

    fn ring_time(&self, f: u64) -> u64 {
        self.time[(f as usize) % RING]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// A click track: a short noise burst on every beat, accented on the 1,
    /// and with `tick` > 0 a quieter one on the eighths between, like a
    /// hi-hat. `beat_s(i)` is when beat `i` sounds; returns the audio and the
    /// beats' frames.
    fn clicks(seconds: f32, tick: f32, beat_s: impl Fn(u32) -> f32) -> (Vec<f32>, Vec<f64>) {
        let len = (seconds * SR) as usize;
        let mut out = vec![0.0f32; len];
        let mut times = Vec::new();
        let mut seed = 1u32;
        let mut burst = |out: &mut [f32], start: usize, amp: f32| {
            let end = (start + (0.03 * SR) as usize).min(len);
            for (k, x) in out[start..end].iter_mut().enumerate() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
                *x += noise * amp * (-(k as f32) / (0.005 * SR)).exp();
            }
        };
        for i in 0.. {
            let t = beat_s(i);
            let start = (t * SR) as usize;
            if start >= len {
                break;
            }
            times.push(start as f64);
            burst(&mut out, start, if i % 4 == 0 { 0.5 } else { 0.3 });
            let next = beat_s(i + 1);
            if tick > 0.0 && next < 1e8 {
                burst(&mut out, ((t + next) * 0.5 * SR) as usize, tick);
            }
        }
        (out, times)
    }

    /// Runs the tracker over `signal` in blocks of 128 and returns its beats
    /// and whether it was locked after each. `scale` tells it at a time in
    /// seconds that the tempo is twice or half what it found.
    fn track(signal: &[f32], scale: Option<(f32, f32)>) -> Vec<(TrackedBeat, bool)> {
        let mut tracker = BeatTracker::new(SR);
        let mut beats = Vec::new();
        for (i, block) in signal.chunks(128).enumerate() {
            if let Some((at, factor)) = scale
                && i == (at * SR) as usize / 128
            {
                tracker.scale_tempo(factor);
            }
            let mut out = BeatList::default();
            tracker.push(block, (i * 128) as u64, &mut out);
            beats.extend(out.as_slice().iter().map(|b| (*b, tracker.locked())));
        }
        beats
    }

    /// The error of each locked beat from the nearest true beat, in ms.
    fn errors(beats: &[(TrackedBeat, bool)], truth: &[f64]) -> Vec<f64> {
        beats
            .iter()
            .filter(|(_, locked)| *locked)
            .map(|(b, _)| {
                let i = truth.partition_point(|&t| t < b.at);
                let near = [i.saturating_sub(1), i.min(truth.len() - 1)].map(|k| b.at - truth[k]);
                let e = if near[0].abs() < near[1].abs() { near[0] } else { near[1] };
                e / SR as f64 * 1000.0
            })
            .collect()
    }

    fn first_lock_s(beats: &[(TrackedBeat, bool)]) -> f64 {
        beats.iter().find(|(_, l)| *l).map_or(f64::INFINITY, |(b, _)| b.at / SR as f64)
    }

    /// Tracks `signal`, given `guide` (at seconds, beat frame, period frames, confidence) once.
    fn track_guided(signal: &[f32], guides: &[(f32, f64, f64, f32)]) -> Vec<(TrackedBeat, bool)> {
        let mut tracker = BeatTracker::new(SR);
        let mut beats = Vec::new();
        for (i, block) in signal.chunks(128).enumerate() {
            for &(at, frame, period, confidence) in guides {
                if i == (at * SR) as usize / 128 {
                    tracker.guide(frame, period, confidence);
                }
            }
            let mut out = BeatList::default();
            tracker.push(block, (i * 128) as u64, &mut out);
            beats.extend(out.as_slice().iter().map(|b| (*b, tracker.locked())));
        }
        beats
    }

    #[test]
    fn a_guide_locks_at_once_and_wins_over_the_programme() {
        // 174 BPM, which the programme alone hears as 87 (see below), guided at
        // 4 s, a little late, by a listener that heard it right.
        let beat = 60.0 / 174.0;
        let (signal, truth) = clicks(12.0, 0.0, |i| 0.3 + i as f32 * beat);
        let period = beat as f64 * SR as f64;
        let beats = track_guided(&signal, &[(4.0, truth[8], period, 0.9)]);
        let lock = first_lock_s(&beats);
        assert!(lock < 4.3, "locked at {lock} s");
        let after: Vec<_> = beats.iter().filter(|(b, _)| b.at > 4.5 * SR as f64).copied().collect();
        let e = errors(&after, &truth);
        let worst = e.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        assert!(worst < 3.0, "worst {worst} ms");
        let found = 60.0 * SR as f64 / after.last().unwrap().0.period;
        assert!((found - 174.0).abs() < 1.0, "found {found}");
        // Every beat of the guide, none skipped.
        assert!(after.windows(2).all(|w| w[1].0.index == w[0].0.index + 1));
    }

    #[test]
    fn an_unsure_guide_is_ignored_and_an_old_one_lets_go() {
        let beat = 0.5;
        let (signal, truth) = clicks(16.0, 0.0, |i| 0.3 + i as f32 * beat);
        // Off by a third of a beat: ignored when unsure.
        let wrong = truth[4] + (beat / 3.0 * SR) as f64;
        let beats = track_guided(&signal, &[(2.0, wrong, beat as f64 * SR as f64, 0.1)]);
        let e = errors(&beats, &truth);
        assert!(e.iter().all(|x| x.abs() < 5.0));
        // Sure but wrong: followed while fresh, and the programme takes over once it is old.
        let beats = track_guided(&signal, &[(2.0, wrong, beat as f64 * SR as f64, 0.9)]);
        let guided: Vec<_> = beats.iter().filter(|(b, _)| b.at > 3.0 * SR as f64 && b.at < 6.5 * SR as f64).copied().collect();
        assert!(errors(&guided, &truth).iter().all(|x| x.abs() > 100.0));
        let later: Vec<_> = beats.iter().filter(|(b, _)| b.at > 12.0 * SR as f64).copied().collect();
        assert!(errors(&later, &truth).iter().all(|x| x.abs() < 5.0), "{:?}", errors(&later, &truth));
    }

    #[test]
    fn locks_onto_a_steady_click() {
        // 174 BPM is heard in half time, as 87, after it first takes the faster one.
        for (bpm, heard, lock_by) in [(72.0, 72.0, 11.5), (100.0, 100.0, 9.0), (128.0, 128.0, 8.0), (174.0, 87.0, 13.0)] {
            let beat = 60.0 / bpm;
            let (signal, truth) = clicks(20.0, 0.0, |i| 0.3 + i as f32 * beat);
            let beats = track(&signal, None);
            let lock = first_lock_s(&beats);
            assert!(lock < lock_by, "{bpm} BPM locked at {lock} s");
            let e = errors(&beats, &truth);
            let worst = e.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            assert!(worst < 5.0, "{bpm} BPM: worst {worst} ms");
            let last = beats.last().unwrap().0;
            let found = 60.0 * SR as f64 / last.period;
            assert!((found - heard).abs() < 0.3, "{bpm} BPM found {found}");
            let steps: Vec<u32> = beats.windows(2).map(|w| w[1].0.index - w[0].0.index).collect();
            assert!(steps.iter().all(|&s| s == 1), "{bpm}: {steps:?}");
        }
    }

    #[test]
    fn the_player_can_double_or_halve_the_tempo() {
        // 174 BPM clicks are heard as 87; a 72 BPM ballad's eighths as 144.
        for (bpm, tick, factor) in [(174.0, 0.0, 2.0), (72.0, 0.1, 0.5)] {
            let beat = 60.0 / bpm;
            let (signal, truth) = clicks(26.0, tick, |i| 0.3 + i as f32 * beat);
            let before = track(&signal[..(10.0 * SR) as usize], None);
            let heard = 60.0 * SR as f64 / before.last().unwrap().0.period;
            assert!((heard - bpm as f64 / factor as f64).abs() < 0.5, "{bpm} BPM heard as {heard}");
            // Told at 10 s; only what follows counts.
            let beats: Vec<_> = track(&signal, Some((10.0, factor))).into_iter().filter(|(b, _)| b.at > 10.0 * SR as f64).collect();
            let lock = first_lock_s(&beats);
            assert!(lock < 10.0 + 10.0 * beat as f64, "{bpm}: locked again at {lock} s");
            let found = 60.0 * SR as f64 / beats.last().unwrap().0.period;
            assert!((found - bpm as f64).abs() < 0.3, "{bpm}: {found} BPM");
            let e = errors(&beats, &truth);
            assert!(e.len() > 10 && e.iter().all(|x| x.abs() < 5.0), "{bpm}: {e:?}");
        }
    }

    #[test]
    fn follows_a_click_that_speeds_up() {
        // 100 to 112 BPM over 40 s.
        let at = |i: u32| {
            let mut t = 0.5f32;
            for k in 0..i {
                t += 60.0 / (100.0 + 12.0 * (k as f32 / 66.0).min(1.0));
            }
            t
        };
        let (signal, truth) = clicks(40.0, 0.0, at);
        let beats = track(&signal, None);
        let e = errors(&beats, &truth);
        assert!(e.len() > 50, "{} locked beats", e.len());
        let mean = e.iter().sum::<f64>() / e.len() as f64;
        let worst = e.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        // The loop lags a little behind such a steep ramp.
        assert!(mean.abs() < 4.5 && worst < 6.0, "mean {mean} ms, worst {worst} ms");
    }

    #[test]
    fn holds_through_a_break_and_lets_go_in_silence() {
        // 120 BPM; bars 9–10 are a near-silent break (a quiet hum), then it
        // comes back on the grid; it ends at 30 s.
        let beat = 0.5;
        let (mut signal, truth) = clicks(36.0, 0.0, |i| if i < 30.0 as u32 * 2 { 0.25 + i as f32 * beat } else { 1e9 });
        let (from, to) = ((0.25 + 32.0 * beat) * SR, (0.25 + 40.0 * beat) * SR);
        for (i, x) in signal.iter_mut().enumerate() {
            let t = i as f32;
            if t >= from && t < to {
                *x = 0.003 * (2.0 * std::f32::consts::PI * 110.0 * t / SR).sin();
            }
        }
        let beats = track(&signal, None);
        let during: Vec<_> = beats.iter().filter(|(b, _)| b.at >= from as f64 && b.at < to as f64).collect();
        assert!(during.len() >= 7 && during.iter().all(|(_, l)| *l), "keeps the beat through the break: {}", during.len());
        let after: Vec<_> = beats.iter().filter(|(b, _)| b.at >= to as f64 && b.at < 29.5 * SR as f64).cloned().collect();
        let e = errors(&after, &truth);
        assert!(!e.is_empty() && e.iter().all(|x| x.abs() < 4.0), "back on the grid: {e:?}");
        // It ends at 30 s; within the next few seconds the beats stop for good.
        assert!(beats.iter().all(|(b, _)| b.at < 33.0 * SR as f64), "silence stops the beats");
    }

    #[test]
    fn noise_never_locks() {
        let mut seed = 7u32;
        let signal: Vec<f32> = (0..(15.0 * SR) as usize)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0) * 0.1
            })
            .collect();
        assert!(track(&signal, None).iter().all(|(_, l)| !l));
    }

    #[test]
    fn works_at_44_1_khz() {
        let mut tracker = BeatTracker::new(44_100.0);
        let beat = 0.5f32;
        let len = (12.0 * 44_100.0) as usize;
        let signal: Vec<f32> = (0..len)
            .map(|i| {
                let t = i as f32 / 44_100.0;
                let since = (t - 0.2).rem_euclid(beat);
                if t >= 0.2 && since < 0.02 { (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * (-since / 0.004).exp() * 0.5 } else { 0.0 }
            })
            .collect();
        let mut last = None;
        for (i, block) in signal.chunks(128).enumerate() {
            let mut out = BeatList::default();
            tracker.push(block, (i * 128) as u64, &mut out);
            if let Some(b) = out.as_slice().last() {
                last = Some(*b);
            }
        }
        assert!(tracker.locked());
        let b = last.unwrap();
        assert!((60.0 * 44_100.0 / b.period - 120.0).abs() < 0.5);
        let into = (b.at / 44_100.0 - 0.2).rem_euclid(0.5);
        let err = into.min(0.5 - into) * 1000.0;
        assert!(err < 3.0, "{err} ms off");
    }
}
