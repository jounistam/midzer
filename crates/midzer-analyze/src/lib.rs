//! Offline analysis of a recording into a song for the band: the beat grid
//! and tempo, the key, a chord per beat, bars and sections, and a melody.
//!
//! The beats, the 1s and the chords come from the networks of `midzer-hear`
//! run over the whole song (plain DSP when they find no steady beat); the
//! key, tuning, sections and melody are plain DSP. It all runs the same in
//! the browser (as wasm in a worker, see `ffi`) and in the desktop app. The
//! desktop app's stem separation (`midzer-stems`) feeds cleaner sources into
//! [`analyze`]: the harmony without drums and vocals, and transcribed notes.

// DSP loops index several parallel arrays (bins, keys, frames) at once.
#![allow(clippy::needless_range_loop)]

pub mod beats;
pub mod chords;
pub mod chroma;
pub mod decode;
pub mod melody;
pub mod resample;
pub mod stft;
pub mod structure;
pub mod track;
#[cfg(target_arch = "wasm32")]
mod ffi;

use midzer_core::Chord;
use serde::Serialize;

pub use decode::{Decoded, decode};
pub use melody::TimedNote;
pub use resample::resample;
pub use midzer_core::Track;
pub use track::backing_track;

/// The sample rate everything is analysed at.
pub const RATE: u32 = 22_050;
/// The level envelope's frames a second.
pub const ENVELOPE_FPS: usize = 100;
/// Mirrors `MAX_SONG_BARS` in the engine.
pub const MAX_BARS: usize = 512;
const MAX_MELODY_NOTES: usize = 8192;
const MIN_BEATS: usize = 16;
/// The longest bar, in beats (`MAX_BAR_BEATS` in midzer-core's song.rs).
pub const MAX_BAR_BEATS: usize = 7;
/// The share of [`analyze`]'s progress done after the beats and the chords.
const BEATS_DONE: f32 = 0.35;
const CHORDS_DONE: f32 = 0.6;

/// What the analysis is doing, for progress reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Step {
    Decoding = 0,
    Beats = 1,
    Chords = 2,
    Sections = 3,
    Melody = 4,
    Stems = 5,
    Notes = 6,
}

/// Progress: the share of the whole job done (0..1) and the current step.
pub type Progress<'a> = &'a mut dyn FnMut(f32, Step);

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChordOut {
    pub root: u8,
    /// Index of the engine's chord quality (`Quality` order).
    pub quality: u8,
    pub bass: u8,
}

impl From<Chord> for ChordOut {
    fn from(c: Chord) -> Self {
        Self { root: c.root, quality: c.quality as u8, bass: c.bass }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SectionOut {
    pub bar: usize,
    /// "intro", "A", "B", "C" or "ending".
    pub part: &'static str,
}

/// A note in beats from the first downbeat.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteOut {
    pub at: f32,
    pub len: f32,
    pub key: u8,
    pub velocity: u8,
}

/// A transcribed part, for MIDI export.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrackOut {
    pub name: String,
    /// General MIDI program (0-based); ignored for drums.
    pub program: u8,
    pub drums: bool,
    pub notes: Vec<NoteOut>,
}

/// The same, in seconds, as transcription produces it.
#[derive(Clone, Debug)]
pub struct TimedTrack {
    pub name: String,
    pub program: u8,
    pub drums: bool,
    pub notes: Vec<TimedNote>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KeyOut {
    pub tonic: u8,
    pub minor: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Length of the recording in seconds.
    pub duration: f32,
    pub bpm: f32,
    pub key: KeyOut,
    /// How far the recording is from A = 440 Hz.
    pub tuning_cents: f32,
    /// Time of each bar's downbeat in the recording, in seconds.
    pub bar_times: Vec<f32>,
    /// How many beats each bar has (1 to [`MAX_BAR_BEATS`], mostly 4).
    pub bar_beats: Vec<u8>,
    /// The tempo map: the time of every beat of the bars, plus the end of
    /// the last bar, in seconds. Beats past the tracked ones keep the usual tempo.
    pub beat_times: Vec<f32>,
    /// One chord per beat of every bar; `None` is no chord.
    pub chords: Vec<Option<ChordOut>>,
    pub sections: Vec<SectionOut>,
    /// The tune, quantized to sixteenths, for Learn.
    pub melody: Vec<NoteOut>,
    /// "mix" when guessed from the full recording, "vocals" from the separated voice.
    pub melody_from: Option<String>,
    /// Transcribed parts on the bar grid; only with stem separation.
    pub tracks: Vec<TrackOut>,
    /// Where the separated stems were saved; only with stem separation.
    pub stems_dir: Option<String>,
    /// Things the user should know about the result.
    pub warnings: Vec<String>,
    /// The level, [`ENVELOPE_FPS`] a second, from −60 dBFS (0) to 0 dBFS (255):
    /// enough to draw the song without its audio.
    pub envelope: Vec<u8>,
    /// Landmarks to recognise the song by (`midzer_hear::fingerprint`), as hash, frame pairs.
    pub fingerprint: Vec<u32>,
}

impl Analysis {
    /// For each of `beat_times`, whether a bar starts on it.
    pub fn downbeats(&self) -> Vec<bool> {
        let mut out = vec![false; self.beat_times.len()];
        let mut at = 0;
        for &n in &self.bar_beats {
            if let Some(d) = out.get_mut(at) {
                *d = true;
            }
            at += n as usize;
        }
        out
    }
}

/// Better sources than the mix, from stem separation.
#[derive(Default)]
pub struct Extras {
    /// The pitched instruments without drums and vocals, at [`RATE`].
    pub harmony: Option<Vec<f32>>,
    /// The melody already transcribed, and where it came from.
    pub melody: Option<(Vec<TimedNote>, String)>,
    pub tracks: Vec<TimedTrack>,
}

/// Decodes an audio file and analyses it.
pub fn analyze_file(bytes: Vec<u8>, progress: Progress) -> Result<Analysis, String> {
    progress(0.0, Step::Decoding);
    let decoded = decode(bytes)?;
    progress(0.15, Step::Decoding);
    let mix = resample(&decoded.mono(), decoded.sample_rate, RATE);
    let mut scaled = |f: f32, step: Step| progress(0.2 + 0.8 * f, step);
    let mut analysis = analyze(&mix, Extras::default(), &mut scaled)?;
    analysis.title = decoded.title.clone();
    analysis.artist = decoded.artist.clone();
    if decoded.truncated {
        analysis.warnings.push(format!("Only the first {} minutes were read.", decode::MAX_SECONDS / 60));
    }
    Ok(analysis)
}

/// Analyses a song heard live: 16-bit mono at [`RATE`], with no file to read a title from.
pub fn analyze_heard(samples: &[i16], progress: Progress) -> Result<Analysis, String> {
    let mix: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    analyze(&mix, Extras::default(), progress)
}

/// Analyses a mono mix at [`RATE`].
pub fn analyze(mix: &[f32], extras: Extras, progress: Progress) -> Result<Analysis, String> {
    let mut warnings = Vec::new();
    progress(0.0, Step::Beats);
    // The networks hear the beats and the 1s; plain DSP when they find too few.
    let heard = midzer_hear::hear_beats(mix, &mut |f| progress(BEATS_DONE * f, Step::Beats)).unwrap_or_else(|e| {
        eprintln!("midzer: the beat network did not run: {e}");
        Default::default()
    });
    let neural = heard.0.len() >= MIN_BEATS;
    let (beat_times, dsp) = if neural {
        (heard.0.iter().map(|&t| t as f32).collect::<Vec<f32>>(), None)
    } else {
        let onsets = beats::onsets(mix);
        let period = beats::tempo_period(&onsets.all);
        let beat_frames = beats::track(&onsets.all, period);
        if beat_frames.len() < MIN_BEATS {
            return Err("No steady beat was found in this recording.".into());
        }
        let fps = beats::frames_per_second();
        (beat_frames.iter().map(|&f| f as f32 / fps).collect(), Some((onsets, beat_frames)))
    };
    let mut intervals: Vec<f32> = beat_times.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_by(f32::total_cmp);
    let beat_seconds = intervals[intervals.len() / 2];
    let spread = intervals[intervals.len() * 9 / 10] / intervals[intervals.len() / 10];
    if spread > 1.15 {
        warnings.push("The tempo changes during the recording: the band plays at its usual tempo, the recording keeps its own.".into());
    }
    progress(BEATS_DONE, Step::Chords);

    let harmony = extras.harmony.as_deref().unwrap_or(mix);
    let frames = chroma::frame_peaks(harmony);
    let tuning = chroma::tuning(&frames);
    let per_beat = chroma::per_beat(&frames, tuning, &beat_times);
    drop(frames);
    let key = chords::key(&per_beat);
    let (lines, mut beat_chords) = match dsp {
        None => {
            let (beats, downbeats) = &heard;
            let lines = bar_lines(beats, downbeats);
            let spans = midzer_hear::hear_chords(harmony, &mut |f| progress(BEATS_DONE + (CHORDS_DONE - BEATS_DONE) * f, Step::Chords))?;
            let chords = beats.iter().enumerate().map(|(i, &t)| midzer_hear::chord_over(&spans, t, beats.get(i + 1).copied().unwrap_or(t + beat_seconds as f64)).flatten()).collect();
            (lines, chords)
        }
        Some((onsets, beat_frames)) => {
            let first_pass = chords::decode(&per_beat, key, chords::Meter::Unknown);
            let strength = |env: &[f32]| -> Vec<f32> {
                beat_frames.iter().map(|&f| env[f.saturating_sub(2)..(f + 3).min(env.len())].iter().fold(0.0f32, |m, &v| m.max(v))).collect()
            };
            let phase = structure::downbeat_phase(&first_pass, &strength(&onsets.low), &strength(&onsets.all));
            let chords = chords::decode(&per_beat, key, chords::Meter::Bars { phase });
            ((phase..beat_times.len()).step_by(4).collect(), chords)
        }
    };
    chords::hold_short_gaps(&mut beat_chords, 8);
    chords::with_bass(&mut beat_chords, &per_beat);
    progress(CHORDS_DONE, Step::Sections);

    // Bars start at the first downbeat; beats before it are a pickup and left out.
    let mut starts = bar_starts(&lines, beat_times.len());
    if starts.len() > MAX_BARS + 1 {
        starts.truncate(MAX_BARS + 1);
        warnings.push(format!("Only the first {MAX_BARS} bars were kept."));
    }
    if starts.len() > 8 && starts.windows(2).filter(|w| w[1] - w[0] == 3).count() * 2 > starts.len() {
        warnings.push("The song sounds like it has three beats to the bar: the band plays its four-beat patterns cut short.".into());
    }
    let first = starts[0];
    let bar_count = starts.len() - 1;
    let bar_beats: Vec<u8> = starts.windows(2).map(|w| (w[1] - w[0]) as u8).collect();
    let beat_at = |i: usize| beat_times.get(i).copied().unwrap_or_else(|| beat_times[beat_times.len() - 1] + (i + 1 - beat_times.len()) as f32 * beat_seconds);
    let bar_times: Vec<f32> = starts[..bar_count].iter().map(|&i| beat_at(i)).collect();
    let beat_times: Vec<f32> = (first..=starts[bar_count]).map(beat_at).collect();
    let mut chord_slots = Vec::with_capacity(beat_times.len());
    let mut bars = Vec::with_capacity(bar_count);
    for w in starts.windows(2) {
        let chroma = |i: usize| per_beat.get(i).copied().unwrap_or_default();
        for i in w[0]..w[1] {
            let chord = beat_chords.get(i).copied().unwrap_or_else(|| beat_chords.last().copied().flatten());
            chord_slots.push(chord.map(ChordOut::from));
        }
        // Sections compare bars as four beats: other lengths are spread over four.
        let len = w[1] - w[0];
        bars.push(std::array::from_fn::<_, 4, _>(|j| chroma(w[0] + j * len / 4)));
    }
    let loudness: Vec<f32> = (0..bar_count)
        .map(|b| {
            let a = (bar_times[b] * RATE as f32) as usize;
            let z = ((beat_at(starts[b + 1])) * RATE as f32) as usize;
            let s = &mix[a.min(mix.len())..z.min(mix.len())];
            (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
        })
        .collect();
    let sections = structure::sections(&bars, &loudness).into_iter().map(|(bar, p)| SectionOut { bar, part: p.name() }).collect();
    progress(CHORDS_DONE + 0.1, Step::Melody);

    // Times in the recording to beats from the first downbeat.
    let to_beats = |t: f32| -> f32 {
        let i = beat_times.partition_point(|&b| b <= t);
        let pos = if i == 0 {
            (t - beat_times[0]) / beat_seconds
        } else if i >= beat_times.len() {
            (beat_times.len() - 1) as f32 + (t - beat_times[beat_times.len() - 1]) / beat_seconds
        } else {
            (i - 1) as f32 + (t - beat_times[i - 1]) / (beat_times[i] - beat_times[i - 1])
        };
        pos - first as f32
    };
    let total_beats = (starts[bar_count] - first) as f32;
    let (melody_notes, melody_from) = match extras.melody {
        Some((notes, from)) => (notes, from),
        None => (melody::track(mix, tuning), "mix".to_string()),
    };
    let melody = quantize_melody(&melody_notes, &to_beats, total_beats);
    let melody_from = (melody.len() >= 8).then_some(melody_from);
    let melody = if melody_from.is_some() { melody } else { Vec::new() };

    let tracks = extras
        .tracks
        .into_iter()
        .map(|t| TrackOut {
            notes: t
                .notes
                .iter()
                .filter_map(|n| {
                    let at = to_beats(n.start);
                    let end = to_beats(n.end);
                    (at >= -4.0 && at < total_beats).then(|| NoteOut {
                        at: (at * 96.0).round() / 96.0,
                        len: ((end - at) * 96.0).round().max(1.0) / 96.0,
                        key: n.key,
                        velocity: velocity(n.level),
                    })
                })
                .collect(),
            name: t.name,
            program: t.program,
            drums: t.drums,
        })
        .collect();
    let fingerprint = midzer_hear::fingerprint(mix).into_iter().flat_map(|(h, t)| [h, t]).collect();
    progress(1.0, Step::Melody);

    Ok(Analysis {
        title: None,
        artist: None,
        duration: mix.len() as f32 / RATE as f32,
        bpm: (60.0 / beat_seconds * 10.0).round() / 10.0,
        key: KeyOut { tonic: key.tonic, minor: key.minor },
        tuning_cents: (tuning * 100.0).round(),
        bar_times,
        bar_beats,
        beat_times,
        chords: chord_slots,
        sections,
        melody,
        melody_from,
        tracks,
        stems_dir: None,
        warnings,
        envelope: envelope(mix),
        fingerprint,
    })
}

/// The level of each 10 ms, in dB from −60 (0) to 0 (255).
fn envelope(mix: &[f32]) -> Vec<u8> {
    // 220.5 samples a frame: the bounds are rounded so they don't drift.
    let at = |k: usize| (k * RATE as usize / ENVELOPE_FPS).min(mix.len());
    (0..(mix.len() * ENVELOPE_FPS).div_ceil(RATE as usize))
        .map(|k| {
            let c = &mix[at(k)..at(k + 1)];
            let power = c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32;
            let db = 10.0 * power.max(1e-12).log10();
            ((db + 60.0) / 60.0 * 255.0).clamp(0.0, 255.0).round() as u8
        })
        .collect()
}

/// The beats the bars start on, from the network's downbeats.
///
/// The network's 1s are taken as they come, bars of other lengths too
/// (tuned on `files24`: its 1s score 0.91 F; one phase for the whole song
/// 0.64; checking each odd bar against the 1s after it 0.87). Only a 1 a
/// single beat after the last is passed over, and a gap of two bars or more
/// (1s missed) is filled with bars as long as most (4, or 3 for a waltz): 0.92. Fewer than four 1s heard: bars of four from the
/// beats' phase the 1s agree on, or from the first beat.
fn bar_lines(beats: &[f64], downbeats: &[f64]) -> Vec<usize> {
    let mut index: Vec<usize> = downbeats
        .iter()
        .filter_map(|&d| {
            let i = beats.partition_point(|&b| b < d);
            [i.checked_sub(1), Some(i)].into_iter().flatten().filter(|&k| k < beats.len()).min_by(|&a, &b| (beats[a] - d).abs().total_cmp(&(beats[b] - d).abs()))
        })
        .collect();
    index.dedup();
    if index.len() < 4 {
        let mut votes = [0usize; 4];
        for &i in &index {
            votes[i % 4] += 1;
        }
        let phase = (0..4).max_by_key(|&p| (votes[p], std::cmp::Reverse(p))).unwrap_or(0);
        return (phase..beats.len()).step_by(4).collect();
    }
    let gaps: Vec<usize> = index.windows(2).map(|w| w[1] - w[0]).collect();
    let meter = if gaps.len() >= 4 && gaps.iter().filter(|&&g| g == 3).count() * 2 > gaps.len() { 3 } else { 4 };
    let mut lines = vec![index[0]];
    let mut pos = index[0];
    for &d in &index[1..] {
        while d - pos >= 2 * meter || d - pos > MAX_BAR_BEATS {
            pos += meter;
            lines.push(pos);
        }
        if d - pos > 1 {
            lines.push(d);
            pos = d;
        }
    }
    lines
}

/// Where each bar starts, for bar lines `lines` among `beats` beats: from
/// the first line on, the bars after the last one are as long as most bars
/// until the beats run out (the last bar may run past them). Ends with the
/// end of the last bar.
fn bar_starts(lines: &[usize], beats: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = lines.iter().copied().filter(|&l| l < beats).collect();
    if starts.is_empty() {
        starts.push(0);
    }
    let mut counts = [0usize; MAX_BAR_BEATS + 1];
    for w in starts.windows(2) {
        counts[(w[1] - w[0]).min(MAX_BAR_BEATS)] += 1;
    }
    let len = (2..=MAX_BAR_BEATS).max_by_key(|&n| (counts[n], n == 4)).filter(|&n| counts[n] > 0).unwrap_or(4);
    while *starts.last().unwrap() < beats {
        let next = starts.last().unwrap() + len;
        starts.push(next);
    }
    starts
}

fn velocity(level: f32) -> u8 {
    (40.0 + 80.0 * level.clamp(0.0, 1.0)).round() as u8
}

/// One line of notes on sixteenths: later notes cut earlier ones short.
fn quantize_melody(notes: &[TimedNote], to_beats: &dyn Fn(f32) -> f32, total_beats: f32) -> Vec<NoteOut> {
    let q = |b: f32| (b * 4.0).round() / 4.0;
    let mut sorted: Vec<&TimedNote> = notes.iter().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out: Vec<NoteOut> = Vec::new();
    for n in sorted {
        let at = q(to_beats(n.start));
        if at < 0.0 || at >= total_beats || out.len() >= MAX_MELODY_NOTES {
            continue;
        }
        let len = q(to_beats(n.end) - to_beats(n.start)).max(0.25);
        if let Some(prev) = out.last_mut() {
            if prev.at == at {
                // Two notes on one sixteenth: keep the louder.
                if velocity(n.level) > prev.velocity {
                    *prev = NoteOut { at, len, key: n.key, velocity: velocity(n.level) };
                }
                continue;
            }
            if prev.at + prev.len > at {
                prev.len = at - prev.at;
            }
        }
        out.push(NoteOut { at, len, key: n.key, velocity: velocity(n.level) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(beats: &[f64], index: &[usize]) -> Vec<f64> {
        index.iter().map(|&i| beats[i] + 0.01).collect()
    }

    #[test]
    fn bars_start_on_the_network_s_downbeats() {
        let beats: Vec<f64> = (0..48).map(|i| i as f64 * 0.5).collect();
        // The 1s on 2, 6, 10, then a 2-beat bar: 12, 16, then bars of 5 and 3.
        let lines = [2, 6, 10, 12, 16, 21, 24, 28, 32, 36, 40, 44];
        assert_eq!(bar_lines(&beats, &at(&beats, &lines)), lines);
        // A 1 a beat after the last is passed over; missed ones are filled in.
        let heard = [2, 6, 7, 10, 12, 16, 21, 24, 36, 40, 44];
        assert_eq!(bar_lines(&beats, &at(&beats, &heard)), lines);
        // A waltz is filled in threes.
        let waltz: Vec<usize> = (0..15).map(|b| 1 + 3 * b).filter(|&b| b != 22 && b != 25).collect();
        assert_eq!(bar_lines(&beats, &at(&beats, &waltz)), (0..15).map(|b| 1 + 3 * b).collect::<Vec<_>>());
        // Too few 1s: fours from their phase.
        assert_eq!(bar_lines(&beats, &at(&beats, &[3, 7]))[..3], [3, 7, 11]);
        assert_eq!(bar_lines(&beats, &[])[..2], [0, 4]);
    }

    #[test]
    fn bars_go_on_in_the_last_length_past_the_last_line() {
        assert_eq!(bar_starts(&[2, 6, 9, 13], 20), [2, 6, 9, 13, 17, 21]);
        assert_eq!(bar_starts(&[0, 3, 6, 9], 11), [0, 3, 6, 9, 12]);
        assert_eq!(bar_starts(&[1], 9), [1, 5, 9]);
        assert_eq!(bar_starts(&[], 3), [0, 4]);
    }

    #[test]
    fn the_envelope_spans_silence_to_full_scale() {
        // Frames of 220, 221, 220… samples: silence, full scale, −40 dB.
        let mut mix = vec![0.0f32; 220];
        mix.extend((0..221).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }));
        mix.extend(vec![0.01f32; 100]);
        assert_eq!(envelope(&mix), vec![0, 255, 85]);
    }
}
