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
    }
}
