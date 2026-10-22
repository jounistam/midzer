//! Listening to a microphone or instrument: single notes (YIN pitch), strummed
//! chords (chroma matched against chord templates) and attacks (onsets).
//!
//! The input is low-passed and decimated to about 22–24 kHz, then analysed
//! every hop of [`HOP`] samples. Every buffer is allocated in
//! [`Listener::new`]; [`Listener::push`] never allocates.

use crate::chord::{Chord, Quality};
use crate::fft::Fft;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListenMode {
    #[default]
    Off,
    /// One note at a time: voice, whistling, a single-note guitar line.
    Notes,
    /// Strummed or picked chords, e.g. a guitar.
    Chords,
}

impl ListenMode {
    pub const ALL: [ListenMode; 3] = [ListenMode::Off, ListenMode::Notes, ListenMode::Chords];

    /// Roughly how long after a sound starts the listener reports it.
    pub fn delay_ms(self) -> f32 {
        match self {
            ListenMode::Off => 0.0,
            ListenMode::Notes => 45.0,
            ListenMode::Chords => 120.0,
        }
    }
}

/// Something the listener heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Heard {
    /// Notes mode: a note started (or was sung again), with a velocity from its loudness.
    NoteOn { key: u8, velocity: u8 },
    NoteOff { key: u8 },
    /// Chords mode: a chord rings, new or strummed again.
    Chord { chord: Chord, velocity: u8 },
    /// Chords mode: the chord died away.
    Silence,
    /// Chords mode: a new strum or pick.
    Onset { velocity: u8 },
    /// For meters, a few times a second: loudness 0..=255 over -80..0 dBFS,
    /// and the note sounding in Notes mode.
    Level { level: u8, key: Option<u8> },
}

/// Most [`Heard`] items one [`Listener::push`] of up to 128 frames can give.
pub const MAX_HEARD: usize = 16;

/// Fixed-capacity list of what one push heard.
pub struct HeardList {
    items: [Heard; MAX_HEARD],
    len: usize,
}

impl Default for HeardList {
    fn default() -> Self {
        Self { items: [Heard::Silence; MAX_HEARD], len: 0 }
    }
}

impl HeardList {
    fn push(&mut self, heard: Heard) {
        if self.len < MAX_HEARD {
            self.items[self.len] = heard;
            self.len += 1;
        }
    }

    pub fn as_slice(&self) -> &[Heard] {
        &self.items[..self.len]
    }
}

/// Decimated samples kept; also the chroma FFT size.
const RING: usize = 4096;
/// Analysis step in decimated samples (about 11 ms).
pub const HOP: usize = 256;
const LEVEL_WINDOW: usize = 2 * HOP;
const LEVEL_EVERY_HOPS: u32 = 4;
/// YIN integration window and pitch range.
const YIN_WINDOW: usize = 640;
const YIN_THRESHOLD: f32 = 0.15;
const MIN_HZ: f32 = 60.0;
const MAX_HZ: f32 = 1500.0;
const MAX_TAU: usize = 420;
/// Chroma uses spectral peaks in this range.
const CHROMA_LO_HZ: f32 = 70.0;
const CHROMA_HI_HZ: f32 = 2200.0;
/// How far above the decaying envelope a level must jump to count as an attack.
const ONSET_RISE_DB: f32 = 4.5;
const ENVELOPE_DECAY_DB: f32 = 0.6;
const ONSET_REFRACTORY_HOPS: u32 = 4;
const SILENCE_FLOOR_DB: f32 = -100.0;
/// Chords must match a template this well (cosine) and win this many analyses in a row.
const CHORD_MIN_SCORE: f32 = 0.6;
const CHORD_STABLE: u32 = 3;
/// Hops below the gate before a chord counts as gone.
const CHORD_RELEASE_HOPS: u32 = 15;
const NOTE_STABLE: u32 = 2;
const NOTE_RELEASE_HOPS: u32 = 3;
/// A held note only changes when the pitch moves this many semitones away.
const NOTE_HYSTERESIS: f32 = 0.7;

/// Chords the listener knows, with a handicap so plainer chords win near-ties:
/// overtones of a triad easily suggest a seventh.
const CHORD_QUALITIES: [(Quality, f32); 9] = [
    (Quality::Major, 0.0),
    (Quality::Minor, 0.0),
    (Quality::Power, 0.02),
    (Quality::Sus4, 0.03),
    (Quality::Sus2, 0.03),
    (Quality::Dominant7, 0.03),
    (Quality::Minor7, 0.03),
    (Quality::Major7, 0.04),
    (Quality::Diminished, 0.04),
];
/// Hops after an attack before its chroma is trusted.
const CHORD_SETTLE_HOPS: u32 = 6;

/// Second-order low-pass (RBJ cookbook), run twice before decimating.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn low_pass(sample_rate: f32, cutoff: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let cos = w.cos();
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            ..Self::default()
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        // Flush denormals so silence stays cheap.
        self.y1 = if y.abs() < 1e-20 { 0.0 } else { y };
        self.y1
    }
}

pub struct Listener {
    mode: ListenMode,
    /// Sounds quieter than this (dBFS) are ignored.
    gate_db: f32,
    /// Decimated sample rate.
    rate: f32,
    decimation: usize,
    phase: usize,
    filters: [Biquad; 2],
    ring: Box<[f32]>,
    /// Total decimated samples written.
    written: usize,
    since_hop: usize,
    hops: u32,
    /// Scratch: the latest samples in order, then the FFT's buffers.
    linear: Box<[f32]>,
    re: Box<[f32]>,
    im: Box<[f32]>,
    window: Box<[f32]>,
    diff: Box<[f32]>,
    fft: Fft,
    /// Decaying peak level over the last three hops, oldest first.
    envelope: [f32; 3],
    hops_since_onset: u32,
    peak_db: f32,
    // Notes mode.
    note: Option<u8>,
    candidate: Option<u8>,
    candidate_hops: u32,
    unvoiced_hops: u32,
    // Chords mode.
    chord: Option<Chord>,
    chord_candidate: Option<Chord>,
    chord_candidate_hops: u32,
    /// A strum since the last chord report: the chord is reported again even if unchanged.
    restruck: bool,
    quiet_hops: u32,
}

impl Listener {
    pub fn new(sample_rate: f32) -> Self {
        let decimation = ((sample_rate / 22_050.0).round() as usize).max(1);
        let rate = sample_rate / decimation as f32;
        let cutoff = (rate * 0.2).min(sample_rate * 0.45);
        let filter = Biquad::low_pass(sample_rate, cutoff);
        let hann = (0..RING).map(|i| {
            let x = std::f32::consts::PI * 2.0 * i as f32 / RING as f32;
            0.5 - 0.5 * x.cos()
        });
        Self {
            mode: ListenMode::Off,
            gate_db: -50.0,
            rate,
            decimation,
            phase: 0,
            filters: [filter; 2],
            ring: vec![0.0; RING].into_boxed_slice(),
            written: 0,
            since_hop: 0,
            hops: 0,
            linear: vec![0.0; RING].into_boxed_slice(),
            re: vec![0.0; RING].into_boxed_slice(),
            im: vec![0.0; RING].into_boxed_slice(),
            window: hann.collect(),
            diff: vec![0.0; MAX_TAU + 2].into_boxed_slice(),
            fft: Fft::new(RING),
            envelope: [SILENCE_FLOOR_DB; 3],
            hops_since_onset: u32::MAX / 2,
            peak_db: SILENCE_FLOOR_DB,
            note: None,
            candidate: None,
            candidate_hops: 0,
            unvoiced_hops: 0,
            chord: None,
            chord_candidate: None,
            chord_candidate_hops: 0,
            restruck: false,
            quiet_hops: 0,
        }
    }

    pub fn mode(&self) -> ListenMode {
        self.mode
    }

    /// Switches mode; anything sounding is released into `out`.
    pub fn set_mode(&mut self, mode: ListenMode, out: &mut HeardList) {
        if mode == self.mode {
            return;
        }
        self.release(out);
        self.mode = mode;
        self.envelope = [SILENCE_FLOOR_DB; 3];
    }

    pub fn set_gate(&mut self, gate_db: f32) {
        self.gate_db = gate_db.clamp(-90.0, -10.0);
    }

    /// Whether a note or chord is ringing.
    pub fn sounding(&self) -> bool {
        self.note.is_some() || self.chord.is_some()
    }

    fn release(&mut self, out: &mut HeardList) {
        if let Some(key) = self.note.take() {
            out.push(Heard::NoteOff { key });
        }
        if self.chord.take().is_some() {
            out.push(Heard::Silence);
        }
        self.candidate = None;
        self.chord_candidate = None;
    }

    /// Feeds mono input frames at the engine's sample rate.
    pub fn push(&mut self, input: &[f32], out: &mut HeardList) {
        if self.mode == ListenMode::Off {
            return;
        }
        for &x in input {
            let low = self.filters[0].run(x);
            let y = self.filters[1].run(low);
            self.phase += 1;
            if self.phase < self.decimation {
                continue;
            }
            self.phase = 0;
            self.ring[self.written % RING] = y;
            self.written += 1;
            self.since_hop += 1;
            if self.since_hop == HOP {
                self.since_hop = 0;
                self.hop(out);
            }
        }
    }

    /// Copies the latest `len` samples, oldest first, into `self.linear`.
    fn latest(&mut self, len: usize) {
        let start = self.written.wrapping_sub(len);
        for i in 0..len {
            self.linear[i] = self.ring[start.wrapping_add(i) % RING];
        }
    }

    fn hop(&mut self, out: &mut HeardList) {
        self.hops = self.hops.wrapping_add(1);
        self.hops_since_onset = self.hops_since_onset.saturating_add(1);

        self.latest(LEVEL_WINDOW);
        let power = self.linear[..LEVEL_WINDOW].iter().map(|x| x * x).sum::<f32>() / LEVEL_WINDOW as f32;
        let db = (10.0 * power.max(1e-12).log10()).max(SILENCE_FLOOR_DB);
        let audible = db > self.gate_db;
        // The level window spans two hops and an attack can straddle them, so
        // compare with the envelope from before all three.
        let [before, older, last] = self.envelope;
        let onset = audible && db > before + ONSET_RISE_DB && self.hops_since_onset >= ONSET_REFRACTORY_HOPS;
        self.envelope = [older, last, db.max(last - ENVELOPE_DECAY_DB)];
        if onset {
            self.hops_since_onset = 0;
            self.peak_db = db;
        } else {
            self.peak_db = self.peak_db.max(db);
        }

        match self.mode {
            ListenMode::Notes => self.track_note(db, audible, onset, out),
            ListenMode::Chords => {
                if onset {
                    self.restruck = true;
                    out.push(Heard::Onset { velocity: self.velocity(db) });
                }
                self.track_chord(db, audible, out);
            }
            ListenMode::Off => {}
        }

        if self.hops.is_multiple_of(LEVEL_EVERY_HOPS) {
            let level = ((db + 80.0) / 80.0 * 255.0).clamp(0.0, 255.0) as u8;
            out.push(Heard::Level { level, key: self.note });
        }
    }

    fn velocity(&self, db: f32) -> u8 {
        (30.0 + (db - self.gate_db) / 40.0 * 97.0).clamp(1.0, 127.0) as u8
    }

    fn track_note(&mut self, db: f32, audible: bool, onset: bool, out: &mut HeardList) {
        let pitch = if audible { self.pitch() } else { None };
        let Some(midi) = pitch else {
            self.candidate = None;
            self.unvoiced_hops += 1;
            if self.unvoiced_hops >= NOTE_RELEASE_HOPS
                && let Some(key) = self.note.take()
            {
                out.push(Heard::NoteOff { key });
            }
            return;
        };
        self.unvoiced_hops = 0;
        if let Some(key) = self.note
            && (midi - key as f32).abs() < NOTE_HYSTERESIS
        {
            self.candidate = None;
            if onset {
                out.push(Heard::NoteOff { key });
                out.push(Heard::NoteOn { key, velocity: self.velocity(db) });
            }
            return;
        }
        let key = midi.round().clamp(0.0, 127.0) as u8;
        if self.candidate == Some(key) {
            self.candidate_hops += 1;
        } else {
            self.candidate = Some(key);
            self.candidate_hops = 1;
        }
        if self.candidate_hops >= NOTE_STABLE {
            if let Some(old) = self.note {
                out.push(Heard::NoteOff { key: old });
            }
            self.note = Some(key);
            self.candidate = None;
            out.push(Heard::NoteOn { key, velocity: self.velocity(self.peak_db) });
        }
    }

    /// YIN: the fundamental of the latest window as a fractional MIDI note,
    /// or `None` when the sound has no clear pitch.
    fn pitch(&mut self) -> Option<f32> {
        let max_tau = ((self.rate / MIN_HZ) as usize).min(MAX_TAU);
        let min_tau = ((self.rate / MAX_HZ) as usize).max(2);
        self.latest(YIN_WINDOW + max_tau);
        let x = &self.linear;
        let d = &mut self.diff;
        d[0] = 1.0;
        let mut running = 0.0;
        for tau in 1..=max_tau {
            let mut sum = 0.0;
            for j in 0..YIN_WINDOW {
                let delta = x[j] - x[j + tau];
                sum += delta * delta;
            }
            running += sum;
            // Cumulative mean normalised difference.
            d[tau] = if running > 0.0 { sum * tau as f32 / running } else { 1.0 };
        }
        let mut tau = min_tau;
        while tau < max_tau && d[tau] >= YIN_THRESHOLD {
            tau += 1;
        }
        if tau >= max_tau {
            return None;
        }
        while tau + 1 < max_tau && d[tau + 1] < d[tau] {
            tau += 1;
        }
        let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
        let denom = a - 2.0 * b + c;
        let shift = if denom.abs() > 1e-9 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
        let hz = self.rate / (tau as f32 + shift);
        Some(69.0 + 12.0 * (hz / 440.0).log2())
    }

    fn track_chord(&mut self, db: f32, audible: bool, out: &mut HeardList) {
        if !audible {
            self.chord_candidate = None;
            self.quiet_hops += 1;
            if self.quiet_hops >= CHORD_RELEASE_HOPS && self.chord.take().is_some() {
                out.push(Heard::Silence);
            }
            return;
        }
        self.quiet_hops = 0;
        // Right after a strum the window still holds what came before.
        if !self.hops.is_multiple_of(2) || self.hops_since_onset < CHORD_SETTLE_HOPS {
            return;
        }
        let Some(chord) = self.chroma_chord() else {
            self.chord_candidate = None;
            return;
        };
        if self.chord_candidate == Some(chord) {
            self.chord_candidate_hops += 1;
        } else {
            self.chord_candidate = Some(chord);
            self.chord_candidate_hops = 1;
        }
        if self.chord_candidate_hops >= CHORD_STABLE && (self.chord != Some(chord) || self.restruck) {
            self.chord = Some(chord);
            self.restruck = false;
            out.push(Heard::Chord { chord, velocity: self.velocity(self.peak_db.max(db)) });
        }
    }

    /// The chord whose template best matches the latest window's chroma.
    fn chroma_chord(&mut self) -> Option<Chord> {
        self.latest(RING);
        for i in 0..RING {
            self.re[i] = self.linear[i] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);
        let bin_hz = self.rate / RING as f32;
        let lo = (CHROMA_LO_HZ / bin_hz) as usize;
        let hi = ((CHROMA_HI_HZ / bin_hz) as usize).min(RING / 2 - 2);
        for k in lo - 1..=hi + 1 {
            self.linear[k] = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt();
        }
        let mags = &self.linear;
        let loudest = mags[lo..=hi].iter().fold(0.0f32, |m, v| m.max(*v));
        if loudest <= 0.0 {
            return None;
        }
        let mut chroma = [0.0f32; 12];
        for k in lo..=hi {
            let (a, b, c) = (mags[k - 1], mags[k], mags[k + 1]);
            // Peaks only, and not the noise floor between them.
            if b <= a || b < c || b < loudest * 0.05 {
                continue;
            }
            let denom = a - 2.0 * b + c;
            let shift = if denom.abs() > 1e-12 { 0.5 * (a - c) / denom } else { 0.0 };
            let hz = (k as f32 + shift) * bin_hz;
            let midi = 69.0 + 12.0 * (hz / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            // Off-pitch peaks (inharmonic partials, noise) count less.
            let detune = (midi - midi.round()).abs();
            chroma[pc] += b.sqrt() * (1.0 - detune);
        }
        let norm = chroma.iter().map(|c| c * c).sum::<f32>().sqrt();
        if norm <= 0.0 {
            return None;
        }
        let mut best: Option<(f32, Chord)> = None;
        for root in 0..12u8 {
            for (quality, handicap) in CHORD_QUALITIES {
                let tones = quality.intervals();
                let dot: f32 = tones.iter().map(|i| chroma[((root + i) % 12) as usize]).sum();
                let score = dot / (norm * (tones.len() as f32).sqrt()) - handicap;
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, Chord::new(root, quality)));
                }
            }
        }
        best.filter(|(score, _)| *score >= CHORD_MIN_SCORE).map(|(_, chord)| chord)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn hz(key: f32) -> f32 {
        440.0 * 2f32.powf((key - 69.0) / 12.0)
    }

    /// A plucked-string-like tone: harmonics with a decay.
    fn tone(keys: &[f32], seconds: f32, amp: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SR;
                let env = amp * (-t * 1.5).exp();
                keys.iter()
                    .map(|&k| {
                        (1..=6)
                            .map(|h| (2.0 * std::f32::consts::PI * hz(k) * h as f32 * t).sin() / h as f32)
                            .sum::<f32>()
                    })
                    .sum::<f32>()
                    * env
                    / keys.len() as f32
            })
            .collect()
    }

    fn listen(mode: ListenMode, signal: &[f32]) -> Vec<Heard> {
        let mut listener = Listener::new(SR);
        let mut out = HeardList::default();
        listener.set_mode(mode, &mut out);
        let mut heard = Vec::new();
        for block in signal.chunks(128) {
            let mut out = HeardList::default();
            listener.push(block, &mut out);
            heard.extend(out.as_slice().iter().filter(|h| !matches!(h, Heard::Level { .. })));
        }
        heard
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * SR) as usize]
    }

    #[test]
    fn fft_finds_a_sine() {
        let fft = Fft::new(64);
        let mut re: Vec<f32> = (0..64).map(|i| (2.0 * std::f32::consts::PI * 5.0 * i as f32 / 64.0).cos()).collect();
        let mut im = vec![0.0; 64];
        fft.run(&mut re, &mut im);
        let mags: Vec<f32> = re.iter().zip(&im).map(|(r, i)| (r * r + i * i).sqrt()).collect();
        assert!((mags[5] - 32.0).abs() < 1e-3);
        assert!(mags[6] < 1e-3 && mags[0] < 1e-3);
    }

    #[test]
    fn hears_single_notes_across_the_range() {
        for key in [40.0, 45.0, 57.0, 64.0, 69.0, 76.0, 84.0] {
            let mut signal = tone(&[key], 0.5, 0.3);
            signal.extend(silence(0.3));
            let heard = listen(ListenMode::Notes, &signal);
            let ons: Vec<_> = heard.iter().filter_map(|h| if let Heard::NoteOn { key, .. } = h { Some(*key) } else { None }).collect();
            assert_eq!(ons, [key as u8], "key {key}: {heard:?}");
            assert_eq!(heard.last(), Some(&Heard::NoteOff { key: key as u8 }));
        }
    }

    #[test]
    fn follows_a_melody_and_ignores_quiet_noise() {
        let mut signal = silence(0.1);
        for key in [60.0, 62.0, 64.0, 60.0] {
            signal.extend(tone(&[key], 0.3, 0.3));
        }
        // Noise under the gate: nothing.
        let mut seed = 1u32;
        signal.extend((0..(0.3 * SR) as usize).map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 9) as f32 / (1 << 23) as f32 * 0.001 - 0.0005
        }));
        let heard = listen(ListenMode::Notes, &signal);
        let ons: Vec<_> = heard.iter().filter_map(|h| if let Heard::NoteOn { key, .. } = h { Some(*key) } else { None }).collect();
        assert_eq!(ons, [60, 62, 64, 60]);
        assert!(matches!(heard.last(), Some(Heard::NoteOff { key: 60 })));
    }

    #[test]
    fn a_louder_note_has_a_higher_velocity() {
        let velocity = |amp| {
            listen(ListenMode::Notes, &tone(&[60.0], 0.3, amp))
                .iter()
                .find_map(|h| if let Heard::NoteOn { velocity, .. } = h { Some(*velocity) } else { None })
                .unwrap()
        };
        assert!(velocity(0.5) > velocity(0.03));
    }

    fn chords_heard(signal: &[f32]) -> Vec<String> {
        listen(ListenMode::Chords, signal)
            .iter()
            .filter_map(|h| if let Heard::Chord { chord, .. } = h { Some(chord.to_string()) } else { None })
            .collect()
    }

    #[test]
    fn hears_strummed_chords() {
        // Guitar-like voicings: C, Am, F, G7, Em.
        let voicings: [(&[f32], &str); 5] = [
            (&[48.0, 52.0, 55.0, 60.0, 64.0], "C"),
            (&[45.0, 52.0, 57.0, 60.0, 64.0], "Am"),
            (&[41.0, 48.0, 53.0, 57.0, 60.0], "F"),
            (&[43.0, 47.0, 50.0, 55.0, 59.0, 65.0], "G7"),
            (&[40.0, 47.0, 52.0, 55.0, 59.0, 64.0], "Em"),
        ];
        let mut signal = Vec::new();
        for (keys, _) in voicings {
            signal.extend(tone(keys, 0.8, 0.4));
        }
        let names: Vec<_> = voicings.iter().map(|(_, n)| n.to_string()).collect();
        assert_eq!(chords_heard(&signal), names);
    }

    #[test]
    fn strums_are_onsets_and_silence_releases() {
        let mut signal = tone(&[48.0, 52.0, 55.0], 0.5, 0.4);
        signal.extend(tone(&[48.0, 52.0, 55.0], 0.5, 0.4));
        signal.extend(silence(0.5));
        let heard = listen(ListenMode::Chords, &signal);
        assert_eq!(heard.iter().filter(|h| matches!(h, Heard::Onset { .. })).count(), 2);
        assert_eq!(heard.iter().filter(|h| matches!(h, Heard::Chord { .. })).count(), 2, "each strum reports its chord");
        assert_eq!(heard.last(), Some(&Heard::Silence));
    }

    #[test]
    fn off_hears_nothing_and_switching_releases() {
        assert!(listen(ListenMode::Off, &tone(&[60.0], 0.3, 0.3)).is_empty());
        let mut listener = Listener::new(SR);
        let mut out = HeardList::default();
        listener.set_mode(ListenMode::Notes, &mut out);
        for block in tone(&[60.0], 0.2, 0.3).chunks(128) {
            listener.push(block, &mut out);
            out.len = 0;
        }
        assert!(listener.sounding());
        listener.set_mode(ListenMode::Off, &mut out);
        assert_eq!(out.as_slice(), [Heard::NoteOff { key: 60 }]);
    }

    #[test]
    fn works_at_44_1_khz() {
        let mut listener = Listener::new(44_100.0);
        let mut out = HeardList::default();
        listener.set_mode(ListenMode::Notes, &mut out);
        let signal: Vec<f32> = (0..22_050).map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44_100.0).sin() * 0.3).collect();
        let mut keys = Vec::new();
        for block in signal.chunks(128) {
            let mut out = HeardList::default();
            listener.push(block, &mut out);
            keys.extend(out.as_slice().iter().filter_map(|h| if let Heard::NoteOn { key, .. } = h { Some(*key) } else { None }));
        }
        assert_eq!(keys, [69]);
    }
}
