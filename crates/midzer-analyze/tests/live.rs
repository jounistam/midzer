//! The live beat tracker on the band's own renders through the SoundFont:
//! it must find the band's tempo, lock within a few bars and sit on its
//! beats. Skipped when the SoundFont hasn't been fetched (`pnpm fetch:sounds`).

use midzer_core::{BeatList, BeatTracker, Chord, Command, Engine, Quality, Slot};
use midzer_synth::Synth;

const SR: f32 = 48_000.0;
/// The band's first beat, after a bit of silence like a recording.
const LEAD_S: f32 = 0.5;

fn soundfont() -> Option<Vec<u8>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/web/public/soundfonts/GeneralUser-GS.sf2");
    std::fs::read(path).ok()
}

/// Renders `bars` of the band over a four-chord loop, as mono.
fn render(font: &[u8], bpm: f32, style: u8, bars: usize) -> Vec<f32> {
    let mut engine = Engine::new(SR);
    let mut synth = Synth::new(SR);
    synth.load_soundfont(font).unwrap();
    engine.handle(Command::SetStyle { index: style });
    engine.handle(Command::SetTempo { bpm });
    engine.handle(Command::SongClear { bars: bars as u16 });
    let chart = [Chord::new(0, Quality::Major), Chord::new(9, Quality::Minor), Chord::new(5, Quality::Major), Chord::new(7, Quality::Major)];
    for b in 0..bars {
        engine.handle(Command::SongChord { beat: (b * 4) as u16, slot: Slot::Chord(chart[b % 4]) });
    }
    engine.handle(Command::SetSong { on: true });
    let mut out = vec![0.0f32; (LEAD_S * SR) as usize];
    let total = out.len() + (bars as f32 * 4.0 * SR * 60.0 / bpm) as usize;
    let (mut left, mut right) = ([0.0f32; 128], [0.0f32; 128]);
    engine.handle(Command::Play);
    while out.len() < total {
        let events = engine.process(128);
        synth.render(events, &mut left, &mut right);
        engine.end_block();
        out.extend(left.iter().zip(&right).map(|(l, r)| 0.5 * (l + r)));
    }
    out
}

struct Run {
    lock_s: Option<f32>,
    bpm: f32,
    /// Each locked beat's distance from the nearest band beat, in ms.
    errors: Vec<f32>,
}

fn run(audio: &[f32], bpm: f32) -> Run {
    let mut tracker = BeatTracker::new(SR);
    let beat = 60.0 / bpm;
    let mut run = Run { lock_s: None, bpm: 0.0, errors: Vec::new() };
    for (i, block) in audio.chunks(128).enumerate() {
        let mut out = BeatList::default();
        tracker.push(block, (i * 128) as u64, &mut out);
        for b in out.as_slice() {
            if !tracker.locked() {
                continue;
            }
            let t = b.at as f32 / SR;
            run.lock_s.get_or_insert(t);
            let into = (t - LEAD_S).rem_euclid(beat);
            run.errors.push(if into > beat / 2.0 { into - beat } else { into } * 1000.0);
            run.bpm = 60.0 * SR / b.period as f32;
        }
    }
    run
}

#[test]
fn locks_onto_the_band() {
    let Some(font) = soundfont() else {
        eprintln!("skipped: no SoundFont");
        return;
    };
    // The bossa's clave-like guitar and side stick never settle into a beat.
    for (bpm, style) in [(100.0, 0u8), (124.0, 1), (96.0, 2), (132.0, 3), (72.0, 4), (110.0, 5)] {
        let audio = render(&font, bpm, style, 32);
        let r = run(&audio, bpm);
        let mean = r.errors.iter().sum::<f32>() / r.errors.len().max(1) as f32;
        let spread = (r.errors.iter().map(|e| (e - mean) * (e - mean)).sum::<f32>() / r.errors.len().max(1) as f32).sqrt();
        let worst = r.errors.iter().fold(0.0f32, |m, e| m.max(e.abs()));
        eprintln!("style {style} at {bpm}: locked at {:?} s, {} BPM, mean {mean:.1} ms, spread {spread:.1}, worst {worst:.1}", r.lock_s, r.bpm);
        if style == 3 {
            assert!(r.lock_s.is_none(), "never locks onto a wrong beat");
            continue;
        }
        let lock = r.lock_s.expect("locks");
        assert!(lock < LEAD_S + 2.0 + 12.0 * 60.0 / bpm, "style {style}: locked at {lock} s");
        assert!((r.bpm - bpm).abs() < 1.0, "style {style}: {bpm} BPM tracked as {}", r.bpm);
        // The synth's attacks put the beats about 5 ms late; calibration takes that out.
        assert!(mean.abs() < 8.0 && spread < 4.0 && worst < 20.0, "style {style}: mean {mean}, spread {spread}, worst {worst}");
    }
}
