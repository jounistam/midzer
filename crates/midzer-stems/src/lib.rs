//! The desktop app's full transcription: HT-Demucs separates the recording
//! into drums, bass, other instruments and vocals; basic-pitch transcribes
//! the pitched stems; the drum stem gives kick, snare and hi-hat hits. The
//! cleaner sources then go through the same analysis as the browser's
//! (`midzer-analyze`): chords from the stems without drums and vocals, the
//! melody from the voice, and the parts as tracks for MIDI export.
//!
//! The models (about 166 MB) are downloaded on first use; see [`models`].

// DSP loops index several parallel arrays (bins, keys, frames) at once.
#![allow(clippy::needless_range_loop)]

pub mod basic_pitch;
pub mod demucs;
pub mod drums;
pub mod models;
mod wav;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use midzer_analyze::{Analysis, Extras, Progress, RATE, Step, TimedTrack, analyze, decode, resample};
use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;

/// The error of a cancelled run.
pub const CANCELLED: &str = "Cancelled.";

static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();

fn session(model: &Path) -> Result<Session, String> {
    let env = ENVIRONMENT.get_or_init(|| ort::init().with_name("midzer").build().map_err(|e| e.to_string())).as_ref()?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let build = || -> ort::Result<Session> {
        Session::builder(env)?
            // Optimizing the Demucs graph (its fp16 weights) takes over 6 GB; unoptimized
            // it peaks near 1 GB and runs about as fast.
            .with_optimization_level(GraphOptimizationLevel::Disable)?
            .with_intra_threads(threads)?
            // Without the arena and memory patterns, memory stays flat across segments.
            .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(false).build()])?
            .with_memory_pattern(false)?
            .commit_from_file(model)
    };
    build().map_err(|e| format!("Could not load {}: {e}", model.display()))
}

pub struct Options<'a> {
    /// Where the models are kept (and downloaded to).
    pub models: &'a Path,
    /// Where to write the stems as WAV files, if anywhere.
    pub stems_out: Option<PathBuf>,
    /// Set to stop at the next segment.
    pub cancel: &'a AtomicBool,
}

/// GM programs for the transcribed parts.
const BASS_PROGRAM: u8 = 33;
const OTHER_PROGRAM: u8 = 0;
const VOICE_PROGRAM: u8 = 53;

/// Separates, transcribes and analyses an audio file.
pub fn transcribe_file(bytes: Vec<u8>, options: &Options, progress: Progress) -> Result<Analysis, String> {
    // Shares of the whole job: downloads, decoding, stems, notes, analysis.
    let missing = models::missing_bytes(options.models).max(1) as f32;
    let mut fetched = 0u64;
    for model in models::ALL {
        let base = fetched;
        let path = models::ensure(options.models, model, &mut |done| progress(0.1 * (base + done) as f32 / missing, Step::Decoding))?;
        fetched += std::fs::metadata(path).map_or(0, |m| m.len());
    }
    progress(0.1, Step::Decoding);
    let decoded = decode(bytes)?;
    let left = resample(&decoded.left, decoded.sample_rate, demucs::RATE);
    let right = resample(&decoded.right, decoded.sample_rate, demucs::RATE);
    progress(0.12, Step::Stems);

    let mut separator = session(&options.models.join(models::DEMUCS.file))?;
    let go = || !options.cancel.load(Ordering::Relaxed);
    let stems = demucs::separate(&mut separator, &left, &right, &mut |f| {
        progress(0.12 + 0.6 * f, Step::Stems);
        go()
    })?;
    drop(separator);
    if let Some(dir) = &options.stems_out {
        // Kept for practice: e.g. play along with everything but the vocals.
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        for (i, name) in demucs::STEMS.iter().enumerate() {
            wav::write(&dir.join(format!("{name}.wav")), demucs::RATE, &stems.stems[i]).map_err(|e| format!("Could not write the {name} stem: {e}"))?;
        }
    }
    let at_rate = |i: usize| resample(&stems.mono(i), demucs::RATE, RATE);
    let (drums, bass, other, vocals) = (at_rate(0), at_rate(1), at_rate(2), at_rate(3));
    drop(stems);
    let mix = resample(&decoded.mono(), decoded.sample_rate, RATE);
    let harmony: Vec<f32> = bass.iter().zip(&other).map(|(b, o)| b + o).collect();

    progress(0.72, Step::Notes);
    let mut pitch = session(&options.models.join(models::BASIC_PITCH.file))?;
    use basic_pitch::{Range, transcribe};
    let bass_notes = transcribe(&mut pitch, &bass, Range { low: 28, high: 67 }, &mut |f| {
        progress(0.72 + 0.06 * f, Step::Notes);
        go()
    })?;
    let other_notes = transcribe(&mut pitch, &other, Range { low: 36, high: 96 }, &mut |f| {
        progress(0.78 + 0.06 * f, Step::Notes);
        go()
    })?;
    let voice_notes = transcribe(&mut pitch, &vocals, Range { low: 45, high: 84 }, &mut |f| {
        progress(0.84 + 0.06 * f, Step::Notes);
        go()
    })?;
    let drum_hits = drums::hits(&drums);
    let melody = basic_pitch::top_line(&voice_notes);
    let has_voice = melody.len() >= 16;

    let mut tracks = vec![
        TimedTrack { name: "Drums".into(), program: 0, drums: true, notes: drum_hits },
        TimedTrack { name: "Bass".into(), program: BASS_PROGRAM, drums: false, notes: basic_pitch::top_line(&bass_notes) },
        TimedTrack { name: "Keys & guitars".into(), program: OTHER_PROGRAM, drums: false, notes: other_notes },
    ];
    if has_voice {
        tracks.push(TimedTrack { name: "Vocal melody".into(), program: VOICE_PROGRAM, drums: false, notes: melody.clone() });
    }
    let extras = Extras { harmony: Some(harmony), melody: has_voice.then(|| (melody, "vocals".to_string())), tracks };
    let mut scaled = |f: f32, step: Step| progress(0.9 + 0.1 * f, step);
    let mut analysis = analyze(&mix, extras, &mut scaled)?;
    analysis.stems_dir = options.stems_out.as_ref().map(|d| d.display().to_string());
    analysis.title = decoded.title.clone();
    analysis.artist = decoded.artist.clone();
    if decoded.truncated {
        analysis.warnings.push(format!("Only the first {} minutes were read.", midzer_analyze::decode::MAX_SECONDS / 60));
    }
    Ok(analysis)
}

/// A song's recording to play along with, from its saved stems at `rate`:
/// everything but the drums, and the drum stem apart so it can be muted.
pub fn backing_from_stems(dir: &Path, rate: u32) -> Result<midzer_analyze::Track, String> {
    let read = |name: &str| -> Result<(Vec<f32>, Vec<f32>), String> {
        let path = dir.join(format!("{name}.wav"));
        let bytes = std::fs::read(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        midzer_analyze::track::decode_at(bytes, rate)
    };
    let (drum_l, drum_r) = read(demucs::STEMS[0])?;
    let len = drum_l.len();
    let (mut left, mut right) = (vec![0.0f32; len], vec![0.0f32; len]);
    for name in &demucs::STEMS[1..] {
        let (l, r) = read(name)?;
        for (out, x) in left.iter_mut().zip(&l).chain(right.iter_mut().zip(&r)) {
            *out += x;
        }
    }
    Ok(midzer_analyze::Track {
        main: midzer_analyze::track::interleave(&left, &right, len),
        drums: Some(midzer_analyze::track::interleave(&drum_l, &drum_r, len)),
    })
}
