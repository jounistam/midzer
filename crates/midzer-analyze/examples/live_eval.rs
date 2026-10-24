//! Runs the live beat tracker over recordings as if they came from a tab, and
//! compares it with the offline analysis of the whole file (the reference):
//!   cargo run --release -p midzer-analyze --example live_eval -- a.mp3 b.mp3 …
//!
//! The analysis is cached in `$EVAL_CACHE` (default: the system's temp dir).
//! `BT_REF=<dir>` reads the reference from `<dir>/<name>.bt` instead ("time
//! is_downbeat" per line, e.g. Beat This! on the whole song). `HEAR=1` runs
//! the listener (`midzer-hear`) alongside, as the app's worker does, and
//! sends its guides to the engine once each run would be done: the audio it
//! heard plus its compute time here times `SLOW` (default 2, for wasm).
//! Per file: when it first locks, the tempo it ends on, the share of the
//! song's time it is locked at the reference tempo (or twice/half it), how
//! many beats shown fall on a reference beat (±70 ms), and how many of the
//! 1s shown are reference 1s (with which beat of the bar they were). The
//! "oracle" 1 is a second reference: the whole song's bass, snare and harmony
//! changes read with the same weights the live guess uses.

use midzer_analyze::{RATE, analyze_file, beats, chroma, decode, resample};
use midzer_core::{Command, Engine, EngineEvent};

const SR: u32 = 48_000;

struct Reference {
    bpm: f32,
    /// Beat times, and whether each is a 1: by the analysis, and by [`oracle`].
    beats: Vec<(f32, bool)>,
    oracle: Vec<(f32, bool)>,
}

/// A hand-made or neural reference (`$BT_REF/<name>.bt`: "time is_downbeat" per line), if given.
fn external_reference(path: &str) -> Option<Reference> {
    let dir = std::env::var("BT_REF").ok()?;
    let name = path.rsplit('/').next().unwrap_or(path);
    let text = std::fs::read_to_string(format!("{dir}/{name}.bt")).ok()?;
    let beats: Vec<(f32, bool)> = text.lines().filter_map(|l| l.split_once(' ')).map(|(t, d)| (t.parse().unwrap(), d == "1")).collect();
    let mut iv: Vec<f32> = beats.windows(2).map(|w| w[1].0 - w[0].0).collect();
    iv.sort_by(f32::total_cmp);
    Some(Reference { bpm: 60.0 / iv[iv.len() / 2], oracle: beats.clone(), beats })
}

fn reference(path: &str, bytes: &[u8]) -> Reference {
    if let Some(r) = external_reference(path) {
        return r;
    }
    let dir = std::env::var("EVAL_CACHE").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    let name = path.rsplit('/').next().unwrap_or(path);
    let cache = format!("{dir}/{name}.ref");
    let text = std::fs::read_to_string(&cache).unwrap_or_else(|_| {
        let a = analyze_file(bytes.to_vec(), &mut |_, _| {}).expect("analysis");
        let phase = oracle(bytes, &a.beat_times);
        let mut text = format!("{}\n", a.bpm);
        let ones = a.downbeats();
        for (i, t) in a.beat_times.iter().enumerate() {
            text += &format!("{t} {}\n", ones[i] as u8 + 2 * (i % 4 == phase) as u8);
        }
        std::fs::write(&cache, &text).expect("write the cache");
        text
    });
    let mut lines = text.lines();
    let bpm = lines.next().unwrap().parse().unwrap();
    let beats = lines.filter_map(|l| l.split_once(' ')).map(|(t, one)| (t.parse().unwrap(), one.parse::<u8>().unwrap())).collect::<Vec<(f32, u8)>>();
    // The analysis's 1 and the whole-song guess from the cues the live tracker uses.
    let ones = beats.iter().map(|b| (b.0, b.1 & 1 == 1)).collect();
    let oracle = beats.iter().map(|b| (b.0, b.1 & 2 == 2)).collect();
    Reference { bpm, beats: ones, oracle }
}

/// Which beat of the analysis's bars (0–3) the whole song's bass, snare and
/// harmony changes say is the 1.
fn oracle(bytes: &[u8], times: &[f32]) -> usize {
    let decoded = decode(bytes.to_vec()).expect("decode");
    let mix = resample(&decoded.mono(), decoded.sample_rate, RATE);
    let on = beats::onsets(&mix);
    let fps = beats::frames_per_second();
    let frames = chroma::frame_peaks(&mix);
    let per = chroma::per_beat(&frames, chroma::tuning(&frames), times);
    let at = |env: &[f32], t: f32| {
        let f = (t * fps) as usize;
        env[f.saturating_sub(2).min(env.len())..(f + 3).min(env.len())].iter().fold(0.0f32, |m, &v| m.max(v))
    };
    let dist = |a: &[f32; 12], b: &[f32; 12]| {
        let d: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let n = (a.iter().map(|x| x * x).sum::<f32>() * b.iter().map(|x| x * x).sum::<f32>()).sqrt();
        if n > 0.0 { 1.0 - d / n } else { 0.0 }
    };
    let (mut low, mut high, mut change) = ([0.0f32; 4], [0.0f32; 4], [0.0f32; 4]);
    for i in 1..times.len().min(per.len()) {
        let s = i % 4;
        let l = at(&on.low, times[i]);
        low[s] += l;
        high[s] += (at(&on.all, times[i]) - l).max(0.0);
        change[s] += (dist(&per[i].bass, &per[i - 1].bass) + 0.5 * dist(&per[i].treble, &per[i - 1].treble)) / 1.5;
    }
    let norm = |v: [f32; 4]| {
        let sum: f32 = v.iter().sum();
        v.map(|x| if sum > 0.0 { x / sum } else { 0.0 })
    };
    let (low, high, change) = (norm(low), norm(high), norm(change));
    let score = |d: usize| 2.0 * change[d] + low[d] + 0.5 * low[(d + 2) % 4] + 0.5 * (high[(d + 1) % 4] + high[(d + 3) % 4]) - 0.25 * (high[d] + high[(d + 2) % 4]);
    (0..4).max_by(|&a, &b| score(a).total_cmp(&score(b))).unwrap()
}

#[derive(Default)]
struct Score {
    lock_s: Option<f32>,
    end_bpm: f32,
    /// Seconds locked at the reference tempo, at twice or half it, and at anything else.
    right_s: f32,
    octave_s: f32,
    wrong_s: f32,
    seconds: f32,
    on_beat: usize,
    shown: usize,
    /// The 1s shown that fell on reference beats, by the beat of the reference bar.
    ones: [usize; 4],
    oracle_ones: [usize; 4],
    /// The oracle's 1 against the analysis's (0–3).
    oracle_phase: usize,
    unlocks: u32,
    /// While locked: seconds, how often the tempo shown (whole BPM) changed,
    /// and its distance from the reference's (or twice or half it), in BPM × seconds.
    locked_s: f32,
    shown_changes: u32,
    off_bpm_s: f32,
}

fn evaluate(path: &str) -> Score {
    let bytes = std::fs::read(path).expect("read the file");
    let reference = reference(path, &bytes);
    let decoded = decode(bytes).expect("decode");
    let audio = resample(&decoded.mono(), decoded.sample_rate, SR);
    let oracle_phase = reference.oracle.iter().position(|b| b.1).unwrap_or(0);
    let mut score = Score { seconds: audio.len() as f32 / SR as f32, oracle_phase, ..Score::default() };
    let beats: Vec<f32> = reference.beats.iter().map(|b| b.0).collect();

    let mut engine = Engine::new(SR as f32);
    engine.handle(Command::SetLive { on: true });
    let slow: f64 = std::env::var("SLOW").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let mut hearing = (std::env::var("HEAR").as_deref() == Ok("1")).then(|| midzer_hear::Hearing::with_models(SR as f32).unwrap());
    // Guides waiting for their run to finish: (ready at, in samples, command).
    let mut guides: std::collections::VecDeque<(u64, Command)> = Default::default();
    let mut busy_until = 0u64;
    let (mut locked, mut bpm, mut since) = (false, 0.0f32, 0.0f32);
    let tally = |locked: bool, bpm: f32, from: f32, to: f32, score: &mut Score| {
        if !locked {
            return;
        }
        let ratio = bpm / reference.bpm;
        let near = |r: f32| (ratio / r - 1.0).abs() < 0.04;
        let span = to - from;
        score.locked_s += span;
        let octave = [0.5, 1.0, 2.0].into_iter().min_by(|a, b| (ratio / a - 1.0).abs().total_cmp(&(ratio / b - 1.0).abs())).unwrap();
        score.off_bpm_s += span * (bpm.round() - reference.bpm * octave).abs();
        if near(1.0) {
            score.right_s += span;
        } else if near(2.0) || near(0.5) {
            score.octave_s += span;
        } else {
            score.wrong_s += span;
        }
    };
    for (i, block) in audio.chunks(128).enumerate() {
        let now = (i * 128 + block.len()) as u64;
        if let Some(h) = hearing.as_mut() {
            h.push(block, (i * 128) as u64);
            // A busy worker takes the audio that came meanwhile when it is free again.
            while busy_until <= now {
                let start = std::time::Instant::now();
                let Some(heard) = h.step() else { break };
                let took = (start.elapsed().as_secs_f64() * slow * SR as f64) as u64;
                busy_until = now + took;
                if let midzer_hear::Heard::Grid(g) = heard {
                    let command = Command::LiveGuide { frame: g.beat.round() as u64 as u32, period: g.period as f32, one: g.one, confidence: (g.confidence * 255.0) as u8 };
                    guides.push_back((busy_until, command));
                }
            }
            while guides.front().is_some_and(|g| g.0 <= now) {
                engine.handle(guides.pop_front().unwrap().1);
            }
        }
        engine.listen_live(block);
        let events = engine.process(block.len() as u32);
        for t in events {
            let at = (i * 128 + t.frame as usize) as f32 / SR as f32;
            match t.event {
                EngineEvent::Live { locked: l, .. } if l != locked => {
                    tally(locked, bpm, since, at, &mut score);
                    since = at;
                    if l {
                        score.lock_s.get_or_insert(at);
                    } else {
                        score.unlocks += 1;
                    }
                    locked = l;
                }
                EngineEvent::Tempo { bpm: b } => {
                    if locked && b.round() != bpm.round() {
                        score.shown_changes += 1;
                    }
                    tally(locked, bpm, since, at, &mut score);
                    since = at;
                    bpm = b;
                }
                EngineEvent::Beat { beat, .. } => {
                    score.shown += 1;
                    let i = beats.partition_point(|&b| b < at);
                    let Some(near) = [i.saturating_sub(1), i.min(beats.len() - 1)].into_iter().min_by(|&a, &b| (beats[a] - at).abs().total_cmp(&(beats[b] - at).abs())) else { continue };
                    if (at - beats[near]).abs() < 0.07 {
                        score.on_beat += 1;
                        if beat == 0 {
                            let one = reference.beats[..=near].iter().rposition(|b| b.1).unwrap_or(0);
                            score.ones[(near - one) % 4] += 1;
                            let one = reference.oracle[..=near].iter().rposition(|b| b.1).unwrap_or(0);
                            score.oracle_ones[(near - one) % 4] += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        engine.end_block();
    }
    tally(locked, bpm, since, score.seconds, &mut score);
    score.end_bpm = bpm / reference.bpm;
    score
}

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    // A few files at a time, each on its own thread.
    let mut scores = Vec::new();
    for chunk in files.chunks(6) {
        let handles: Vec<_> = chunk.iter().cloned().map(|f| std::thread::spawn(move || evaluate(&f))).collect();
        scores.extend(handles.into_iter().map(|h| h.join().unwrap()));
    }
    let (mut oracle_right, mut agreed) = (0, (0, 0));
    let (mut locked_s, mut changes, mut off) = (0.0f32, 0u32, 0.0f32);
    let (mut right, mut octave, mut wrong, mut total, mut on, mut shown, mut ones, mut ones_right, mut locks) = (0.0, 0.0, 0.0, 0.0, 0, 0, 0, 0, Vec::new());
    for (path, s) in files.iter().zip(&scores) {
        let name: String = path.rsplit('/').next().unwrap_or(path).chars().take(44).collect();
        let n1: usize = s.ones.iter().sum();
        println!(
            "{name:<44} lock {:>5.1}s end {:.2}x | right {:>3.0}% octave {:>3.0}% wrong {:>3.0}% | on beat {:>3}% | 1s {:>3}% {:?} oracle {} {:>3}% | unlocks {} | bpm moves {:>4.1}/min off {:.1}",
            s.lock_s.unwrap_or(f32::NAN),
            s.end_bpm,
            100.0 * s.right_s / s.seconds,
            100.0 * s.octave_s / s.seconds,
            100.0 * s.wrong_s / s.seconds,
            100 * s.on_beat / s.shown.max(1),
            100 * s.ones[0] / n1.max(1),
            s.ones,
            s.oracle_phase,
            100 * s.oracle_ones[0] / n1.max(1),
            s.unlocks,
            60.0 * s.shown_changes as f32 / s.locked_s.max(1.0),
            s.off_bpm_s / s.locked_s.max(1.0),
        );
        right += s.right_s;
        octave += s.octave_s;
        wrong += s.wrong_s;
        total += s.seconds;
        on += s.on_beat;
        shown += s.shown;
        ones += n1;
        ones_right += s.ones[0];
        oracle_right += s.oracle_ones[0];
        if s.oracle_phase == 0 {
            agreed.0 += s.ones[0];
            agreed.1 += n1;
        }
        locks.push(s.lock_s.unwrap_or(999.0));
        locked_s += s.locked_s;
        changes += s.shown_changes;
        off += s.off_bpm_s;
    }
    locks.sort_by(f32::total_cmp);
    println!(
        "ALL: right {:.0}% octave {:.0}% wrong {:.0}% | on beat {}% | 1s right {}% oracle {}% agreed {}% | median lock {:.1}s | bpm moves {:.1}/min off {:.2}",
        100.0 * right / total,
        100.0 * octave / total,
        100.0 * wrong / total,
        100 * on / shown.max(1),
        100 * ones_right / ones.max(1),
        100 * oracle_right / ones.max(1),
        100 * agreed.0 / agreed.1.max(1),
        locks.get(locks.len() / 2).copied().unwrap_or(f32::NAN),
        60.0 * changes as f32 / locked_s.max(1.0),
        off / locked_s.max(1.0),
    );
}
