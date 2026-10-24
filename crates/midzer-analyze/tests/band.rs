//! End to end: the band plays a known chart through the SoundFont, and the
//! analysis must read back its tempo, bars and chords. Skipped when the
//! SoundFont hasn't been fetched (`pnpm fetch:sounds`).

use midzer_analyze::{RATE, analyze, resample, Extras, Step};
use midzer_core::{Chord, Command, Engine, Quality, Slot};
use midzer_synth::Synth;

const SR: u32 = 44_100;

fn soundfont() -> Option<Vec<u8>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/web/public/soundfonts/GeneralUser-GS.sf2");
    std::fs::read(path).ok()
}

/// Renders `bars` of the chart at `bpm`, with a melody played on top (key, beat, beats).
fn render(font: &[u8], bpm: f32, style: u8, chart: &[Chord], melody: &[(u8, f32, f32)], bars: usize) -> Vec<f32> {
    let mut engine = Engine::new(SR as f32);
    let mut synth = Synth::new(SR as f32);
    synth.load_soundfont(font).unwrap();
    engine.handle(Command::SetStyle { index: style });
    engine.handle(Command::SetTempo { bpm });
    engine.handle(Command::SongClear { bars: bars as u16 });
    for b in 0..bars {
        let slot = Slot::Chord(chart[b % chart.len()]);
        engine.handle(Command::SongChord { beat: (b * 4) as u16, slot });
    }
    engine.handle(Command::SetSong { on: true });
    // Half a second of silence first, like a recording.
    let lead = SR as usize / 2;
    let mut out = vec![0.0f32; lead];
    let beat_frames = SR as f32 * 60.0 / bpm;
    let total = ((bars + 2) as f32 * 4.0 * beat_frames) as usize;
    let (mut left, mut right) = ([0.0f32; 128], [0.0f32; 128]);
    let mut frame = 0usize;
    let mut played = false;
    // Melody note on/off times in frames.
    let mut cues: Vec<(usize, Command)> = melody
        .iter()
        .flat_map(|&(key, at, len)| {
            [
                ((at * beat_frames) as usize, Command::NoteOn { note: key, velocity: 100 }),
                (((at + len) * beat_frames) as usize - 200, Command::NoteOff { note: key }),
            ]
        })
        .collect();
    cues.sort_by_key(|c| c.0);
    let mut next_cue = 0;
    while frame < total {
        if !played {
            engine.handle(Command::Play);
            played = true;
        }
        while next_cue < cues.len() && cues[next_cue].0 <= frame {
            engine.handle(cues[next_cue].1);
            next_cue += 1;
        }
        let events = engine.process(128);
        synth.render(events, &mut left, &mut right);
        engine.end_block();
        out.extend(left.iter().zip(&right).map(|(l, r)| 0.5 * (l + r)));
        frame += 128;
    }
    out
}

fn names(chords: &[Option<midzer_analyze::ChordOut>]) -> Vec<String> {
    chords
        .iter()
        .step_by(4)
        .map(|c| c.map_or("N".into(), |c| Chord { root: c.root, quality: Quality::ALL[c.quality as usize], bass: c.bass }.to_string()))
        .collect()
}

#[test]
fn reads_back_what_the_band_played() {
    let Some(font) = soundfont() else {
        eprintln!("skipped: no SoundFont");
        return;
    };
    let chart = [
        Chord::new(0, Quality::Major),
        Chord::new(9, Quality::Minor),
        Chord::new(5, Quality::Major),
        Chord::new(7, Quality::Major),
    ];
    for (bpm, style) in [(100.0, 0u8), (124.0, 1), (84.0, 4)] {
        let audio = render(&font, bpm, style, &chart, &[], 16);
        let mix = resample(&audio, SR, RATE);
        let a = analyze(&mix, Extras::default(), &mut |_: f32, _: Step| {}).unwrap();
        assert!((a.bpm - bpm).abs() < 2.0, "style {style}: {bpm} BPM read as {}", a.bpm);
        // The first downbeat is the band's first beat, half a second in.
        assert!((a.bar_times[0] - 0.5).abs() < 0.05, "style {style}: first bar at {}", a.bar_times[0]);
        let found = names(&a.chords);
        let expected: Vec<String> = (0..found.len()).map(|b| chart[b % 4].to_string()).collect();
        let right = found.iter().zip(&expected).take(16).filter(|(f, e)| f == e).count();
        assert!(right >= 14, "style {style}: {found:?}");
        // Enough to draw and recognise it by, without the audio.
        assert_eq!(a.envelope.len(), (a.duration * 100.0).ceil() as usize);
        assert!(a.envelope.iter().any(|&l| l > 128), "style {style}: never loud");
        assert!(a.fingerprint.len() / 2 > a.duration as usize * 10 && a.fingerprint.len().is_multiple_of(2), "style {style}: {} landmarks", a.fingerprint.len() / 2);
    }
}

/// Clicks speeding up from 100 to 112 BPM over the recording, a kick-like
/// thump on every fourth; returns the audio at [`RATE`] and the click times.
fn drifting_clicks(seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let mut x = vec![0.0f32; (RATE as f32 * seconds) as usize];
    let mut seed = 7u32;
    let (mut t, mut k) = (0.5f32, 0);
    let mut times = Vec::new();
    while ((t * RATE as f32) as usize) + 3000 < x.len() {
        let at = (t * RATE as f32) as usize;
        times.push(t);
        for i in 0..1500 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 9) as f32 / (1 << 23) as f32 * 2.0 - 1.0;
            let env = (-(i as f32) / 300.0).exp();
            let thump = if k % 4 == 0 { (i as f32 / RATE as f32 * 60.0 * std::f32::consts::TAU).sin() * 0.8 } else { 0.0 };
            x[at + i] += (0.4 * noise + thump) * env;
        }
        let bpm = 100.0 + 12.0 * t / seconds;
        t += 60.0 / bpm;
        k += 1;
    }
    (x, times)
}

#[test]
fn the_tempo_map_follows_a_recording_that_speeds_up() {
    let (audio, clicks) = drifting_clicks(60.0);
    let a = analyze(&audio, Extras::default(), &mut |_: f32, _: Step| {}).unwrap();
    assert_eq!(a.beat_times.len(), a.bar_times.len() * 4 + 1);
    // Every mapped beat inside the recording sits on a click, though the
    // usual tempo alone would be far off by the end.
    let last_click = *clicks.last().unwrap();
    let mut worst = 0.0f32;
    for &b in a.beat_times.iter().filter(|&&b| b <= last_click) {
        let nearest = clicks.iter().map(|c| (c - b).abs()).fold(f32::MAX, f32::min);
        worst = worst.max(nearest);
    }
    // Beat This! hears these noise clicks about 10 ms late, give or take a frame.
    assert!(worst < 0.035, "a beat {worst} s off its click");
    let drift = a.beat_times.iter().enumerate().map(|(i, b)| (a.bar_times[0] + i as f32 * 60.0 / a.bpm - b).abs()).fold(0.0, f32::max);
    assert!(drift > 0.3, "a steady grid would be only {drift} s off");
}
