//! Prints what the analysis reads from an audio file:
//!   cargo run --release -p midzer-analyze --example analyze -- song.mp3 [--json]

use midzer_analyze::{Step, analyze_file};
use midzer_core::{Chord, Quality};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: analyze <file> [--json]");
    let bytes = std::fs::read(path).expect("read the file");
    let start = std::time::Instant::now();
    let mut last = None;
    let analysis = analyze_file(bytes, &mut |_, step: Step| {
        if last != Some(step) {
            eprintln!("{:>6.2}s {step:?}", start.elapsed().as_secs_f32());
            last = Some(step);
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    eprintln!("{:>6.2}s done", start.elapsed().as_secs_f32());
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&analysis).unwrap());
        return;
    }
    let a = &analysis;
    println!("{:?} – {:?}, {:.0}s, {} BPM, key {}{}, tuning {} cents", a.title, a.artist, a.duration, a.bpm, ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"][a.key.tonic as usize], if a.key.minor { "m" } else { "" }, a.tuning_cents);
    let name = |c: &Option<midzer_analyze::ChordOut>| c.map_or(".".to_string(), |c| Chord { root: c.root, quality: Quality::ALL[c.quality as usize], bass: c.bass }.to_string());
    let mut at = 0;
    for (b, &n) in a.bar_beats.iter().enumerate() {
        let bar = &a.chords[at..at + n as usize];
        at += n as usize;
        if let Some(s) = a.sections.iter().find(|s| s.bar == b) {
            print!("\n[{}] ", s.part);
        } else if b % 4 == 0 {
            println!();
        }
        print!("{:>5.1}s {:<22}|", a.bar_times[b], bar.iter().map(name).collect::<Vec<_>>().join(" "));
    }
    println!("\nmelody: {} notes from {:?}; warnings: {:?}", a.melody.len(), a.melody_from, a.warnings);
}
