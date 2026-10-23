//! Plays recordings into [`Hearing`] as if live and scores what it would
//! show, moment by moment, against references made from the whole songs:
//!   HEAR_REF=<dir> cargo run --release -p midzer-hear --example hear_eval -- a.mp3 …
//!
//! References: `<dir>/beats/<name>.bt` ("time is_downbeat" per line, e.g. from
//! Beat This! final0 on the whole song) and `<dir>/chords/<name>.lab` (Harte
//! labels, e.g. from BTC on the whole song).
//!
//! Each run's result counts from when it would be ready: the audio it heard,
//! plus its compute time here times `SLOW` (default 2, for wasm). Scores:
//! - beats: the share of reference beats the grid then shown puts a beat
//!   within 70 ms of, from the song's start; and when 8 in a row first are;
//! - tempo: the share of reference beats shown at the right tempo (±4 %);
//! - the 1: of the beats shown with a 1, the share in the right place of the bar;
//! - chords: every 0.1 s, the chord shown (the latest run's chord for that
//!   time, or its newest one) against the reference, major/minor (mir_eval's
//!   "majmin": sevenths count as their triad; diminished, augmented and
//!   suspended reference chords are left out). Also in hindsight (what the
//!   last run that heard that moment said: how well the chords are
//!   recognised, apart from how late), and how often the chord shown was
//!   predicted from the song's loop and how often that was right.
//!
//! Settings: WINDOW, EVERY (beat runs), CHORD_EVERY, EDGE, FIT (seconds).

use midzer_analyze::{decode, resample};
use midzer_core::Quality;
use midzer_hear::{Heard, Hearing};

const SR: u32 = 48_000;

fn env(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// Root and major (true) or minor, or None for no chord; Err for chords majmin leaves out.
type MajMin = Result<Option<(u8, bool)>, ()>;

fn majmin_label(label: &str) -> MajMin {
    if label == "N" || label == "X" {
        return Ok(None);
    }
    let (root, quality) = label.split_once(':').unwrap_or((label, "maj"));
    let quality = quality.split('/').next().unwrap();
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let flats = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];
    let r = names.iter().position(|n| *n == root).or_else(|| flats.iter().position(|n| *n == root)).ok_or(())? as u8;
    match quality {
        "maj" | "7" | "maj7" | "maj6" | "9" | "maj9" => Ok(Some((r, true))),
        "min" | "min7" | "min6" | "minmaj7" | "min9" => Ok(Some((r, false))),
        _ => Err(()),
    }
}

fn majmin_chord(c: Option<midzer_core::Chord>) -> MajMin {
    let Some(c) = c else { return Ok(None) };
    match c.quality {
        Quality::Major | Quality::Dominant7 | Quality::Major7 | Quality::Major6 | Quality::Add9 => Ok(Some((c.root, true))),
        Quality::Minor | Quality::Minor7 | Quality::Minor6 => Ok(Some((c.root, false))),
        _ => Err(()),
    }
}

#[derive(Default)]
struct Score {
    beats: usize,
    hit: usize,
    tempo: usize,
    ones: usize,
    ones_right: usize,
    lock_s: Option<f64>,
    chord_n: usize,
    chord_right: usize,
    hindsight_right: usize,
    predicted_n: usize,
    predicted_right: usize,
    runs: usize,
    compute_s: f64,
    seconds: f64,
}

fn evaluate(path: &str, dir: &str, slow: f64) -> Score {
    let name = path.rsplit('/').next().unwrap();
    let text = std::fs::read_to_string(format!("{dir}/beats/{name}.bt")).expect("beat reference");
    let reference: Vec<(f64, bool)> = text.lines().filter_map(|l| l.split_once(' ')).map(|(t, d)| (t.parse().unwrap(), d == "1")).collect();
    // The reference's place in the bar for each beat (None before the first 1).
    let mut place: Vec<Option<u8>> = Vec::new();
    let mut since: Option<u8> = None;
    for &(_, one) in &reference {
        since = if one { Some(0) } else { since.map(|s| s + 1) };
        place.push(since.filter(|&s| s < 4));
    }
    let labels: Vec<(f64, f64, MajMin)> = std::fs::read_to_string(format!("{dir}/chords/{name}.lab"))
        .expect("chord reference")
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, majmin_label(it.next()?)))
        })
        .collect();

    let d = decode(std::fs::read(path).unwrap()).unwrap();
    let audio = resample(&d.mono(), d.sample_rate, SR);
    let mut hearing = Hearing::with_models(SR as f32).unwrap();
    if let Some(v) = env("WINDOW") {
        hearing.settings.beat_window_s = v;
    }
    if let Some(v) = env("EVERY") {
        hearing.settings.beat_every_s = v;
    }
    if let Some(v) = env("CHORD_EVERY") {
        hearing.settings.chord_every_s = v;
    }
    if let Some(v) = env("EDGE") {
        hearing.settings.edge_s = v;
    }
    if let Some(v) = env("FIT") {
        hearing.settings.fit_s = v;
    }
    // (ready at, result), in seconds.
    let mut grids = Vec::new();
    let mut chords = Vec::new();
    let mut score = Score { seconds: audio.len() as f64 / SR as f64, ..Score::default() };
    // When the worker is free again.
    let mut free_at = 0.0f64;
    for (i, block) in audio.chunks(2048).enumerate() {
        hearing.push(block, (i * 2048) as u64);
        let now = ((i + 1) * 2048) as f64 / SR as f64;
        // A busy worker takes the audio that came meanwhile when it is free again.
        while free_at <= now {
            let start = std::time::Instant::now();
            let Some(heard) = hearing.step() else { break };
            let took = start.elapsed().as_secs_f64() * slow;
            score.runs += 1;
            score.compute_s += took;
            let ready = now + took;
            free_at = ready;
            match heard {
                Heard::Grid(g) => grids.push((ready, g)),
                Heard::Chords(c) => chords.push((ready, c)),
            }
        }
    }

    let mut streak = 0;
    for (i, &(r, _)) in reference.iter().enumerate() {
        score.beats += 1;
        let Some((_, g)) = grids.iter().rev().find(|(ready, _)| *ready <= r) else { continue };
        let frame = r * SR as f64;
        let k = ((frame - g.beat) / g.period).round();
        let err = (frame - (g.beat + k * g.period)).abs() / SR as f64;
        let local = if i + 1 < reference.len() { reference[i + 1].0 - r } else { r - reference[i - 1].0 };
        if (g.period / SR as f64 / local - 1.0).abs() < 0.04 {
            score.tempo += 1;
        }
        if err < 0.07 {
            score.hit += 1;
            streak += 1;
            if streak == 8 && score.lock_s.is_none() {
                score.lock_s = Some(reference[i + 1 - 8].0);
            }
            if let (Some(one), Some(p)) = (g.one, place[i]) {
                score.ones += 1;
                if (one as i64 + k as i64).rem_euclid(4) as u8 == p {
                    score.ones_right += 1;
                }
            }
        } else {
            streak = 0;
        }
    }

    let mut t = 0.0;
    while t < score.seconds {
        let reference = labels.iter().find(|l| l.0 <= t && t < l.1).map(|l| l.2);
        if let Some(Ok(want)) = reference {
            score.chord_n += 1;
            let frame = t * SR as f64;
            if let Some((_, spans)) = chords.iter().rev().find(|(ready, _)| *ready <= t) {
                let span = spans.iter().find(|s| s.from <= frame && frame < s.to).or(spans.last());
                if let Some(s) = span {
                    let right = majmin_chord(s.chord) == Ok(want);
                    score.chord_right += right as usize;
                    if s.predicted && s.from <= frame && frame < s.to {
                        score.predicted_n += 1;
                        score.predicted_right += right as usize;
                    }
                }
            }
            let heard = chords.iter().rev().find_map(|(_, spans)| spans.iter().find(|s| !s.predicted && s.from <= frame && frame < s.to));
            if heard.is_some_and(|s| majmin_chord(s.chord) == Ok(want)) {
                score.hindsight_right += 1;
            }
        }
        t += 0.1;
    }
    score
}

fn main() {
    let dir = std::env::var("HEAR_REF").expect("HEAR_REF");
    let slow = env("SLOW").unwrap_or(2.0);
    let files: Vec<String> = std::env::args().skip(1).collect();
    let mut scores = Vec::new();
    for chunk in files.chunks(4) {
        let handles: Vec<_> = chunk.iter().cloned().map(|f| {
            let dir = dir.clone();
            std::thread::spawn(move || evaluate(&f, &dir, slow))
        }).collect();
        scores.extend(handles.into_iter().map(|h| h.join().unwrap()));
    }
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    let mut all = Score::default();
    let mut locks = Vec::new();
    for (path, s) in files.iter().zip(&scores) {
        let name: String = path.rsplit('/').next().unwrap().chars().take(44).collect();
        println!(
            "{name:<44} beats {:>3.0}% tempo {:>3.0}% 1s {:>3.0}% | lock {:>5.1}s | chords {:>3.0}% (hindsight {:>3.0}%, predicted {:>3.0}% of the time, {:>3.0}% right) | {:.0} ms a run",
            pct(s.hit, s.beats),
            pct(s.tempo, s.beats),
            pct(s.ones_right, s.ones),
            s.lock_s.unwrap_or(f64::NAN),
            pct(s.chord_right, s.chord_n),
            pct(s.hindsight_right, s.chord_n),
            pct(s.predicted_n, s.chord_n),
            pct(s.predicted_right, s.predicted_n),
            1000.0 * s.compute_s / s.runs.max(1) as f64,
        );
        all.beats += s.beats;
        all.hit += s.hit;
        all.tempo += s.tempo;
        all.ones += s.ones;
        all.ones_right += s.ones_right;
        all.chord_n += s.chord_n;
        all.chord_right += s.chord_right;
        all.hindsight_right += s.hindsight_right;
        all.predicted_n += s.predicted_n;
        all.predicted_right += s.predicted_right;
        all.runs += s.runs;
        all.compute_s += s.compute_s;
        all.seconds += s.seconds;
        locks.push(s.lock_s.unwrap_or(999.0));
    }
    locks.sort_by(f64::total_cmp);
    println!(
        "ALL: beats {:.0}% tempo {:.0}% 1s {:.0}% | median lock {:.1}s | chords {:.0}% (hindsight {:.0}%, predicted {:.0}% of the time, {:.0}% right) | busy {:.0}% of the time",
        pct(all.hit, all.beats),
        pct(all.tempo, all.beats),
        pct(all.ones_right, all.ones),
        locks[locks.len() / 2],
        pct(all.chord_right, all.chord_n),
        pct(all.hindsight_right, all.chord_n),
        pct(all.predicted_n, all.chord_n),
        pct(all.predicted_right, all.predicted_n),
        100.0 * all.compute_s / all.seconds,
    );
}
